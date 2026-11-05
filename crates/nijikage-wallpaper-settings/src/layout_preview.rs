//! 光碟模式（`LayoutMode::Free`）的 16:9 螢幕版面預覽、CPU 光柵化光碟四邊形貼圖與滑鼠互動控制，
//! 以及全桌面模式（`LayoutMode::Desktop`）的斜向光學光帶漸層示意繪製。
//!
//! 實作規則：
//! - 光碟中心沒有洞，預覽不畫中心孔與內環。
//! - 光碟盤面與自訂圖片已在 `SettingsApp::sync_previews` 中以 CPU 光柵化為 320×320 RGBA 貼圖
//!   （圓外 alpha=0、圓內雙線性取樣未預先裁切的色調圖片，`zoom=1` 時短邊＝光碟直徑），
//!   此處直接以單張貼圖四邊形（`painter.image`）繪製於預覽區，縮小時邊緣不拖影。
//! - 光碟超出 16:9 螢幕矩形的部分以淡色顯示於外框區域；螢幕內則以正常不透明度顯示。
//! - 互動支援（所有拖曳皆須從光碟上開始拖曳才有效）：
//!   - 拖曳光碟本體（不按 Shift／Alt）＝移動光碟圓心（`center_x`, `center_y`）
//!   - 按住 Shift + 拖曳＝移動圖片位置（`art_offset_x`, `art_offset_y`）
//!   - 按住 Alt + 拖曳＝旋轉圖片角度（`art_rotation_deg`）
//!   - 滾輪（游標在光碟上，同時讀取 `x + y` 並歸零以支援 Windows Shift+滾輪）：
//!     - 不按鍵＝縮放光碟直徑（`diameter`）
//!     - 按住 Shift + 滾輪＝縮放圖片（`art_zoom`：0.2～4.0）
//!     - 按住 Alt + 滾輪＝旋轉圖片角度（`art_rotation_deg`）

use crate::{app::SettingsApp, tabs::compute_fit_preview_rect};
use nijikage_wallpaper_core::{BackgroundMode, DiscStyle, FreeLayout, i18n::tr};
use eframe::egui;

/// 繪製光碟模式的完整版面編輯介面（16:9 螢幕＋出界預覽畫布、操作提示、快捷還原按鈕與數字微調欄位）。
pub fn show_free_layout_editor(app: &mut SettingsApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    // 1. 計算預覽畫布尺寸：中央為嚴格 16:9 的螢幕框，四周留出淡色出界預覽區
    let avail_w = ui.available_width().max(320.0);
    let canvas_w = avail_w.min(640.0);
    let screen_w = (canvas_w * 0.72).max(220.0);
    let screen_h = screen_w * (9.0 / 16.0);
    let pad_y = (screen_h * 0.24).clamp(36.0, 68.0);
    let canvas_h = screen_h + pad_y * 2.0;

    let (outer_rect, response) =
        ui.allocate_exact_size(egui::vec2(canvas_w, canvas_h), egui::Sense::click_and_drag());
    let screen_rect =
        egui::Rect::from_center_size(outer_rect.center(), egui::vec2(screen_w, screen_h));

    // 2. 依目前 `disc.free` 換算螢幕預覽座標系中的光碟圓心與半徑
    let sw = screen_rect.width().max(1.0);
    let sh = screen_rect.height().max(1.0);
    let disc_cx = screen_rect.left() + app.config.disc.free.center_x * sw;
    let disc_cy = screen_rect.top() + app.config.disc.free.center_y * sh;
    let disc_center = egui::pos2(disc_cx, disc_cy);
    let disc_diameter = (app.config.disc.free.diameter * sh).max(4.0);
    let disc_radius = disc_diameter * 0.5;

    // 3. 處理滑鼠拖曳與滾輪互動（所有拖曳包含 Alt 旋轉皆須從光碟範圍內開始）
    let modifiers = ctx.input(|i| i.modifiers);
    let drag_id = ui.id().with("free_layout_disc_drag_active");

    if response.drag_started_by(egui::PointerButton::Primary) {
        let started_on_disc = response
            .interact_pointer_pos()
            .map(|p| p.distance(disc_center) <= disc_radius.max(18.0))
            .unwrap_or(false);
        ctx.data_mut(|d| d.insert_temp(drag_id, started_on_disc));
    }

    let drag_started_on_disc = ctx
        .data_mut(|d| d.get_temp::<bool>(drag_id))
        .unwrap_or(false);

    if response.drag_stopped() {
        ctx.data_mut(|d| d.insert_temp(drag_id, false));
    }

    if response.dragged_by(egui::PointerButton::Primary) && drag_started_on_disc {
        let delta = response.drag_delta();
        if delta != egui::Vec2::ZERO {
            if modifiers.alt {
                // 按住 Alt 拖曳（從光碟上開始）：旋轉光碟圖片角度
                if let Some(pos) = response.interact_pointer_pos() {
                    let prev = pos - delta;
                    if prev.distance(disc_center) > 6.0 && pos.distance(disc_center) > 6.0 {
                        let a0 = (prev.y - disc_center.y).atan2(prev.x - disc_center.x);
                        let a1 = (pos.y - disc_center.y).atan2(pos.x - disc_center.x);
                        let mut da = (a1 - a0).to_degrees();
                        if da > 180.0 {
                            da -= 360.0;
                        } else if da < -180.0 {
                            da += 360.0;
                        }
                        app.config.disc.free.art_rotation_deg =
                            wrap_deg_180(app.config.disc.free.art_rotation_deg + da);
                    } else {
                        app.config.disc.free.art_rotation_deg =
                            wrap_deg_180(app.config.disc.free.art_rotation_deg + delta.x * 0.6);
                    }
                }
                app.config.sanitize();
            } else if modifiers.shift {
                // 按住 Shift 拖曳（從光碟上開始）：移動圖片位置（單位為光碟直徑）
                app.config.disc.free.art_offset_x += delta.x / disc_diameter;
                app.config.disc.free.art_offset_y += delta.y / disc_diameter;
                app.config.sanitize();
            } else {
                // 不按修飾鍵拖曳光碟本體：移動光碟圓心
                app.config.disc.free.center_x += delta.x / sw;
                app.config.disc.free.center_y += delta.y / sh;
                app.config.sanitize();
            }
        }
    }

    // 游標在光碟上時的滾輪互動與游標樣式
    let pointer_on_disc = response
        .hover_pos()
        .map(|p| p.distance(disc_center) <= disc_radius.max(18.0))
        .unwrap_or(false);

    if response.dragged_by(egui::PointerButton::Primary) && drag_started_on_disc {
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if pointer_on_disc {
        ctx.set_cursor_icon(egui::CursorIcon::Grab);
    }

    if response.hovered() && pointer_on_disc {
        // Windows 上按住 Shift 滾輪在 egui 會將垂直量轉為水平量，因此同時讀取 x + y 並將兩者清為 0
        let scroll_delta = ctx.input_mut(|i| {
            let smooth = i.smooth_scroll_delta.x + i.smooth_scroll_delta.y;
            let raw = i.raw_scroll_delta.x + i.raw_scroll_delta.y;
            let d = if smooth.abs() > raw.abs() { smooth } else { raw };
            i.smooth_scroll_delta = egui::Vec2::ZERO;
            i.raw_scroll_delta = egui::Vec2::ZERO;
            d
        });

        if scroll_delta.abs() > 0.01 {
            let steps = (scroll_delta / 40.0).clamp(-3.0, 3.0);
            if modifiers.alt {
                app.config.disc.free.art_rotation_deg =
                    wrap_deg_180(app.config.disc.free.art_rotation_deg + steps * 5.0);
            } else if modifiers.shift {
                app.config.disc.free.art_zoom =
                    (app.config.disc.free.art_zoom + steps * 0.08).clamp(0.2, 4.0);
            } else {
                app.config.disc.free.diameter =
                    (app.config.disc.free.diameter + steps * 0.05).clamp(0.2, 3.0);
            }
            app.config.sanitize();
        }
    }

    // 若拖曳或滾輪改變了圖片參數，立即同步 320×320 CPU 光柵化貼圖使畫面零延遲更新
    app.sync_previews(ctx);

    // 重新計算互動更新後的圓心與半徑以供繪製
    let disc_cx = screen_rect.left() + app.config.disc.free.center_x * sw;
    let disc_cy = screen_rect.top() + app.config.disc.free.center_y * sh;
    let disc_center = egui::pos2(disc_cx, disc_cy);
    let disc_diameter = (app.config.disc.free.diameter * sh).max(4.0);
    let disc_radius = disc_diameter * 0.5;

    // 4. 繪製預覽畫布
    let outer_painter = ui.painter().with_clip_rect(outer_rect);

    // (a) 外圍出界預覽區底色與淡色光碟
    outer_painter.rect_filled(outer_rect, 6.0, egui::Color32::from_rgb(20, 22, 30));
    outer_painter.rect_stroke(
        outer_rect,
        6.0,
        egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(46, 50, 66)),
        egui::StrokeKind::Inside,
    );
    outer_painter.text(
        outer_rect.left_top() + egui::vec2(8.0, 6.0),
        egui::Align2::LEFT_TOP,
        tr("畫面外區域（淡色顯示光碟出界範圍）"),
        egui::FontId::proportional(11.0),
        egui::Color32::from_rgb(115, 122, 145),
    );

    // 在整個 outer_rect 畫出淡色版光碟四邊形貼圖（超出 16:9 螢幕的部分會保留此淡色外觀）
    draw_disc_quad(&outer_painter, app, disc_center, disc_radius, 0.28);

    // (b) 16:9 螢幕矩形內部：先鋪不透明背景純色與背景圖，再畫完整不透明度的光碟四邊形貼圖
    let screen_painter = ui.painter().with_clip_rect(screen_rect);
    let [bg_r, bg_g, bg_b] = app.config.background.color;
    let bg_color = egui::Color32::from_rgb(bg_r, bg_g, bg_b);
    screen_painter.rect_filled(screen_rect, 0.0, bg_color);

    if app.config.background.mode == BackgroundMode::Image {
        if let Some(bg_tex) = &app.bg_texture {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            let target_rect = compute_fit_preview_rect(
                screen_rect,
                bg_tex.size_vec2(),
                app.config.background.fit,
            );
            screen_painter.image(bg_tex.id(), target_rect, uv, egui::Color32::WHITE);
        }
    }

    draw_disc_quad(&screen_painter, app, disc_center, disc_radius, 1.0);

    // (c) 繪製 16:9 螢幕邊框、圓心十字準星與標籤
    outer_painter.rect_stroke(
        screen_rect,
        0.0,
        egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(130, 165, 230)),
        egui::StrokeKind::Inside,
    );
    outer_painter.text(
        screen_rect.left_top() + egui::vec2(6.0, 4.0),
        egui::Align2::LEFT_TOP,
        tr("16:9 螢幕範圍"),
        egui::FontId::proportional(11.0),
        egui::Color32::from_rgba_unmultiplied(210, 225, 250, 200),
    );

    // 繪製光碟圓心微細十字標記（光碟中心無洞，僅以輕量準星協助定位圓心）
    let cross_half = 4.5_f32;
    let cross_stroke = egui::Stroke::new(
        1.2_f32,
        egui::Color32::from_rgba_unmultiplied(235, 242, 255, 185),
    );
    outer_painter.line_segment(
        [
            disc_center - egui::vec2(cross_half, 0.0),
            disc_center + egui::vec2(cross_half, 0.0),
        ],
        cross_stroke,
    );
    outer_painter.line_segment(
        [
            disc_center - egui::vec2(0.0, cross_half),
            disc_center + egui::vec2(0.0, cross_half),
        ],
        cross_stroke,
    );

    ui.add_space(6.0);

    // 5. 預覽下方的單行小字操作說明
    ui.label(
        egui::RichText::new(tr(
            "💡 操作說明：拖曳光碟＝移動圓心｜滾輪（游標在光碟上）＝縮放直徑｜Shift + 拖曳＝移動圖片｜Shift + 滾輪＝縮放圖片（0.2～4.0）｜Alt + 拖曳或滾輪＝旋轉圖片",
        ))
        .size(11.5)
        .weak(),
    );

    ui.add_space(8.0);

    // 6. 快捷還原按鈕列（「光碟置中」、「圖片還原」與「預設靠右位置」）
    ui.horizontal_wrapped(|ui| {
        if ui
            .button(tr("🎯 光碟置中"))
            .on_hover_text(tr("將光碟圓心移至畫面正中央 (center_x = 0.5, center_y = 0.5)"))
            .clicked()
        {
            app.config.disc.free.center_x = 0.5;
            app.config.disc.free.center_y = 0.5;
            app.config.sanitize();
        }

        if ui
            .button(tr("🔄 圖片還原"))
            .on_hover_text(tr("將圖片位移歸零、縮放還原為 1.0×、旋轉角度歸零"))
            .clicked()
        {
            app.config.disc.free.art_offset_x = 0.0;
            app.config.disc.free.art_offset_y = 0.0;
            app.config.disc.free.art_zoom = 1.0;
            app.config.disc.free.art_rotation_deg = 0.0;
            app.config.sanitize();
        }

        if ui
            .button(tr("📐 預設靠右位置"))
            .on_hover_text(tr(
                "將光碟位置與直徑恢復為預設靠右版面 (center_x = 0.92, center_y = 0.5, diameter = 0.95)",
            ))
            .clicked()
        {
            let def = FreeLayout::default();
            app.config.disc.free.center_x = def.center_x;
            app.config.disc.free.center_y = def.center_y;
            app.config.disc.free.diameter = def.diameter;
            app.config.sanitize();
        }
    });

    ui.add_space(8.0);

    // 7. 數字欄位與滑桿微調區（範圍皆與 Config::sanitize() 嚴格一致）
    egui::Grid::new("free_layout_numeric_grid")
        .num_columns(2)
        .spacing([16.0, 8.0])
        .show(ui, |ui| {
            ui.label(tr("圓心水平位置 X（0=左緣, 1=右緣, -0.5～1.5）："));
            ui.add(
                egui::Slider::new(&mut app.config.disc.free.center_x, -0.5..=1.5).step_by(0.01),
            );
            ui.end_row();

            ui.label(tr("圓心垂直位置 Y（0=上緣, 1=下緣, -0.5～1.5）："));
            ui.add(
                egui::Slider::new(&mut app.config.disc.free.center_y, -0.5..=1.5).step_by(0.01),
            );
            ui.end_row();

            ui.label(tr("光碟直徑（占畫面高度比例 0.2～3.0）："));
            ui.add(
                egui::Slider::new(&mut app.config.disc.free.diameter, 0.2..=3.0)
                    .step_by(0.01)
                    .suffix(" ×"),
            );
            ui.end_row();

            ui.label(tr("圖片水平位移 X（光碟直徑比例 -1.0～1.0）："));
            ui.add(
                egui::Slider::new(&mut app.config.disc.free.art_offset_x, -1.0..=1.0)
                    .step_by(0.01),
            );
            ui.end_row();

            ui.label(tr("圖片垂直位移 Y（光碟直徑比例 -1.0～1.0）："));
            ui.add(
                egui::Slider::new(&mut app.config.disc.free.art_offset_y, -1.0..=1.0)
                    .step_by(0.01),
            );
            ui.end_row();

            ui.label(tr("圖片縮放倍率（1.0=短邊等於光碟直徑, 0.2～4.0）："));
            ui.add(
                egui::Slider::new(&mut app.config.disc.free.art_zoom, 0.2..=4.0)
                    .step_by(0.01)
                    .suffix(" ×"),
            );
            ui.end_row();

            ui.label(tr("圖片旋轉角度（-180°～180°，不隨光碟轉動）："));
            ui.add(
                egui::Slider::new(&mut app.config.disc.free.art_rotation_deg, -180.0..=180.0)
                    .step_by(0.5)
                    .suffix("°"),
            );
            ui.end_row();
        });
}

/// 使用一張 CPU 光柵化的 320×320 RGBA 貼圖四邊形繪製光碟（中心無洞，圓外 alpha=0）。
/// `alpha_mul` 在螢幕內為 `1.0`，在螢幕外為淡色比例（如 `0.28`）。
pub fn draw_disc_quad(
    painter: &egui::Painter,
    app: &SettingsApp,
    center: egui::Pos2,
    radius: f32,
    alpha_mul: f32,
) {
    if radius <= 0.5 {
        return;
    }

    let disc_rect = egui::Rect::from_center_size(center, egui::vec2(radius * 2.0, radius * 2.0));
    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    let tint = scale_alpha(egui::Color32::WHITE, alpha_mul);

    if let Some(tex) = &app.disc_raster_texture {
        painter.image(tex.id(), disc_rect, uv, tint);
    } else {
        let fallback = match app.config.disc.style {
            DiscStyle::Cd => scale_alpha(egui::Color32::from_rgb(190, 202, 218), alpha_mul),
            DiscStyle::Dvd => scale_alpha(egui::Color32::from_rgb(174, 118, 178), alpha_mul),
            DiscStyle::Bd => scale_alpha(egui::Color32::from_rgb(92, 122, 212), alpha_mul),
        };
        painter.circle_filled(center, radius, fallback);
    }

    // 光碟最外圈邊界細線
    let line_color = scale_alpha(egui::Color32::from_rgb(235, 242, 255), alpha_mul * 0.55);
    painter.circle_stroke(center, radius, egui::Stroke::new(1.1_f32, line_color));
}

/// 在 16:9 預覽區上蓋一層示意用的斜向光帶漸層（供全桌面模式 `LayoutMode::Desktop` 預覽使用）。
pub fn draw_desktop_fx_overlay(
    painter: &egui::Painter,
    rect: egui::Rect,
    intensity: f32,
    pattern_scale: f32,
    style: DiscStyle,
) {
    let eff = intensity.clamp(0.0, 1.0);
    if eff <= 0.005 || rect.width() <= 1.0 || rect.height() <= 1.0 {
        return;
    }

    let scale = pattern_scale.clamp(0.5, 3.0);
    let cols = 28_usize;
    let rows = 16_usize;
    let mut mesh = egui::Mesh::default();
    mesh.vertices.reserve((cols + 1) * (rows + 1));
    mesh.indices.reserve(cols * rows * 6);

    let (c_primary, c_secondary, c_accent) = match style {
        DiscStyle::Cd => (
            [185.0_f32, 230.0, 255.0],
            [255.0_f32, 195.0, 235.0],
            [190.0_f32, 255.0, 220.0],
        ),
        DiscStyle::Dvd => (
            [255.0_f32, 210.0, 125.0],
            [215.0_f32, 145.0, 245.0],
            [255.0_f32, 175.0, 195.0],
        ),
        DiscStyle::Bd => (
            [130.0_f32, 185.0, 255.0],
            [175.0_f32, 140.0, 255.0],
            [120.0_f32, 235.0, 255.0],
        ),
    };

    let freq = 3.2 / scale;
    for r in 0..=rows {
        let ty = r as f32 / rows as f32;
        let y = rect.top() + rect.height() * ty;
        for c in 0..=cols {
            let tx = c as f32 / cols as f32;
            let x = rect.left() + rect.width() * tx;

            // 斜向座標組合，營造斜向掃過的光帶與彩虹繞射漸層感
            let diag = (tx * 1.15 + ty * 0.85) - 1.0;
            let cross = tx * 0.7 - ty * 0.9;
            let band1 = (diag * freq * std::f32::consts::PI).cos() * 0.5 + 0.5;
            let band2 = ((diag * 1.7 - 0.35) * freq * std::f32::consts::PI).sin() * 0.5 + 0.5;
            let sheen = (1.0 - (diag * 1.35).abs().clamp(0.0, 1.0)).powf(1.8);
            let wave = (cross * 2.5).sin() * 0.5 + 0.5;

            let w1 = band1.powf(2.0);
            let w2 = band2.powf(2.2);
            let cr = c_primary[0] * w1 * 0.55 + c_secondary[0] * w2 * 0.45 + c_accent[0] * sheen * 0.25;
            let cg = c_primary[1] * w1 * 0.55 + c_secondary[1] * w2 * 0.45 + c_accent[1] * sheen * 0.25;
            let cb = c_primary[2] * w1 * 0.55 + c_secondary[2] * w2 * 0.45 + c_accent[2] * sheen * 0.25;

            let band_strength = (w1 * 0.48 + w2 * 0.32 + sheen * 0.55) * (0.75 + 0.25 * wave);
            let alpha = (band_strength * eff * 115.0).round().clamp(0.0, 150.0) as u8;

            mesh.vertices.push(egui::epaint::Vertex {
                pos: egui::pos2(x, y),
                uv: egui::epaint::WHITE_UV,
                color: egui::Color32::from_rgba_unmultiplied(
                    cr.clamp(0.0, 255.0) as u8,
                    cg.clamp(0.0, 255.0) as u8,
                    cb.clamp(0.0, 255.0) as u8,
                    alpha,
                ),
            });
        }
    }

    let stride = cols + 1;
    for r in 0..rows {
        for c in 0..cols {
            let i0 = (r * stride + c) as u32;
            let i1 = i0 + 1;
            let i2 = ((r + 1) * stride + c) as u32;
            let i3 = i2 + 1;
            mesh.indices.extend_from_slice(&[i0, i2, i3, i0, i3, i1]);
        }
    }

    painter.with_clip_rect(rect).add(egui::Shape::mesh(mesh));
}

/// 將角度正規化至 `[-180.0, 180.0]` 範圍內。
fn wrap_deg_180(deg: f32) -> f32 {
    if !deg.is_finite() {
        return 0.0;
    }
    let mut d = (deg + 180.0).rem_euclid(360.0) - 180.0;
    if d == -180.0 && deg > 0.0 {
        d = 180.0;
    }
    d.clamp(-180.0, 180.0)
}

/// 依比例縮放 `egui::Color32` 的不透明度。
pub fn scale_alpha(color: egui::Color32, factor: f32) -> egui::Color32 {
    let f = factor.clamp(0.0, 1.0);
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    let new_a = ((a as f32) * f).round().clamp(0.0, 255.0) as u8;
    egui::Color32::from_rgba_unmultiplied(r, g, b, new_a)
}
