//! wgpu 表面與繪製後端。
//! `render` feature 開啟時使用 nijikage-wallpaper-render::Renderer；未開啟時用純色後備畫面（開發期間）。

#[cfg(not(feature = "render"))]
use nijikage_wallpaper_core::BackgroundMode;
use nijikage_wallpaper_core::Config;
use nijikage_wallpaper_render::FrameInput;
use std::sync::Arc;
use winit::window::Window;

#[cfg(feature = "render")]
type Backend = nijikage_wallpaper_render::Renderer;

#[cfg(not(feature = "render"))]
struct Backend {
    color: wgpu::Color,
}

#[cfg(not(feature = "render"))]
impl Backend {
    fn from_cfg(cfg: &Config) -> Self {
        let c = if cfg.background.mode == BackgroundMode::Color { cfg.background.color } else { [16, 18, 28] };
        let lin = |v: u8| (v as f64 / 255.0).powf(2.2);
        Backend { color: wgpu::Color { r: lin(c[0]), g: lin(c[1]), b: lin(c[2]), a: 1.0 } }
    }
}

pub struct Gfx {
    surface: Option<wgpu::Surface<'static>>,
    readback: Option<Readback>,
    window: Arc<Window>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    sc: wgpu::SurfaceConfiguration,
    backend: Backend,
}

struct Readback {
    texture: wgpu::Texture,
    buffer: wgpu::Buffer,
    stride: u32,
    bitmap: crate::win32::LayeredBitmap,
    hwnd: isize,
}

impl Readback {
    fn new(device: &wgpu::Device, hwnd: isize, width: u32, height: u32) -> Result<Self, String> {
        let stride = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ULW offscreen BGRA"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ULW readback"), size: stride as u64 * height as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false,
        });
        let bitmap = crate::win32::LayeredBitmap::new(hwnd, width, height)?;
        Ok(Self { texture, buffer, stride, bitmap, hwnd })
    }

    fn present(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, w: u32, h: u32) -> Result<(), String> {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("ULW copy") });
        encoder.copy_texture_to_buffer(
            self.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo { buffer: &self.buffer, layout: wgpu::TexelCopyBufferLayout {
                offset: 0, bytes_per_row: Some(self.stride), rows_per_image: Some(h),
            } },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        queue.submit(Some(encoder.finish()));
        let slice = self.buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        slice.map_async(wgpu::MapMode::Read, move |result| { let _ = tx.send(result); });
        let _ = device.poll(wgpu::Maintain::Wait);
        rx.recv().map_err(|e| format!("Readback callback: {e}"))?
            .map_err(|e| format!("Readback map: {e}"))?;
        let result = {
            let data = slice.get_mapped_range();
            self.bitmap.present(&data, self.stride as usize)
        };
        self.buffer.unmap();
        result
    }
}

impl Gfx {
    pub fn new(window: Arc<Window>, cfg: &Config, bitmap_hwnd: Option<isize>) -> Result<Self, String> {
        let size = window.inner_size();
        let (w, h) = (size.width.max(1), size.height.max(1));
        // 明確讀取 WGPU_BACKEND，並記錄真正選到的後端，避免僅改環境變數卻未切換。
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
        let surface = if bitmap_hwnd.is_some() { None } else {
            Some(instance.create_surface(window.clone()).map_err(|e| format!("建立表面失敗：{e}"))?)
        };
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: surface.as_ref(),
            force_fallback_adapter: false,
        }))
        .ok_or("找不到可用的顯示卡")?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None))
            .map_err(|e| format!("建立 GPU 裝置失敗：{e}"))?;
        crate::log(&format!("GPU adapter: {:?}; bitmap_readback={}", adapter.get_info(), bitmap_hwnd.is_some()));
        let sc = if let Some(surface) = &surface {
            let mut sc = surface.get_default_config(&adapter, w, h).ok_or("表面不相容")?;
            let caps = surface.get_capabilities(&adapter);
            crate::log(&format!("Surface caps: {caps:?}"));
            if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
                sc.alpha_mode = wgpu::CompositeAlphaMode::Opaque;
            }
            surface.configure(&device, &sc);
            sc
        } else {
            wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT, format: wgpu::TextureFormat::Bgra8UnormSrgb,
                width: w, height: h, present_mode: wgpu::PresentMode::Fifo,
                desired_maximum_frame_latency: 2, alpha_mode: wgpu::CompositeAlphaMode::Opaque, view_formats: vec![],
            }
        };
        crate::log(&format!("Presentation configuration: {sc:?}"));
        let readback = bitmap_hwnd.map(|hwnd| Readback::new(&device, hwnd, w, h)).transpose()?;

        #[cfg(feature = "render")]
        let backend = nijikage_wallpaper_render::Renderer::new(&device, &queue, sc.format, [w, h], cfg);
        #[cfg(not(feature = "render"))]
        let backend = Backend::from_cfg(cfg);

        Ok(Self { surface, readback, window, device, queue, sc, backend })
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return;
        }
        if let Some(readback) = &self.readback {
            match Readback::new(&self.device, readback.hwnd, w, h) {
                Ok(r) => self.readback = Some(r),
                Err(e) => { crate::log(&e); return; }
            }
        }
        self.sc.width = w;
        self.sc.height = h;
        if let Some(surface) = &self.surface { surface.configure(&self.device, &self.sc); }
        #[cfg(feature = "render")]
        self.backend.resize(&self.device, &self.queue, [w, h]);
    }

    pub fn apply_config(&mut self, cfg: &Config) {
        #[cfg(feature = "render")]
        self.backend.apply_config(&self.device, &self.queue, cfg);
        #[cfg(not(feature = "render"))]
        {
            self.backend = Backend::from_cfg(cfg);
        }
    }

    /// 提供目前的系統音訊強度（音量與頻帶）給繪製器
    pub fn set_audio(&mut self, _a: &nijikage_wallpaper_render::AudioInput) {
        #[cfg(feature = "render")]
        self.backend.set_audio(_a);
    }

    pub fn draw(&mut self, input: &FrameInput) -> bool {
        let frame = if let Some(surface) = &self.surface { Some(match surface.get_current_texture() {
            Ok(f) => f,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                crate::log_once("交換鏈需要重建（Lost/Outdated）");
                surface.configure(&self.device, &self.sc);
                return false;
            }
            Err(e) => {
                crate::log_once(&format!("取得畫面失敗：{e:?}"));
                return false;
            }
        }) } else { None };
        let texture = match (&frame, &self.readback) {
            (Some(frame), _) => &frame.texture,
            (_, Some(readback)) => &readback.texture,
            _ => return false,
        };
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        #[cfg(feature = "render")]
        self.backend.render(&self.device, &self.queue, &view, input);

        #[cfg(not(feature = "render"))]
        {
            let _ = input;
            let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("fallback") });
            {
                let _pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("clear"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(self.backend.color), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
            }
            self.queue.submit(Some(enc.finish()));
        }

        if let Some(frame) = frame {
            self.window.pre_present_notify();
            frame.present();
        } else if let Some(readback) = self.readback.as_mut() {
            if let Err(e) = readback.present(&self.device, &self.queue, self.sc.width, self.sc.height) {
                crate::log_once(&e);
                return false;
            }
        }
        true
    }
}
