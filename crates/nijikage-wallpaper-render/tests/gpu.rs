//! v0.3 像素回歸；實機執行 cargo test -p nijikage-wallpaper-render --test gpu -- --nocapture。
//! 所有比較組均另存 PNG 到 target/v03-regression，方便定位實機差異。
mod support;
use nijikage_wallpaper_core::{BackgroundMode, DiscStyle, EffectSet, FitMode, LayoutMode};
use nijikage_wallpaper_render::AudioInput;
use support::*;

// 獨立計算原圖的線性雙線性取樣與三種盤色，能抓出「先裁正方形」造成的內容錯誤。
fn expected_art(image: &image::RgbaImage, uv: [f32; 2], style: DiscStyle) -> [u8; 3] {
    let linear = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let x = uv[0] * image.width() as f32 - 0.5;
    let y = uv[1] * image.height() as f32 - 0.5;
    let (base, shadow) = match style {
        DiscStyle::Cd => ([0.53, 0.57, 0.62], [0.19, 0.22, 0.27]),
        DiscStyle::Dvd => ([0.59, 0.43, 0.31], [0.26, 0.12, 0.25]),
        DiscStyle::Bd => ([0.26, 0.38, 0.63], [0.12, 0.14, 0.29]),
    };
    let art: [f32; 3] = std::array::from_fn(|channel| {
        let sample = |dx, dy| {
            linear(
                image
                    .get_pixel(x.floor() as u32 + dx, y.floor() as u32 + dy)
                    .0[channel],
            )
        };
        let top = sample(0, 0) * (1.0 - x.fract()) + sample(1, 0) * x.fract();
        let bottom = sample(0, 1) * (1.0 - x.fract()) + sample(1, 1) * x.fract();
        top * (1.0 - y.fract()) + bottom * y.fract()
    });
    let luminance = art[0] * 0.2126 + art[1] * 0.7152 + art[2] * 0.0722;
    std::array::from_fn(|i| {
        let weight = luminance.powf(0.8);
        let toned = shadow[i] * 0.12 * (1.0 - weight) + base[i] * 1.32 * weight;
        let v = toned * 0.78 + art[i] * base[i] * 0.38;
        let encoded = if v <= 0.0031308 {
            v * 12.92
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        };
        (encoded * 255.0).round() as u8
    })
}

#[test]
fn gpu_render_regression() -> TestResult {
    pollster::block_on(async {
        let gpu = Gpu::new().await?;
        let out = folder("v03-regression")?;
        let size = [512, 288];
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let silence = AudioInput::default();
        let mut cfg = disc_config();
        cfg.disc.rotate = false;
        cfg.disc.reflection = false;
        // 檢查自由版面、多種螢幕比例與超出畫面的圓心，中心不再是背景孔洞。
        for dimensions in [size, [513, 289], [320, 400], [640, 240]] {
            for (cx, cy, diameter) in [(0.5, 0.5, 0.8), (-0.2, 0.5, 1.5), (1.2, 0.0, 2.0)] {
                cfg.disc.free.center_x = cx;
                cfg.disc.free.center_y = cy;
                cfg.disc.free.diameter = diameter;
                let mut c = Capture::new(&gpu, &cfg, dimensions, format);
                c.step(silence);
                let image = c.save(out.join(format!(
                    "geometry-{}x{}-{cx}-{cy}-{diameter}.png",
                    dimensions[0], dimensions[1]
                )))?;
                let radius = dimensions[1] as f32 * diameter * 0.5;
                for (x, y, pixel) in image.enumerate_pixels() {
                    let r = (x as f32 + 0.5 - dimensions[0] as f32 * cx)
                        .hypot(y as f32 + 0.5 - dimensions[1] as f32 * cy);
                    if (r - radius).abs() < 3.0 {
                        continue;
                    }
                    assert_eq!(
                        &pixel.0[..3] == cfg.background.color,
                        r > radius,
                        "光碟遮罩錯誤：{dimensions:?} ({x},{y})"
                    );
                }
            }
        }
        cfg = disc_config();
        cfg.disc.rotate = false;
        cfg.disc.reflection = false;
        let mut free = Capture::new(&gpu, &cfg, size, format);
        free.step(silence);
        let reference = free.save(out.join("free-reference.png"))?;
        // 舊模式唯一相容行為：與同一組自由版面完全相同。
        cfg.disc.layout_mode = LayoutMode::Simple;
        let mut old = Capture::new(&gpu, &cfg, size, format);
        old.step(silence);
        let old_image = old.save(out.join("old-config-as-free.png"))?;
        assert_eq!(reference, old_image);
        cfg.disc.layout_mode = LayoutMode::Free;
        let mut unorm = Capture::new(&gpu, &cfg, size, wgpu::TextureFormat::Rgba8Unorm);
        unorm.step(silence);
        let image = unorm.save(out.join("color-unorm.png"))?;
        assert!(
            reference
                .as_raw()
                .iter()
                .zip(image.as_raw())
                .all(|(a, b)| a.abs_diff(*b) <= 1)
        );

        // 三種比例的完整原圖。座標遮罩獨立以 CPU 反算，檢查沒有越界拖影／平鋪。
        for (w, h) in [(64, 112), (112, 64), (64, 64)] {
            let fixture = out.join(format!("fixture-{w}x{h}.png"));
            image::RgbaImage::from_fn(w, h, |x, y| {
                image::Rgba([(30 + x * 180 / w) as u8, (25 + y * 200 / h) as u8, 110, 255])
            })
            .save(&fixture)?;
            let source = image::open(&fixture)?.to_rgba8();
            for style in [DiscStyle::Cd, DiscStyle::Dvd, DiscStyle::Bd] {
                cfg.disc.style = style;
                cfg.disc.art_path = None;
                let mut c = Capture::new(&gpu, &cfg, size, format);
                c.step(silence);
                let bare = c.pixels()?;
                cfg.disc.art_path = Some(fixture.clone());
                for (zoom, rotation, ox, oy) in [
                    (1.0_f32, 0.0_f32, 0.0, 0.0),
                    (0.6, 0.0, 0.0, 0.0),
                    (0.4, 0.0, 0.0, 0.0),
                    (0.25, 0.0, 0.0, 0.0),
                    (4.0, 0.0, 0.0, 0.0),
                    (0.6, 37.0, 0.2, -0.1),
                    (1.0, -90.0, -0.15, 0.1),
                ] {
                    cfg.disc.free.art_zoom = zoom;
                    cfg.disc.free.art_rotation_deg = rotation;
                    cfg.disc.free.art_offset_x = ox;
                    cfg.disc.free.art_offset_y = oy;
                    c.config(&cfg);
                    c.step(silence);
                    let image =
                        c.save(out.join(format!("art-{w}x{h}-{style:?}-z{zoom}-r{rotation}.png")))?;
                    assert_ne!(image, bare);
                    let (s, co) = rotation.to_radians().sin_cos();
                    let radius = cfg.disc.free.diameter * size[1] as f32 * 0.5;
                    let aspect = [w as f32 / w.min(h) as f32, h as f32 / w.min(h) as f32];
                    for (x, y, pixel) in image.enumerate_pixels() {
                        let p = [
                            (x as f32 + 0.5 - 256.0) / (2.0 * radius),
                            (y as f32 + 0.5 - 144.0) / (2.0 * radius),
                        ];
                        let q = [p[0] - ox, p[1] - oy];
                        let uv = [
                            (co * q[0] + s * q[1]) / (zoom * aspect[0]) + 0.5,
                            (-s * q[0] + co * q[1]) / (zoom * aspect[1]) + 0.5,
                        ];
                        if uv.iter().any(|v| *v < -0.02 || *v > 1.02) || p[0].hypot(p[1]) > 0.505 {
                            assert_eq!(pixel, bare.get_pixel(x, y), "越界圖片污染盤面 ({x},{y})");
                        }
                        if x % 43 == 0
                            && y % 37 == 0
                            && p[0].hypot(p[1]) < 0.46
                            && uv.iter().all(|v| (0.02..0.98).contains(v))
                        {
                            let expected = expected_art(&source, uv, style);
                            assert!(
                                pixel.0[..3]
                                    .iter()
                                    .zip(expected)
                                    .all(|(a, b)| a.abs_diff(b) <= 2),
                                "完整原圖內容取樣錯誤：{w}x{h} ({x},{y})，uv={uv:?}"
                            );
                        }
                    }
                    let fixed = image;
                    cfg.disc.rotate = true;
                    c.config(&cfg);
                    c.input.time += 11.0;
                    c.step(silence);
                    assert_eq!(fixed, c.pixels()?, "關閉反射後，圖片不可隨光碟旋轉");
                    cfg.disc.rotate = false;
                }
                cfg.disc.free = disc_config().disc.free;
            }
        }
        // 音訊只有外緣改變；0.5 秒後與靜音逐位元組相同，停用忽略新輸入。
        cfg = disc_config();
        cfg.disc.rotate = false;
        let mut c = Capture::new(&gpu, &cfg, size, format);
        c.step(silence);
        let silent = c.save(out.join("audio-silent.png"))?;
        let loud = AudioInput {
            level: 0.8,
            bands: std::array::from_fn(|i| i as f32 / 15.0),
        };
        c.settle(loud, 20);
        let active = c.save(out.join("audio-active.png"))?;
        assert_ne!(silent, active);
        let radius = cfg.disc.free.diameter * size[1] as f32 * 0.5;
        for (x, y, pixel) in active.enumerate_pixels() {
            let r = (x as f32 + 0.5 - 256.0).hypot(y as f32 + 0.5 - 144.0);
            if r < radius * 0.85 {
                assert_eq!(pixel, silent.get_pixel(x, y), "音訊環遮住主體");
            }
        }
        c.settle(silence, 8);
        let halfway = c.save(out.join("audio-release-027s.png"))?;
        assert_ne!(silent, halfway);
        c.settle(silence, 9);
        assert_eq!(silent, c.save(out.join("audio-release-057s.png"))?);
        c.settle(loud, 20);
        cfg.audio_ring.enabled = false;
        c.config(&cfg);
        c.step(loud);
        assert_eq!(silent, c.save(out.join("audio-disabled.png"))?);

        // intensity=0 及 reflection=false 必須精確保留每一種背景排列。
        cfg = disc_config();
        cfg.disc.layout_mode = LayoutMode::Desktop;
        cfg.disc.rotate = false;
        cfg.background.mode = BackgroundMode::Image;
        cfg.background.image_path = Some(standard_art()?);
        for fit in [
            FitMode::Fill,
            FitMode::Fit,
            FitMode::Stretch,
            FitMode::Tile,
            FitMode::Center,
            FitMode::Span,
        ] {
            cfg.background.fit = fit;
            cfg.desktop_fx.intensity = 0.0;
            let mut c = Capture::new(&gpu, &cfg, size, format);
            c.step(silence);
            let original = c.save(out.join(format!("desktop-{fit:?}-original.png")))?;
            cfg.enabled = false;
            c.config(&cfg);
            c.step(silence);
            assert_eq!(original, c.pixels()?);
            cfg.enabled = true;
            cfg.desktop_fx.intensity = 1.0;
            cfg.disc.reflection = false;
            c.config(&cfg);
            c.step(silence);
            assert_eq!(
                original,
                c.save(out.join(format!("desktop-{fit:?}-reflection-off.png")))?
            );
            cfg.disc.reflection = true;
        }
        cfg.background.fit = FitMode::Fill;
        cfg.disc.cursor_reflection = false;
        let mut c = Capture::new(&gpu, &cfg, size, format);
        c.input.cursor = Some([30.0, 30.0]);
        c.settle(silence, 30);
        let stationary = c.save(out.join("desktop-cursor-off-left.png"))?;
        c.input.cursor = Some([470.0, 240.0]);
        c.settle(silence, 30);
        assert_eq!(
            stationary,
            c.save(out.join("desktop-cursor-off-right.png"))?,
            "旋轉及游標關閉後光學應靜止"
        );
        cfg.disc.cursor_reflection = true;
        c.config(&cfg);
        c.step(silence);
        let early = c.save(out.join("desktop-cursor-inertia-early.png"))?;
        c.settle(silence, 40);
        let late = c.save(out.join("desktop-cursor-inertia-late.png"))?;
        assert_ne!(stationary, early);
        assert_ne!(early, late);
        cfg.disc.rotate = true;
        c.config(&cfg);
        let before = c.pixels()?;
        c.input.time += 10.0;
        c.step(silence);
        assert_ne!(before, c.save(out.join("desktop-rotating-sweep.png"))?);
        cfg.disc.rotate = false;
        cfg.disc.reflection_intensity = 0.0;
        c.config(&cfg);
        c.step(silence);
        let no_reflection = c.save(out.join("desktop-reflection-strength0.png"))?;
        cfg.enabled = false;
        c.config(&cfg);
        c.step(silence);
        assert_eq!(no_reflection, c.pixels()?);
        cfg.enabled = true;
        cfg.disc.reflection_intensity = 0.8;
        // 全桌面絕不使用 on_disc，包含模式切換當下的既有粒子。
        cfg.effects.on_disc = EffectSet {
            rain: true,
            wind: true,
            leaves: true,
            sakura: true,
        };
        let mut ignored = Capture::new(&gpu, &cfg, size, format);
        ignored.settle(silence, 40);
        cfg.effects.on_disc = EffectSet::default();
        let mut empty = Capture::new(&gpu, &cfg, size, format);
        empty.settle(silence, 40);
        assert_eq!(
            ignored.save(out.join("desktop-on-disc-ignored.png"))?,
            empty.pixels()?
        );

        // 純色也要有可見反光；強度零仍逐像素還原，避免舊版的小色差上限壓低效果。
        cfg.background.mode = BackgroundMode::Color;
        cfg.disc.cursor_reflection = false;
        for color in [[0, 0, 0], [255, 255, 255], [40, 80, 120]] {
            cfg.background.color = color;
            let mut c = Capture::new(&gpu, &cfg, size, format);
            c.step(silence);
            let image = c.save(out.join(format!(
                "desktop-solid-{}-{}-{}.png",
                color[0], color[1], color[2]
            )))?;
            assert!(
                image.pixels().any(|p| p != image.get_pixel(0, 0)),
                "純色背景必須有反光紋理"
            );
            cfg.desktop_fx.intensity = 0.0;
            c.config(&cfg);
            c.step(silence);
            assert!(c.pixels()?.pixels().all(|p| p.0[..3] == color));
            cfg.desktop_fx.intensity = 1.0;
        }

        // 雨／風／落葉／櫻花在兩模式的開啟、游標互動、淡出均各存 PNG。
        cfg = disc_config();
        cfg.disc.rotate = false;
        cfg.disc.cursor_reflection = false;
        cfg.effects.intensity = 1.0;
        for mode in [LayoutMode::Free, LayoutMode::Desktop] {
            cfg.disc.layout_mode = mode;
            for kind in 0..4 {
                let mut c = Capture::new(&gpu, &cfg, size, format);
                c.step(silence);
                let bare = c.pixels()?;
                let set = EffectSet {
                    rain: kind == 0,
                    wind: kind == 1,
                    leaves: kind == 2,
                    sakura: kind == 3,
                };
                if mode == LayoutMode::Free {
                    cfg.effects.on_disc = set;
                } else {
                    cfg.effects.on_desktop = set;
                }
                c.config(&cfg);
                c.settle(silence, 60);
                let particles = c.save(out.join(format!("particle-{mode:?}-{kind}-on.png")))?;
                assert_ne!(bare, particles);
                if mode == LayoutMode::Free {
                    let r = cfg.disc.free.diameter * size[1] as f32 * 0.5;
                    for (x, y, pixel) in particles.enumerate_pixels() {
                        if (x as f32 + 0.5 - 256.0).hypot(y as f32 + 0.5 - 144.0) > r + 1.0 {
                            assert_eq!(pixel, bare.get_pixel(x, y), "盤面粒子超出圓形");
                        }
                    }
                }
                c.input.cursor = Some([256.0, 144.0]);
                c.settle(silence, 20);
                c.save(out.join(format!("particle-{mode:?}-{kind}-interactive.png")))?;
                cfg.effects.on_disc = EffectSet::default();
                cfg.effects.on_desktop = EffectSet::default();
                c.config(&cfg);
                c.settle(silence, 100);
                assert_eq!(
                    bare,
                    c.save(out.join(format!("particle-{mode:?}-{kind}-off.png")))?
                );
            }
        }
        // 讀圖失敗與不合法輸入不得 panic；使用與無圖設定相同的備援畫面。
        cfg = disc_config();
        cfg.disc.rotate = false;
        let mut c = Capture::new(&gpu, &cfg, size, format);
        c.step(silence);
        let fallback = c.pixels()?;
        cfg.background.image_path = Some(out.join("不存在.png"));
        cfg.background.mode = BackgroundMode::Image;
        cfg.disc.art_path = Some(out.join("不存在.png"));
        c.config(&cfg);
        c.input.time = f32::NAN;
        c.input.dt = f32::INFINITY;
        c.input.cursor = Some([f32::NAN, 0.0]);
        c.step(AudioInput {
            level: f32::NAN,
            bands: [f32::INFINITY; 16],
        });
        assert_eq!(fallback, c.save(out.join("missing-images-fallback.png"))?);
        c.renderer.resize(&gpu.device, &gpu.queue, [0, 0]);
        c.input.size = [0, 0];
        c.step(silence);
        Ok(())
    })
}
