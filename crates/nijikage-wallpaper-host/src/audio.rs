//! 系統音訊擷取（WASAPI loopback）：只計算「整體音量」與 16 個頻帶的能量，
//! **不錄音、不儲存、不上傳、不讀取其他程式的記憶體**；原始取樣只在記憶體的小緩衝區停留幾十毫秒，用完即丟。
//! 設定中關閉「音訊動態環」時不會建立此物件，因此完全不會擷取。

use nijikage_wallpaper_render::AudioInput;
use std::f32::consts::PI;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator,
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, WAVEFORMATEX,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED};

const FFT_N: usize = 1024;
const BANDS: usize = 16;

pub struct AudioCapture {
    shared: Arc<Mutex<AudioInput>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl AudioCapture {
    pub fn start() -> Self {
        let shared = Arc::new(Mutex::new(AudioInput::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (s2, st2) = (shared.clone(), stop.clone());
        let handle = std::thread::Builder::new()
            .name("nijikage-audio".into())
            .spawn(move || capture_thread(s2, st2))
            .ok();
        Self { shared, stop, handle }
    }

    /// 最新的（已平滑的）音量與頻帶
    pub fn latest(&self) -> AudioInput {
        self.shared.lock().map(|g| *g).unwrap_or_default()
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn capture_thread(shared: Arc<Mutex<AudioInput>>, stop: Arc<AtomicBool>) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let mut last_err = String::new();
    while !stop.load(Ordering::Relaxed) {
        if let Err(e) = run_capture(&shared, &stop) {
            if e != last_err {
                crate::log(&format!("音訊擷取中斷：{e}（稍後重試）"));
                last_err = e;
            }
        }
        if let Ok(mut g) = shared.lock() {
            *g = AudioInput::default();
        }
        // 預設輸出裝置可能被切換或暫時不可用，稍後重試
        for _ in 0..20 {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// 把交錯的取樣轉成單聲道 f32
fn to_mono(data: *const u8, frames: usize, channels: usize, bits: u16, out: &mut Vec<f32>) {
    unsafe {
        match bits {
            32 => {
                let s = std::slice::from_raw_parts(data as *const f32, frames * channels);
                for f in 0..frames {
                    let mut acc = 0.0;
                    for c in 0..channels {
                        acc += s[f * channels + c];
                    }
                    out.push(acc / channels as f32);
                }
            }
            16 => {
                let s = std::slice::from_raw_parts(data as *const i16, frames * channels);
                for f in 0..frames {
                    let mut acc = 0.0;
                    for c in 0..channels {
                        acc += s[f * channels + c] as f32 / 32768.0;
                    }
                    out.push(acc / channels as f32);
                }
            }
            _ => out.extend(std::iter::repeat(0.0).take(frames)),
        }
    }
}

fn run_capture(shared: &Arc<Mutex<AudioInput>>, stop: &Arc<AtomicBool>) -> Result<(), String> {
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|e| format!("建立裝置列舉器失敗：{e}"))?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).map_err(|e| format!("找不到預設輸出裝置：{e}"))?;
        let client: IAudioClient = device.Activate(CLSCTX_ALL, None).map_err(|e| format!("啟用音訊用戶端失敗：{e}"))?;
        let fmt: *mut WAVEFORMATEX = client.GetMixFormat().map_err(|e| format!("取得混音格式失敗：{e}"))?;
        let (channels, rate, bits) = ((*fmt).nChannels as usize, (*fmt).nSamplesPerSec as f32, (*fmt).wBitsPerSample);
        let init = client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, 2_000_000, 0, fmt, None);
        CoTaskMemFree(Some(fmt as *const _));
        init.map_err(|e| format!("初始化 loopback 失敗：{e}"))?;
        let capture: IAudioCaptureClient = client.GetService().map_err(|e| format!("取得擷取服務失敗：{e}"))?;
        client.Start().map_err(|e| format!("啟動擷取失敗：{e}"))?;

        let window: Vec<f32> = (0..FFT_N).map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / FFT_N as f32).cos()).collect();
        // 16 個對數頻帶的邊界（Hz）：60 ～ 12000
        let edges: Vec<f32> = (0..=BANDS).map(|i| 60.0 * (12000.0f32 / 60.0).powf(i as f32 / BANDS as f32)).collect();
        let mut ring: Vec<f32> = vec![0.0; FFT_N];
        let mut tmp: Vec<f32> = Vec::with_capacity(4096);
        let mut smooth = AudioInput::default();
        let mut last_data = Instant::now();
        let mut last_analyze = Instant::now();

        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(8));
            loop {
                let pkt = capture.GetNextPacketSize().map_err(|e| format!("讀取封包失敗：{e}"))?;
                if pkt == 0 {
                    break;
                }
                let (mut data, mut frames, mut flags) = (std::ptr::null_mut::<u8>(), 0u32, 0u32);
                capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None).map_err(|e| format!("取得緩衝失敗：{e}"))?;
                tmp.clear();
                if flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0 {
                    tmp.resize(frames as usize, 0.0);
                } else {
                    to_mono(data, frames as usize, channels, bits, &mut tmp);
                    last_data = Instant::now();
                }
                capture.ReleaseBuffer(frames).map_err(|e| format!("釋放緩衝失敗：{e}"))?;
                // 把新取樣推進環形緩衝（保留最近 FFT_N 個）
                if tmp.len() >= FFT_N {
                    ring.copy_from_slice(&tmp[tmp.len() - FFT_N..]);
                } else if !tmp.is_empty() {
                    ring.rotate_left(tmp.len());
                    let n = ring.len();
                    ring[n - tmp.len()..].copy_from_slice(&tmp);
                }
            }
            // 約 60 次／秒分析一次
            if last_analyze.elapsed() >= Duration::from_millis(16) {
                last_analyze = Instant::now();
                // 超過 120ms 沒有新資料（沒有聲音輸出）→ 視為靜音，讓能量自然衰減
                let silent = last_data.elapsed() > Duration::from_millis(120);
                let (level_raw, bands_raw) =
                    if silent { (0.0, [0.0f32; BANDS]) } else { analyze(&ring, &window, rate, &edges) };
                // 快速上升、緩慢下降
                let blend = |cur: f32, target: f32| -> f32 {
                    let k = if target > cur { 0.55 } else { 0.10 };
                    cur + (target - cur) * k
                };
                smooth.level = blend(smooth.level, level_raw);
                for i in 0..BANDS {
                    smooth.bands[i] = blend(smooth.bands[i], bands_raw[i]);
                }
                if let Ok(mut g) = shared.lock() {
                    *g = smooth;
                }
            }
        }
        let _ = client.Stop();
    }
    Ok(())
}

/// 取最近 FFT_N 個取樣做 FFT，回傳（整體音量 0–1，16 頻帶 0–1）
fn analyze(ring: &[f32], window: &[f32], rate: f32, edges: &[f32]) -> (f32, [f32; BANDS]) {
    let rms = (ring.iter().map(|v| v * v).sum::<f32>() / ring.len() as f32).sqrt();
    let level = (((20.0 * (rms + 1e-6).log10()) + 56.0) / 50.0).clamp(0.0, 1.0);
    let mut re: Vec<f32> = ring.iter().zip(window).map(|(a, w)| a * w).collect();
    let mut im = vec![0.0f32; FFT_N];
    fft(&mut re, &mut im);
    let bin_hz = rate / FFT_N as f32;
    let mut bands = [0.0f32; BANDS];
    for b in 0..BANDS {
        let lo = ((edges[b] / bin_hz).floor() as usize).max(1);
        let hi = ((edges[b + 1] / bin_hz).ceil() as usize).clamp(lo + 1, FFT_N / 2);
        let mut acc = 0.0f32;
        for k in lo..hi {
            acc += (re[k] * re[k] + im[k] * im[k]).sqrt();
        }
        let mag = acc / (hi - lo) as f32 / (FFT_N as f32 * 0.25);
        bands[b] = (((20.0 * (mag + 1e-6).log10()) + 62.0) / 52.0).clamp(0.0, 1.0);
    }
    (level, bands)
}

/// 原地 radix-2 FFT（長度須為 2 的冪）
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * PI / len as f32;
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let tr = re[b] * cr - im[b] * ci;
                let ti = re[b] * ci + im[b] * cr;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let ncr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = ncr;
            }
        }
        len <<= 1;
    }
}
