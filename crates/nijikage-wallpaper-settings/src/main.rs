//! Nijikage-Wallpaper（虹影）設定視窗程式進入點與視窗框架。
//!
//! 提供類似 Windows 11「個人化」設定的繁體中文圖形介面，
//! 讀寫 `%APPDATA%\Nijikage-Wallpaper\config.json` 與桌布主程式即時同步。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod layout_preview;
mod tabs;

use app::SettingsApp;
use nijikage_wallpaper_core::{
    Config,
    i18n::{set_language, tr},
};
use eframe::egui;

fn main() -> eframe::Result<()> {
    let initial_lang = Config::try_load().map(|c| c.language).unwrap_or_default();
    set_language(initial_lang);

    let title = tr("Nijikage-Wallpaper 設定");
    let viewport = egui::ViewportBuilder::default()
        .with_title(title)
        .with_app_id("nijikage-wallpaper-settings")
        .with_inner_size([920.0, 650.0])
        .with_min_inner_size([740.0, 520.0]);

    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        title,
        native_options,
        Box::new(|cc| {
            configure_fonts(&cc.egui_ctx);
            configure_visuals(&cc.egui_ctx);
            Ok(Box::new(SettingsApp::new(cc)))
        }),
    )
}

/// 載入 Windows 系統內建的繁體中文字型與日文後備字型（Yu Gothic／Meiryo 等），若找不到則略過而不當機。
fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let tc_font_candidates = [
        r"C:\Windows\Fonts\msjh.ttc",
        r"C:\Windows\Fonts\msjh.ttf",
        r"C:\Windows\Fonts\msjhbd.ttc",
        r"C:\Windows\Fonts\mingliu.ttc",
        r"C:\Windows\Fonts\kaiu.ttf",
    ];
    let ja_font_candidates = [
        r"C:\Windows\Fonts\YuGothM.ttc",
        r"C:\Windows\Fonts\YuGothR.ttc",
        r"C:\Windows\Fonts\meiryo.ttc",
        r"C:\Windows\Fonts\msgothic.ttc",
    ];

    let mut any_loaded = false;
    let mut tc_loaded = false;

    for path in tc_font_candidates {
        if let Ok(bytes) = std::fs::read(path) {
            fonts
                .font_data
                .insert("tc_system_font".to_owned(), egui::FontData::from_owned(bytes).into());

            if let Some(proportional) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                proportional.insert(0, "tc_system_font".to_owned());
            }
            if let Some(monospace) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
                monospace.push("tc_system_font".to_owned());
            }
            any_loaded = true;
            tc_loaded = true;
            break;
        }
    }

    for path in ja_font_candidates {
        if let Ok(bytes) = std::fs::read(path) {
            fonts
                .font_data
                .insert("ja_system_font".to_owned(), egui::FontData::from_owned(bytes).into());

            if let Some(proportional) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                let idx = if tc_loaded { 1.min(proportional.len()) } else { 0 };
                proportional.insert(idx, "ja_system_font".to_owned());
            }
            if let Some(monospace) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
                monospace.push("ja_system_font".to_owned());
            }
            any_loaded = true;
            break;
        }
    }

    if any_loaded {
        ctx.set_fonts(fonts);
    }
}

/// 設定類似 Windows 個人化設定的深色現代視覺風格與元件間距。
fn configure_visuals(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    style.spacing.indent = 18.0;

    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = egui::Color32::from_rgb(24, 25, 32);
    visuals.window_fill = egui::Color32::from_rgb(28, 30, 38);
    visuals.faint_bg_color = egui::Color32::from_rgb(34, 36, 46);
    visuals.extreme_bg_color = egui::Color32::from_rgb(16, 18, 24);
    visuals.selection.bg_fill = egui::Color32::from_rgb(76, 130, 230);
    visuals.selection.stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(210, 228, 255));
    visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(32, 34, 44);
    visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(44, 47, 60);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(58, 63, 80);
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(72, 118, 210);

    style.visuals = visuals;
    ctx.set_style(style);
}
