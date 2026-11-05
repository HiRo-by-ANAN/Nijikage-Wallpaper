//! nijikage-wallpaper-core：三個程式共用的設定格式與路徑。
//! 設定檔位置：%APPDATA%\Nijikage-Wallpaper\config.json（所有程式只讀寫這份，不另開通道）。
//! 規則：欄位只增不改名；新增欄位一律 #[serde(default)]，舊設定檔才讀得進來。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub mod i18n;
pub mod image_prep;

pub use i18n::Language;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiscStyle {
    /// CD：銀白，彩虹繞射
    Cd,
    /// DVD：金紫
    Dvd,
    /// 藍光：藍紫（預設，接近參考圖）
    Bd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackgroundMode {
    Image,
    Color,
}

/// 與 Windows 個人化「選擇符合項目」相同的排列方式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FitMode {
    Fill,
    Fit,
    Stretch,
    Tile,
    Center,
    Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Background {
    pub mode: BackgroundMode,
    pub image_path: Option<PathBuf>,
    /// 純色 RGB
    pub color: [u8; 3],
    pub fit: FitMode,
}

/// 光碟版面模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayoutMode {
    /// 【已停用】舊的滑桿模式。讀到舊設定時一律視同 `Free`；新程式不再提供此模式，v0.3 完成後會移除此成員。
    Simple,
    /// 光碟模式（原「自由模式」）：光碟圓心、直徑任意；圖片可移動、縮放、旋轉
    Free,
    /// 全桌面模式：不顯示光碟，把光碟的光學效果（折射、反光、彩虹繞射、光線互動、粒子特效）套用到整個桌面背景
    Desktop,
}

/// 自由模式的版面。座標都是「相對值」，與解析度無關。
/// 露出多少由圓心位置與直徑自然決定，不另設露出比例。
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct FreeLayout {
    /// 光碟圓心 X：0=畫面左緣、1=畫面右緣；可超出（-0.5～1.5）
    pub center_x: f32,
    /// 光碟圓心 Y：0=畫面上緣、1=畫面下緣；可超出（-0.5～1.5）
    pub center_y: f32,
    /// 光碟直徑，占畫面高度的比例（0.2～3.0）
    pub diameter: f32,
    /// 圖片位移，單位為資料面直徑的比例（-1～1），(0,0)=置中
    pub art_offset_x: f32,
    pub art_offset_y: f32,
    /// 圖片縮放，1＝圖片「短邊」等於光碟直徑（cover，且不預先裁切圖片），0.2～4；縮小時被「超出光碟」而看不見的部分會回來顯示
    pub art_zoom: f32,
    /// 圖片旋轉角度（度，-180～180）。這是圖片自己的角度，不是光碟旋轉；圖片不隨光碟轉動
    pub art_rotation_deg: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Disc {
    /// 版面模式；兩種模式的數值各自保留，切換不會互相覆蓋
    pub layout_mode: LayoutMode,
    pub free: FreeLayout,
    /// 光碟面圖片（原圖路徑；裁切與色調由程式處理，圖片本身不旋轉）
    pub art_path: Option<PathBuf>,
    pub style: DiscStyle,
    /// 旋轉開關與速度（每分鐘轉數，慢速預設 2）
    pub rotate: bool,
    pub rpm: f32,
    /// 反射光開關與強度 0–1
    pub reflection: bool,
    pub reflection_intensity: f32,
    /// 游標在光碟上的互動反射
    pub cursor_reflection: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct EffectSet {
    pub rain: bool,
    pub wind: bool,
    pub leaves: bool,
    pub sakura: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Effects {
    /// 只落在光碟上的特效
    pub on_disc: EffectSet,
    /// 整個桌面的特效
    pub on_desktop: EffectSet,
    /// 特效量 0–1
    pub intensity: f32,
    /// 游標與特效的互動（滑過時粒子被撥動、水波等）
    pub interactive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Performance {
    /// 更新率上限，預設 30（Intel UHD 750）
    pub fps_cap: u32,
    /// 全螢幕程式執行時自動暫停
    pub pause_on_fullscreen: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct System {
    /// 開機自啟動（寫入 HKCU\...\Run）
    pub autostart: bool,
    pub tray_icon: bool,
}

/// 光碟最外圈的音訊動態環（擷取系統播放的聲音，只取音量與頻帶，不錄音、不儲存、不上傳）
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioRing {
    pub enabled: bool,
    /// 靈敏度 0.2–3.0（1.0 為預設）
    pub sensitivity: f32,
    /// 環的寬度，占光碟半徑的比例 0.02–0.15
    pub width: f32,
}

/// 全桌面模式的參數
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct DesktopFx {
    /// 效果強度 0–1
    pub intensity: f32,
    /// 光學紋路的尺度 0.5–3.0（越大紋路越寬）
    pub pattern_scale: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub background: Background,
    pub disc: Disc,
    pub effects: Effects,
    pub audio_ring: AudioRing,
    pub desktop_fx: DesktopFx,
    /// 介面語言（Auto＝依系統顯示語言）
    pub language: Language,
    pub performance: Performance,
    pub system: System,
    /// 全域開關：false 時桌布停止繪製（托盤「暫停」）
    pub enabled: bool,
}

impl Default for Background {
    fn default() -> Self {
        Self { mode: BackgroundMode::Color, image_path: None, color: [16, 18, 28], fit: FitMode::Fill }
    }
}
impl Default for Disc {
    fn default() -> Self {
        Self {
            layout_mode: LayoutMode::Simple, free: FreeLayout::default(),
            art_path: None, style: DiscStyle::Bd, rotate: true, rpm: 2.0, reflection: true,
            reflection_intensity: 0.8, cursor_reflection: true,
        }
    }
}
impl Default for FreeLayout {
    fn default() -> Self {
        // 預設等同簡易模式的預設畫面（1920×1080：靠右、露出約 65%）
        Self {
            center_x: 0.92, center_y: 0.5, diameter: 0.95,
            art_offset_x: 0.0, art_offset_y: 0.0, art_zoom: 1.0, art_rotation_deg: 0.0,
        }
    }
}
impl Default for AudioRing {
    fn default() -> Self {
        Self { enabled: true, sensitivity: 1.0, width: 0.06 }
    }
}
impl Default for DesktopFx {
    fn default() -> Self {
        Self { intensity: 0.7, pattern_scale: 1.0 }
    }
}
impl Default for EffectSet {
    fn default() -> Self {
        Self { rain: false, wind: false, leaves: false, sakura: false }
    }
}
impl Default for Effects {
    fn default() -> Self {
        Self { on_disc: EffectSet::default(), on_desktop: EffectSet::default(), intensity: 0.5, interactive: true }
    }
}
impl Default for Performance {
    fn default() -> Self {
        Self { fps_cap: 30, pause_on_fullscreen: true }
    }
}
impl Default for System {
    fn default() -> Self {
        Self { autostart: false, tray_icon: true }
    }
}
impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1, background: Background::default(), disc: Disc::default(),
            effects: Effects::default(), audio_ring: AudioRing::default(), desktop_fx: DesktopFx::default(), language: Language::Auto,
            performance: Performance::default(),
            system: System::default(), enabled: true,
        }
    }
}

/// %APPDATA%\Nijikage-Wallpaper
pub fn app_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("Nijikage-Wallpaper")
}

pub fn config_path() -> PathBuf {
    app_dir().join("config.json")
}

impl Config {
    /// 嚴格讀取：檔案不存在 → 預設值；讀取失敗或格式壞掉 → Err。
    /// 呼叫端遇到 Err 應保留舊設定並稍後重試，不要用預設值覆蓋使用者的設定。
    pub fn try_load_from(path: &Path) -> Result<Config, String> {
        match std::fs::read_to_string(path) {
            Ok(s) => serde_json::from_str::<Config>(&s)
                .map(|mut c| {
                    c.sanitize();
                    c
                })
                .map_err(|e| format!("設定檔格式錯誤：{e}")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(format!("讀取設定檔失敗：{e}")),
        }
    }
    pub fn try_load() -> Result<Config, String> {
        Self::try_load_from(&config_path())
    }
    /// 讀不到或格式壞掉時回傳預設值，不 panic。（只在不怕覆蓋的場合使用）
    pub fn load_from(path: &Path) -> Config {
        Self::try_load_from(path).unwrap_or_default()
    }
    pub fn load() -> Config {
        Self::load_from(&config_path())
    }
    /// 把手改或壞掉的數值限制在規格範圍內（NaN／無限大退回預設值）。
    pub fn sanitize(&mut self) {
        fn fix(v: &mut f32, lo: f32, hi: f32, def: f32) {
            *v = if v.is_finite() { v.clamp(lo, hi) } else { def };
        }
        fix(&mut self.disc.rpm, -60.0, 60.0, 2.0);
        fix(&mut self.disc.reflection_intensity, 0.0, 1.0, 0.8);
        fix(&mut self.effects.intensity, 0.0, 1.0, 0.5);
        fix(&mut self.audio_ring.sensitivity, 0.2, 3.0, 1.0);
        fix(&mut self.audio_ring.width, 0.02, 0.15, 0.06);
        fix(&mut self.desktop_fx.intensity, 0.0, 1.0, 0.7);
        fix(&mut self.desktop_fx.pattern_scale, 0.5, 3.0, 1.0);
        let f = &mut self.disc.free;
        fix(&mut f.center_x, -0.5, 1.5, 0.92);
        fix(&mut f.center_y, -0.5, 1.5, 0.5);
        fix(&mut f.diameter, 0.2, 3.0, 0.95);
        fix(&mut f.art_offset_x, -1.0, 1.0, 0.0);
        fix(&mut f.art_offset_y, -1.0, 1.0, 0.0);
        fix(&mut f.art_zoom, 0.2, 4.0, 1.0);
        fix(&mut f.art_rotation_deg, -180.0, 180.0, 0.0);
        self.performance.fps_cap = self.performance.fps_cap.clamp(5, 120);
    }
    /// 先寫暫存檔再改名，避免桌布程式讀到寫一半的檔案。
    /// 暫存檔名含程序 ID，兩個程式同時存檔不會互踩。
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
        std::fs::write(&tmp, serde_json::to_string_pretty(self).map_err(std::io::Error::other)?)?;
        std::fs::rename(&tmp, path)
    }
    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&config_path())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_partial_json() {
        let c = Config::default();
        let s = serde_json::to_string(&c).unwrap();
        let _: Config = serde_json::from_str(&s).unwrap();
        // 舊檔只有部分欄位也要讀得進來
        let p: Config = serde_json::from_str(r#"{"disc":{"rpm":5.0}}"#).unwrap();
        assert_eq!(p.disc.rpm, 5.0);
        assert!(p.enabled);
    }

    #[test]
    fn sanitize_and_strict_load() {
        let mut c: Config = serde_json::from_str(r#"{"disc":{"visible_ratio":0.0,"rpm":9999.0,"free":{"art_zoom":0.05}},"performance":{"fps_cap":0}}"#).unwrap();
        c.sanitize();
        // 舊設定裡已停用的欄位會被忽略（不得因此讀檔失敗）；縮放被限制在 0.2～4
        assert_eq!(c.disc.free.art_zoom, 0.2);
        assert_eq!(c.disc.rpm, 60.0);
        assert_eq!(c.performance.fps_cap, 5);
        // 不存在 → 預設；壞掉 → Err（不可偷偷回傳預設值）
        let dir = std::env::temp_dir().join(format!("nijikage-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(Config::try_load_from(&dir.join("nope.json")).is_ok());
        let bad = dir.join("bad.json");
        std::fs::write(&bad, "{ not json").unwrap();
        assert!(Config::try_load_from(&bad).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
