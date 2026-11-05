//! 一般視窗預覽。R／W／L／S 切換特效，空白鍵切換旋轉，C 切換盤面風格。
use nijikage_wallpaper_core::{BackgroundMode, Config, DiscStyle, EffectSet, LayoutMode};
use nijikage_wallpaper_render::{AudioInput, FrameInput, Renderer};
use std::{
    error::Error,
    ffi::OsString,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId},
};

type PreviewResult<T> = Result<T, Box<dyn Error>>;

struct Graphics {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface_config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
}

impl Graphics {
    async fn new(window: Arc<Window>, config: &Config) -> PreviewResult<Self> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12 | wgpu::Backends::VULKAN,
            ..Default::default()
        });
        let surface = instance.create_surface(window.clone())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or("找不到支援視窗輸出的 GPU")?;
        eprintln!("GPU {:?}", adapter.get_info());
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("Nijikage-Wallpaper 預覽"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    memory_hints: wgpu::MemoryHints::MemoryUsage,
                },
                None,
            )
            .await?;
        let size = window.inner_size();
        let mut surface_config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or("GPU 沒有可用的表面格式")?;
        let capabilities = surface.get_capabilities(&adapter);
        if let Some(format) = capabilities.formats.iter().find(|format| format.is_srgb()) {
            surface_config.format = *format;
        }
        surface_config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&device, &surface_config);
        let renderer = Renderer::new(
            &device,
            &queue,
            surface_config.format,
            [surface_config.width, surface_config.height],
            config,
        );
        Ok(Self {
            window,
            surface,
            device,
            queue,
            surface_config,
            renderer,
        })
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);
        self.renderer
            .resize(&self.device, &self.queue, [width, height]);
    }
}

struct Preview {
    config: Config,
    graphics: Option<Graphics>,
    cursor: Option<[f32; 2]>,
    start: Instant,
    last: Instant,
    next: Instant,
    frame_limit: Option<u64>,
    frames: u64,
    failure: Option<String>,
    desktop_fx: bool,
    disc_fx: bool,
    effects: EffectSet,
    audio_level: f32,
    audio_demo: bool,
}

impl Preview {
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: impl ToString) {
        self.failure = Some(error.to_string());
        event_loop.exit();
    }

    fn sync_effects(&mut self) {
        self.config.effects.on_disc = if self.disc_fx {
            self.effects
        } else {
            EffectSet::default()
        };
        self.config.effects.on_desktop = if self.desktop_fx {
            self.effects
        } else {
            EffectSet::default()
        };
    }
}

impl ApplicationHandler for Preview {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.graphics.is_some() {
            return;
        }
        let window = match event_loop.create_window(
            Window::default_attributes()
                .with_title("Nijikage-Wallpaper 預覽 · R/W/L/S 特效 · D/B 圖層 · C 風格 · 空白鍵旋轉")
                .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0)),
        ) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                self.fail(event_loop, error);
                return;
            }
        };
        match pollster::block_on(Graphics::new(window, &self.config)) {
            Ok(graphics) => {
                self.start = Instant::now();
                self.last = self.start;
                self.next = self.start;
                graphics.window.request_redraw();
                self.graphics = Some(graphics);
            }
            Err(error) => self.fail(event_loop, error),
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(graphics) = &mut self.graphics {
                    graphics.resize(size.width, size.height);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = Some([position.x as f32, position.y as f32])
            }
            WindowEvent::CursorLeft { .. } => self.cursor = None,
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed && !event.repeat =>
            {
                match event.physical_key {
                    PhysicalKey::Code(KeyCode::Escape) => event_loop.exit(),
                    PhysicalKey::Code(KeyCode::Space) => {
                        self.config.disc.rotate = !self.config.disc.rotate
                    }
                    PhysicalKey::Code(KeyCode::KeyR) => self.effects.rain = !self.effects.rain,
                    PhysicalKey::Code(KeyCode::KeyW) => self.effects.wind = !self.effects.wind,
                    PhysicalKey::Code(KeyCode::KeyL) => self.effects.leaves = !self.effects.leaves,
                    PhysicalKey::Code(KeyCode::KeyS) => self.effects.sakura = !self.effects.sakura,
                    PhysicalKey::Code(KeyCode::KeyD) => self.disc_fx = !self.disc_fx,
                    PhysicalKey::Code(KeyCode::KeyB) => self.desktop_fx = !self.desktop_fx,
                    PhysicalKey::Code(KeyCode::KeyC) => {
                        self.config.disc.style = match self.config.disc.style {
                            DiscStyle::Cd => DiscStyle::Dvd,
                            DiscStyle::Dvd => DiscStyle::Bd,
                            DiscStyle::Bd => DiscStyle::Cd,
                        }
                    }
                    _ => return,
                }
                self.sync_effects();
                if let Some(graphics) = &mut self.graphics {
                    graphics
                        .renderer
                        .apply_config(&graphics.device, &graphics.queue, &self.config);
                }
            }
            WindowEvent::RedrawRequested => {
                let Some(graphics) = &mut self.graphics else {
                    return;
                };
                let size = graphics.window.inner_size();
                if size.width == 0
                    || size.height == 0
                    || graphics.window.is_minimized() == Some(true)
                {
                    return;
                }
                let frame = match graphics.surface.get_current_texture() {
                    Ok(frame) => frame,
                    Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                        graphics.resize(size.width, size.height);
                        return;
                    }
                    Err(wgpu::SurfaceError::Timeout) => return,
                    Err(error) => {
                        self.fail(event_loop, error);
                        return;
                    }
                };
                let now = Instant::now();
                let input = FrameInput {
                    time: self.start.elapsed().as_secs_f32(),
                    dt: now.duration_since(self.last).as_secs_f32(),
                    cursor: self.cursor,
                    size: [size.width, size.height],
                };
                self.last = now;
                let t = input.time;
                let audio = if self.audio_demo {
                    AudioInput {
                        level: self.audio_level.max(0.65) * (0.65 + 0.35 * (t * 1.7).sin()),
                        bands: std::array::from_fn(|i| {
                            (0.5 + 0.5 * (t * (1.2 + i as f32 * 0.07) + i as f32 * 0.8).sin())
                                .powi(2)
                        }),
                    }
                } else {
                    AudioInput {
                        level: self.audio_level,
                        bands: [self.audio_level; 16],
                    }
                };
                graphics.renderer.set_audio(&audio);
                graphics.renderer.render(
                    &graphics.device,
                    &graphics.queue,
                    &frame.texture.create_view(&Default::default()),
                    &input,
                );
                frame.present();
                self.frames += 1;
                self.next = now
                    + Duration::from_secs_f64(1.0 / self.config.performance.fps_cap.max(1) as f64);
                if self.frame_limit.is_some_and(|limit| self.frames >= limit) {
                    eprintln!(
                        "已完成 {} 幀，經過 {:.2} 秒",
                        self.frames,
                        self.start.elapsed().as_secs_f64()
                    );
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(graphics) = &self.graphics else {
            return;
        };
        let size = graphics.window.inner_size();
        if size.width == 0 || size.height == 0 || graphics.window.is_minimized() == Some(true) {
            event_loop.set_control_flow(ControlFlow::Wait);
        } else {
            if Instant::now() >= self.next {
                graphics.window.request_redraw();
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.next));
        }
    }
}

/// 參數以逗號分隔，拒絕缺值、非有限值與超出核心規格的數值。
fn numbers<const N: usize>(
    value: Option<OsString>,
    flag: &str,
    ranges: [(f32, f32); N],
) -> PreviewResult<[f32; N]> {
    let value = value.ok_or_else(|| format!("{flag} 需要數值"))?;
    let text = value
        .to_str()
        .ok_or_else(|| format!("{flag} 數值格式錯誤"))?;
    let parts: Vec<_> = text.split(',').collect();
    if parts.len() != N {
        return Err(format!("{flag} 需要 {N} 個以逗號分隔的數值").into());
    }
    let mut result = [0.0; N];
    for (index, part) in parts.iter().enumerate() {
        let value: f32 = part.trim().parse()?;
        let (low, high) = ranges[index];
        if !value.is_finite() || !(low..=high).contains(&value) {
            return Err(format!("{flag} 第 {} 個數值必須介於 {low}～{high}", index + 1).into());
        }
        result[index] = value;
    }
    Ok(result)
}

fn main() -> PreviewResult<()> {
    let mut config = Config::default();
    config.disc.layout_mode = LayoutMode::Free;
    let mut effects = EffectSet::default();
    let mut disc_fx = false;
    let mut desktop_fx = false;
    let mut frame_limit = None;
    let mut audio_level = 0.0;
    let mut audio_demo = false;
    let mut arguments = std::env::args_os().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--mode") => {
                config.disc.layout_mode = match arguments.next().as_deref().and_then(|s| s.to_str())
                {
                    Some("disc") => LayoutMode::Free,
                    Some("desktop") => LayoutMode::Desktop,
                    _ => return Err("--mode 必須是 disc 或 desktop".into()),
                };
            }
            Some("--audio") => audio_level = numbers(arguments.next(), "--audio", [(0.0, 1.0)])?[0],
            Some("--audio-demo") => audio_demo = true,
            Some("--pattern-scale") => {
                config.desktop_fx.pattern_scale =
                    numbers(arguments.next(), "--pattern-scale", [(0.5, 3.0)])?[0]
            }
            Some("--fx-intensity") => {
                config.desktop_fx.intensity =
                    numbers(arguments.next(), "--fx-intensity", [(0.0, 1.0)])?[0]
            }
            Some("--bg") => {
                config.background.image_path =
                    Some(PathBuf::from(arguments.next().ok_or("--bg 需要圖片路徑")?));
                config.background.mode = BackgroundMode::Image;
            }
            Some("--art") => {
                config.disc.art_path =
                    Some(PathBuf::from(arguments.next().ok_or("--art 需要圖片路徑")?))
            }
            Some("--free") => {
                let [cx, cy, diameter] = numbers(
                    arguments.next(),
                    "--free",
                    [(-0.5, 1.5), (-0.5, 1.5), (0.2, 3.0)],
                )?;
                config.disc.free.center_x = cx;
                config.disc.free.center_y = cy;
                config.disc.free.diameter = diameter;
            }
            Some("--art-offset") => {
                let [x, y] = numbers(arguments.next(), "--art-offset", [(-1.0, 1.0); 2])?;
                config.disc.free.art_offset_x = x;
                config.disc.free.art_offset_y = y;
            }
            Some("--art-zoom") => {
                config.disc.free.art_zoom =
                    numbers(arguments.next(), "--art-zoom", [(0.2, 4.0)])?[0];
            }
            Some("--art-rot") => {
                config.disc.free.art_rotation_deg =
                    numbers(arguments.next(), "--art-rot", [(-180.0, 180.0)])?[0];
            }
            Some("--style") => {
                config.disc.style =
                    match arguments.next().as_deref().and_then(|value| value.to_str()) {
                        Some("cd") => DiscStyle::Cd,
                        Some("dvd") => DiscStyle::Dvd,
                        Some("bd") => DiscStyle::Bd,
                        _ => return Err("--style 必須是 cd、dvd 或 bd".into()),
                    };
            }
            Some("--rain") => effects.rain = true,
            Some("--wind") => effects.wind = true,
            Some("--leaves") => effects.leaves = true,
            Some("--sakura") => effects.sakura = true,
            Some("--disc-fx") => disc_fx = true,
            Some("--desktop-fx") => desktop_fx = true,
            Some("--frames") => {
                frame_limit = Some(
                    arguments
                        .next()
                        .ok_or("--frames 需要幀數")?
                        .to_str()
                        .ok_or("幀數格式錯誤")?
                        .parse::<u64>()?,
                );
            }
            Some("--help" | "-h") => {
                println!(
                    "Nijikage-Wallpaper 預覽\n--mode disc|desktop（預設 disc）\n--bg <圖片> --art <圖片> --style cd|dvd|bd\n--free <cx>,<cy>,<diameter> 設定光碟版面\n--art-offset <x>,<y> --art-zoom <0.2～4> --art-rot <deg>（正角度為順時針）\n--audio <0～1> 固定假音量 --audio-demo 假頻帶動畫\n--pattern-scale <0.5～3> --fx-intensity <0～1> 桌面光學\n--rain --wind --leaves --sakura --disc-fx --desktop-fx\n--frames <幀數> 可供自動驗證後結束\n未指定特效圖層時使用桌面層，desktop 忽略光碟層。R/W/L/S 切換種類，D/B 切換圖層，C 切換風格，空白鍵切換旋轉，Esc 結束。"
                );
                return Ok(());
            }
            _ => return Err(format!("未知參數 {}", argument.to_string_lossy()).into()),
        }
    }
    if !disc_fx && !desktop_fx {
        desktop_fx = true;
    }
    let now = Instant::now();
    let mut preview = Preview {
        config,
        graphics: None,
        cursor: None,
        start: now,
        last: now,
        next: now,
        frame_limit,
        frames: 0,
        failure: None,
        desktop_fx,
        disc_fx,
        effects,
        audio_level,
        audio_demo,
    };
    preview.sync_effects();
    EventLoop::new()?.run_app(&mut preview)?;
    if let Some(error) = preview.failure {
        return Err(error.into());
    }
    Ok(())
}
