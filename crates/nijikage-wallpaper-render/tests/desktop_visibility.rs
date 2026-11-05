//! 任務 5：真實圖片的六項桌面量化門檻，以及局部透鏡、純色與控制開關驗證。
//! 固定 1280×720、Fill、起始時間 3 秒、40 幀、中央游標；不以替代圖片掩蓋缺圖。
#[allow(dead_code)] // 共用工具也提供其他 GPU 測試使用的存圖與切換設定方法。
mod support;
use nijikage_wallpaper_core::{BackgroundMode, FitMode, LayoutMode};
use nijikage_wallpaper_render::AudioInput;
use support::*;

struct Metrics {
    psnr: f64,
    changed: usize,
    over24: usize,
    pixels: usize,
    brightness: f64,
}

// PSNR 使用全部 RGB 通道；差異比例以任一通道嚴格大於 24 計算，不含 alpha。
fn metrics(reference: &image::RgbaImage, actual: &image::RgbaImage, local: bool) -> Metrics {
    let mut squared = 0.0;
    let mut changed = 0;
    let mut over24 = 0;
    let mut pixels = 0;
    let mut before = 0.0;
    let mut after = 0.0;
    for (x, y, a) in reference.enumerate_pixels() {
        if local && (x as f64 + 0.5 - 640.0).hypot(y as f64 + 0.5 - 360.0) > 260.0 {
            continue;
        }
        let b = actual.get_pixel(x, y);
        let delta: [u8; 3] = std::array::from_fn(|i| a[i].abs_diff(b[i]));
        squared += delta.iter().map(|v| (*v as f64).powi(2)).sum::<f64>();
        changed += usize::from(a != b);
        over24 += usize::from(delta.iter().any(|v| *v > 24));
        pixels += 1;
        for (i, weight) in [0.2126, 0.7152, 0.0722].iter().enumerate() {
            before += a[i] as f64 * weight;
            after += b[i] as f64 * weight;
        }
    }
    let mse = squared / (pixels * 3) as f64;
    Metrics {
        psnr: if mse == 0.0 {
            f64::INFINITY
        } else {
            10.0 * (255.0 * 255.0 / mse).log10()
        },
        changed,
        over24,
        pixels,
        brightness: (after / before - 1.0) * 100.0,
    }
}

impl Metrics {
    fn ratio(&self) -> f64 {
        self.over24 as f64 / self.pixels as f64
    }
    fn print(&self, name: &str) {
        eprintln!(
            "{name}: PSNR={:.4} dB，>24={}/{} ({:.4}%)，不同像素={}，平均亮度變化={:+.4}%",
            self.psnr,
            self.over24,
            self.pixels,
            self.ratio() * 100.0,
            self.changed,
            self.brightness
        );
    }
}

fn render(
    gpu: &Gpu,
    cfg: &nijikage_wallpaper_core::Config,
    cursor: Option<[f32; 2]>,
) -> TestResult<image::RgbaImage> {
    let mut c = Capture::new(gpu, cfg, [1280, 720], wgpu::TextureFormat::Rgba8UnormSrgb);
    c.input.cursor = cursor;
    c.settle(AudioInput::default(), 40);
    c.pixels()
}

#[test]
fn desktop_six_acceptance_thresholds() -> TestResult {
    pollster::block_on(async {
        let gpu = Gpu::new().await?;
        let out = folder("v03-dump")?;
        let mut cfg = disc_config();
        cfg.disc.layout_mode = LayoutMode::Desktop;
        cfg.background.mode = BackgroundMode::Image;
        cfg.background.image_path = Some(standard_art()?);
        cfg.background.fit = FitMode::Fill;
        cfg.desktop_fx.pattern_scale = 1.0;
        cfg.desktop_fx.intensity = 0.0;
        let center = Some([640.0, 360.0]);
        let original = render(&gpu, &cfg, center)?;
        original.save(out.join("desktop-acceptance-original.png"))?;
        cfg.enabled = false;
        let disabled = render(&gpu, &cfg, center)?;
        let zero = metrics(&disabled, &original, false);
        zero.print("驗收 4 強度 0");
        assert_eq!(original, disabled, "強度 0 必須與無效果背景逐像素相同");
        cfg.enabled = true;
        let mut overview = vec![original.clone()];
        for (intensity, minimum, maximum_psnr) in [
            (0.3, 0.03, f64::INFINITY),
            (0.7, 0.10, 30.0),
            (1.0, 0.18, 26.0),
        ] {
            cfg.desktop_fx.intensity = intensity;
            let image = render(&gpu, &cfg, center)?;
            image.save(out.join(format!("desktop-acceptance-i{intensity:.1}.png")))?;
            diff_heatmap(&original, &image)
                .save(out.join(format!("desktop-diff-i{intensity:.1}.png")))?;
            let measured = metrics(&original, &image, false);
            measured.print(&format!("驗收強度 {intensity:.1}"));
            assert!(
                measured.ratio() >= minimum,
                "強度 {intensity} 差異像素未達門檻"
            );
            assert!(measured.psnr < maximum_psnr, "強度 {intensity} PSNR 過高");
            if intensity == 1.0 {
                assert!(
                    measured.brightness.abs() <= 18.0,
                    "驗收 6：平均亮度變動超過 ±18%"
                );
                let without = render(&gpu, &cfg, None)?;
                without.save(out.join("desktop-acceptance-no-cursor.png"))?;
                let absent = metrics(&original, &without, true);
                let present = metrics(&original, &image, true);
                absent.print("驗收 5 無游標，半徑 260 px");
                present.print("驗收 5 中央游標，半徑 260 px");
                let count_change =
                    present.over24.abs_diff(absent.over24) as f64 / absent.over24.max(1) as f64;
                eprintln!(
                    "驗收 5：局部 >24 差異像素數變化={:.4}%",
                    count_change * 100.0
                );
                assert!(count_change >= 0.20, "游標附近的差異像素數變化未達 20%");
                let lens = metrics(&without, &image, true);
                lens.print("透鏡本身，中央有／無游標直接比較");
                assert!(lens.ratio() >= 0.10, "局部折射本身必須清楚可見");
                // 桌面光帶不隨游標改色；局部半徑以外必須完全一致。
                for (x, y, p) in image.enumerate_pixels() {
                    if (x as f64 + 0.5 - 640.0).hypot(y as f64 + 0.5 - 360.0) > 260.0 {
                        assert_eq!(p, without.get_pixel(x, y), "游標不應改變遠處桌面");
                    }
                }
            }
            overview.push(image);
        }
        sheet(
            &overview,
            2,
            [640, 360],
            &out.join("desktop-acceptance-overview.png"),
        )?;

        // 純色背景仍須有空間變化，輸出深藍 0.7／1.0；不再限制效果小於 42 階。
        cfg.background.mode = BackgroundMode::Color;
        cfg.background.color = [12, 28, 65];
        cfg.desktop_fx.intensity = 0.0;
        let solid = render(&gpu, &cfg, None)?;
        for intensity in [0.7, 1.0] {
            cfg.desktop_fx.intensity = intensity;
            let image = render(&gpu, &cfg, None)?;
            image.save(out.join(format!("desktop-solid-blue-i{intensity:.1}.png")))?;
            let m = metrics(&solid, &image, false);
            m.print(&format!("深藍純色 {intensity}"));
            assert!(m.ratio() >= 0.10, "純色桌面也要看得見彩虹");
        }
        Ok(())
    })
}

#[test]
fn desktop_cursor_lifecycle_and_controls() -> TestResult {
    pollster::block_on(async {
        let gpu = Gpu::new().await?;
        let mut cfg = disc_config();
        cfg.disc.layout_mode = LayoutMode::Desktop;
        cfg.disc.rotate = false;
        cfg.background.mode = BackgroundMode::Image;
        cfg.background.image_path = Some(standard_art()?);
        cfg.desktop_fx.intensity = 1.0;
        let absent = render(&gpu, &cfg, None)?;
        for cursor in [
            Some([-1.0, 360.0]),
            Some([1280.0, 360.0]),
            Some([640.0, -1.0]),
            Some([640.0, 720.0]),
        ] {
            assert_eq!(
                absent,
                render(&gpu, &cfg, cursor)?,
                "畫面外游標不得顯示透鏡"
            );
        }
        let mut c = Capture::new(&gpu, &cfg, [1280, 720], wgpu::TextureFormat::Rgba8UnormSrgb);
        c.input.cursor = Some([640.0, 360.0]);
        c.step(AudioInput::default());
        let early = c.pixels()?;
        c.settle(AudioInput::default(), 39);
        let active = c.pixels()?;
        assert_ne!(early, active, "游標必須慣性淡入");
        c.input.cursor = None;
        c.step(AudioInput::default());
        let leaving = c.pixels()?;
        assert_ne!(leaving, absent, "游標離開應平滑淡出");
        c.settle(AudioInput::default(), 160);
        assert_eq!(absent, c.pixels()?, "游標離開後完全還原");
        cfg.disc.cursor_reflection = false;
        assert_eq!(absent, render(&gpu, &cfg, Some([640.0, 360.0]))?);
        cfg.disc.reflection = false;
        let reflection_off = render(&gpu, &cfg, None)?;
        cfg.desktop_fx.intensity = 0.0;
        assert_eq!(reflection_off, render(&gpu, &cfg, None)?);
        Ok(())
    })
}
