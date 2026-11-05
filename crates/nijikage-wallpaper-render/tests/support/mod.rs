//! 離屏 GPU 共用工具；PNG 一律寫入此 crate 的 target，不需建立預覽視窗。
use nijikage_wallpaper_core::{Config, LayoutMode};
use nijikage_wallpaper_render::{AudioInput, FrameInput, Renderer};
use std::{
    error::Error,
    path::{Path, PathBuf},
    sync::mpsc,
};
pub type TestResult<T = ()> = Result<T, Box<dyn Error>>;
pub fn read_pixels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    size: [u32; 2],
) -> TestResult<Vec<u8>> {
    let row = (size[0] * 4).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("測試像素讀回"),
        size: (row * size[1]) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(size[1]),
            },
        },
        wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));
    let (sender, receiver) = mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
    device.poll(wgpu::Maintain::Wait);
    receiver.recv()??;
    let mapped = buffer.slice(..).get_mapped_range();
    let pixels = mapped
        .chunks_exact(row as usize)
        .flat_map(|row| row[..size[0] as usize * 4].iter().copied())
        .collect();
    drop(mapped);
    buffer.unmap();
    Ok(pixels)
}

pub fn output_texture(
    device: &wgpu::Device,
    size: [u32; 2],
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("測試輸出"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}
impl Gpu {
    pub async fn new() -> TestResult<Self> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12 | wgpu::Backends::VULKAN,
            ..Default::default()
        });
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .ok_or("未能執行：離屏測試需要 DX12 或 Vulkan GPU")?;
        eprintln!("測試 GPU {:?}", adapter.get_info());
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("v0.3 離屏驗證"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    memory_hints: wgpu::MemoryHints::MemoryUsage,
                },
                None,
            )
            .await?;
        Ok(Self { device, queue })
    }
}

pub fn folder(name: &str) -> TestResult<PathBuf> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(name);
    std::fs::create_dir_all(&path)?;
    Ok(path)
}
/// 測試用標準圖。整個測試程序只產生一次（OnceLock 會讓同時呼叫的其他測試等待），避免平行測試互相覆蓋暫存檔。
pub fn standard_art() -> TestResult<PathBuf> {
    static ART: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    Ok(ART
        .get_or_init(|| build_standard_art().expect("無法產生測試用標準圖"))
        .clone())
}

fn build_standard_art() -> TestResult<PathBuf> {
    // 測試用標準圖：程式產生的直式（941×1672）風景插畫風圖案，不依賴任何外部檔案。
    // 內含不對稱的標記（左上三角、右上太陽、底部山脈），方便目視檢查縮放、位移、旋轉與裁切是否正確。
    let path = folder("fixtures")?.join("standard-art.png");
    if path.is_file() {
        return Ok(path);
    }
    let (w, h) = (941u32, 1672u32);
    let mut img = image::RgbaImage::new(w, h);
    let hash = |x: u32, y: u32| -> u32 {
        let mut v = x.wrapping_mul(374_761_393) ^ y.wrapping_mul(668_265_263);
        v = (v ^ (v >> 13)).wrapping_mul(1_274_126_177);
        v ^ (v >> 16)
    };
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            // 天空漸層：上方深藍 → 下方暖橘
            let mut c = [20.0 + 200.0 * fy.powf(1.6), 30.0 + 90.0 * fy, 90.0 - 40.0 * fy];
            // 右上的太陽
            let (dx, dy) = (fx - 0.68, (fy - 0.22) * (h as f32 / w as f32));
            let d = (dx * dx + dy * dy).sqrt();
            if d < 0.16 {
                let k = (1.0 - d / 0.16).powf(0.6);
                c = [c[0] + (255.0 - c[0]) * k, c[1] + (230.0 - c[1]) * k, c[2] + (150.0 - c[2]) * k];
            }
            // 星點
            if fy < 0.5 && hash(x, y) % 4000 == 0 {
                c = [255.0, 255.0, 240.0];
            }
            // 底部山脈
            let ridge = 0.74 + 0.05 * (fx * 9.0).sin() + 0.03 * (fx * 23.0 + 1.0).sin();
            if fy > ridge {
                let t = (fy - ridge) / (1.0 - ridge);
                c = [20.0 + 30.0 * t, 60.0 + 40.0 * t, 70.0 + 20.0 * t];
            }
            // 斜向光條與細網格（讓位移與旋轉一目了然）
            if ((x as i32 + y as i32) % 160) < 4 {
                c = [c[0] * 0.8 + 50.0, c[1] * 0.8 + 50.0, c[2] * 0.8 + 50.0];
            }
            // 左上白色三角（方向標記）
            if fx < 0.18 && fy < 0.10 && fx * 0.10 + (0.10 - fy) * 0.18 > 0.0 && fy < 0.10 - fx * 0.55 {
                c = [250.0, 250.0, 250.0];
            }
            img.put_pixel(x, y, image::Rgba([c[0].clamp(0.0, 255.0) as u8, c[1].clamp(0.0, 255.0) as u8, c[2].clamp(0.0, 255.0) as u8, 255]));
        }
    }
    // 先寫暫存檔再原子改名：多個測試同時執行時不會讀到寫到一半的檔案
    let tmp = path.with_extension(format!("{}.tmp.png", std::process::id()));
    img.save(&tmp)?;
    std::fs::rename(&tmp, &path)?;
    Ok(path)
}
pub fn disc_config() -> Config {
    let mut cfg = Config::default();
    cfg.disc.layout_mode = LayoutMode::Free;
    cfg.disc.free.center_x = 0.5;
    cfg.disc.free.center_y = 0.5;
    cfg.disc.free.diameter = 0.86;
    cfg.background.color = [20, 24, 40];
    cfg
}

pub struct Capture<'a> {
    pub gpu: &'a Gpu,
    pub renderer: Renderer,
    pub texture: wgpu::Texture,
    pub input: FrameInput,
}
impl<'a> Capture<'a> {
    pub fn new(gpu: &'a Gpu, cfg: &Config, size: [u32; 2], format: wgpu::TextureFormat) -> Self {
        let renderer = Renderer::new(&gpu.device, &gpu.queue, format, size, cfg);
        let texture = output_texture(&gpu.device, size, format);
        Self {
            gpu,
            renderer,
            texture,
            input: FrameInput {
                time: 3.0,
                dt: 1.0 / 30.0,
                cursor: None,
                size,
            },
        }
    }
    pub fn config(&mut self, cfg: &Config) {
        self.renderer
            .apply_config(&self.gpu.device, &self.gpu.queue, cfg);
    }
    pub fn step(&mut self, audio: AudioInput) {
        self.renderer.set_audio(&audio);
        self.renderer.render(
            &self.gpu.device,
            &self.gpu.queue,
            &self.texture.create_view(&Default::default()),
            &self.input,
        );
        self.input.time += self.input.dt;
    }
    pub fn settle(&mut self, audio: AudioInput, frames: usize) {
        for _ in 0..frames {
            self.step(audio);
        }
    }
    pub fn pixels(&self) -> TestResult<image::RgbaImage> {
        let size = self.input.size;
        image::RgbaImage::from_raw(
            size[0],
            size[1],
            read_pixels(&self.gpu.device, &self.gpu.queue, &self.texture, size)?,
        )
        .ok_or_else(|| "像素尺寸錯誤".into())
    }
    pub fn save(&self, path: impl AsRef<Path>) -> TestResult<image::RgbaImage> {
        let image = self.pixels()?;
        image.save(path)?;
        Ok(image)
    }
}

/// 總覽保留固定欄位，排列順序記在各測試註解，原尺寸圖另存供檢查細節。
#[allow(dead_code)] // 此輔助模組也由僅做逐像素比較的 gpu.rs 使用。
pub fn sheet(images: &[image::RgbaImage], columns: u32, tile: [u32; 2], path: &Path) -> TestResult {
    let mut result = image::RgbaImage::new(
        columns * tile[0],
        (images.len() as u32).div_ceil(columns) * tile[1],
    );
    for (i, image) in images.iter().enumerate() {
        let small = image::imageops::resize(
            image,
            tile[0],
            tile[1],
            image::imageops::FilterType::Triangle,
        );
        image::imageops::replace(
            &mut result,
            &small,
            (i as u32 % columns * tile[0]) as i64,
            (i as u32 / columns * tile[1]) as i64,
        );
    }
    result.save(path)?;
    Ok(())
}

/// 與背景的 RGB 絕對差異放大四倍，飽和至 255，供肉眼檢查光帶範圍。
#[allow(dead_code)]
pub fn diff_heatmap(reference: &image::RgbaImage, actual: &image::RgbaImage) -> image::RgbaImage {
    image::RgbaImage::from_fn(reference.width(), reference.height(), |x, y| {
        let a = reference.get_pixel(x, y);
        let b = actual.get_pixel(x, y);
        image::Rgba([
            a[0].abs_diff(b[0]).saturating_mul(4),
            a[1].abs_diff(b[1]).saturating_mul(4),
            a[2].abs_diff(b[2]).saturating_mul(4),
            255,
        ])
    })
}
