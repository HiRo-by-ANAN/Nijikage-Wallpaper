//! Nijikage-Wallpaper 桌布程式：掛到桌面圖示後方的 GPU 視窗。
//! 參數：--windowed 以一般視窗執行（開發與預覽用，不掛到桌面）。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]


mod audio;
mod gfx;
mod win32;

use nijikage_wallpaper_core::i18n::{set_language, tr};
use nijikage_wallpaper_core::{config_path, Config};
use nijikage_wallpaper_render::FrameInput;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{Window, WindowId};

/// 把訊息寫進 %APPDATA%\Nijikage-Wallpaper\log.txt（背景程式沒有主控台）
fn log(msg: &str) {
    use std::io::Write;
    let dir = nijikage_wallpaper_core::app_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("log.txt")) {
        let _ = writeln!(f, "{msg}");
    }
    eprintln!("{msg}");
}

/// 相同訊息只記錄一次，避免洗版
fn log_once(msg: &str) {
    use std::sync::Mutex;
    static LAST: Mutex<String> = Mutex::new(String::new());
    if let Ok(mut l) = LAST.lock() {
        if *l != msg {
            *l = msg.to_string();
            log(msg);
        }
    }
}

fn make_tray_icon() -> Option<Icon> {
    // 程式繪製的小光碟圖示：藍紫漸層圓環，中心挖孔
    let n = 32u32;
    let mut px = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let (dx, dy) = (x as f32 - 15.5, y as f32 - 15.5);
            let d = (dx * dx + dy * dy).sqrt();
            if d < 14.5 && d > 4.0 {
                let t = (x + y) as f32 / 62.0;
                px.extend_from_slice(&[(120.0 + 100.0 * t) as u8, (150.0 - 60.0 * t) as u8, 255, 255]);
            } else {
                px.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
    Icon::from_rgba(px, n, n).ok()
}

struct TrayState {
    icon: TrayIcon,
    settings_item: MenuItem,
    quit_item: MenuItem,
    toggle_item: MenuItem,
    settings_id: MenuId,
    toggle_id: MenuId,
    quit_id: MenuId,
}

fn build_tray(enabled: bool) -> Option<TrayState> {
    let menu = Menu::new();
    let settings = MenuItem::new(tr("設定…"), true, None);
    let toggle = MenuItem::new(if enabled { tr("暫停桌布") } else { tr("繼續桌布") }, true, None);
    let quit = MenuItem::new(tr("結束 Nijikage-Wallpaper"), true, None);
    menu.append_items(&[&settings, &toggle, &PredefinedMenuItem::separator(), &quit]).ok()?;
    let mut b = TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip(tr("Nijikage-Wallpaper 虹影"));
    if let Some(i) = make_tray_icon() {
        b = b.with_icon(i);
    }
    let icon = b.build().map_err(|e| log(&format!("建立托盤圖示失敗：{e}"))).ok()?;
    Some(TrayState {
        icon,
        settings_id: settings.id().clone(),
        toggle_id: toggle.id().clone(),
        quit_id: quit.id().clone(),
        settings_item: settings,
        quit_item: quit,
        toggle_item: toggle,
    })
}

fn open_settings() {
    let exe = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("nijikage-wallpaper-settings.exe")));
    match exe {
        Some(p) if p.exists() => {
            if let Err(e) = std::process::Command::new(&p).spawn() {
                log(&format!("啟動設定視窗失敗：{e}"));
            }
        }
        _ => log("找不到 nijikage-wallpaper-settings.exe（應與本程式放在同一資料夾）"),
    }
}

struct App {
    windowed: bool,
    attach_mode: win32::AttachMode,
    gdi_test: bool,
    gdi_bitmap: Option<win32::LayeredBitmap>,
    shown: bool,
    cfg: Config,
    cfg_mtime: Option<SystemTime>,
    window: Option<Arc<Window>>,
    gfx: Option<gfx::Gfx>,
    tray: Option<TrayState>,
    start: Instant,
    last_frame: Instant,
    last_cfg_check: Instant,
    last_fs_check: Instant,
    fullscreen_paused: bool,
    applied_autostart: Option<bool>,
    cfg_error_logged: bool,
    frames: u64,
    audio: Option<audio::AudioCapture>,
    next_frame: Instant,
    hwnd: Option<isize>,
    last_primary: (u32, u32),
}

fn mtime() -> Option<SystemTime> {
    std::fs::metadata(config_path()).and_then(|m| m.modified()).ok()
}

impl App {
    fn new(windowed: bool) -> Self {
        let now = Instant::now();
        let cfg = Config::load();
        set_language(cfg.language);
        Self {
            attach_mode: win32::AttachMode::from_env(),
            gdi_test: !windowed && std::env::var_os("NIJIKAGE_WALLPAPER_GDI_TEST").is_some(),
            gdi_bitmap: None, shown: false,
            windowed, cfg, cfg_mtime: mtime(), window: None, gfx: None, tray: None,
            start: now, last_frame: now, last_cfg_check: now, last_fs_check: now,
            fullscreen_paused: false, applied_autostart: None, audio: None,
            cfg_error_logged: false, frames: 0, next_frame: now, hwnd: None, last_primary: win32::primary_size(),
        }
    }

    fn active(&self) -> bool {
        self.cfg.enabled && !self.fullscreen_paused
    }

    fn sync_system(&mut self) {
        if self.applied_autostart != Some(self.cfg.system.autostart) {
            if let Ok(exe) = std::env::current_exe() {
                match win32::set_autostart(self.cfg.system.autostart, &exe) {
                    Ok(()) => self.applied_autostart = Some(self.cfg.system.autostart),
                    Err(e) => log(&e),
                }
            }
        }
        // 音訊動態環：只有設定開啟時才啟動系統聲音擷取（僅分析音量與頻帶，不錄音）
        let want_audio = self.cfg.audio_ring.enabled && self.cfg.enabled;
        match (want_audio, self.audio.is_some()) {
            (true, false) => self.audio = Some(audio::AudioCapture::start()),
            (false, true) => self.audio = None,
            _ => {}
        }
        match (self.cfg.system.tray_icon, self.tray.is_some()) {
            (true, false) => self.tray = build_tray(self.cfg.enabled),
            (false, true) => self.tray = None,
            _ => {}
        }
        // 只更新選單文字，不重建托盤圖示（避免閃爍、選單被關掉）；語言切換時文字也會跟著換
        set_language(self.cfg.language);
        if let Some(t) = self.tray.as_ref() {
            t.settings_item.set_text(tr("設定…"));
            t.quit_item.set_text(tr("結束 Nijikage-Wallpaper"));
            t.toggle_item.set_text(if self.cfg.enabled { tr("暫停桌布") } else { tr("繼續桌布") });
            let _ = t.icon.set_tooltip(Some(tr("Nijikage-Wallpaper 虹影")));
        }
    }

    fn frame_interval(&self) -> Duration {
        Duration::from_secs_f64(1.0 / self.cfg.performance.fps_cap.clamp(5, 120) as f64)
    }

    fn reload_config_if_changed(&mut self) {
        let m = mtime();
        if m != self.cfg_mtime {
            // 讀取失敗（檔案被鎖住或格式壞掉）時保留舊設定，不記下修改時間，下一輪再試
            match Config::try_load() {
                Ok(c) => {
                    self.cfg_mtime = m;
                    self.cfg = c;
                    self.cfg_error_logged = false;
                    if let Some(g) = self.gfx.as_mut() {
                        g.apply_config(&self.cfg);
                    }
                    self.sync_system();
                }
                Err(e) => {
                    if !self.cfg_error_logged {
                        self.cfg_error_logged = true;
                        log(&format!("{e}（保留目前設定，稍後重試）"));
                    }
                }
            }
        }
    }

    /// 檔案總管重啟或解析度改變後的善後
    fn check_desktop(&mut self, el: &ActiveEventLoop) {
        if self.windowed {
            return;
        }
        let Some(hwnd) = self.hwnd else { return };
        if !win32::window_alive(hwnd) || !win32::is_attached(hwnd) {
            // Progman 被銷毀（explorer 重啟）：重新啟動自己再掛一次
            log("桌面視窗已消失（可能是檔案總管重啟），重新啟動 Nijikage-Wallpaper");
            if let Ok(exe) = std::env::current_exe() {
                let _ = std::process::Command::new(exe).arg("--restarted").spawn();
            }
            el.exit();
            return;
        }
        let size = win32::primary_size();
        if size != self.last_primary {
            self.last_primary = size;
            if let Err(e) = win32::resize_attached(hwnd, size.0, size.1) {
                log(&e);
            }
        }
    }

    fn handle_tray(&mut self, el: &ActiveEventLoop) {
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            let Some(t) = self.tray.as_ref() else { continue };
            if ev.id == t.quit_id {
                el.exit();
            } else if ev.id == t.settings_id {
                open_settings();
            } else if ev.id == t.toggle_id {
                // 讀、改、寫：只動 enabled 一個欄位，避免蓋掉設定視窗剛存的內容
                let mut fresh = Config::try_load().unwrap_or_else(|_| self.cfg.clone());
                fresh.enabled = !fresh.enabled;
                if let Err(e) = fresh.save() {
                    log(&format!("儲存設定失敗：{e}"));
                }
                self.cfg = fresh;
                self.cfg_mtime = mtime();
                if let Some(g) = self.gfx.as_mut() {
                    g.apply_config(&self.cfg);
                }
                self.sync_system();
            }
        }
    }

    fn cursor_in_window(&self) -> Option<[f32; 2]> {
        let (x, y) = win32::cursor_pos()?;
        let (w, h) = self.window.as_ref().map(|w| { let s = w.inner_size(); (s.width as i32, s.height as i32) })?;
        if self.windowed {
            let p = self.window.as_ref()?.inner_position().ok()?;
            let (lx, ly) = (x - p.x, y - p.y);
            return (lx >= 0 && ly >= 0 && lx < w && ly < h).then_some([lx as f32, ly as f32]);
        }
        (x >= 0 && y >= 0 && x < w && y < h).then_some([x as f32, y as f32])
    }

    fn draw_frame(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.25);
        self.last_frame = now;
        let size = self.window.as_ref().map(|w| { let s = w.inner_size(); [s.width, s.height] }).unwrap_or([1, 1]);
        let input = FrameInput { time: now.duration_since(self.start).as_secs_f32(), dt, cursor: self.cursor_in_window(), size };
        if let Some(g) = self.gfx.as_mut() {
            let a = self.audio.as_ref().map(|c| c.latest()).unwrap_or_default();
            g.set_audio(&a);
        }
        let success = if self.gdi_test {
            if let Some(bitmap) = self.gdi_bitmap.as_mut() {
                match bitmap.red_test() {
                    Ok(()) => true,
                    Err(e) => { log_once(&e); false }
                }
            } else if let Some(hwnd) = self.hwnd {
                // GetDC 診斷必須先可見；此模式完全不建立 GPU/swapchain，避免互相覆蓋。
                if !self.shown {
                    win32::show_attached(hwnd, self.attach_mode);
                    self.shown = true;
                }
                win32::debug_gdi_fill(hwnd, size[0] as i32, size[1] as i32);
                true
            } else { false }
        } else {
            self.gfx.as_mut().is_some_and(|g| g.draw(&input))
        };
        if success {
            if !self.shown {
                if let Some(hwnd) = self.hwnd { win32::show_attached(hwnd, self.attach_mode); }
                self.shown = true;
            }
            self.frames += 1;
            if self.frames == 1 || self.frames % 600 == 0 {
                log(&format!("已繪製 {} 幀 (gdi_test={})", self.frames, self.gdi_test));
                if let Some(h) = self.hwnd { log(&format!("視窗狀態：{}", win32::debug_state(h))); }
                if self.frames == 1 { log(&win32::debug_tree()); }
            }
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let (pw, ph) = win32::primary_size();
        let mut attrs = Window::default_attributes().with_title("Nijikage-Wallpaper 虹影");
        if self.windowed {
            attrs = attrs.with_inner_size(PhysicalSize::new(1280, 720));
        } else {
            attrs = attrs.with_decorations(false).with_active(false).with_visible(false).with_inner_size(PhysicalSize::new(pw, ph));
        }
        let window = match el.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                log(&format!("建立視窗失敗：{e}"));
                el.exit();
                return;
            }
        };
        log(&format!("Startup: windowed={} attach_mode={:?} gdi_test={} WGPU_BACKEND={:?}; embedded Win8/8.1/10+ manifest",
            self.windowed, self.attach_mode, self.gdi_test, std::env::var("WGPU_BACKEND").ok()));
        if !self.windowed {
            let hwnd = match window.window_handle().map(|h| h.as_raw()) {
                Ok(RawWindowHandle::Win32(h)) => h.hwnd.get(),
                other => { log(&format!("Cannot obtain wallpaper HWND: {other:?}")); el.exit(); return; }
            };
            if self.attach_mode == win32::AttachMode::Warmup && !self.gdi_test {
                let warmup = (|| -> Result<gfx::Gfx, String> {
                    win32::prepare_warmup(hwnd, pw, ph)?;
                    let mut g = gfx::Gfx::new(window.clone(), &self.cfg, None)?;
                    if !g.draw(&FrameInput { time: 0.0, dt: 0.0, cursor: None, size: [pw, ph] }) {
                        return Err("Warmup first present failed".into());
                    }
                    log("Mode 2: top-level offscreen first present completed");
                    Ok(g)
                })();
                match warmup {
                    Ok(g) => self.gfx = Some(g),
                    Err(e) => { log(&e); el.exit(); return; }
                }
            }
            if let Err(e) = win32::attach_to_desktop(hwnd, self.attach_mode) {
                log(&format!("掛到桌面失敗：{e}")); el.exit(); return;
            }
            self.hwnd = Some(hwnd);
            if self.gdi_test && self.attach_mode == win32::AttachMode::Bitmap {
                match win32::LayeredBitmap::new(hwnd, pw, ph) {
                    Ok(bitmap) => self.gdi_bitmap = Some(bitmap),
                    Err(e) => { log(&e); el.exit(); return; }
                }
            }
        }
        if !self.gdi_test {
            if let Some(g) = self.gfx.as_mut() {
                // SetParent 可能改變 DPI/大小，重設已建立的 swapchain。
                let s = window.inner_size();
                g.resize(s.width, s.height);
            } else {
                let bitmap_hwnd = if self.attach_mode == win32::AttachMode::Bitmap { self.hwnd } else { None };
                match gfx::Gfx::new(window.clone(), &self.cfg, bitmap_hwnd) {
                    Ok(g) => self.gfx = Some(g),
                    Err(e) => { log(&e); el.exit(); return; }
                }
            }
        }
        self.window = Some(window);
        // 隱藏視窗可能收不到 WM_PAINT：同步送出首幀，成功後才顯示，不能等 RedrawRequested。
        self.draw_frame();
        self.sync_system();
        el.set_control_flow(ControlFlow::WaitUntil(Instant::now() + self.frame_interval()));
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(s) => {
                if let Some(g) = self.gfx.as_mut() {
                    g.resize(s.width, s.height);
                }
                if self.gdi_bitmap.is_some() && s.width > 0 && s.height > 0 {
                    if let Some(hwnd) = self.hwnd {
                        match win32::LayeredBitmap::new(hwnd, s.width, s.height) {
                            Ok(bitmap) => self.gdi_bitmap = Some(bitmap),
                            Err(e) => log(&e),
                        }
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                if !self.active() {
                    return;
                }
                self.draw_frame();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        self.handle_tray(el);
        if std::env::var_os("NIJIKAGE_WALLPAPER_ZTOP").is_some() {
            if let Some(h) = self.hwnd { win32::debug_raise(h); }
        }
        let now = Instant::now();
        if now.duration_since(self.last_cfg_check) >= Duration::from_millis(500) {
            self.last_cfg_check = now;
            self.reload_config_if_changed();
        }
        if now.duration_since(self.last_fs_check) >= Duration::from_secs(1) {
            self.last_fs_check = now;
            self.fullscreen_paused = self.cfg.performance.pause_on_fullscreen && win32::fullscreen_app_running();
            self.check_desktop(el);
        }
        if self.hwnd.is_some() && !self.shown && now >= self.next_frame {
            // 若首次 acquire/present 失敗，隱藏期間仍主動重試，避免等待不會來的 WM_PAINT。
            self.draw_frame();
            self.next_frame = now + self.frame_interval();
        }
        if self.active() {
            // 只在到達下一幀時間時才要求重繪，避免無上限狂畫
            if now >= self.next_frame {
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
                self.next_frame = now + self.frame_interval();
            }
            el.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
        } else {
            // 暫停時只做低頻檢查，不繪製
            el.set_control_flow(ControlFlow::WaitUntil(now + Duration::from_millis(500)));
        }
    }
}

fn main() {
    // 解除安裝時由安裝程式呼叫：移除「開機自啟動」登錄值後立即結束（不建立視窗、不影響設定檔）
    if std::env::args().any(|a| a == "--cleanup") {
        if let Ok(exe) = std::env::current_exe() {
            let _ = win32::set_autostart(false, &exe);
        }
        return;
    }
    let windowed = std::env::args().any(|a| a == "--windowed");
    let restarted = std::env::args().any(|a| a == "--restarted");
    if restarted {
        // 檔案總管重啟後自動重新啟動：等桌面重建完成、舊程序釋放互斥鎖
        std::thread::sleep(Duration::from_secs(3));
    }
    if !windowed {
        let mut ok = win32::acquire_single_instance();
        if !ok && restarted {
            for _ in 0..10 {
                std::thread::sleep(Duration::from_secs(1));
                if win32::acquire_single_instance() {
                    ok = true;
                    break;
                }
            }
        }
        if !ok {
            return; // 已有另一個 Nijikage-Wallpaper 在執行
        }
    }
    let el = match EventLoop::new() {
        Ok(e) => e,
        Err(e) => {
            log(&format!("建立事件迴圈失敗：{e}"));
            return;
        }
    };
    let mut app = App::new(windowed);
    if let Err(e) = el.run_app(&mut app) {
        log(&format!("事件迴圈結束：{e}"));
    }
}
