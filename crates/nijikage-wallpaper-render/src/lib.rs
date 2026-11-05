//! Nijikage-Wallpaper 的 GPU 繪製器。視窗、設定檔與幀率排程由宿主負責。
//! 以線性色彩中繼表面合成，再依宿主表面格式進行色彩編碼。
//! 光學調參在 scene.wgsl 開頭（中心收斂、細紋、光帶、繞射、桌面柔光、折射與音訊光暈）。
//! 本檔常數控制粒子數量／互動／淡出、游標慣性，以及音訊環段數／平滑／靜音門檻。

use bytemuck::{Pod, Zeroable};
use nijikage_wallpaper_core::{BackgroundMode, Config, DiscStyle, EffectSet, FitMode, LayoutMode};
use std::{
    f32::consts::TAU,
    path::{Path, PathBuf},
    time::SystemTime,
};
use wgpu::util::DeviceExt;

/// 每幀由宿主程式提供的輸入
#[derive(Debug, Clone, Copy)]
pub struct FrameInput {
    /// 程式啟動後經過的秒數（光碟旋轉角度由此與 rpm 推得）
    pub time: f32,
    /// 與上一幀的間隔（秒）
    pub dt: f32,
    /// 游標位置（像素，相對繪製表面左上角）；不在本螢幕時為 None
    pub cursor: Option<[f32; 2]>,
    /// 繪製表面大小（像素）
    pub size: [u32; 2],
}

/// 系統音訊輸入（由宿主程式以 WASAPI loopback 擷取後換算；只有音量與頻帶，不含原始聲音）
#[derive(Debug, Clone, Copy, Default)]
pub struct AudioInput {
    /// 整體音量 0–1（已平滑）；沒有聲音輸出時為 0
    pub level: f32,
    /// 16 個頻帶（低→高）的能量 0–1（已平滑）
    pub bands: [f32; 16],
}

// 每種類／圖層最多 192 個；全部開啟最多 1536 個，桌面模式最多 768 個。
const PARTICLES_PER_KIND: usize = 192;
const PARTICLE_LIMIT: usize = PARTICLES_PER_KIND * 8;
// 音訊環固定 128 段，不依音量增加幾何數量；靜音時完全略過此 draw。
const AUDIO_SEGMENTS: u32 = 128;
const AUDIO_RELEASE: f32 = 0.5;
const AUDIO_ATTACK: f32 = 0.045;
const AUDIO_SILENCE: f32 = 0.001;
const CURSOR_RESPONSE: f32 = 3.2;
const PARTICLE_FADE_RATE: f32 = 3.5;
const PARTICLE_PUSH_RADIUS: f32 = 110.0;
const PARTICLE_PUSH_FORCE: f32 = 420.0;
const SCENE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    screen: [f32; 4],
    disc: [f32; 4],
    light: [f32; 4],
    background: [f32; 4],
    image: [f32; 4],
    options: [f32; 4],
    art_transform: [f32; 4],
    art_size: [f32; 4],
    desktop_fx: [f32; 4],
    audio: [f32; 4],
    bands: [[f32; 4]; 4],
}

/// 輸入只保留固定的 16 頻帶；淡出在 render 依 dt 推進，重複送入 0 不會重設淡出。
#[derive(Default)]
struct AudioState {
    input: AudioInput,
    current: AudioInput,
    release: AudioInput,
    silence: f32,
}

impl AudioState {
    fn set(&mut self, audio: &AudioInput) {
        self.input = AudioInput {
            level: finite(audio.level, 0.0).clamp(0.0, 1.0),
            bands: audio.bands.map(|v| finite(v, 0.0).clamp(0.0, 1.0)),
        };
    }

    fn advance(&mut self, dt: f32) {
        if self.input.level > AUDIO_SILENCE {
            let response = 1.0 - (-dt / AUDIO_ATTACK).exp();
            self.current.level += (self.input.level - self.current.level) * response;
            for (band, target) in self.current.bands.iter_mut().zip(self.input.bands) {
                *band += (target - *band) * response;
            }
            self.release = self.current;
            self.silence = 0.0;
        } else {
            self.silence = (self.silence + dt).min(AUDIO_RELEASE);
            let fade = 1.0 - self.silence / AUDIO_RELEASE;
            self.current.level = self.release.level * fade;
            self.current.bands = self.release.bands.map(|v| v * fade);
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    rect: [f32; 4],
    detail: [f32; 4],
}

struct Picture {
    view: wgpu::TextureView,
    original_size: [f32; 2],
    loaded: bool,
}

struct Particle {
    position: [f32; 2],
    push: [f32; 2],
    seed: f32,
    angle: f32,
    opacity: f32,
}

/// 持有 GPU 資源。呼叫端提供相同裝置的輸出 view，取樣數須為 1。
pub struct Renderer {
    cfg: Config,
    size: [u32; 2],
    uniform: wgpu::Buffer,
    instances: wgpu::Buffer,
    scene_layout: wgpu::BindGroupLayout,
    present_layout: wgpu::BindGroupLayout,
    scene_bind: wgpu::BindGroup,
    present_bind: wgpu::BindGroup,
    sampler: wgpu::Sampler,
    background: Picture,
    art: Picture,
    /// 目前已載入的圖片來源（路徑＋修改時間），沒變就不重讀
    bg_key: Option<(PathBuf, Option<SystemTime>)>,
    art_key: Option<(PathBuf, Option<SystemTime>)>,
    scene_view: wgpu::TextureView,
    scene_pipeline: wgpu::RenderPipeline,
    particle_pipeline: wgpu::RenderPipeline,
    audio_pipeline: wgpu::RenderPipeline,
    present_pipeline: wgpu::RenderPipeline,
    output_srgb: bool,
    light: [f32; 2],
    desktop_cursor_visibility: f32,
    phase: f32,
    last_time: Option<f32>,
    particles: Vec<Particle>,
    particle_data: Vec<Instance>,
    audio: AudioState,
    audio_phase: f32,
}

impl Renderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        size: [u32; 2],
        cfg: &nijikage_wallpaper_core::Config,
    ) -> Self {
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("繪製參數"),
            contents: bytemuck::bytes_of(&Uniforms::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("粒子實例"),
            size: (PARTICLE_LIMIT * size_of::<Instance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("圖片取樣"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let scene_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("場景資源配置"),
            entries: &[
                uniform_entry(),
                sampler_entry(1),
                texture_entry(2),
                texture_entry(3),
            ],
        });
        let present_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("輸出資源配置"),
            entries: &[uniform_entry(), sampler_entry(1), texture_entry(2)],
        });
        let background = load_picture(device, queue, None, false);
        let art = load_picture(device, queue, None, true);
        let scene_bind = scene_bind(device, &scene_layout, &uniform, &sampler, &background, &art);
        let size = valid_size(device, size);
        let scene_view = scene_texture(device, size);
        let present_bind = present_bind(device, &present_layout, &uniform, &sampler, &scene_view);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("盤面與粒子著色器"),
            source: wgpu::ShaderSource::Wgsl(include_str!("scene.wgsl").into()),
        });
        let present_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("色彩輸出著色器"),
            source: wgpu::ShaderSource::Wgsl(include_str!("present.wgsl").into()),
        });
        let scene_pipeline = pipeline(
            device,
            &scene_layout,
            &shader,
            "fullscreen",
            "scene",
            SCENE_FORMAT,
            &[],
            false,
        );
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: size_of::<Instance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4],
        };
        let particle_pipeline = pipeline(
            device,
            &scene_layout,
            &shader,
            "particle_vertex",
            "particle_fragment",
            SCENE_FORMAT,
            &[vertex_layout],
            true,
        );
        let audio_pipeline = pipeline(
            device,
            &scene_layout,
            &shader,
            "audio_vertex",
            "audio_fragment",
            SCENE_FORMAT,
            &[],
            true,
        );
        let present_pipeline = pipeline(
            device,
            &present_layout,
            &present_shader,
            "fullscreen",
            "present",
            format,
            &[],
            false,
        );
        let mut random = 0x6d2b_79f5_u32;
        let mut next = || {
            random ^= random << 13;
            random ^= random >> 17;
            random ^= random << 5;
            (random as f64 / u32::MAX as f64) as f32
        };
        let particles = (0..PARTICLE_LIMIT)
            .map(|_| Particle {
                position: [next(), next()],
                push: [0.0; 2],
                seed: next(),
                angle: next() * TAU,
                opacity: 0.0,
            })
            .collect();
        let mut renderer = Self {
            cfg: cfg.clone(),
            size,
            uniform,
            instances,
            scene_layout,
            present_layout,
            scene_bind,
            present_bind,
            sampler,
            background,
            art,
            bg_key: None,
            art_key: None,
            scene_view,
            scene_pipeline,
            particle_pipeline,
            audio_pipeline,
            present_pipeline,
            output_srgb: format.is_srgb(),
            light: [-0.45, -0.55],
            desktop_cursor_visibility: 0.0,
            phase: 0.0,
            last_time: None,
            particles,
            particle_data: Vec::with_capacity(PARTICLE_LIMIT),
            audio: AudioState::default(),
            audio_phase: 0.0,
        };
        renderer.apply_config(device, queue, cfg);
        renderer
    }

    /// 每幀送入音量與頻帶。停用時忽略輸入；靜音後約半秒完全停止音訊環繪製。
    pub fn set_audio(&mut self, audio: &AudioInput) {
        if self.cfg.audio_ring.enabled && self.cfg.enabled {
            self.audio.set(audio);
        }
    }

    pub fn resize(&mut self, device: &wgpu::Device, _queue: &wgpu::Queue, size: [u32; 2]) {
        let size = valid_size(device, size);
        if self.size == size {
            return;
        }
        self.size = size;
        self.scene_view = scene_texture(device, size);
        self.present_bind = present_bind(
            device,
            &self.present_layout,
            &self.uniform,
            &self.sampler,
            &self.scene_view,
        );
    }

    /// 設定變更時重新讀圖。失敗時記錄訊息並使用預設盤面或背景色。
    pub fn apply_config(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        cfg: &nijikage_wallpaper_core::Config,
    ) {
        let bg_key = picture_key(cfg.background.image_path.as_deref());
        let art_key = picture_key(cfg.disc.art_path.as_deref());
        let mut changed = false;
        if bg_key != self.bg_key {
            self.background =
                load_picture(device, queue, cfg.background.image_path.as_deref(), false);
            self.bg_key = bg_key;
            changed = true;
        }
        if art_key != self.art_key {
            self.art = load_picture(device, queue, cfg.disc.art_path.as_deref(), true);
            self.art_key = art_key;
            changed = true;
        }
        if changed {
            self.scene_bind = scene_bind(
                device,
                &self.scene_layout,
                &self.uniform,
                &self.sampler,
                &self.background,
                &self.art,
            );
        }
        self.cfg = cfg.clone();
        // 舊設定與光碟模式共用唯一路徑，不再讀取已停用的版面欄位。
        if self.cfg.disc.layout_mode != LayoutMode::Desktop {
            self.cfg.disc.layout_mode = LayoutMode::Free;
        }
        if !self.cfg.audio_ring.enabled || !self.cfg.enabled {
            self.audio = AudioState::default();
        }
    }

    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        input: &FrameInput,
    ) {
        if input.size.contains(&0) {
            return;
        }
        self.resize(device, queue, input.size);
        let dt = finite(input.dt, 0.0).clamp(0.0, 0.1);
        let time = finite(input.time, self.last_time.unwrap_or(0.0));
        let elapsed = self
            .last_time
            .map_or(time.max(0.0), |last| (time - last).max(0.0));
        self.last_time = Some(time);
        if self.cfg.enabled && self.cfg.disc.rotate {
            self.phase = (self.phase
                + elapsed * finite(self.cfg.disc.rpm, 2.0).clamp(-60.0, 60.0) * TAU / 60.0)
                .rem_euclid(TAU);
        }
        let width = self.size[0] as f32;
        let height = self.size[1] as f32;
        let (center, radius) = disc_geometry(&self.cfg, [width, height]);
        let desktop = self.cfg.disc.layout_mode == LayoutMode::Desktop;
        if self.cfg.audio_ring.enabled && self.cfg.enabled {
            self.audio.advance(dt);
        }
        self.audio_phase = (self.audio_phase + elapsed * 0.65).rem_euclid(TAU);
        // 強度上限 1.25，避免高靈敏度把外緣過曝；靜音閾值以下不送出繪製指令。
        let audio_strength = (self.audio.current.level
            * finite(self.cfg.audio_ring.sensitivity, 1.0).clamp(0.2, 3.0))
        .min(1.25);
        let cursor = input.cursor.filter(|p| p.iter().all(|v| v.is_finite()));
        let desktop_cursor = cursor.filter(|p| {
            self.cfg.disc.cursor_reflection
                && p[0] >= 0.0
                && p[0] < width
                && p[1] >= 0.0
                && p[1] < height
        });
        let response = 1.0 - (-dt * CURSOR_RESPONSE).exp();
        // 桌面透鏡獨立淡入淡出；保留既有盤面追光與粒子輸入。
        self.desktop_cursor_visibility +=
            (f32::from(desktop_cursor.is_some()) - self.desktop_cursor_visibility) * response;
        let target = cursor
            .filter(|p| {
                self.cfg.disc.cursor_reflection
                    && if desktop {
                        desktop_cursor.is_some()
                    } else {
                        distance(*p, center) < radius
                    }
            })
            .map(|p| [(p[0] - center[0]) / radius, (p[1] - center[1]) / radius])
            .unwrap_or([-0.45, -0.55]);
        for (light, target) in self.light.iter_mut().zip(target) {
            // 離開畫面時透鏡停留原處淡出，避免滑向預設光源。
            if !desktop || desktop_cursor.is_some() {
                *light += (target - *light) * response;
            }
        }
        let fit = match self.cfg.background.fit {
            FitMode::Fill | FitMode::Span => 0.0,
            FitMode::Fit => 1.0,
            FitMode::Stretch => 2.0,
            FitMode::Tile => 3.0,
            FitMode::Center => 4.0,
        };
        let uniforms = Uniforms {
            screen: [width, height, self.phase, f32::from(self.cfg.enabled)],
            disc: [
                center[0],
                center[1],
                radius,
                match self.cfg.disc.style {
                    DiscStyle::Cd => 0.0,
                    DiscStyle::Dvd => 1.0,
                    DiscStyle::Bd => 2.0,
                },
            ],
            light: [
                self.light[0],
                self.light[1],
                if self.cfg.disc.reflection {
                    finite(self.cfg.disc.reflection_intensity, 0.8).clamp(0.0, 1.0)
                } else {
                    0.0
                },
                time.rem_euclid(3600.0),
            ],
            background: [
                linear(self.cfg.background.color[0]),
                linear(self.cfg.background.color[1]),
                linear(self.cfg.background.color[2]),
                f32::from(
                    self.background.loaded && self.cfg.background.mode == BackgroundMode::Image,
                ),
            ],
            image: [
                self.background.original_size[0],
                self.background.original_size[1],
                fit,
                f32::from(self.art.loaded),
            ],
            options: [
                f32::from(self.output_srgb),
                f32::from(desktop),
                f32::from(self.cfg.disc.cursor_reflection),
                0.0,
            ],
            art_transform: art_transform(&self.cfg),
            art_size: [
                self.art.original_size[0],
                self.art.original_size[1],
                0.0,
                0.0,
            ],
            desktop_fx: [
                finite(self.cfg.desktop_fx.intensity, 0.7).clamp(0.0, 1.0),
                finite(self.cfg.desktop_fx.pattern_scale, 1.0).clamp(0.5, 3.0),
                if self.cfg.disc.cursor_reflection {
                    self.desktop_cursor_visibility
                } else {
                    0.0
                },
                0.0,
            ],
            audio: [
                audio_strength,
                finite(self.cfg.audio_ring.width, 0.06).clamp(0.02, 0.15),
                self.audio_phase,
                0.0,
            ],
            bands: std::array::from_fn(|i| {
                std::array::from_fn(|j| self.audio.current.bands[i * 4 + j])
            }),
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniforms));
        self.update_particles(dt, cursor, center, radius);
        if !self.particle_data.is_empty() {
            queue.write_buffer(
                &self.instances,
                0,
                bytemuck::cast_slice(&self.particle_data),
            );
        }
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Nijikage-Wallpaper 畫面"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("背景、光碟與特效"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.scene_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.scene_pipeline);
            pass.set_bind_group(0, &self.scene_bind, &[]);
            pass.draw(0..3, 0..1);
            if !desktop && audio_strength > AUDIO_SILENCE {
                pass.set_pipeline(&self.audio_pipeline);
                pass.draw(0..AUDIO_SEGMENTS * 6, 0..1);
            }
            if !self.particle_data.is_empty() {
                pass.set_pipeline(&self.particle_pipeline);
                pass.set_vertex_buffer(0, self.instances.slice(..));
                pass.draw(0..6, 0..self.particle_data.len() as u32);
            }
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("輸出色彩轉換"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.present_pipeline);
            pass.set_bind_group(0, &self.present_bind, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit(Some(encoder.finish()));
    }

    fn update_particles(
        &mut self,
        dt: f32,
        cursor: Option<[f32; 2]>,
        center: [f32; 2],
        radius: f32,
    ) {
        self.particle_data.clear();
        let intensity = finite(self.cfg.effects.intensity, 0.5).clamp(0.0, 1.0);
        let fade = 1.0 - (-dt * PARTICLE_FADE_RATE).exp();
        for (index, particle) in self.particles.iter_mut().enumerate() {
            let group = index / PARTICLES_PER_KIND;
            let kind = group % 4;
            let on_disc = group < 4;
            if on_disc && self.cfg.disc.layout_mode == LayoutMode::Desktop {
                particle.opacity = 0.0;
                continue;
            }
            let set = if on_disc {
                self.cfg.effects.on_disc
            } else {
                self.cfg.effects.on_desktop
            };
            let rank = (index % PARTICLES_PER_KIND) as f32;
            let target = if self.cfg.enabled && effect_enabled(set, kind) {
                (intensity * PARTICLES_PER_KIND as f32 - rank).clamp(0.0, 1.0)
            } else {
                0.0
            };
            particle.opacity += (target - particle.opacity) * fade;
            if particle.opacity < 0.001 {
                continue;
            }
            let extent = if on_disc {
                [radius * 2.0; 2]
            } else {
                [self.size[0] as f32, self.size[1] as f32]
            };
            let origin = if on_disc {
                [center[0] - radius, center[1] - radius]
            } else {
                [0.0; 2]
            };
            let wind = if set.wind { 0.06 } else { 0.0 };
            let speed = 0.5 + particle.seed;
            let velocity = match kind {
                0 => [wind + 0.012, if on_disc { 0.025 } else { 0.38 }],
                1 => [0.22, -0.012],
                _ => [wind + particle.angle.sin() * 0.018, 0.035],
            };
            let position = [
                origin[0] + particle.position[0] * extent[0],
                origin[1] + particle.position[1] * extent[1],
            ];
            if self.cfg.effects.interactive
                && let Some(cursor) = cursor
            {
                let delta = [position[0] - cursor[0], position[1] - cursor[1]];
                let length = distance(position, cursor);
                if length < PARTICLE_PUSH_RADIUS && length > 0.1 {
                    let force =
                        (1.0 - length / PARTICLE_PUSH_RADIUS) * PARTICLE_PUSH_FORCE * dt / length;
                    particle.push[0] += delta[0] * force;
                    particle.push[1] += delta[1] * force;
                }
            }
            for axis in 0..2 {
                particle.push[axis] *= (-dt * 2.8).exp();
                particle.position[axis] = (particle.position[axis]
                    + dt * (velocity[axis] * speed + particle.push[axis] / extent[axis]))
                    .rem_euclid(1.0);
            }
            particle.angle += dt * if kind >= 2 { 0.65 * speed } else { 0.08 };
            let edge = particle.position[0]
                .min(1.0 - particle.position[0])
                .min(particle.position[1].min(1.0 - particle.position[1]));
            let opacity = particle.opacity * (edge * 22.0).clamp(0.0, 1.0);
            let scale = (self.size[1] as f32 / 1080.0).clamp(0.6, 2.0);
            // 各種類的尺寸常數（1080p 像素）；速度常數在上方 velocity，顏色／形狀在 WGSL particle_fragment。
            let half_size = match kind {
                0 if on_disc => [3.5, 5.0],
                0 => [0.85, 13.0],
                1 => [30.0 + particle.seed * 32.0, 1.0],
                2 => [8.0, 4.0],
                _ => [5.5, 4.0],
            };
            self.particle_data.push(Instance {
                rect: [
                    origin[0] + particle.position[0] * extent[0],
                    origin[1] + particle.position[1] * extent[1],
                    half_size[0] * scale,
                    half_size[1] * scale,
                ],
                detail: [
                    if kind == 0 { -0.12 } else { particle.angle },
                    opacity,
                    kind as f32,
                    f32::from(on_disc),
                ],
            });
        }
    }
}

/// 光碟只讀取自由版面；桌面光學以畫面中心與高度為座標基準。
fn disc_geometry(cfg: &Config, size: [f32; 2]) -> ([f32; 2], f32) {
    let [width, height] = size;
    if cfg.disc.layout_mode != LayoutMode::Desktop {
        let free = &cfg.disc.free;
        return (
            [
                width * finite(free.center_x, 0.92).clamp(-0.5, 1.5),
                height * finite(free.center_y, 0.5).clamp(-0.5, 1.5),
            ],
            height * finite(free.diameter, 0.95).clamp(0.2, 3.0) * 0.5,
        );
    }
    ([width * 0.5, height * 0.5], height * 0.5)
}

fn art_transform(cfg: &Config) -> [f32; 4] {
    let free = &cfg.disc.free;
    [
        finite(free.art_offset_x, 0.0).clamp(-1.0, 1.0),
        finite(free.art_offset_y, 0.0).clamp(-1.0, 1.0),
        finite(free.art_zoom, 1.0).clamp(0.2, 4.0),
        finite(free.art_rotation_deg, 0.0)
            .clamp(-180.0, 180.0)
            .to_radians(),
    ]
}

fn finite(value: f32, fallback: f32) -> f32 {
    if value.is_finite() { value } else { fallback }
}
fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}
fn linear(value: u8) -> f32 {
    let value = value as f32 / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}
fn effect_enabled(set: EffectSet, kind: usize) -> bool {
    match kind {
        0 => set.rain,
        1 => set.wind,
        2 => set.leaves,
        _ => set.sakura,
    }
}
fn valid_size(device: &wgpu::Device, size: [u32; 2]) -> [u32; 2] {
    size.map(|side| side.clamp(1, device.limits().max_texture_dimension_2d))
}
fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}
fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}
fn uniform_entry() -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}
fn scene_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform: &wgpu::Buffer,
    sampler: &wgpu::Sampler,
    background: &Picture,
    art: &Picture,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("場景圖片"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&background.view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&art.view),
            },
        ],
    })
}
fn present_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform: &wgpu::Buffer,
    sampler: &wgpu::Sampler,
    view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("合成輸出"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(view),
            },
        ],
    })
}
fn scene_texture(device: &wgpu::Device, size: [u32; 2]) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("線性色彩合成表面"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SCENE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}
#[allow(clippy::too_many_arguments)]
fn pipeline(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    shader: &wgpu::ShaderModule,
    vertex: &str,
    fragment: &str,
    format: wgpu::TextureFormat,
    buffers: &[wgpu::VertexBufferLayout<'_>],
    blend: bool,
) -> wgpu::RenderPipeline {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("繪製管線配置"),
        bind_group_layouts: &[layout],
        push_constant_ranges: &[],
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(fragment),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(vertex),
            compilation_options: Default::default(),
            buffers,
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fragment),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: blend.then_some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview: None,
        cache: None,
    })
}

fn load_picture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    path: Option<&Path>,
    artwork: bool,
) -> Picture {
    let max_side = device
        .limits()
        .max_texture_dimension_2d
        .min(if artwork { 2048 } else { 4096 });
    let loaded = path.and_then(|path| {
        let result = (|| -> Result<image::DynamicImage, String> {
            let mut reader = image::ImageReader::open(path)
                .map_err(|error| error.to_string())?
                .with_guessed_format()
                .map_err(|error| error.to_string())?;
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(32768);
            limits.max_image_height = Some(32768);
            limits.max_alloc = Some(256 * 1024 * 1024);
            reader.limits(limits);
            reader.decode().map_err(|error| error.to_string())
        })();
        match result {
            Ok(image) if image.width() == 0 || image.height() == 0 => {
                eprintln!("Nijikage-Wallpaper 圖片尺寸為 0，使用預設畫面：{}", path.display());
                None
            }
            Ok(image) => Some(image),
            Err(error) => {
                eprintln!(
                    "Nijikage-Wallpaper 無法讀取圖片 {}，使用預設畫面。{error}",
                    path.display()
                );
                None
            }
        }
    });
    let available = loaded.is_some();
    let original_size = loaded
        .as_ref()
        .map(|image| [image.width() as f32, image.height() as f32])
        .unwrap_or([1.0; 2]);
    let mut image = loaded.unwrap_or_else(|| image::DynamicImage::new_rgba8(1, 1));
    // 完整保留原圖，僅限制 GPU 上傳尺寸；比例以原始尺寸傳入著色器。
    if image.width().max(image.height()) > max_side {
        image = image.resize(max_side, max_side, image::imageops::FilterType::Triangle);
    }
    let image = image.to_rgba8();
    let size = wgpu::Extent3d {
        width: image.width(),
        height: image.height(),
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("使用者圖片"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &image,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(image.width() * 4),
            rows_per_image: Some(image.height()),
        },
        size,
    );
    Picture {
        view: texture.create_view(&Default::default()),
        original_size,
        loaded: available,
    }
}

/// 圖片來源的識別（路徑＋修改時間）；兩次相同就不必重新解碼。
fn picture_key(path: Option<&Path>) -> Option<(PathBuf, Option<SystemTime>)> {
    path.map(|p| {
        (
            p.to_path_buf(),
            std::fs::metadata(p).and_then(|m| m.modified()).ok(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_sanitize_attack_and_half_second_release() {
        let mut audio = AudioState::default();
        audio.set(&AudioInput {
            level: f32::NAN,
            bands: [f32::INFINITY; 16],
        });
        audio.advance(0.1);
        assert_eq!(audio.current.level, 0.0);
        assert_eq!(audio.current.bands, [0.0; 16]);
        for fps in [30, 60, 120] {
            let mut audio = AudioState::default();
            audio.set(&AudioInput {
                level: 2.0,
                bands: [-1.0; 16],
            });
            for _ in 0..fps {
                audio.advance(1.0 / fps as f32);
            }
            assert!((audio.current.level - 1.0).abs() < 0.001);
            assert_eq!(audio.current.bands, [0.0; 16]);
            // 每幀重複送入 0 不得重新開始淡出。
            for _ in 0..fps / 2 + 1 {
                audio.set(&AudioInput::default());
                audio.advance(1.0 / fps as f32);
            }
            assert_eq!(audio.current.level, 0.0);
            assert_eq!(audio.current.bands, [0.0; 16]);
        }
    }

    #[test]
    fn geometry_compatibility_and_uniform_alignment() {
        assert_eq!(size_of::<Uniforms>(), 224);
        let mut cfg = Config::default();
        let free = disc_geometry(&cfg, [1920.0, 1080.0]);
        cfg.disc.layout_mode = LayoutMode::Free;
        assert_eq!(free, disc_geometry(&cfg, [1920.0, 1080.0]));
        cfg.disc.free.art_zoom = 0.2;
        cfg.disc.free.art_offset_x = 0.4;
        cfg.disc.free.art_rotation_deg = 90.0;
        let transform = art_transform(&cfg);
        assert_eq!(transform[..3], [0.4, 0.0, 0.2]);
        assert!((transform[3] - TAU / 4.0).abs() < 0.0001);
        cfg.disc.layout_mode = LayoutMode::Desktop;
        assert_eq!(
            disc_geometry(&cfg, [1920.0, 1080.0]),
            ([960.0, 540.0], 540.0)
        );
        assert_eq!(AUDIO_SEGMENTS, 128);
    }
}
