//! 產生 README 用的示意圖（離屏渲染，不需要視窗，也不含任何個人內容）。
//!
//! 用法：`cargo run --release -p nijikage-wallpaper-render --example screenshots -- <輸出資料夾>`
//! 產生：disc-mode.png、audio-ring.png、desktop-mode.png（1920×1080）。
//! 示意用的日落風景圖由程式產生（`sunset()`），不依賴任何外部檔案。

#[path = "../tests/support/mod.rs"]
#[allow(dead_code)]
mod support;

use nijikage_wallpaper_core::{BackgroundMode, Config, DiscStyle, FitMode, LayoutMode};
use nijikage_wallpaper_render::AudioInput;
use std::path::PathBuf;
use support::{Capture, Gpu, TestResult};

/// 程式產生的日落風景（1920×1080）：漸層天空、太陽光暈、星點與三層山脈。
fn sunset(w: u32, h: u32) -> image::RgbaImage {
    let mut img = image::RgbaImage::new(w, h);
    let hash = |x: u32, y: u32| -> u32 {
        let mut v = x.wrapping_mul(374_761_393) ^ y.wrapping_mul(668_265_263);
        v = (v ^ (v >> 13)).wrapping_mul(1_274_126_177);
        v ^ (v >> 16)
    };
    let mix = |a: [f32; 3], b: [f32; 3], t: f32| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            // 天空：頂部深紫 → 中段洋紅 → 地平線橘黃
            let sky_t = (fy / 0.72).clamp(0.0, 1.0);
            let mut c = if sky_t < 0.55 {
                mix([18.0, 16.0, 56.0], [140.0, 52.0, 110.0], sky_t / 0.55)
            } else {
                mix([140.0, 52.0, 110.0], [255.0, 170.0, 80.0], (sky_t - 0.55) / 0.45)
            };
            // 太陽與光暈
            let (dx, dy) = ((fx - 0.62) * (w as f32 / h as f32), fy - 0.60);
            let d = (dx * dx + dy * dy).sqrt();
            if d < 0.30 {
                let k = (1.0 - d / 0.30).powf(2.2);
                c = mix(c, [255.0, 226.0, 150.0], k);
            }
            if d < 0.075 {
                c = [255.0, 245.0, 215.0];
            }
            // 星點（只在上半部）
            if fy < 0.38 && hash(x, y) % 2600 == 0 {
                c = mix(c, [255.0, 255.0, 245.0], 0.9);
            }
            // 三層山脈：越近越暗
            for (i, (base, amp, freq, col)) in [
                (0.70f32, 0.06f32, 5.0f32, [96.0f32, 50.0, 96.0]),
                (0.78, 0.05, 8.0, [58.0, 30.0, 70.0]),
                (0.88, 0.04, 12.0, [26.0, 16.0, 44.0]),
            ]
            .iter()
            .enumerate()
            {
                let ridge = base + amp * (fx * freq + i as f32 * 1.7).sin() + amp * 0.45 * (fx * freq * 2.6 + 0.8 * i as f32).sin();
                if fy > ridge {
                    c = *col;
                }
            }
            img.put_pixel(x, y, image::Rgba([c[0].clamp(0.0, 255.0) as u8, c[1].clamp(0.0, 255.0) as u8, c[2].clamp(0.0, 255.0) as u8, 255]));
        }
    }
    img
}

fn demo_audio() -> AudioInput {
    // 低頻較強、高頻漸弱的音樂頻譜
    let mut bands = [0.0f32; 16];
    for (i, b) in bands.iter_mut().enumerate() {
        let t = i as f32 / 15.0;
        *b = (0.85 - 0.55 * t + 0.18 * (i as f32 * 1.9).sin()).clamp(0.05, 1.0);
    }
    AudioInput { level: 0.85, bands }
}

fn main() -> TestResult {
    let out = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "docs/images".into()));
    std::fs::create_dir_all(&out)?;
    let art_path = out.join(".demo-sunset.png");
    sunset(1920, 1080).save(&art_path)?;

    pollster::block_on(async {
        let gpu = Gpu::new().await?;
        let size = [1920u32, 1080u32];
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;

        // 1. 光碟模式
        let mut cfg = Config::default();
        cfg.background.mode = BackgroundMode::Color;
        cfg.background.color = [14, 16, 30];
        cfg.disc.layout_mode = LayoutMode::Free;
        cfg.disc.art_path = Some(art_path.clone());
        cfg.disc.style = DiscStyle::Bd;
        cfg.disc.free.center_x = 0.70;
        cfg.disc.free.center_y = 0.52;
        cfg.disc.free.diameter = 0.92;
        cfg.disc.rotate = true;
        cfg.effects.on_desktop.sakura = true;
        cfg.effects.intensity = 0.3;
        cfg.audio_ring.enabled = true;
        let mut cap = Capture::new(&gpu, &cfg, size, format);
        cap.input.cursor = Some([1450.0, 430.0]);
        cap.settle(AudioInput::default(), 90);
        cap.save(out.join("disc-mode.png"))?;

        // 2. 音訊動態環
        cap.settle(demo_audio(), 40);
        cap.save(out.join("audio-ring.png"))?;

        // 3. 全桌面模式
        let mut cfg = Config::default();
        cfg.disc.layout_mode = LayoutMode::Desktop;
        cfg.background.mode = BackgroundMode::Image;
        cfg.background.image_path = Some(art_path.clone());
        cfg.background.fit = FitMode::Fill;
        cfg.desktop_fx.intensity = 0.7;
        cfg.effects.on_desktop.sakura = true;
        cfg.effects.intensity = 0.25;
        let mut cap = Capture::new(&gpu, &cfg, size, format);
        cap.input.cursor = Some([760.0, 520.0]);
        cap.settle(AudioInput::default(), 90);
        cap.save(out.join("desktop-mode.png"))?;
        Ok::<(), Box<dyn std::error::Error>>(())
    })?;
    let _ = std::fs::remove_file(&art_path);
    println!("已輸出到 {}", out.display());
    Ok(())
}
