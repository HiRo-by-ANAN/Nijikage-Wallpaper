//! 設定視窗應用程式狀態、圖片預覽快取、CPU 光碟盤面光柵化與 300ms 存檔去抖（Debounce）管理。

use crate::tabs::SettingsTab;
use nijikage_wallpaper_core::{
    Config, DiscStyle, FreeLayout, LayoutMode,
    i18n::{set_language, tr, tr_fmt},
    image_prep::{disc_tone_preview, load_full},
};
use eframe::egui;
use image::{Rgba, RgbaImage, imageops::FilterType};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// 存檔去抖延遲時間：300 毫秒內若有連續改動則合併為一次寫檔。
const SAVE_DEBOUNCE: Duration = Duration::from_millis(300);

/// 背景預覽縮圖的最大邊長（像素）。
const BG_PREVIEW_MAX_SIDE: u32 = 320;

/// 光碟原圖快取的最大邊長（像素），保留原始長寬比以供縮放與旋轉重取樣。
const DISC_CACHE_MAX_SIDE: u32 = 512;

/// CPU 光柵化光碟預覽貼圖的邊長（像素）。
const DISC_RASTER_SIDE: u32 = 320;

/// 用於判斷光碟 CPU 光柵化貼圖是否需要重新計算的快取鍵值。
#[derive(Debug, Clone, PartialEq, Eq)]
struct DiscRasterCacheKey {
    path: Option<PathBuf>,
    style: DiscStyle,
    offset_x_bits: u32,
    offset_y_bits: u32,
    zoom_bits: u32,
    rotation_bits: u32,
}

impl DiscRasterCacheKey {
    fn from_config(config: &Config) -> Self {
        let f = &config.disc.free;
        Self {
            path: config.disc.art_path.clone(),
            style: config.disc.style,
            offset_x_bits: f.art_offset_x.to_bits(),
            offset_y_bits: f.art_offset_y.to_bits(),
            zoom_bits: f.art_zoom.to_bits(),
            rotation_bits: f.art_rotation_deg.to_bits(),
        }
    }
}

/// Nijikage-Wallpaper 設定視窗主狀態。
pub struct SettingsApp {
    /// 目前編輯中的設定資料（對應 `%APPDATA%\Nijikage-Wallpaper\config.json`）。
    pub config: Config,
    /// 目前選中的左側分頁。
    pub active_tab: SettingsTab,

    /// 最近一次修改設定的時間點；用於實作 300ms 去抖存檔。
    last_edit: Option<Instant>,
    /// 設定檔上次已知的修改時間，用來偵測外部變更（例如托盤暫停）。
    last_mtime: Option<std::time::SystemTime>,
    last_poll: Instant,
    /// 啟動讀檔失敗時設為 `true`，禁止自動存檔以防預設值覆蓋損毀或鎖定的設定檔，
    /// 直到成功讀取設定檔或使用者明確按下「還原預設值」為止。
    load_blocked: bool,
    /// 使用者是否明確按了「還原預設值」（允許在原檔損毀時以預設值覆寫）。
    reset_pending: bool,
    /// 使用者是否在本視窗中主動修改過 `enabled`（避免一般存檔時覆蓋托盤剛改的 `enabled`）。
    enabled_edited: bool,
    /// 單一更新幀內的變更比對基準（在「重新載入」或「還原預設值」後會同步更新）。
    frame_baseline: Option<Config>,

    /// 底部狀態列訊息。
    pub status_text: String,
    /// 最近一次檔案存取或圖片讀取的錯誤訊息。
    pub error_message: Option<String>,

    /// 背景預覽已載入的檔案路徑。
    bg_loaded_path: Option<PathBuf>,
    /// 背景圖片預覽紋理。
    pub bg_texture: Option<egui::TextureHandle>,
    /// 背景原圖尺寸 `(寬, 高)`。
    pub bg_dimensions: Option<(u32, u32)>,
    /// 背景圖片讀取錯誤訊息。
    pub bg_error: Option<String>,

    /// 光碟面已載入的檔案路徑。
    disc_loaded_path: Option<PathBuf>,
    /// 光碟面目前套用的色調風格快取。
    disc_loaded_style: Option<DiscStyle>,
    /// 光碟完整原圖快取（保留原始長寬比，不預先裁切）。
    disc_full_cache: Option<RgbaImage>,
    /// 套用 `disc_tone_preview` 後的完整圖片快取（供改變位移／縮放／旋轉時快速雙線性取樣）。
    disc_toned_cache: Option<RgbaImage>,
    /// 光碟原圖預覽紋理（保留長寬比）。
    pub disc_orig_texture: Option<egui::TextureHandle>,
    /// 光碟原圖尺寸 `(寬, 高)`。
    pub disc_orig_dimensions: Option<(u32, u32)>,
    /// 光碟 CPU 光柵化貼圖的快取鍵值。
    disc_raster_key: Option<DiscRasterCacheKey>,
    /// 320×320 的光碟 CPU 光柵化 RGBA 貼圖（圓外 alpha=0、圓內無中心孔並雙線性取樣色調圖）。
    pub disc_raster_texture: Option<egui::TextureHandle>,
    /// 光碟圖片讀取錯誤訊息。
    pub disc_error: Option<String>,
}

impl SettingsApp {
    /// 建立設定視窗實例，啟動時執行 `Config::try_load()` 並載入預覽縮圖。
    /// 若讀檔失敗則顯示錯誤訊息並禁止自動存檔，直到成功讀檔或使用者按「還原預設值」。
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (config, load_blocked, last_mtime, status_text, error_message) = match Config::try_load() {
            Ok(mut c) => {
                normalize_layout_mode_in_place(&mut c);
                set_language(c.language);
                let path_str = nijikage_wallpaper_core::config_path().display().to_string();
                (
                    c,
                    false,
                    config_mtime(),
                    tr_fmt("已載入設定檔：{}", &[&path_str]),
                    None,
                )
            }
            Err(err) => {
                let mut fallback = Config::default();
                normalize_layout_mode_in_place(&mut fallback);
                set_language(fallback.language);
                let msg = tr_fmt(
                    "{}（已暫停自動存檔以保護原設定檔；請修正後按「重新載入」或按「還原預設值」）",
                    &[&err],
                );
                (
                    fallback,
                    true,
                    None,
                    tr("讀取設定檔失敗，已暫停自動存檔").to_string(),
                    Some(msg),
                )
            }
        };

        cc.egui_ctx.send_viewport_cmd(egui::ViewportCommand::Title(
            tr("Nijikage-Wallpaper 設定").to_string(),
        ));

        let mut app = Self {
            config,
            active_tab: initial_tab(),
            last_edit: None,
            last_mtime,
            last_poll: Instant::now(),
            load_blocked,
            reset_pending: false,
            enabled_edited: false,
            frame_baseline: None,
            status_text,
            error_message,
            bg_loaded_path: None,
            bg_texture: None,
            bg_dimensions: None,
            bg_error: None,
            disc_loaded_path: None,
            disc_loaded_style: None,
            disc_full_cache: None,
            disc_toned_cache: None,
            disc_orig_texture: None,
            disc_orig_dimensions: None,
            disc_raster_key: None,
            disc_raster_texture: None,
            disc_error: None,
        };

        app.sync_previews(&cc.egui_ctx);
        app
    }

    /// 標記設定已變更，啟動或重置 300ms 去抖計時器。
    /// 若啟動時讀檔失敗且尚未解除鎖定，則不啟動自動存檔。
    pub fn mark_dirty(&mut self) {
        if self.load_blocked {
            self.status_text = tr("設定檔讀取失敗鎖定中，請先「重新載入」或「還原預設值」").to_string();
            return;
        }
        self.last_edit = Some(Instant::now());
        self.status_text = tr("設定已變更，準備自動儲存…").to_string();
    }

    /// 先重新讀取磁碟上的設定檔（`Config::try_load()`），僅合併設定視窗負責的欄位後寫回磁碟。
    /// - 不覆蓋托盤剛修改的 `enabled`（除非使用者在本視窗主動切換了 `enabled` 或按了還原預設值）。
    /// - 讀檔或寫檔失敗時保留 dirty 狀態並延遲重試，絕不以預設值覆蓋使用者的設定檔。
    pub fn flush_save(&mut self) {
        if self.load_blocked {
            return;
        }

        let mut disk_cfg = match Config::try_load() {
            Ok(c) => c,
            Err(e) => {
                if self.reset_pending {
                    Config::default()
                } else {
                    // 讀檔失敗時不得以預設值寫檔，保留未存狀態稍後重試
                    self.last_edit = Some(Instant::now());
                    let msg = tr_fmt("儲存前讀取設定檔失敗（將稍後重試）：{}", &[&e]);
                    self.status_text = msg.clone();
                    self.error_message = Some(msg);
                    return;
                }
            }
        };

        merge_settings_into(
            &self.config,
            &mut disk_cfg,
            self.enabled_edited || self.reset_pending,
        );
        disk_cfg.sanitize();

        match disk_cfg.save() {
            Ok(()) => {
                self.config.enabled = disk_cfg.enabled;
                if let Some(baseline) = &mut self.frame_baseline {
                    baseline.enabled = disk_cfg.enabled;
                }
                self.last_edit = None;
                self.enabled_edited = false;
                self.reset_pending = false;
                self.last_mtime = config_mtime();
                self.error_message = None;
                let path_str = nijikage_wallpaper_core::config_path().display().to_string();
                self.status_text = tr_fmt("已自動儲存至 {}", &[&path_str]);
            }
            Err(e) => {
                // 存檔失敗後保留 dirty 狀態、延遲重試
                self.last_edit = Some(Instant::now());
                let e_str = e.to_string();
                let msg = tr_fmt("儲存設定檔失敗（將稍後重試）：{}", &[&e_str]);
                self.status_text = msg.clone();
                self.error_message = Some(msg);
            }
        }
    }

    /// 重新從磁碟讀取 `Config::try_load()` 並更新預覽；同步更新變更偵測基準以避免多餘寫檔。
    pub fn reload_from_disk(&mut self, ctx: &egui::Context) {
        match Config::try_load() {
            Ok(mut c) => {
                normalize_layout_mode_in_place(&mut c);
                self.config = c;
            }
            Err(e) => {
                self.error_message = Some(e);
                return;
            }
        }
        set_language(self.config.language);
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(
            tr("Nijikage-Wallpaper 設定").to_string(),
        ));
        self.load_blocked = false;
        self.reset_pending = false;
        self.enabled_edited = false;
        self.last_mtime = config_mtime();
        self.last_edit = None;
        self.error_message = None;
        self.status_text = tr("已從磁碟重新載入設定檔").to_string();
        self.bg_loaded_path = None;
        self.disc_loaded_path = None;
        self.disc_loaded_style = None;
        self.disc_raster_key = None;
        self.sync_previews(ctx);
        self.frame_baseline = Some(self.config.clone());
    }

    /// 將所有設定還原為預設值，解除讀檔鎖定並走 300ms 去抖存檔；同步更新變更偵測基準。
    pub fn reset_to_defaults(&mut self, ctx: &egui::Context) {
        let mut def = Config::default();
        normalize_layout_mode_in_place(&mut def);
        self.config = def;
        set_language(self.config.language);
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(
            tr("Nijikage-Wallpaper 設定").to_string(),
        ));
        self.load_blocked = false;
        self.reset_pending = true;
        self.enabled_edited = true;
        self.error_message = None;
        self.mark_dirty();
        self.sync_previews(ctx);
        self.frame_baseline = Some(self.config.clone());
    }

    /// 使用 `rfd` 開啟檔案對話框挑選背景圖片。
    pub fn pick_background_image(&mut self, ctx: &egui::Context) {
        let dialog = rfd::FileDialog::new()
            .set_title(tr("選擇背景圖片"))
            .add_filter(
                tr("圖片檔案 (PNG, JPG, WebP, BMP)"),
                &["png", "jpg", "jpeg", "webp", "bmp"],
            );

        if let Some(path) = dialog.pick_file() {
            self.config.background.image_path = Some(path);
            self.config.background.mode = nijikage_wallpaper_core::BackgroundMode::Image;
            self.mark_dirty();
            self.sync_previews(ctx);
        }
    }

    /// 使用 `rfd` 開啟檔案對話框挑選光碟面圖片。
    pub fn pick_disc_image(&mut self, ctx: &egui::Context) {
        let dialog = rfd::FileDialog::new()
            .set_title(tr("選擇光碟面圖片"))
            .add_filter(
                tr("圖片檔案 (PNG, JPG, WebP, BMP)"),
                &["png", "jpg", "jpeg", "webp", "bmp"],
            );

        if let Some(path) = dialog.pick_file() {
            self.config.disc.art_path = Some(path);
            self.mark_dirty();
            self.sync_previews(ctx);
        }
    }

    /// 檢查預覽快取是否與目前設定一致，若路徑、光碟風格或圖片版面參數改變則更新預覽紋理。
    pub fn sync_previews(&mut self, ctx: &egui::Context) {
        // 1. 同步背景預覽
        if self.bg_loaded_path != self.config.background.image_path {
            self.bg_loaded_path = self.config.background.image_path.clone();
            self.bg_texture = None;
            self.bg_dimensions = None;
            self.bg_error = None;

            if let Some(path) = &self.bg_loaded_path {
                match load_background_thumbnail(path, BG_PREVIEW_MAX_SIDE) {
                    Ok((rgba, orig_size)) => {
                        self.bg_dimensions = Some(orig_size);
                        self.bg_texture = Some(rgba_to_texture(ctx, "bg_preview_tex", &rgba));
                    }
                    Err(err) => {
                        self.bg_error = Some(err);
                    }
                }
            }
        }

        // 2. 同步光碟面完整原圖（不預先裁切，保留原始長寬比）與色調快取
        let path_changed = self.disc_loaded_path != self.config.disc.art_path;
        let style_changed = self.disc_loaded_style != Some(self.config.disc.style);

        if path_changed {
            self.disc_loaded_path = self.config.disc.art_path.clone();
            self.disc_full_cache = None;
            self.disc_toned_cache = None;
            self.disc_orig_texture = None;
            self.disc_orig_dimensions = None;
            self.disc_error = None;

            if let Some(path) = &self.disc_loaded_path {
                match load_full(path, DISC_CACHE_MAX_SIDE) {
                    Ok(full_img) => {
                        self.disc_orig_dimensions = Some(full_img.dimensions());
                        self.disc_orig_texture =
                            Some(rgba_to_texture(ctx, "disc_orig_tex", &full_img));
                        self.disc_full_cache = Some(full_img);
                    }
                    Err(err) => {
                        self.disc_error = Some(err);
                    }
                }
            }
        }

        if path_changed || style_changed {
            self.disc_loaded_style = Some(self.config.disc.style);
            self.disc_toned_cache = self
                .disc_full_cache
                .as_ref()
                .map(|full| disc_tone_preview(full, self.config.disc.style));
        }

        // 3. 當路徑、風格或圖片變換參數（位移／縮放／旋轉）改變時，在 CPU 重新光柵化 320×320 光碟貼圖
        let new_key = DiscRasterCacheKey::from_config(&self.config);
        if path_changed
            || style_changed
            || self.disc_raster_texture.is_none()
            || self.disc_raster_key.as_ref() != Some(&new_key)
        {
            let raster = rasterize_disc_preview(
                self.disc_toned_cache.as_ref(),
                self.config.disc.style,
                &self.config.disc.free,
                DISC_RASTER_SIDE,
            );
            self.disc_raster_texture =
                Some(rgba_to_texture(ctx, "disc_raster_preview_tex", &raster));
            self.disc_raster_key = Some(new_key);
        }
    }

    /// 是否有尚未寫入磁碟的變更。
    pub fn is_dirty(&self) -> bool {
        self.last_edit.is_some()
    }
}

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 每 500ms 檢查設定檔是否被外部改動（例如托盤暫停）；有未存的編輯時不覆蓋。
        // 讀取成功才更新 last_mtime；讀取失敗時保留舊設定並於下一輪重試。
        if self.last_poll.elapsed() >= Duration::from_millis(500) {
            self.last_poll = Instant::now();
            let m = config_mtime();
            if (m != self.last_mtime || self.load_blocked) && !self.is_dirty() {
                match Config::try_load() {
                    Ok(mut c) => {
                        normalize_layout_mode_in_place(&mut c);
                        self.config = c;
                        set_language(self.config.language);
                        ctx.send_viewport_cmd(egui::ViewportCommand::Title(
                            tr("Nijikage-Wallpaper 設定").to_string(),
                        ));
                        self.last_mtime = m;
                        self.load_blocked = false;
                        self.error_message = None;
                        self.sync_previews(ctx);
                        self.status_text = tr("偵測到外部變更，已同步").to_string();
                    }
                    Err(e) => {
                        self.error_message = Some(e);
                    }
                }
            }
            ctx.request_repaint_after(Duration::from_millis(500));
        }

        // 確保舊設定中的 Simple 模式在進入介面繪製前一律視為 Free（光碟模式）
        normalize_layout_mode_in_place(&mut self.config);
        self.frame_baseline = Some(self.config.clone());

        // 頂部標題與桌布總開關工具列
        egui::TopBottomPanel::top("top_header_panel")
            .frame(
                egui::Frame::side_top_panel(&ctx.style())
                    .inner_margin(egui::Margin::symmetric(18, 12)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.heading(
                            egui::RichText::new(tr("Nijikage-Wallpaper 虹影 設定"))
                                .size(20.0)
                                .strong(),
                        );
                        ui.label(
                            egui::RichText::new(tr("Windows 動態光學桌布個人化設定"))
                                .size(12.0)
                                .weak(),
                        );
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let status_label = if self.config.enabled {
                            egui::RichText::new(tr("● 桌布執行中"))
                                .color(egui::Color32::from_rgb(110, 220, 140))
                                .strong()
                        } else {
                            egui::RichText::new(tr("○ 桌布已暫停"))
                                .color(egui::Color32::from_rgb(180, 180, 195))
                        };
                        ui.checkbox(&mut self.config.enabled, tr("啟用動態桌布"));
                        ui.label(status_label);
                    });
                });
            });

        // 底部狀態列（顯示自動存檔狀態與設定檔路徑）
        egui::TopBottomPanel::bottom("bottom_status_panel")
            .frame(
                egui::Frame::side_top_panel(&ctx.style())
                    .inner_margin(egui::Margin::symmetric(16, 8)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if let Some(err) = &self.error_message {
                        ui.colored_label(egui::Color32::from_rgb(255, 110, 110), err);
                    } else {
                        let dot_color = if self.is_dirty() {
                            egui::Color32::from_rgb(250, 200, 80)
                        } else {
                            egui::Color32::from_rgb(100, 210, 140)
                        };
                        ui.colored_label(dot_color, "●");
                        ui.label(egui::RichText::new(&self.status_text).size(12.0).weak());
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(tr("還原預設值")).clicked() {
                            self.reset_to_defaults(ctx);
                        }
                        if ui.button(tr("重新載入")).clicked() {
                            self.reload_from_disk(ctx);
                        }
                    });
                });
            });

        // 左側個人化風格導覽列
        egui::SidePanel::left("left_nav_panel")
            .resizable(false)
            .exact_width(215.0)
            .frame(
                egui::Frame::side_top_panel(&ctx.style())
                    .inner_margin(egui::Margin::symmetric(12, 14)),
            )
            .show(ctx, |ui| {
                ui.label(egui::RichText::new(tr("個人化類別")).size(12.0).weak());
                ui.add_space(6.0);

                for tab in SettingsTab::ALL {
                    let selected = self.active_tab == tab;
                    let text = egui::RichText::new(tab.title()).size(15.0);
                    let w = ui.available_width();
                    if ui
                        .add_sized([w, 38.0], egui::SelectableLabel::new(selected, text))
                        .clicked()
                    {
                        self.active_tab = tab;
                    }
                    ui.add_space(4.0);
                }

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    let ver_str = self.config.version.to_string();
                    ui.label(
                        egui::RichText::new(tr_fmt("設定格式版本 v{}", &[&ver_str]))
                            .size(11.5)
                            .weak(),
                    );
                });
            });

        // 中央主內容區（四個分頁）
        egui::CentralPanel::default()
            .frame(
                egui::Frame::central_panel(&ctx.style())
                    .inner_margin(egui::Margin::symmetric(22, 18)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        crate::tabs::show_active_tab(self, ui, ctx);
                    });
            });

        // 偵測任何控件是否改動了 Config（若剛按過「重新載入」或「還原預設值」，frame_baseline 已同步）
        if let Some(before) = self.frame_baseline.take() {
            if before.enabled != self.config.enabled {
                self.enabled_edited = true;
            }
            if before.language != self.config.language {
                set_language(self.config.language);
                ctx.send_viewport_cmd(egui::ViewportCommand::Title(
                    tr("Nijikage-Wallpaper 設定").to_string(),
                ));
                if self.bg_error.is_some() {
                    self.bg_loaded_path = None;
                }
                if self.disc_error.is_some() {
                    self.disc_loaded_path = None;
                }
            }
            if config_changed(&before, &self.config) {
                self.config.sanitize();
                normalize_layout_mode_in_place(&mut self.config);
                self.mark_dirty();
                self.sync_previews(ctx);
            }
        }

        // 處理 300ms 去抖自動存檔
        if let Some(edit_instant) = self.last_edit {
            if !self.load_blocked {
                let elapsed = edit_instant.elapsed();
                if elapsed >= SAVE_DEBOUNCE {
                    self.flush_save();
                } else {
                    ctx.request_repaint_after(SAVE_DEBOUNCE - elapsed);
                }
            }
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // 視窗關閉前若有尚未寫入的變更且未處於讀檔鎖定狀態，再試一次存檔
        if self.is_dirty() && !self.load_blocked {
            self.flush_save();
        }
    }
}

/// 將已停用的 `LayoutMode::Simple` 一律轉為 `LayoutMode::Free`（光碟模式）。
pub fn normalize_layout_mode(mode: LayoutMode) -> LayoutMode {
    match mode {
        LayoutMode::Simple | LayoutMode::Free => LayoutMode::Free,
        LayoutMode::Desktop => LayoutMode::Desktop,
    }
}

fn normalize_layout_mode_in_place(config: &mut Config) {
    config.disc.layout_mode = normalize_layout_mode(config.disc.layout_mode);
}

/// 只將設定視窗負責的欄位合併至剛從磁碟讀出的 `disk_cfg`，
/// 不讀寫已停用的簡易模式欄位，且在未主動編輯 `enabled` 時保留磁碟上的 `enabled`。
fn merge_settings_into(src: &Config, dst: &mut Config, include_enabled: bool) {
    dst.version = src.version;
    dst.background = src.background.clone();

    dst.disc.layout_mode = normalize_layout_mode(src.disc.layout_mode);
    dst.disc.free = src.disc.free;
    dst.disc.art_path = src.disc.art_path.clone();
    dst.disc.style = src.disc.style;
    dst.disc.rotate = src.disc.rotate;
    dst.disc.rpm = src.disc.rpm;
    dst.disc.reflection = src.disc.reflection;
    dst.disc.reflection_intensity = src.disc.reflection_intensity;
    dst.disc.cursor_reflection = src.disc.cursor_reflection;

    dst.effects = src.effects.clone();
    dst.audio_ring = src.audio_ring;
    dst.desktop_fx = src.desktop_fx;
    dst.language = src.language;
    dst.performance = src.performance.clone();
    dst.system = src.system.clone();

    if include_enabled {
        dst.enabled = src.enabled;
    }
}

/// 讀取背景圖片並縮放為預覽縮圖，回傳 `(縮圖, 原始寬高)`。失敗時回傳錯誤字串，不 panic。
fn load_background_thumbnail(
    path: &Path,
    max_side: u32,
) -> Result<(RgbaImage, (u32, u32)), String> {
    let path_str = path.display().to_string();
    let mut reader = image::ImageReader::open(path)
        .map_err(|e| tr_fmt("無法開啟背景圖片「{}」：{}", &[&path_str, &e.to_string()]))?
        .with_guessed_format()
        .map_err(|e| tr_fmt("無法判別背景圖片格式「{}」：{}", &[&path_str, &e.to_string()]))?;

    let mut limits = image::Limits::default();
    limits.max_image_width = Some(32768);
    limits.max_image_height = Some(32768);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);

    let decoded = reader
        .decode()
        .map_err(|e| tr_fmt("無法解碼背景圖片「{}」：{}", &[&path_str, &e.to_string()]))?;

    let w = decoded.width();
    let h = decoded.height();
    if w == 0 || h == 0 {
        return Err(tr("背景圖片尺寸無效（寬或高為 0）").to_string());
    }

    let thumb = if w > max_side || h > max_side {
        decoded.resize(max_side, max_side, FilterType::Triangle)
    } else {
        decoded
    };

    Ok((thumb.to_rgba8(), (w, h)))
}

/// 在 CPU 上將光碟光柵化為 `side × side` 的 RGBA 貼圖：
/// - 圓外 `alpha = 0`；**光碟中心沒有洞**，也不畫中心孔與內環。
/// - 圓內以光碟直徑為 1、圓心為原點的盤面座標 `(px, py)` 計算：
///   先減去 `art_offset`、再依 `-art_rotation_deg` 反向旋轉、再除以 `art_zoom`。
/// - `art_zoom = 1` 時圖片**短邊＝光碟直徑**；縮小時原本超出光碟的部分會回來顯示；
///   超出圖片範圍處顯示光碟盤面金屬本色。
fn rasterize_disc_preview(
    toned_art: Option<&RgbaImage>,
    style: DiscStyle,
    free: &FreeLayout,
    side: u32,
) -> RgbaImage {
    let side = side.max(16);
    let mut out = RgbaImage::new(side, side);

    let inv_side = 1.0 / (side as f32);
    let zoom = free.art_zoom.clamp(0.2, 4.0);
    let (sin_r, cos_r) = free.art_rotation_deg.to_radians().sin_cos();

    for y in 0..side {
        let py = (y as f32 + 0.5) * inv_side - 0.5;
        for x in 0..side {
            let px = (x as f32 + 0.5) * inv_side - 0.5;
            let dist = px.hypot(py);

            // 圓外透明，圓周 1 像素平滑抗鋸齒
            let edge_alpha = ((0.5 - dist) / inv_side + 0.5).clamp(0.0, 1.0);
            if edge_alpha <= 0.0 {
                out.put_pixel(x, y, Rgba([0, 0, 0, 0]));
                continue;
            }

            // 1. 計算光碟盤面金屬本色（中心平滑收斂，無中心孔與內環）
            let r_norm = (dist * 2.0).clamp(0.0, 1.0);
            let mut rgb = disc_base_surface_color(px, py, r_norm, style);

            // 2. 若有光碟圖片，進行座標逆變換並雙線性取樣
            if let Some(img) = toned_art {
                let w = img.width() as f32;
                let h = img.height() as f32;
                if w >= 1.0 && h >= 1.0 {
                    let short_side = w.min(h);
                    let dx = px - free.art_offset_x;
                    let dy = py - free.art_offset_y;
                    let rot_x = dx * cos_r + dy * sin_r;
                    let rot_y = -dx * sin_r + dy * cos_r;
                    let qx = rot_x / zoom;
                    let qy = rot_y / zoom;

                    // zoom = 1 時圖片短邊 = 光碟直徑 (1.0)
                    let u = 0.5 + qx * (short_side / w);
                    let v = 0.5 + qy * (short_side / h);

                    if (0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v) {
                        let [sr, sg, sb, sa] = bilinear_sample_rgba(img, u, v);
                        rgb[0] = rgb[0] * (1.0 - sa) + sr * sa;
                        rgb[1] = rgb[1] * (1.0 - sa) + sg * sa;
                        rgb[2] = rgb[2] * (1.0 - sa) + sb * sa;
                    }
                }
            }

            // 3. 光碟最外緣微細金屬倒角高光
            if r_norm > 0.965 {
                let rim_t = ((r_norm - 0.965) / 0.035).clamp(0.0, 1.0);
                rgb[0] = rgb[0] * (1.0 - rim_t * 0.35) + 0.90 * (rim_t * 0.35);
                rgb[1] = rgb[1] * (1.0 - rim_t * 0.35) + 0.94 * (rim_t * 0.35);
                rgb[2] = rgb[2] * (1.0 - rim_t * 0.35) + 1.00 * (rim_t * 0.35);
            }

            let out_r = (rgb[0].clamp(0.0, 1.0) * 255.0).round() as u8;
            let out_g = (rgb[1].clamp(0.0, 1.0) * 255.0).round() as u8;
            let out_b = (rgb[2].clamp(0.0, 1.0) * 255.0).round() as u8;
            let out_a = (edge_alpha * 255.0).round() as u8;
            out.put_pixel(x, y, Rgba([out_r, out_g, out_b, out_a]));
        }
    }

    out
}

/// 計算無圖片或圖片範圍外的光碟盤面金屬底色（中心無洞且角度項於圓心平滑收斂為 0）。
fn disc_base_surface_color(px: f32, py: f32, r_norm: f32, style: DiscStyle) -> [f32; 3] {
    let center_smooth = (r_norm * 4.0).clamp(0.0, 1.0);
    let angle = py.atan2(px);
    let aniso = (angle * 2.0 - 0.7).cos() * center_smooth;
    let ring_wave = (r_norm * 24.0).sin() * 0.025 * center_smooth;
    let highlight = (aniso * 0.5 + 0.5).powf(2.2) * 0.18 * center_smooth;

    match style {
        DiscStyle::Cd => {
            let irid_r = 0.035 * center_smooth * (angle * 2.0 + r_norm * 10.0).cos();
            let irid_g = 0.035 * center_smooth * (angle * 2.0 + r_norm * 10.0 + 2.094).cos();
            let irid_b = 0.035 * center_smooth * (angle * 2.0 + r_norm * 10.0 + 4.188).cos();
            [
                (0.68 + highlight + ring_wave + irid_r).clamp(0.0, 1.0),
                (0.73 + highlight + ring_wave + irid_g).clamp(0.0, 1.0),
                (0.80 + highlight + ring_wave + irid_b).clamp(0.0, 1.0),
            ]
        }
        DiscStyle::Dvd => [
            (0.62 + highlight * 1.2 + ring_wave).clamp(0.0, 1.0),
            (0.42 + highlight * 0.9 + ring_wave * 0.8).clamp(0.0, 1.0),
            (0.66 + highlight * 0.5 - ring_wave * 0.5).clamp(0.0, 1.0),
        ],
        DiscStyle::Bd => [
            (0.32 + highlight * 0.7 + ring_wave * 0.6).clamp(0.0, 1.0),
            (0.44 + highlight * 0.9 + ring_wave * 0.8).clamp(0.0, 1.0),
            (0.80 + highlight * 1.1 + ring_wave).clamp(0.0, 1.0),
        ],
    }
}

/// 對 `RgbaImage` 進行雙線性插值取樣（`u, v` 範圍為 `0.0..=1.0`），回傳 `0.0..=1.0` 的 `[r, g, b, a]`。
fn bilinear_sample_rgba(img: &RgbaImage, u: f32, v: f32) -> [f32; 4] {
    let w = img.width();
    let h = img.height();
    if w == 0 || h == 0 {
        return [0.0, 0.0, 0.0, 0.0];
    }

    let fx = (u.clamp(0.0, 1.0) * (w.saturating_sub(1) as f32)).max(0.0);
    let fy = (v.clamp(0.0, 1.0) * (h.saturating_sub(1) as f32)).max(0.0);

    let x0 = (fx.floor() as u32).min(w - 1);
    let y0 = (fy.floor() as u32).min(h - 1);
    let x1 = (x0 + 1).min(w - 1);
    let y1 = (y0 + 1).min(h - 1);

    let tx = fx - (x0 as f32);
    let ty = fy - (y0 as f32);

    let p00 = img.get_pixel(x0, y0).0;
    let p10 = img.get_pixel(x1, y0).0;
    let p01 = img.get_pixel(x0, y1).0;
    let p11 = img.get_pixel(x1, y1).0;

    let mut out = [0.0_f32; 4];
    for c in 0..4 {
        let v00 = p00[c] as f32 / 255.0;
        let v10 = p10[c] as f32 / 255.0;
        let v01 = p01[c] as f32 / 255.0;
        let v11 = p11[c] as f32 / 255.0;
        let top = v00 + (v10 - v00) * tx;
        let bottom = v01 + (v11 - v01) * tx;
        out[c] = top + (bottom - top) * ty;
    }
    out
}

/// 將 `RgbaImage` 轉換為 `egui::TextureHandle`。
fn rgba_to_texture(ctx: &egui::Context, name: &str, img: &RgbaImage) -> egui::TextureHandle {
    let size = [img.width() as usize, img.height() as usize];
    let pixels = img.as_flat_samples();
    let color_image = egui::ColorImage::from_rgba_unmultiplied(size, pixels.as_slice());
    ctx.load_texture(name, color_image, egui::TextureOptions::LINEAR)
}

/// 比對兩份 `Config` 中由設定視窗負責的欄位是否有任何差異。
fn config_changed(a: &Config, b: &Config) -> bool {
    a.version != b.version
        || a.enabled != b.enabled
        || a.background.mode != b.background.mode
        || a.background.image_path != b.background.image_path
        || a.background.color != b.background.color
        || a.background.fit != b.background.fit
        || normalize_layout_mode(a.disc.layout_mode) != normalize_layout_mode(b.disc.layout_mode)
        || a.disc.free.center_x.to_bits() != b.disc.free.center_x.to_bits()
        || a.disc.free.center_y.to_bits() != b.disc.free.center_y.to_bits()
        || a.disc.free.diameter.to_bits() != b.disc.free.diameter.to_bits()
        || a.disc.free.art_offset_x.to_bits() != b.disc.free.art_offset_x.to_bits()
        || a.disc.free.art_offset_y.to_bits() != b.disc.free.art_offset_y.to_bits()
        || a.disc.free.art_zoom.to_bits() != b.disc.free.art_zoom.to_bits()
        || a.disc.free.art_rotation_deg.to_bits() != b.disc.free.art_rotation_deg.to_bits()
        || a.disc.art_path != b.disc.art_path
        || a.disc.style != b.disc.style
        || a.disc.rotate != b.disc.rotate
        || a.disc.rpm.to_bits() != b.disc.rpm.to_bits()
        || a.disc.reflection != b.disc.reflection
        || a.disc.reflection_intensity.to_bits() != b.disc.reflection_intensity.to_bits()
        || a.disc.cursor_reflection != b.disc.cursor_reflection
        || a.audio_ring.enabled != b.audio_ring.enabled
        || a.audio_ring.sensitivity.to_bits() != b.audio_ring.sensitivity.to_bits()
        || a.audio_ring.width.to_bits() != b.audio_ring.width.to_bits()
        || a.desktop_fx.intensity.to_bits() != b.desktop_fx.intensity.to_bits()
        || a.desktop_fx.pattern_scale.to_bits() != b.desktop_fx.pattern_scale.to_bits()
        || a.effects.on_disc.rain != b.effects.on_disc.rain
        || a.effects.on_disc.wind != b.effects.on_disc.wind
        || a.effects.on_disc.leaves != b.effects.on_disc.leaves
        || a.effects.on_disc.sakura != b.effects.on_disc.sakura
        || a.effects.on_desktop.rain != b.effects.on_desktop.rain
        || a.effects.on_desktop.wind != b.effects.on_desktop.wind
        || a.effects.on_desktop.leaves != b.effects.on_desktop.leaves
        || a.effects.on_desktop.sakura != b.effects.on_desktop.sakura
        || a.effects.intensity.to_bits() != b.effects.intensity.to_bits()
        || a.effects.interactive != b.effects.interactive
        || a.performance.fps_cap != b.performance.fps_cap
        || a.performance.pause_on_fullscreen != b.performance.pause_on_fullscreen
        || a.language != b.language
        || a.system.autostart != b.system.autostart
        || a.system.tray_icon != b.system.tray_icon
}

/// 設定檔的修改時間；檔案不存在回傳 None。
fn config_mtime() -> Option<std::time::SystemTime> {
    std::fs::metadata(nijikage_wallpaper_core::config_path())
        .and_then(|m| m.modified())
        .ok()
}

/// 起始分頁。診斷與截圖用：環境變數 `NIJIKAGE_WALLPAPER_SETTINGS_TAB` 設為 background／disc／effects／system 可直接開在該分頁。
fn initial_tab() -> SettingsTab {
    match std::env::var("NIJIKAGE_WALLPAPER_SETTINGS_TAB").ok().as_deref() {
        Some("disc") => SettingsTab::Disc,
        Some("effects") => SettingsTab::Effects,
        Some("system") => SettingsTab::PerformanceAndSystem,
        _ => SettingsTab::Background,
    }
}
