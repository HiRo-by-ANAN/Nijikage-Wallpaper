//! 效能實測需單獨執行，避免與其他 GPU 測試搶資源造成誤判。
//! cargo test --release -p nijikage-wallpaper-render --test desktop_performance -- --ignored --nocapture
#[allow(dead_code)]
mod support;
use nijikage_wallpaper_core::{BackgroundMode, EffectSet, LayoutMode};
use nijikage_wallpaper_render::AudioInput;
use std::time::Instant;
use support::*;

#[test]
#[ignore = "需獨佔 GPU 測量 30 fps 預算，請以 --ignored 單獨執行"]
fn desktop_30fps_budget() -> TestResult {
    pollster::block_on(async {
        let gpu = Gpu::new().await?;
        let mut cfg = disc_config();
        cfg.disc.layout_mode = LayoutMode::Desktop;
        cfg.background.mode = BackgroundMode::Image;
        cfg.background.image_path = Some(standard_art()?);
        cfg.desktop_fx.intensity = 1.0;
        cfg.effects.intensity = 1.0;
        // 四種桌面粒子同時啟用，包含局部折射取樣與輸出轉換。
        cfg.effects.on_desktop = EffectSet {
            rain: true,
            wind: true,
            leaves: true,
            sakura: true,
        };
        for size in [[1280, 720], [1920, 1080]] {
            let mut c = Capture::new(&gpu, &cfg, size, wgpu::TextureFormat::Rgba8UnormSrgb);
            c.input.cursor = Some([size[0] as f32 * 0.5, size[1] as f32 * 0.5]);
            c.settle(AudioInput::default(), 90);
            gpu.device.poll(wgpu::Maintain::Wait);
            let mut samples = Vec::new();
            for i in 0..180 {
                c.input.cursor = Some([
                    size[0] as f32 * 0.5 + (i as f32 * 0.04).sin() * 180.0,
                    size[1] as f32 * 0.5 + (i as f32 * 0.04).cos() * 100.0,
                ]);
                let start = Instant::now();
                c.step(AudioInput::default());
                // 等候 GPU 完成，時間包含 CPU 提交與 GPU 全部繪製，不只測提交速度。
                gpu.device.poll(wgpu::Maintain::Wait);
                samples.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            samples.sort_by(f64::total_cmp);
            let mean = samples.iter().sum::<f64>() / samples.len() as f64;
            let p95 = samples[170];
            eprintln!(
                "效能 {}×{}，180 幀，平均={mean:.3} ms，中位數={:.3} ms，P95={p95:.3} ms，最慢={:.3} ms",
                size[0], size[1], samples[90], samples[179]
            );
            assert!(p95 < 1000.0 / 30.0, "P95 超過 30 fps 的 33.33 ms 預算");
        }
        Ok(())
    })
}
