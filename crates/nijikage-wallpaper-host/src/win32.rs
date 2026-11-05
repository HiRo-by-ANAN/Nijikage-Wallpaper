//! Win32 相關：掛到桌面圖示後方、游標位置、全螢幕偵測、開機自啟動、單一執行個體。
//! 此檔是整個專案唯一需要 unsafe 的地方（呼叫 Windows API）。

use std::ffi::c_void;
use std::path::Path;
use windows::Win32::Foundation::{COLORREF, CloseHandle, GetLastError, SetLastError, ERROR_ALREADY_EXISTS, WIN32_ERROR, HWND, LPARAM, POINT, WPARAM};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ,
};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Shell::{
    SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, FindWindowW, GetCursorPos, GetSystemMetrics, GetWindowLongPtrW, SendMessageTimeoutW,
    SetParent, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, GWL_STYLE, HWND_BOTTOM, LWA_ALPHA, SMTO_NORMAL, SM_CXSCREEN, SM_CYSCREEN,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SetLayeredWindowAttributes, WS_CHILD, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    SWP_FRAMECHANGED, ShowWindow, SW_HIDE, SW_SHOWNOACTIVATE, WS_CLIPCHILDREN, WS_CLIPSIBLINGS,
    WINDOW_LONG_PTR_INDEX,
};
use windows::core::{w, BOOL, PCWSTR};

fn hwnd_from_isize(h: isize) -> HWND {
    HWND(h as *mut c_void)
}

/// 主螢幕大小（像素）
pub fn primary_size() -> (u32, u32) {
    unsafe { (GetSystemMetrics(SM_CXSCREEN).max(1) as u32, GetSystemMetrics(SM_CYSCREEN).max(1) as u32) }
}

/// 游標的螢幕座標
pub fn cursor_pos() -> Option<(i32, i32)> {
    let mut p = POINT::default();
    unsafe { GetCursorPos(&mut p).ok().map(|_| (p.x, p.y)) }
}

/// 全螢幕程式（遊戲、影片、簡報）是否正在執行
pub fn fullscreen_app_running() -> bool {
    unsafe {
        match SHQueryUserNotificationState() {
            Ok(s) => s == QUNS_BUSY || s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_PRESENTATION_MODE,
            Err(_) => false,
        }
    }
}

struct EnumCtx {
    /// 含有 SHELLDLL_DefView 的 WorkerW（舊版 Windows 結構）
    workerw_with_view: Option<HWND>,
}

unsafe extern "system" fn enum_proc(top: HWND, lparam: LPARAM) -> BOOL {
    let ctx = unsafe { &mut *(lparam.0 as *mut EnumCtx) };
    let has_view = unsafe { FindWindowExW(Some(top), None, w!("SHELLDLL_DefView"), PCWSTR::null()) };
    if has_view.is_ok() {
        ctx.workerw_with_view = Some(top);
        return BOOL(0);
    }
    BOOL(1)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachMode {
    // 0: 舊掛載順序 + 新 manifest，作為只修正相容性宣告的對照組。
    Baseline,
    // 1 (預設): 隱藏掛載、FRAMECHANGED、掛載後重設 layered/SLA，首幀完成才顯示。
    Rearm,
    // 2: 先在螢幕外的頂層視窗 present 一幀，再掛載並重新 configure 表面。
    Warmup,
    // 3: 完全繞過 HWND swapchain，GPU 離屏繪製讀回 BGRA，再 UpdateLayeredWindow。
    Bitmap,
    // 4: 方案 1 + 首幀後短暫 hide/show DefView，測試 ShellView 不透明快照殘留。
    RefreshShell,
}

impl AttachMode {
    pub fn from_env() -> Self {
        match std::env::var("NIJIKAGE_WALLPAPER_ATTACH_MODE").as_deref() {
            Ok("0") => Self::Baseline,
            Ok("1") | Err(_) => Self::Rearm,
            Ok("2") => Self::Warmup,
            Ok("3") => Self::Bitmap,
            Ok("4") => Self::RefreshShell,
            Ok(v) => {
                crate::log(&format!("Unknown NIJIKAGE_WALLPAPER_ATTACH_MODE={v:?}; using 1 (Rearm)"));
                Self::Rearm
            }
        }
    }
}

fn set_style(hwnd: HWND, index: WINDOW_LONG_PTR_INDEX, value: isize) -> Result<(), String> {
    unsafe {
        SetLastError(WIN32_ERROR(0));
        let old = SetWindowLongPtrW(hwnd, index, value);
        let error = GetLastError();
        if old == 0 && error.0 != 0 {
            return Err(format!("SetWindowLongPtrW({index:?}, 0x{value:X}): {error:?}"));
        }
        let actual = GetWindowLongPtrW(hwnd, index);
        if actual != value {
            crate::log(&format!("Style normalized by Windows ({index:?}): requested=0x{value:X}, actual=0x{actual:X}"));
            // Windows 可替仍為頂層的 HWND 自動補 WS_CLIPSIBLINGS（舊版實測也有此旗標）。
            let mask = if index == GWL_STYLE { !(WS_CLIPSIBLINGS.0 as isize) } else { -1 };
            if (actual & mask) != (value & mask) {
                return Err(format!("Style readback mismatch ({index:?}): 0x{actual:X}"));
            }
        }
    }
    Ok(())
}

/// SetParent 的 NULL 回傳也可能代表成功（原本無父視窗），不能只用 Result 判斷。
fn reparent(child: HWND, parent: HWND) -> Result<(), String> {
    unsafe {
        SetLastError(WIN32_ERROR(0));
        let result = SetParent(child, Some(parent));
        let error = GetLastError();
        let actual = windows::Win32::UI::WindowsAndMessaging::GetParent(child).ok();
        crate::log(&format!("SetParent result={result:?} last_error={error:?} actual={actual:?} expected={parent:?}"));
        if actual != Some(parent) {
            return Err(format!("SetParent failed: {error:?}; actual={actual:?}"));
        }
    }
    Ok(())
}

/// 隱藏、配置、掛載，再依方案建立新的分層重導狀態。一般視窗模式不會呼叫。
pub fn attach_to_desktop(child_hwnd: isize, mode: AttachMode) -> Result<(), String> {
    unsafe {
        let child = hwnd_from_isize(child_hwnd);
        let progman = FindWindowW(w!("Progman"), PCWSTR::null()).map_err(|e| format!("找不到 Progman：{e}"))?;
        // 要求 Progman 產生 WorkerW 圖層
        let mut res = 0usize;
        SetLastError(WIN32_ERROR(0));
        let sent = SendMessageTimeoutW(progman, 0x052C, WPARAM(0xD), LPARAM(1), SMTO_NORMAL, 1000, Some(&mut res as *mut usize));
        crate::log(&format!("Attach mode={mode:?}; Progman 0x052C return={} result={res} last_error={:?}", sent.0, GetLastError()));
        crate::log(&format!("Before attach: {}", debug_state(child_hwnd)));

        let (w, h) = primary_size();
        let baseline = mode == AttachMode::Baseline;
        let _ = ShowWindow(child, SW_HIDE);
        let style = if baseline {
            WS_CHILD.0 | windows::Win32::UI::WindowsAndMessaging::WS_VISIBLE.0
        } else {
            WS_CHILD.0 | WS_CLIPCHILDREN.0 | WS_CLIPSIBLINGS.0
        };
        set_style(child, GWL_STYLE, style as isize)?;
        // 清除 APPWINDOW/NOREDIRECTIONBITMAP/TRANSPARENT，避免繼承不合適的合成路徑。
        set_style(child, GWL_EXSTYLE, (WS_EX_LAYERED.0 | WS_EX_NOACTIVATE.0) as isize)?;
        if baseline {
            SetLayeredWindowAttributes(child, COLORREF(0), 255, LWA_ALPHA)
                .map_err(|e| format!("SetLayeredWindowAttributes before parent: {e}"))?;
        }

        let view = FindWindowExW(Some(progman), None, w!("SHELLDLL_DefView"), PCWSTR::null()).ok();
        let (parent, insert_after) = if let Some(view) = view {
            (progman, Some(view))
        } else {
            let mut ctx = EnumCtx { workerw_with_view: None };
            let _ = EnumWindows(Some(enum_proc), LPARAM(&mut ctx as *mut EnumCtx as isize));
            let holder = ctx.workerw_with_view.ok_or("找不到桌面圖示視窗")?;
            let target = FindWindowExW(None, Some(holder), w!("WorkerW"), PCWSTR::null())
                .map_err(|e| format!("找不到桌布層 WorkerW：{e}"))?;
            (target, None)
        };
        reparent(child, parent)?;
        if !baseline {
            // 在最終父視窗下重新建立分層狀態，避免保留頂層視窗時的 redirection。
            set_style(child, GWL_EXSTYLE, WS_EX_NOACTIVATE.0 as isize)?;
            set_style(child, GWL_EXSTYLE, (WS_EX_LAYERED.0 | WS_EX_NOACTIVATE.0) as isize)?;
            if mode != AttachMode::Bitmap {
                SetLayeredWindowAttributes(child, COLORREF(0), 255, LWA_ALPHA)
                    .map_err(|e| format!("SetLayeredWindowAttributes after parent: {e}"))?;
            }
            // Bitmap 模式絕不能呼叫 SLA，否則後續 UpdateLayeredWindow 會失敗。
        }
        let flags = SWP_NOACTIVATE | if baseline { SWP_SHOWWINDOW } else { SWP_FRAMECHANGED };
        SetWindowPos(child, insert_after, 0, 0, w as i32, h as i32, flags)
            .map_err(|e| format!("SetWindowPos 失敗：{e}"))?;
        if view.is_some() {
            if let Ok(wk) = FindWindowExW(Some(progman), None, w!("WorkerW"), PCWSTR::null()) {
                SetWindowPos(wk, Some(HWND_BOTTOM), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE)
                    .map_err(|e| format!("WorkerW z-order: {e}"))?;
            }
        }
        crate::log(&format!("After attach (before first frame): {}", debug_state(child_hwnd)));
        Ok(())
    }
}

pub fn show_attached(hwnd: isize, mode: AttachMode) {
    unsafe {
        let _ = ShowWindow(hwnd_from_isize(hwnd), SW_SHOWNOACTIVATE);
        if mode == AttachMode::RefreshShell {
            // 僅在明確選擇模式 4 時執行一次；不更改 Explorer 的任何 style。
            if let Ok(progman) = FindWindowW(w!("Progman"), PCWSTR::null()) {
                if let Ok(view) = FindWindowExW(Some(progman), None, w!("SHELLDLL_DefView"), PCWSTR::null()) {
                    if windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(view).as_bool() {
                        let _ = ShowWindow(view, SW_HIDE);
                        let _ = ShowWindow(view, SW_SHOWNOACTIVATE);
                        crate::log("Mode 4: DefView hide/show refresh completed");
                    }
                }
            }
        }
    }
    crate::log(&format!("First frame shown: {}", debug_state(hwnd)));
    crate::log(&debug_tree());
}

/// 方案 2 僅在建立 surface 前使用；不讓測試用頂層視窗閃到桌面上。
pub fn prepare_warmup(hwnd: isize, w: u32, h: u32) -> Result<(), String> {
    unsafe {
        SetWindowPos(hwnd_from_isize(hwnd), None, -(w as i32) - 64, -(h as i32) - 64,
            w as i32, h as i32, SWP_NOACTIVATE | SWP_SHOWWINDOW)
            .map_err(|e| format!("Warmup position: {e}"))
    }
}

/// 持有 top-down BGRA DIB；供離屏 GPU 讀回與純 GDI 的 ULW 診斷共用。
pub struct LayeredBitmap {
    hwnd: HWND,
    dc: windows::Win32::Graphics::Gdi::HDC,
    bitmap: windows::Win32::Graphics::Gdi::HBITMAP,
    old: windows::Win32::Graphics::Gdi::HGDIOBJ,
    bits: *mut u8,
    width: u32,
    height: u32,
}

impl LayeredBitmap {
    pub fn new(hwnd: isize, width: u32, height: u32) -> Result<Self, String> {
        use windows::Win32::Graphics::Gdi::*;
        unsafe {
            let dc = CreateCompatibleDC(None);
            if dc.0.is_null() { return Err("CreateCompatibleDC failed".into()); }
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width as i32, biHeight: -(height as i32),
                    biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let bitmap = match CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
                Ok(bitmap) => bitmap,
                Err(e) => { let _ = DeleteDC(dc); return Err(format!("CreateDIBSection: {e}")); }
            };
            let old = SelectObject(dc, bitmap.into());
            if old.0.is_null() || old.0 as isize == -1 {
                let _ = DeleteObject(bitmap.into());
                let _ = DeleteDC(dc);
                return Err("SelectObject(DIB) failed".into());
            }
            Ok(Self { hwnd: hwnd_from_isize(hwnd), dc, bitmap, old, bits: bits.cast(), width, height })
        }
    }

    pub fn present(&mut self, bgra: &[u8], stride: usize) -> Result<(), String> {
        let row_bytes = self.width as usize * 4;
        if stride < row_bytes || bgra.len() < stride * self.height as usize {
            return Err("Invalid BGRA readback dimensions".into());
        }
        unsafe {
            for y in 0..self.height as usize {
                std::ptr::copy_nonoverlapping(bgra.as_ptr().add(y * stride), self.bits.add(y * row_bytes), row_bytes);
            }
        }
        self.update()
    }

    pub fn red_test(&mut self) -> Result<(), String> {
        use windows::Win32::Foundation::RECT;
        use windows::Win32::Graphics::Gdi::{CreateSolidBrush, DeleteObject, FillRect};
        unsafe {
            let brush = CreateSolidBrush(COLORREF(0x0000_00FF));
            let r = RECT { left: 0, top: 0, right: self.width as i32, bottom: self.height as i32 };
            let result = FillRect(self.dc, &r, brush);
            let _ = DeleteObject(brush.into());
            if result == 0 { return Err("FillRect(DIB) failed".into()); }
        }
        self.update()
    }

    fn update(&self) -> Result<(), String> {
        use windows::Win32::Foundation::SIZE;
        use windows::Win32::UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_OPAQUE};
        let size = SIZE { cx: self.width as i32, cy: self.height as i32 };
        let src = POINT::default();
        // ULW_OPAQUE 不依賴渲染器/GDI 寫出的 alpha；整個桌布矩形都是不透明。
        // pptDst=NULL 保留 SetWindowPos 設定的子視窗位置，避免混用螢幕與父視窗座標。
        unsafe { UpdateLayeredWindow(self.hwnd, None, None, Some(&size), Some(self.dc), Some(&src),
            COLORREF(0), None, ULW_OPAQUE).map_err(|e| format!("UpdateLayeredWindow: {e}")) }
    }
}

impl Drop for LayeredBitmap {
    fn drop(&mut self) {
        use windows::Win32::Graphics::Gdi::{DeleteDC, DeleteObject, SelectObject};
        unsafe {
            let _ = SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN_VALUE: &str = "Nijikage-Wallpaper";

/// 開機自啟動：寫入／移除 HKCU\...\Run。
pub fn set_autostart(enable: bool, exe: &Path) -> Result<(), String> {
    unsafe {
        let key_name = to_wide(RUN_KEY);
        let mut key = HKEY::default();
        let rc = RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(key_name.as_ptr()), None, KEY_SET_VALUE, &mut key);
        if rc.is_err() {
            return Err(format!("開啟登錄機碼失敗：{rc:?}"));
        }
        let name = to_wide(RUN_VALUE);
        let result = if enable {
            let cmd = to_wide(&format!("\"{}\"", exe.display()));
            let bytes: &[u8] = std::slice::from_raw_parts(cmd.as_ptr() as *const u8, cmd.len() * 2);
            let rc = RegSetValueExW(key, PCWSTR(name.as_ptr()), None, REG_SZ, Some(bytes));
            if rc.is_err() { Err(format!("寫入自啟動失敗：{rc:?}")) } else { Ok(()) }
        } else {
            let rc = RegDeleteValueW(key, PCWSTR(name.as_ptr()));
            // ERROR_FILE_NOT_FOUND：本來就沒有這個值，視為成功；其他錯誤要回報，不可吞掉
            if rc.is_err() && rc != windows::Win32::Foundation::ERROR_FILE_NOT_FOUND {
                Err(format!("移除自啟動失敗：{rc:?}"))
            } else {
                Ok(())
            }
        };
        let _ = RegCloseKey(key);
        result
    }
}

/// 單一執行個體：已有另一個在跑回傳 false。
pub fn acquire_single_instance() -> bool {
    unsafe {
        let name = to_wide("Local\\Nijikage.Wallpaper.SingleInstance");
        match CreateMutexW(None, true, PCWSTR(name.as_ptr())) {
            Ok(h) => {
                let already = GetLastError() == ERROR_ALREADY_EXISTS;
                if already {
                    let _ = CloseHandle(h);
                    false
                } else {
                    // 刻意不關閉：程式結束時由系統釋放
                    let _ = h;
                    true
                }
            }
            Err(_) => true,
        }
    }
}

/// 視窗是否仍然存在
pub fn window_alive(hwnd: isize) -> bool {
    unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(hwnd_from_isize(hwnd))).as_bool() }
}

/// 是否仍掛在別的視窗底下（Progman／WorkerW）
pub fn is_attached(hwnd: isize) -> bool {
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::GetParent(hwnd_from_isize(hwnd))
            .map(|p| !p.0.is_null())
            .unwrap_or(false)
    }
}

/// 解析度改變後調整已掛載視窗的大小（不動 z 順序）
pub fn resize_attached(hwnd: isize, w: u32, h: u32) -> Result<(), String> {
    unsafe {
        SetWindowPos(
            hwnd_from_isize(hwnd),
            None,
            0,
            0,
            w as i32,
            h as i32,
            windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER | SWP_NOACTIVATE,
        )
        .map_err(|e| format!("調整桌布大小失敗：{e}"))
    }
}

/// 除錯：回報視窗掛載後的狀態（是否被 DWM 隱藏 cloaked、可見性、矩形、父視窗類別）
pub fn debug_state(hwnd: isize) -> String {
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
    use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetClassLongPtrW, GCL_STYLE, GCLP_HBRBACKGROUND,
        GetLayeredWindowAttributes, GetParent, GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindowVisible, LAYERED_WINDOW_ATTRIBUTES_FLAGS};
    use windows::Win32::UI::HiDpi::{GetWindowDpiAwarenessContext, GetAwarenessFromDpiAwarenessContext, GetDpiForWindow};
    unsafe {
        let h = hwnd_from_isize(hwnd);
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut c_void, 4);
        let mut r = windows::Win32::Foundation::RECT::default();
        let _ = GetWindowRect(h, &mut r);
        let parent = GetParent(h).ok();
        let mut buf = [0u16; 64];
        let pcls = match parent {
            Some(p) => {
                let n = GetClassNameW(p, &mut buf) as usize;
                String::from_utf16_lossy(&buf[..n.min(64)])
            }
            None => "(無)".into(),
        };
        let mut pid = 0;
        let tid = GetWindowThreadProcessId(h, Some(&mut pid));
        let mut alpha = 0;
        let mut flags = LAYERED_WINDOW_ATTRIBUTES_FLAGS(0);
        let layered = GetLayeredWindowAttributes(h, None, Some(&mut alpha), Some(&mut flags));
        let parent_dpi = parent.map(|p| (GetDpiForWindow(p), GetAwarenessFromDpiAwarenessContext(GetWindowDpiAwarenessContext(p))));
        format!(
            "hwnd={h:?} pid={pid} tid={tid} style=0x{:X} ex=0x{:X} class_style=0x{:X} class_brush=0x{:X} dpi={} awareness={:?} SLA={layered:?}/alpha={alpha}/flags={flags:?} cloaked={cloaked} visible={} iconic={} rect=({},{},{},{}) parent={pcls} parent_hwnd={parent:?} parent_dpi_awareness={parent_dpi:?}",
            GetWindowLongPtrW(h, GWL_STYLE), GetWindowLongPtrW(h, GWL_EXSTYLE),
            GetClassLongPtrW(h, GCL_STYLE), GetClassLongPtrW(h, GCLP_HBRBACKGROUND),
            GetDpiForWindow(h), GetAwarenessFromDpiAwarenessContext(GetWindowDpiAwarenessContext(h)),
            IsWindowVisible(h).as_bool(),
            IsIconic(h).as_bool(),
            r.left, r.top, r.right, r.bottom
        )
    }
}

/// 除錯：用 GDI 在視窗上畫一塊紅色（判斷傳統繪製能否顯示在桌面層）
pub fn debug_gdi_fill(hwnd: isize, w: i32, h: i32) {
    use windows::Win32::Foundation::{COLORREF, RECT};
    use windows::Win32::Graphics::Gdi::{CreateSolidBrush, DeleteObject, FillRect, GetDC, ReleaseDC};
    unsafe {
        let hw = hwnd_from_isize(hwnd);
        let dc = GetDC(Some(hw));
        if dc.0.is_null() {
            crate::log_once("GDI 測試：GetDC 回傳空值");
            return;
        }
        let brush = CreateSolidBrush(COLORREF(0x0000_00FF)); // BGR：紅
        let r = RECT { left: w / 2, top: 0, right: w, bottom: h };
        let n = FillRect(dc, &r, brush);
        crate::log_once(&format!("GDI 測試：FillRect 回傳 {n}"));
        let _ = DeleteObject(brush.into());
        ReleaseDC(Some(hw), dc);
    }
}

/// 除錯：列出 Progman 與其直接子視窗的類別、樣式、擴充樣式、矩形（由上而下的 z 順序）
pub fn debug_tree() -> String {
    use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetWindowRect, IsWindowVisible, GW_CHILD, GW_HWNDNEXT, GetWindow};
    unsafe {
        let Ok(prog) = FindWindowW(w!("Progman"), PCWSTR::null()) else { return "找不到 Progman".into() };
        let describe = |h: HWND| -> String {
            let mut buf = [0u16; 64];
            let n = GetClassNameW(h, &mut buf) as usize;
            let cls = String::from_utf16_lossy(&buf[..n.min(64)]);
            let st = GetWindowLongPtrW(h, GWL_STYLE);
            let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
            let mut r = windows::Win32::Foundation::RECT::default();
            let _ = GetWindowRect(h, &mut r);
            format!("{cls} style=0x{st:X} ex=0x{ex:X} rect=({},{},{},{}) vis={}", r.left, r.top, r.right, r.bottom, IsWindowVisible(h).as_bool())
        };
        let mut out = format!("Progman: {}\n", describe(prog));
        let mut c = GetWindow(prog, GW_CHILD).ok();
        let mut i = 0;
        while let Some(h) = c {
            out.push_str(&format!("  [{i}] {}\n", describe(h)));
            c = GetWindow(h, GW_HWNDNEXT).ok();
            i += 1;
            if i > 30 { break; }
        }
        out
    }
}

/// 除錯：把視窗提到父視窗最上層
pub fn debug_raise(hwnd: isize) {
    use windows::Win32::UI::WindowsAndMessaging::HWND_TOP;
    unsafe {
        let _ = SetWindowPos(hwnd_from_isize(hwnd), Some(HWND_TOP), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}
