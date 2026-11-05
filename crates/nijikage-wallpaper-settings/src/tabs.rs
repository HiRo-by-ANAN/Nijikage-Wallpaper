//! 四個設定分頁介面：「背景」、「光碟」、「特效」、「效能與系統」。
//! 風格參照 Windows 個人化設定，採用卡片區塊排列，並涵蓋 `Config` 所有設定視窗負責的欄位。

use crate::{
    app::{SettingsApp, normalize_layout_mode},
    layout_preview::{draw_desktop_fx_overlay, draw_disc_quad},
};
use nijikage_wallpaper_core::{
    BackgroundMode, DiscStyle, EffectSet, FitMode, Language, LayoutMode,
    i18n::{set_language, tr, tr_fmt},
};
use eframe::egui;

/// 設定視窗的四大分頁。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    /// 背景（模式、圖片挑選、排列方式、純色挑選、預覽縮圖）
    Background,
    /// 光碟（光碟模式／全桌面模式、版面預覽、音訊動態環、光碟圖片與色調預覽、光碟風格、旋轉與反射）
    Disc,
    /// 特效（光碟上特效、整個桌面特效、特效量、游標互動）
    Effects,
    /// 效能與系統（FPS 上限、全螢幕暫停、開機自啟動、托盤圖示、桌布總開關）
    PerformanceAndSystem,
}

impl SettingsTab {
    pub const ALL: [SettingsTab; 4] = [
        SettingsTab::Background,
        SettingsTab::Disc,
        SettingsTab::Effects,
        SettingsTab::PerformanceAndSystem,
    ];

    /// 分頁導覽標題。
    pub fn title(self) -> &'static str {
        match self {
            SettingsTab::Background => tr("🖼  背景"),
            SettingsTab::Disc => tr("💿  光碟"),
            SettingsTab::Effects => tr("✨  特效"),
            SettingsTab::PerformanceAndSystem => tr("⚙  效能與系統"),
        }
    }
}

/// 根據目前選中的分頁繪製對應內容。
pub fn show_active_tab(app: &mut SettingsApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    match app.active_tab {
        SettingsTab::Background => show_background_tab(app, ui, ctx),
        SettingsTab::Disc => show_disc_tab(app, ui, ctx),
        SettingsTab::Effects => show_effects_tab(app, ui),
        SettingsTab::PerformanceAndSystem => show_performance_system_tab(app, ui, ctx),
    }
}

/// 繪製類似 Windows 11 個人化設定的區段卡片容器。
fn section_card(
    ui: &mut egui::Ui,
    title: &str,
    subtitle: &str,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::group(ui.style())
        .fill(egui::Color32::from_rgb(34, 36, 46))
        .stroke(egui::Stroke::new(
            1.0_f32,
            egui::Color32::from_rgb(52, 56, 72),
        ))
        .inner_margin(egui::Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(egui::RichText::new(title).size(15.5).strong());
            if !subtitle.is_empty() {
                ui.label(egui::RichText::new(subtitle).size(12.0).weak());
            }
            ui.add_space(8.0);
            add_contents(ui);
        });
    ui.add_space(12.0);
}

// ============================================================================
// 分頁 1：背景
// ============================================================================

fn show_background_tab(app: &mut SettingsApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.heading(tr("背景個人化"));
    ui.label(
        egui::RichText::new(tr("設定桌面背景模式、自訂圖片、畫面排列方式與背景純色。"))
            .size(12.5)
            .weak(),
    );
    ui.add_space(12.0);

    // 1. 預覽縮圖區塊
    section_card(
        ui,
        tr("桌面背景預覽"),
        tr("即時預覽目前的背景色彩或選定的背景圖片縮圖與光學效果示意"),
        |ui| {
            let preview_w = 320.0_f32.min(ui.available_width());
            let preview_h = preview_w * (9.0 / 16.0);
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(preview_w, preview_h), egui::Sense::hover());

            draw_background_rect_contents(ui.painter(), rect, app);

            let mode = normalize_layout_mode(app.config.disc.layout_mode);
            let clipped = ui.painter().with_clip_rect(rect);
            match mode {
                LayoutMode::Desktop => {
                    draw_desktop_fx_overlay(
                        &clipped,
                        rect,
                        app.config.desktop_fx.intensity,
                        app.config.desktop_fx.pattern_scale,
                        app.config.disc.style,
                    );
                }
                LayoutMode::Free | LayoutMode::Simple => {
                    let f = &app.config.disc.free;
                    let disc_diameter = preview_h * f.diameter.clamp(0.2, 3.0);
                    let r = disc_diameter * 0.5;
                    let cx = rect.left() + preview_w * f.center_x.clamp(-0.5, 1.5);
                    let cy = rect.top() + preview_h * f.center_y.clamp(-0.5, 1.5);
                    draw_disc_quad(&clipped, app, egui::pos2(cx, cy), r, 0.90);
                }
            }

            ui.add_space(6.0);
            if let Some((w, h)) = app.bg_dimensions {
                let w_str = w.to_string();
                let h_str = h.to_string();
                ui.label(
                    egui::RichText::new(tr_fmt(
                        "背景圖片原始解析度：{} × {} 像素｜排列方式：{}",
                        &[&w_str, &h_str, fit_mode_label(app.config.background.fit)],
                    ))
                    .size(12.0)
                    .weak(),
                );
            } else if app.config.background.mode == BackgroundMode::Color {
                let [r, g, b] = app.config.background.color;
                let r_str = r.to_string();
                let g_str = g.to_string();
                let b_str = b.to_string();
                ui.label(
                    egui::RichText::new(tr_fmt(
                        "目前使用純色背景：RGB({}, {}, {})",
                        &[&r_str, &g_str, &b_str],
                    ))
                    .size(12.0)
                    .weak(),
                );
            }
            if let Some(err) = &app.bg_error {
                ui.colored_label(egui::Color32::from_rgb(255, 110, 110), err);
            }
        },
    );

    // 2. 背景模式切換
    section_card(
        ui,
        tr("個人化您的背景"),
        tr("選擇使用圖片或純色作為桌面底圖"),
        |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(tr("背景模式："));
                ui.selectable_value(
                    &mut app.config.background.mode,
                    BackgroundMode::Image,
                    tr("圖片"),
                );
                ui.selectable_value(
                    &mut app.config.background.mode,
                    BackgroundMode::Color,
                    tr("純色"),
                );
            });
        },
    );

    // 3. 圖片選擇與排列方式
    section_card(
        ui,
        tr("背景圖片與排列方式"),
        tr("挑選電腦上的圖片檔案，並選擇與 Windows 個人化相同的契合模式"),
        |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button(tr("📂 瀏覽並選擇圖片…")).clicked() {
                    app.pick_background_image(ctx);
                }
                if app.config.background.image_path.is_some()
                    && ui.button(tr("✕ 清除圖片")).clicked()
                {
                    app.config.background.image_path = None;
                }
            });

            let path_display = app
                .config
                .background
                .image_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| tr("（尚未選擇背景圖片）").to_string());
            ui.label(
                egui::RichText::new(tr_fmt("目前檔案：{}", &[&path_display]))
                    .size(12.0)
                    .weak(),
            );

            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(tr("選擇符合項目（排列）："));
                for mode in [
                    FitMode::Fill,
                    FitMode::Fit,
                    FitMode::Stretch,
                    FitMode::Tile,
                    FitMode::Center,
                    FitMode::Span,
                ] {
                    ui.selectable_value(
                        &mut app.config.background.fit,
                        mode,
                        fit_mode_label(mode),
                    );
                }
            });
        },
    );

    // 4. 純色挑選
    section_card(
        ui,
        tr("背景純色挑選"),
        tr("作為純色模式底圖，或在圖片「符合／置中」模式下作為邊緣襯底色"),
        |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(tr("自訂背景色彩："));
                ui.color_edit_button_srgb(&mut app.config.background.color);
                let [r, g, b] = app.config.background.color;
                ui.label(
                    egui::RichText::new(format!(
                        "#{r:02X}{g:02X}{b:02X}  (R:{r}, G:{g}, B:{b})"
                    ))
                    .monospace()
                    .size(12.5),
                );
            });

            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(tr("常用深色主題色票：")).size(12.0).weak());
                let presets: [(&str, [u8; 3]); 6] = [
                    (tr("午夜深藍 (預設)"), [16, 18, 28]),
                    (tr("曜石純黑"), [10, 10, 12]),
                    (tr("深空藍紫"), [22, 20, 42]),
                    (tr("極光墨綠"), [14, 28, 28]),
                    (tr("深邃酒紅"), [34, 16, 24]),
                    (tr("石墨鉛灰"), [32, 34, 38]),
                ];
                for (name, rgb) in presets {
                    if ui.button(name).clicked() {
                        app.config.background.color = rgb;
                    }
                }
            });
        },
    );
}

/// 在指定的 16:9 預覽框內繪製背景底色與背景圖片縮圖。
fn draw_background_rect_contents(painter: &egui::Painter, rect: egui::Rect, app: &SettingsApp) {
    let [r, g, b] = app.config.background.color;
    let bg_color = egui::Color32::from_rgb(r, g, b);
    painter.rect_filled(rect, 6.0, bg_color);

    if app.config.background.mode == BackgroundMode::Image {
        if let Some(tex) = &app.bg_texture {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            let target_rect =
                compute_fit_preview_rect(rect, tex.size_vec2(), app.config.background.fit);
            painter
                .with_clip_rect(rect)
                .image(tex.id(), target_rect, uv, egui::Color32::WHITE);
        }
    }
}

/// 傳回 `FitMode` 的目前語言名稱。
fn fit_mode_label(fit: FitMode) -> &'static str {
    match fit {
        FitMode::Fill => tr("填滿"),
        FitMode::Fit => tr("符合"),
        FitMode::Stretch => tr("伸展"),
        FitMode::Tile => tr("並排"),
        FitMode::Center => tr("置中"),
        FitMode::Span => tr("跨螢幕"),
    }
}

/// 計算桌面預覽框中不同 `FitMode` 的圖片繪製區域。
pub(crate) fn compute_fit_preview_rect(
    container: egui::Rect,
    img_size: egui::Vec2,
    fit: FitMode,
) -> egui::Rect {
    if img_size.x <= 0.0 || img_size.y <= 0.0 {
        return container;
    }
    let cw = container.width();
    let ch = container.height();
    let aspect_img = img_size.x / img_size.y;
    let aspect_box = cw / ch;

    match fit {
        FitMode::Stretch => container,
        FitMode::Fit => {
            let (w, h) = if aspect_img > aspect_box {
                (cw, cw / aspect_img)
            } else {
                (ch * aspect_img, ch)
            };
            egui::Rect::from_center_size(container.center(), egui::vec2(w, h))
        }
        FitMode::Center => {
            let w = (img_size.x * 0.45).min(cw * 0.85);
            let h = (img_size.y * 0.45).min(ch * 0.85);
            egui::Rect::from_center_size(container.center(), egui::vec2(w, h))
        }
        FitMode::Fill | FitMode::Span | FitMode::Tile => {
            let (w, h) = if aspect_img > aspect_box {
                (ch * aspect_img, ch)
            } else {
                (cw, cw / aspect_img)
            };
            egui::Rect::from_center_size(container.center(), egui::vec2(w, h))
        }
    }
}

// ============================================================================
// 分頁 2：光碟（光碟模式 Free ／ 全桌面模式 Desktop）
// ============================================================================

fn show_disc_tab(app: &mut SettingsApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.heading(tr("光學模式與光碟設定"));
    ui.label(
        egui::RichText::new(tr(
            "切換「光碟模式」或「全桌面模式」，自訂盤面版面、音訊動態環、光學紋路與旋轉反射效果。",
        ))
        .size(12.5)
        .weak(),
    );
    ui.add_space(12.0);

    let current_mode = normalize_layout_mode(app.config.disc.layout_mode);

    // 1. 最上方：模式選擇（光碟模式 Free ／ 全桌面模式 Desktop）
    section_card(
        ui,
        tr("模式選擇"),
        tr("選擇在桌面上呈現單一光碟盤面（光碟模式），或將光碟背面的折射與彩虹繞射套用至整個桌面（全桌面模式）"),
        |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(tr("運作模式：")).strong());
                let is_free = current_mode == LayoutMode::Free;
                if ui
                    .add(egui::SelectableLabel::new(
                        is_free,
                        tr("💿 光碟模式（自由版面與音訊動態環）"),
                    ))
                    .clicked()
                {
                    app.config.disc.layout_mode = LayoutMode::Free;
                }

                let is_desktop = current_mode == LayoutMode::Desktop;
                if ui
                    .add(egui::SelectableLabel::new(
                        is_desktop,
                        tr("🖥 全桌面模式（整個桌面套用光學效果）"),
                    ))
                    .clicked()
                {
                    app.config.disc.layout_mode = LayoutMode::Desktop;
                }
            });
        },
    );

    match normalize_layout_mode(app.config.disc.layout_mode) {
        LayoutMode::Free | LayoutMode::Simple => {
            show_disc_mode_contents(app, ui, ctx);
        }
        LayoutMode::Desktop => {
            show_desktop_mode_contents(app, ui);
        }
    }
}

/// 繪製「光碟模式」（`LayoutMode::Free`）的完整控制項：
/// 版面預覽與數字欄位、音訊動態環、光碟面圖片與色調預覽、光碟風格、旋轉與反射設定。
fn show_disc_mode_contents(app: &mut SettingsApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    // 1. 光碟版面預覽與數字微調
    section_card(
        ui,
        tr("光碟版面預覽與位置調整"),
        tr("16:9 螢幕範圍預覽（中心無洞；完整原圖保留長寬比，zoom=1 時短邊等於光碟直徑，縮小時裁切範圍外會回來顯示）"),
        |ui| {
            crate::layout_preview::show_free_layout_editor(app, ui, ctx);
        },
    );

    // 2. 音訊動態環設定
    section_card(
        ui,
        tr("音訊動態環"),
        tr("在光碟最外圈呈現隨系統音樂律動的彩虹色光環，無聲時自動淡出且不耗用繪製資源"),
        |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(
                    &mut app.config.audio_ring.enabled,
                    tr("啟用光碟最外圈音訊動態環"),
                );
                ui.label(
                    egui::RichText::new(tr("擷取系統播放的聲音強度，只分析音量，不錄音也不儲存"))
                        .size(11.5)
                        .weak(),
                );
            });

            ui.add_space(6.0);
            ui.add_enabled_ui(app.config.audio_ring.enabled, |ui| {
                egui::Grid::new("audio_ring_controls_grid")
                    .num_columns(2)
                    .spacing([16.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(tr("靈敏度（0.2～3.0，預設 1.0）："));
                        ui.add(
                            egui::Slider::new(&mut app.config.audio_ring.sensitivity, 0.2..=3.0)
                                .step_by(0.05)
                                .suffix(" ×"),
                        );
                        ui.end_row();

                        ui.label(tr("動態環寬度（占光碟半徑比例 0.02～0.15，預設 0.06）："));
                        ui.add(
                            egui::Slider::new(&mut app.config.audio_ring.width, 0.02..=0.15)
                                .step_by(0.005),
                        );
                        ui.end_row();
                    });
            });
        },
    );

    // 3. 光碟面圖片與色調預覽（不預先裁切、光碟中心無洞）
    section_card(
        ui,
        tr("光碟面圖片與色調預覽"),
        tr("保留原始長寬比不預先裁切，轉換為對應的光碟金屬色調（圖片本身不隨光碟旋轉）"),
        |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button(tr("📂 選擇光碟面圖片…")).clicked() {
                    app.pick_disc_image(ctx);
                }
                if app.config.disc.art_path.is_some()
                    && ui.button(tr("✕ 移除自訂光碟圖")).clicked()
                {
                    app.config.disc.art_path = None;
                }
            });

            let art_path_text = app
                .config
                .disc
                .art_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| tr("（未指定圖片，顯示純光碟金屬盤面）").to_string());
            ui.label(
                egui::RichText::new(tr_fmt("目前圖片：{}", &[&art_path_text]))
                    .size(12.0)
                    .weak(),
            );

            if let Some((w, h)) = app.disc_orig_dimensions {
                let w_str = w.to_string();
                let h_str = h.to_string();
                ui.label(
                    egui::RichText::new(tr_fmt(
                        "原圖尺寸（保留長寬比）：{} × {} 像素",
                        &[&w_str, &h_str],
                    ))
                    .size(11.5)
                    .weak(),
                );
            }

            if let Some(err) = &app.disc_error {
                ui.colored_label(egui::Color32::from_rgb(255, 110, 110), err);
            }

            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                let side = 148.0_f32;

                // 左：完整原圖（等比縮放顯示於預覽框內，不裁切）
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(tr("完整原圖（保留長寬比）"))
                            .size(12.0)
                            .strong(),
                    );
                    if let Some(orig_tex) = &app.disc_orig_texture {
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(side, side),
                            egui::Sense::hover(),
                        );
                        ui.painter()
                            .rect_filled(rect, 6.0, egui::Color32::from_rgb(24, 26, 34));
                        let fit_rect =
                            compute_fit_preview_rect(rect, orig_tex.size_vec2(), FitMode::Fit);
                        let uv =
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                        ui.painter().with_clip_rect(rect).image(
                            orig_tex.id(),
                            fit_rect,
                            uv,
                            egui::Color32::WHITE,
                        );
                    } else {
                        draw_placeholder_box(ui, side, tr("無原圖"));
                    }
                });

                ui.add_space(18.0);

                // 右：套用光碟色調與版面變換後的無孔光碟預覽
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(tr_fmt(
                            "光碟盤面色調預覽（{}）",
                            &[disc_style_label(app.config.disc.style)],
                        ))
                        .size(12.0)
                        .strong(),
                    );
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
                    ui.painter()
                        .rect_filled(rect, 6.0, egui::Color32::from_rgb(24, 26, 34));
                    draw_disc_quad(
                        &ui.painter().with_clip_rect(rect),
                        app,
                        rect.center(),
                        side * 0.46,
                        1.0,
                    );
                });
            });
        },
    );

    // 4. 光碟風格
    section_card(
        ui,
        tr("光碟風格"),
        tr("切換不同光碟世代的金屬鍍膜色調與繞射反光色澤"),
        |ui| {
            ui.horizontal_wrapped(|ui| {
                for style in [DiscStyle::Cd, DiscStyle::Dvd, DiscStyle::Bd] {
                    ui.selectable_value(
                        &mut app.config.disc.style,
                        style,
                        disc_style_label(style),
                    );
                }
            });
        },
    );

    // 5. 旋轉與反射設定
    show_rotation_and_reflection_card(app, ui);
}

/// 繪製「全桌面模式」（`LayoutMode::Desktop`）的控制項：
/// 16:9 背景縮圖＋斜向光帶漸層示意、效果強度、紋路尺度、旋轉與反射設定（不顯示光碟專屬控制項）。
fn show_desktop_mode_contents(app: &mut SettingsApp, ui: &mut egui::Ui) {
    // 1. 全桌面模式 16:9 預覽與光學參數
    section_card(
        ui,
        tr("全桌面模式：整個桌面套用光學效果"),
        tr("不顯示光碟，將各向異性細紋高光、彩虹繞射色帶、慢速光帶與游標光源直接疊加於整個桌面背景"),
        |ui| {
            let preview_w = 420.0_f32.min(ui.available_width().max(240.0));
            let preview_h = preview_w * (9.0 / 16.0);
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(preview_w, preview_h), egui::Sense::hover());

            draw_background_rect_contents(ui.painter(), rect, app);
            draw_desktop_fx_overlay(
                ui.painter(),
                rect,
                app.config.desktop_fx.intensity,
                app.config.desktop_fx.pattern_scale,
                app.config.disc.style,
            );
            ui.painter().rect_stroke(
                rect,
                6.0,
                egui::Stroke::new(1.2_f32, egui::Color32::from_rgb(120, 155, 225)),
                egui::StrokeKind::Inside,
            );

            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(tr("預覽示意：16:9 背景縮圖疊加斜向光學光帶漸層（實際畫面由 GPU 即時計算同心紋與游標折射）"))
                    .size(11.5)
                    .weak(),
            );

            ui.add_space(10.0);
            egui::Grid::new("desktop_fx_controls_grid")
                .num_columns(2)
                .spacing([16.0, 10.0])
                .show(ui, |ui| {
                    ui.label(tr("效果強度（0.0～1.0，0 為等同原背景）："));
                    ui.add(
                        egui::Slider::new(&mut app.config.desktop_fx.intensity, 0.0..=1.0)
                            .step_by(0.01),
                    );
                    ui.end_row();

                    ui.label(tr("紋路尺度（0.5～3.0，越大光學紋路越寬）："));
                    ui.add(
                        egui::Slider::new(&mut app.config.desktop_fx.pattern_scale, 0.5..=3.0)
                            .step_by(0.05)
                            .suffix(" ×"),
                    );
                    ui.end_row();
                });
        },
    );

    // 2. 旋轉、反射與游標反射設定（於全桌面模式同樣生效）
    show_rotation_and_reflection_card(app, ui);
}

/// 繪製共用的「旋轉、反射光與游標互動」設定卡片（轉速滑桿範圍 −60～60 與 `Config::sanitize()` 一致）。
fn show_rotation_and_reflection_card(app: &mut SettingsApp, ui: &mut egui::Ui) {
    section_card(
        ui,
        tr("旋轉、反射光與游標互動"),
        tr("控制光帶與繞射高光的慢速轉動、反射亮度與游標光源互動"),
        |ui| {
            ui.checkbox(
                &mut app.config.disc.rotate,
                tr("啟用光學旋轉（慢速轉動反光光帶與繞射紋理）"),
            );
            ui.add_enabled_ui(app.config.disc.rotate, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(tr("旋轉速度（每分鐘轉數 -60～60）："));
                    ui.add(
                        egui::Slider::new(&mut app.config.disc.rpm, -60.0..=60.0)
                            .step_by(0.1)
                            .suffix(" RPM"),
                    );
                });
            });

            ui.add_space(6.0);
            ui.separator();
            ui.add_space(6.0);

            ui.checkbox(
                &mut app.config.disc.reflection,
                tr("啟用金屬反射與彩虹繞射高光"),
            );
            ui.add_enabled_ui(app.config.disc.reflection, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(tr("反射光強度（0.0～1.0）："));
                    ui.add(
                        egui::Slider::new(&mut app.config.disc.reflection_intensity, 0.0..=1.0)
                            .step_by(0.01),
                    );
                });
            });

            ui.add_space(4.0);
            ui.checkbox(
                &mut app.config.disc.cursor_reflection,
                tr("啟用游標反射互動（高光與局部折射隨游標位置偏移）"),
            );
        },
    );
}

/// 傳回 `DiscStyle` 的目前語言名稱。
fn disc_style_label(style: DiscStyle) -> &'static str {
    match style {
        DiscStyle::Cd => tr("CD（銀白彩虹繞射）"),
        DiscStyle::Dvd => tr("DVD（金紫雙色調）"),
        DiscStyle::Bd => tr("藍光 BD（藍紫雙色調）"),
    }
}

/// 當尚未選擇光碟圖片時，繪製無原圖佔位框。
fn draw_placeholder_box(ui: &mut egui::Ui, side: f32, label: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 6.0, egui::Color32::from_rgb(24, 26, 34));
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(13.0),
        egui::Color32::from_rgb(130, 136, 155),
    );
}

// ============================================================================
// 分頁 3：特效
// ============================================================================

fn show_effects_tab(app: &mut SettingsApp, ui: &mut egui::Ui) {
    let is_desktop_mode =
        normalize_layout_mode(app.config.disc.layout_mode) == LayoutMode::Desktop;

    ui.heading(tr("環境粒子與互動特效"));
    if is_desktop_mode {
        ui.label(
            egui::RichText::new(tr("目前為「全桌面模式」，粒子特效作用於整個桌面（光碟表面特效設定值已自動保留）。"))
                .size(12.5)
                .weak(),
        );
    } else {
        ui.label(
            egui::RichText::new(tr("可分別為「光碟表面」與「整個桌面」開啟雨滴、風、落葉、櫻花四種動態特效。"))
                .size(12.5)
                .weak(),
        );
    }
    ui.add_space(12.0);

    // 1. 光碟上特效四開關（全桌面模式時隱藏此區塊，但數值保留）
    if !is_desktop_mode {
        section_card(
            ui,
            tr("光碟上的特效（On Disc）"),
            tr("只落在光碟盤面範圍內的特效粒子"),
            |ui| {
                show_effect_set_controls(ui, "on_disc", &mut app.config.effects.on_disc);
            },
        );
    }

    // 2. 整個桌面特效四開關
    section_card(
        ui,
        tr("整個桌面的特效（On Desktop）"),
        tr("涵蓋全螢幕桌面的環境特效粒子"),
        |ui| {
            show_effect_set_controls(ui, "on_desktop", &mut app.config.effects.on_desktop);
        },
    );

    // 3. 特效量與游標互動
    section_card(
        ui,
        tr("特效密度與游標互動"),
        tr("控制畫面上的粒子生成數量上限與滑鼠游標撥動效果"),
        |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(tr("特效量（粒子密度 0.0–1.0）："));
                ui.add(
                    egui::Slider::new(&mut app.config.effects.intensity, 0.0..=1.0).step_by(0.01),
                );
            });

            ui.add_space(6.0);
            ui.checkbox(
                &mut app.config.effects.interactive,
                tr("啟用游標與特效互動（游標滑過時撥動落葉、櫻花與雨滴水波）"),
            );
        },
    );
}

/// 繪製一組 `EffectSet`（雨滴、風、落葉、櫻花）的四個核取方塊與快捷鍵。
fn show_effect_set_controls(ui: &mut egui::Ui, id_salt: &str, set: &mut EffectSet) {
    ui.push_id(id_salt, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut set.rain, tr("🌧 雨滴（Rain）"));
            ui.add_space(12.0);
            ui.checkbox(&mut set.wind, tr("🍃 風（Wind）"));
            ui.add_space(12.0);
            ui.checkbox(&mut set.leaves, tr("🍂 落葉（Leaves）"));
            ui.add_space(12.0);
            ui.checkbox(&mut set.sakura, tr("🌸 櫻花（Sakura）"));
        });

        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui.small_button(tr("全部開啟")).clicked() {
                set.rain = true;
                set.wind = true;
                set.leaves = true;
                set.sakura = true;
            }
            if ui.small_button(tr("全部關閉")).clicked() {
                set.rain = false;
                set.wind = false;
                set.leaves = false;
                set.sakura = false;
            }
        });
    });
}

// ============================================================================
// 分頁 4：效能與系統
// ============================================================================

fn language_option_label(lang: Language) -> &'static str {
    match lang {
        Language::Auto => tr("自動（依系統）"),
        Language::ZhTw => "繁體中文",
        Language::En => "English",
        Language::Ja => "日本語",
    }
}

fn show_performance_system_tab(app: &mut SettingsApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.heading(tr("效能與系統設定"));
    ui.label(
        egui::RichText::new(tr("管理畫面更新率上限、全螢幕省電暫停、Windows 開機自啟動與系統匣圖示。"))
            .size(12.5)
            .weak(),
    );
    ui.add_space(12.0);

    // 1. 效能設定
    section_card(
        ui,
        tr("圖形效能與省電"),
        tr("針對內顯（如 Intel UHD Graphics 750）最佳化，預設 30 FPS 即可流暢呈現慢速光碟與特效"),
        |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(tr("快速選擇更新率上限："));
                for fps in [15_u32, 30, 60] {
                    let label = match fps {
                        15 => tr("15 FPS（超省電）"),
                        30 => tr("30 FPS（預設推薦）"),
                        _ => tr("60 FPS（高流暢）"),
                    };
                    ui.selectable_value(&mut app.config.performance.fps_cap, fps, label);
                }
            });

            ui.horizontal_wrapped(|ui| {
                ui.label(tr("自訂更新率上限（5～120 FPS）："));
                ui.add(
                    egui::Slider::new(&mut app.config.performance.fps_cap, 5..=120).suffix(" FPS"),
                );
            });

            ui.add_space(6.0);
            ui.checkbox(
                &mut app.config.performance.pause_on_fullscreen,
                tr("全螢幕應用程式或遊戲執行時自動暫停繪製（釋放 GPU／CPU 資源）"),
            );
        },
    );

    // 2. 系統整合與總開關
    section_card(
        ui,
        tr("Windows 系統整合"),
        tr("設定開機自動啟動、系統托盤圖示與動態桌布總開關"),
        |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(tr("語言"));
                let prev_lang = app.config.language;
                egui::ComboBox::from_id_salt("language_selector")
                    .selected_text(language_option_label(app.config.language))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut app.config.language,
                            Language::Auto,
                            tr("自動（依系統）"),
                        );
                        ui.selectable_value(
                            &mut app.config.language,
                            Language::ZhTw,
                            "繁體中文",
                        );
                        ui.selectable_value(
                            &mut app.config.language,
                            Language::En,
                            "English",
                        );
                        ui.selectable_value(
                            &mut app.config.language,
                            Language::Ja,
                            "日本語",
                        );
                    });
                if app.config.language != prev_lang {
                    set_language(app.config.language);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Title(
                        tr("Nijikage-Wallpaper 設定").to_string(),
                    ));
                    ctx.request_repaint();
                }
            });
            ui.add_space(6.0);

            ui.checkbox(
                &mut app.config.enabled,
                tr("啟用動態桌布（關閉時桌布主程式將暫停繪製畫面）"),
            );
            ui.add_space(4.0);

            ui.checkbox(
                &mut app.config.system.autostart,
                tr("登入 Windows 時開機自啟動（由桌布主程式同步寫入 HKCU\\...\\Run 登錄檔）"),
            );
            ui.add_space(4.0);

            ui.checkbox(
                &mut app.config.system.tray_icon,
                tr("在 Windows 系統匣（工作列右下角托盤）顯示 Nijikage-Wallpaper 圖示"),
            );
        },
    );

    // 3. 設定檔資訊與維護
    section_card(
        ui,
        tr("設定檔資訊"),
        tr("所有設定變更皆於 300 毫秒去抖後自動寫入設定檔，桌布主程式會自動偵測並熱重載"),
        |ui| {
            let path = nijikage_wallpaper_core::config_path();
            let path_str = path.display().to_string();
            let ver_str = app.config.version.to_string();
            ui.label(
                egui::RichText::new(tr_fmt("設定檔路徑：{}", &[&path_str]))
                    .monospace()
                    .size(12.0),
            );
            ui.label(
                egui::RichText::new(tr_fmt("設定檔結構版本：v{}", &[&ver_str]))
                    .size(12.0)
                    .weak(),
            );

            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if ui.button(tr("重新從磁碟讀取")).clicked() {
                    app.reload_from_disk(ctx);
                }
                if ui.button(tr("還原預設值")).clicked() {
                    app.reset_to_defaults(ctx);
                }
            });
        },
    );
}
