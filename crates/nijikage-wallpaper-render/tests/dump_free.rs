//! 執行：cargo test -p nijikage-wallpaper-render --test dump_free -- --nocapture
//! 輸出 target/v03-dump：四種縮放、中心、音訊環、27 組桌面光學與粒子對照。
//! 光學常數見 src/scene.wgsl 開頭；粒子上限、音訊平滑／段數見 src/lib.rs。
mod support;
use nijikage_wallpaper_core::{BackgroundMode, LayoutMode};
use nijikage_wallpaper_render::AudioInput;
use support::*;

#[test]
fn dump_v03_optics() -> TestResult {
    pollster::block_on(async {
        let gpu = Gpu::new().await?;
        let out = folder("v03-dump")?;
        let art = standard_art()?;
        let size = [1280, 720];
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let mut cfg = disc_config();
        cfg.disc.art_path = Some(art.clone());
        cfg.disc.rotate = false;
        let mut zooms = Vec::new();
        for zoom in [1.0, 0.6, 0.4, 0.25] {
            cfg.disc.free.art_zoom = zoom;
            let mut c = Capture::new(&gpu, &cfg, size, format);
            c.step(AudioInput::default());
            zooms.push(c.save(out.join(format!("disc-zoom-{zoom:.2}.png")))?);
        }
        sheet(&zooms, 2, [640, 360], &out.join("disc-zoom-overview.png"))?;
        // 原生中心裁切 160×160，再以最近鄰放大到 800×800，避免插值藏住黑點。
        // 分別有圖／裸盤，時間與游標改變，檢查收斂不只剛好位於單一相位。
        let mut centers = Vec::new();
        cfg.disc.free.art_zoom = 1.0;
        for with_art in [true, false] {
            cfg.disc.art_path = with_art.then(|| art.clone());
            cfg.disc.rotate = true;
            for (i, cursor) in [None, Some([641.0, 360.0]), Some([500.0, 200.0])]
                .into_iter()
                .enumerate()
            {
                let mut c = Capture::new(&gpu, &cfg, size, format);
                c.input.time = i as f32 * 7.0;
                c.input.cursor = cursor;
                c.settle(AudioInput::default(), 30);
                let image = c.save(out.join(format!("center-source-{with_art}-{i}.png")))?;
                let crop = image::imageops::crop_imm(&image, 560, 280, 160, 160).to_image();
                let enlarged =
                    image::imageops::resize(&crop, 800, 800, image::imageops::FilterType::Nearest);
                enlarged.save(out.join(format!("center-crop-{with_art}-{i}.png")))?;
                centers.push(enlarged);
            }
        }
        sheet(&centers, 3, [400, 400], &out.join("center-overview.png"))?;

        cfg.disc.art_path = Some(art.clone());
        cfg.disc.rotate = false;
        let mut rings = Vec::new();
        // 總覽：靜音、均勻 0.3、均勻 0.8、低頻、高頻、交錯頻帶；對側亮度須相同。
        let distributions = [
            ("silent", 0.0, [0.0; 16]),
            ("level-03", 0.3, [0.5; 16]),
            ("level-08", 0.8, [0.5; 16]),
            (
                "bass",
                0.8,
                std::array::from_fn(|i| if i < 4 { 1.0 } else { 0.05 }),
            ),
            (
                "treble",
                0.8,
                std::array::from_fn(|i| if i >= 12 { 1.0 } else { 0.05 }),
            ),
            (
                "alternating",
                0.8,
                std::array::from_fn(|i| if i % 2 == 0 { 0.9 } else { 0.1 }),
            ),
        ];
        for (name, level, bands) in distributions {
            let mut c = Capture::new(&gpu, &cfg, size, format);
            c.settle(AudioInput { level, bands }, 20);
            rings.push(c.save(out.join(format!("audio-{name}.png")))?);
        }
        sheet(&rings, 3, [640, 360], &out.join("audio-overview.png"))?;
        let mut c = Capture::new(&gpu, &cfg, size, format);
        c.settle(
            AudioInput {
                level: 0.8,
                bands: [0.8; 16],
            },
            20,
        );
        c.settle(AudioInput::default(), 8);
        c.save(out.join("audio-release-027s.png"))?;
        c.settle(AudioInput::default(), 9);
        c.save(out.join("audio-release-057s.png"))?;
        c.settle(
            AudioInput {
                level: 0.8,
                bands: [0.8; 16],
            },
            20,
        );
        cfg.audio_ring.enabled = false;
        c.config(&cfg);
        c.step(AudioInput {
            level: 1.0,
            bands: [1.0; 16],
        });
        c.save(out.join("audio-disabled.png"))?;
        cfg.audio_ring.enabled = true;

        cfg.disc.layout_mode = LayoutMode::Desktop;
        cfg.background.mode = BackgroundMode::Image;
        cfg.background.image_path = Some(art.clone());
        cfg.background.fit = nijikage_wallpaper_core::FitMode::Fill;
        cfg.disc.rotate = true;
        cfg.desktop_fx.intensity = 0.0;
        let mut baseline = Capture::new(&gpu, &cfg, size, format);
        baseline.step(AudioInput::default());
        let desktop_original = baseline.pixels()?;
        let mut desktop = Vec::new();
        // 27 張總覽：每列三個游標（左上、中央、右下）；先 intensity，再 scale 排列。
        // 每個 intensity 另輸出 3×3，列為 scale 0.5／1／2，欄為游標位置。
        for intensity in [0.3, 0.7, 1.0] {
            let mut group = Vec::new();
            for scale in [0.5, 1.0, 2.0] {
                for (i, cursor) in [[200.0, 160.0], [640.0, 360.0], [1080.0, 560.0]]
                    .into_iter()
                    .enumerate()
                {
                    cfg.desktop_fx.intensity = intensity;
                    cfg.desktop_fx.pattern_scale = scale;
                    let mut c = Capture::new(&gpu, &cfg, size, format);
                    c.input.cursor = Some(cursor);
                    c.settle(AudioInput::default(), 40);
                    let img = c.save(
                        out.join(format!("desktop-i{intensity:.1}-s{scale:.1}-cursor{i}.png")),
                    )?;
                    diff_heatmap(&desktop_original, &img).save(out.join(format!(
                        "desktop-diff-i{intensity:.1}-s{scale:.1}-cursor{i}.png"
                    )))?;
                    group.push(img.clone());
                    desktop.push(img);
                }
            }
            sheet(
                &group,
                3,
                [480, 270],
                &out.join(format!("desktop-i{intensity:.1}-overview.png")),
            )?;
        }
        sheet(
            &desktop,
            3,
            [480, 270],
            &out.join("desktop-all-overview.png"),
        )?;
        cfg.desktop_fx.intensity = 0.0;
        let mut c = Capture::new(&gpu, &cfg, size, format);
        c.step(AudioInput::default());
        c.save(out.join("desktop-original-intensity0.png"))?;

        // 同一模式同一時間的粒子開關對照；光碟用雨滴，桌面用櫻花。
        let mut particles = Vec::new();
        for mode in [LayoutMode::Free, LayoutMode::Desktop] {
            cfg.disc.layout_mode = mode;
            cfg.disc.rotate = false;
            cfg.desktop_fx.intensity = 0.7;
            for active in [false, true] {
                cfg.effects.on_disc.rain = active;
                cfg.effects.on_desktop.sakura = active && mode == LayoutMode::Desktop;
                let mut c = Capture::new(&gpu, &cfg, size, format);
                c.input.cursor = Some([580.0, 320.0]);
                c.settle(AudioInput::default(), 60);
                particles.push(c.save(out.join(format!("particles-{mode:?}-{active}.png")))?);
            }
        }
        sheet(
            &particles,
            2,
            [640, 360],
            &out.join("particles-overview.png"),
        )?;
        Ok(())
    })
}
