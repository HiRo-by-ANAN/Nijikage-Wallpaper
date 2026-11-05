// 所有顏色在線性空間運算，圖片紋理由 sRGB 自動解碼。
struct Uniforms {
    screen: vec4f, disc: vec4f, light: vec4f,
    background: vec4f, image: vec4f, options: vec4f,
    art_transform: vec4f, art_size: vec4f, desktop_fx: vec4f,
    audio: vec4f, bands: array<vec4f, 4>,
}
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var image_sampler: sampler;
@group(0) @binding(2) var background_image: texture_2d<f32>;
@group(0) @binding(3) var art_image: texture_2d<f32>;

// 視覺調參集中處：中心收斂半徑、細紋頻率／振幅、光帶寬度、繞射／高光強度。
const CENTER_SOFTNESS: f32 = 0.075;
const GROOVE_FREQUENCY: f32 = 1400.0;
const GROOVE_AMPLITUDE: f32 = 0.018;
const SWEEP_WIDTH: f32 = 0.34;
const DISC_DIFFRACTION: f32 = 0.22;
const DISC_GLINT: f32 = 0.20;
// 桌面獨立調參，不共用或改動已驗收的盤面與音訊公式。
const DESKTOP_COLOR_MIX: f32 = 0.55;
const DESKTOP_BAND_FREQUENCY: f32 = 10.0;
const DESKTOP_SWEEP_WIDTH: f32 = 0.42;
const DESKTOP_SWEEP_GAIN: f32 = 0.40;
const DESKTOP_GROOVE_PITCH_PX: f32 = 4.5;
const DESKTOP_GROOVE_CONTRAST: f32 = 0.13;
const DESKTOP_REFRACTION_PX: f32 = 16.0;
const CURSOR_LIGHT_RADIUS_PX: f32 = 220.0;
// 音訊環僅佔外緣；上限與網格段數在 lib.rs，這裡控制亮度、光暈與起伏。
const AUDIO_GLOW: f32 = 0.016;
const AUDIO_RIPPLE: f32 = 0.006;
const AUDIO_BRIGHTNESS: f32 = 0.72;
const TAU: f32 = 6.283185307;

struct Fullscreen {
    @builtin(position) position: vec4f,
    @location(0) uv: vec2f,
}
@vertex fn fullscreen(@builtin(vertex_index) index: u32) -> Fullscreen {
    let p = vec2f(f32((index << 1u) & 2u), f32(index & 2u));
    var out: Fullscreen;
    out.position = vec4f(p * vec2f(2.0, -2.0) + vec2f(-1.0, 1.0), 0.0, 1.0);
    out.uv = p;
    return out;
}

fn background(pixel: vec2f) -> vec3f {
    var color = u.background.rgb;
    if u.background.a < 0.5 { return color; }
    let screen = u.screen.xy;
    let image = max(u.image.xy, vec2f(1.0));
    let ratio = screen / image;
    var scale = max(ratio.x, ratio.y);
    if u.image.z > 0.5 && u.image.z < 1.5 { scale = min(ratio.x, ratio.y); }
    var uv = (pixel - screen * 0.5) / (image * scale) + 0.5;
    if u.image.z > 1.5 && u.image.z < 2.5 { uv = pixel / screen; }
    if u.image.z > 2.5 && u.image.z < 3.5 { uv = fract(pixel / image); }
    if u.image.z > 3.5 { uv = (pixel - screen * 0.5) / image + 0.5; }
    if all(uv >= vec2f(0.0)) && all(uv <= vec2f(1.0)) {
        let sample = textureSampleLevel(background_image, image_sampler, uv, 0.0);
        color = mix(color, sample.rgb, sample.a);
    }
    return color;
}

fn rainbow(value: f32) -> vec3f {
    return 0.5 + 0.5 * cos(value + vec3f(0.0, 2.0943951, 4.1887902));
}

struct Optics {
    diffraction: vec3f,
    sweep: f32,
    glint: f32,
    groove: f32,
}
fn optics(p: vec2f, scale: f32, hue: f32) -> Optics {
    let r2 = dot(p, p);
    let soft_r = sqrt(r2 + CENTER_SOFTNESS * CENTER_SOFTNESS);
    let r = soft_r - CENTER_SOFTNESS;
    let direction = p / soft_r;
    let convergence = r2 / (r2 + 0.025);
    let light_direction = u.light.xy / max(length(u.light.xy), 0.1);
    let tangent = vec2f(-direction.y, direction.x);
    let anisotropic = pow(abs(dot(tangent, light_direction)), 12.0);
    let axis = vec2f(cos(u.screen.z + 0.45), sin(u.screen.z + 0.45));
    let stripe = dot(p, axis) - 0.18;
    let secondary = dot(p, vec2f(-axis.y, axis.x)) + 0.38;
    let sweep = exp(-stripe * stripe / (SWEEP_WIDTH * SWEEP_WIDTH)) * 0.72
        + exp(-secondary * secondary / 0.035) * 0.26;
    // 使用笛卡兒二次方向項，沒有 atan2 接縫；中心各項皆平滑且有限。
    let angular = (direction.x * direction.x - direction.y * direction.y) * 2.4;
    let spectrum = rainbow(r * 15.0 / scale + angular + dot(p, u.light.xy) * 2.2
        - u.screen.z * 1.5 + hue);
    // 每像素相位不超過 2.4 弧度，避免高頻摩爾紋；scale 越大紋路越寬。
    let frequency = min(u.disc.z * 2.4, GROOVE_FREQUENCY) / scale;
    var out: Optics;
    out.groove = cos(r * frequency) * GROOVE_AMPLITUDE * convergence;
    out.diffraction = mix(vec3f(0.5), spectrum, convergence);
    out.sweep = sweep;
    out.glint = anisotropic * convergence;
    return out;
}

fn desktop(pixel: vec2f, bg: vec3f) -> vec3f {
    let strength = u.desktop_fx.x * u.light.z;
    if strength <= 0.0 { return bg; }
    let scale = u.desktop_fx.y;
    let centered = pixel - u.screen.xy * 0.5;
    let p = centered / u.disc.z;
    let soft_r = sqrt(dot(p, p) + 0.075 * 0.075);
    let direction = p / soft_r;
    let radial = (soft_r - 0.075) / scale;
    let axis = vec2f(cos(u.screen.z * 0.45 + 0.45), sin(u.screen.z * 0.45 + 0.45));
    let tangent = vec2f(-direction.y, direction.x);
    let anisotropic = pow(abs(dot(tangent, axis)), 6.0);
    // 多條分離的彩虹帶，中間保留原圖；縮放同時控制帶寬與細紋間距。
    let angular = (direction.x * direction.x - direction.y * direction.y) * 1.4;
    let phase = radial * DESKTOP_BAND_FREQUENCY + angular - u.screen.z * 0.7;
    let band = smoothstep(0.05, 0.85, cos(phase));
    let spectrum = rainbow(radial * 17.0 + angular * 2.0 - u.screen.z);
    let stripe = dot(p, axis) - sin(u.screen.z * 0.35) * 0.65;
    let sweep = exp(-pow(stripe / (DESKTOP_SWEEP_WIDTH * scale), 2.0));
    let grooves = cos(length(centered) * TAU / (DESKTOP_GROOVE_PITCH_PX * scale));
    // 有限半徑的慣性透鏡；無游標時由 CPU 平滑淡出，不使用預設光源假裝游標。
    let delta = pixel - (u.disc.xy + u.light.xy * u.disc.z);
    let local_r = length(delta) / (CURSOR_LIGHT_RADIUS_PX * scale);
    let lens = 1.0 - smoothstep(0.0, 1.0, local_r);
    let local = lens * u.desktop_fx.z;
    var base = bg;
    if local > 0.001 {
        let bend = delta / sqrt(dot(delta, delta) + 36.0);
        let refracted = background(pixel + bend * DESKTOP_REFRACTION_PX * local * strength);
        base = mix(bg, refracted, local * strength);
    }
    let weights = vec3f(0.2126, 0.7152, 0.0722);
    let luminance = dot(base, weights);
    // 彩色目標保留局部明度；暗底仍有反光，避免再次被 base*(1-base) 壓到看不見。
    let tint = spectrum * (0.035 + luminance * 0.965) / max(dot(spectrum, weights), 0.18);
    var color = mix(base, tint, band * strength * DESKTOP_COLOR_MIX);
    color *= 1.0 + strength * (sweep * DESKTOP_SWEEP_GAIN * (1.0 + u.audio.x * 0.075)
        + grooves * DESKTOP_GROOVE_CONTRAST * (0.25 + anisotropic * 0.75));
    let edge = exp(-pow((local_r - 0.58) / 0.18, 2.0))
        * (1.0 - smoothstep(0.85, 1.0, local_r)) * u.desktop_fx.z;
    let cursor_spectrum = rainbow(local_r * 10.0 + dot(direction, axis) * 2.0);
    let cursor_tint = cursor_spectrum * (0.06 + luminance) / max(dot(cursor_spectrum, weights), 0.20);
    color = mix(color, cursor_tint, edge * strength * 0.65);
    color += vec3f(local * strength * 0.12);
    return clamp(color, vec3f(0.0), vec3f(1.0));
}

@fragment fn scene(in: Fullscreen) -> @location(0) vec4f {
    let pixel = in.position.xy;
    let bg = background(pixel);
    if u.screen.w < 0.5 { return vec4f(bg, 1.0); }
    if u.options.y > 0.5 { return vec4f(desktop(pixel, bg), 1.0); }
    let p = (pixel - u.disc.xy) / u.disc.z;
    let r = length(p);
    let aa = 1.25 / u.disc.z;
    if r > 1.0 + aa { return vec4f(bg, 1.0); }
    var base = vec3f(0.53, 0.57, 0.62);
    var shadow = vec3f(0.19, 0.22, 0.27);
    var hue = 0.0;
    if u.disc.w > 0.5 && u.disc.w < 1.5 {
        base = vec3f(0.59, 0.43, 0.31);
        shadow = vec3f(0.26, 0.12, 0.25);
        hue = 1.1;
    }
    if u.disc.w > 1.5 {
        base = vec3f(0.26, 0.38, 0.63);
        shadow = vec3f(0.12, 0.14, 0.29);
        hue = 3.4;
    }
    let optical = optics(p, 1.0, hue);
    var color = mix(shadow, base, 0.62 + 0.18 * r * r + optical.groove);
    if u.image.w > 0.5 {
        // p / 2 為「直徑＝1」座標。先減位移，再反向旋轉，最後除 zoom。
        // 短邊＝直徑，長邊保留完整原圖；圖片不使用光碟旋轉相位。
        let q = p * 0.5 - u.art_transform.xy;
        let c = cos(u.art_transform.w);
        let s = sin(u.art_transform.w);
        let image_extent = u.art_size.xy / min(u.art_size.x, u.art_size.y);
        let art_uv = vec2f(c * q.x + s * q.y, -s * q.x + c * q.y)
            / (u.art_transform.z * image_extent) + 0.5;
        // 解析遮罩只用盤面像素寬度，不依越界取樣延伸顏色；不重複平鋪。
        let edge_pixels = min(art_uv, 1.0 - art_uv) * image_extent * (2.0 * u.disc.z * u.art_transform.z);
        let coverage = smoothstep(-0.5, 0.5, min(edge_pixels.x, edge_pixels.y));
        if coverage > 0.0 {
            let art = textureSampleLevel(art_image, image_sampler, art_uv, 0.0);
            let luminance = dot(art.rgb, vec3f(0.2126, 0.7152, 0.0722));
            let toned = mix(shadow * 0.12, base * 1.32, pow(luminance, 0.8));
            let detail = toned * 0.78 + art.rgb * base * 0.38;
            color = mix(color, detail, art.a * coverage);
        }
    }
    color += u.light.z * ((optical.diffraction * DISC_DIFFRACTION + vec3f(0.07)) * optical.sweep
        + vec3f(DISC_GLINT) * optical.glint * (0.7 + optical.groove * 7.0));
    // 僅保留最外側薄邊；中心不設孔、內環、極角或除以零的運算。
    let rim = smoothstep(0.983, 0.993, r);
    color = mix(color, base * 0.68 + u.light.z * vec3f(0.18) * optical.sweep, rim);
    color *= 1.0 - smoothstep(0.996, 1.0, r) * 0.25;
    let coverage = 1.0 - smoothstep(1.0 - aa, 1.0 + aa, r);
    return vec4f(mix(bg, color, coverage), 1.0);
}

struct AudioVertex {
    @builtin(position) position: vec4f,
    @location(0) p: vec2f,
}
@vertex fn audio_vertex(@builtin(vertex_index) index: u32) -> AudioVertex {
    // 128 段環形四邊形，僅覆蓋外緣和光暈；與 lib.rs AUDIO_SEGMENTS 一致。
    let corners = array<vec2f, 6>(vec2f(0.0, 0.0), vec2f(1.0, 0.0), vec2f(0.0, 1.0),
        vec2f(0.0, 1.0), vec2f(1.0, 0.0), vec2f(1.0, 1.0));
    let corner = corners[index % 6u];
    let angle = (f32(index / 6u) + corner.x) * TAU / 128.0;
    let radius = mix(1.0 - u.audio.y * 1.6 - 0.01, 1.06, corner.y);
    let p = vec2f(cos(angle), sin(angle)) * radius;
    let pixel = u.disc.xy + p * u.disc.z;
    var out: AudioVertex;
    out.position = vec4f(pixel / u.screen.xy * vec2f(2.0, -2.0) + vec2f(-1.0, 1.0), 0.0, 1.0);
    out.p = p;
    return out;
}
fn band(index: u32) -> f32 {
    let i = index % 16u;
    return u.bands[i / 4u][i % 4u];
}
@fragment fn audio_fragment(in: AudioVertex) -> @location(0) vec4f {
    let r = length(in.p);
    // 只在外緣使用極角，遠離中心；16 頻帶每半圈重複，對側亮度完全相同。
    let angle = atan2(in.p.y, in.p.x);
    let coordinate = fract(angle / TAU) * 32.0;
    let index = u32(floor(coordinate));
    let interpolation = 0.5 - 0.5 * cos(fract(coordinate) * 3.14159265);
    let energy = mix(band(index), band(index + 1u), interpolation);
    let strength = u.audio.x;
    let edge = 0.998 + (energy - 0.5) * AUDIO_RIPPLE * strength;
    let inner = edge - u.audio.y;
    let aa = 1.0 / u.disc.z;
    let body = smoothstep(inner, inner + u.audio.y * 0.42, r)
        * (1.0 - smoothstep(edge - aa, edge + aa, r));
    let filament = exp(-pow((r - edge + u.audio.y * 0.13) / max(u.audio.y * 0.12, aa), 2.0));
    let glow = exp(-pow((r - edge) / AUDIO_GLOW, 2.0)) * 0.24;
    let brightness = (0.24 + 0.76 * energy) * strength;
    let color = rainbow(angle - u.audio.z) * 0.78 + vec3f(0.08 + filament * 0.14);
    let alpha = clamp((body * 0.64 + filament * 0.25 + glow) * brightness * AUDIO_BRIGHTNESS, 0.0, 0.85);
    return vec4f(color, alpha);
}

struct ParticleVertex {
    @builtin(position) position: vec4f,
    @location(0) local: vec2f,
    @location(1) @interpolate(flat) detail: vec4f,
    @location(2) pixel: vec2f,
}
@vertex fn particle_vertex(@builtin(vertex_index) index: u32,
    @location(0) rect: vec4f, @location(1) detail: vec4f) -> ParticleVertex {
    let corners = array<vec2f, 6>(vec2f(-1.0, -1.0), vec2f(1.0, -1.0), vec2f(-1.0, 1.0),
        vec2f(-1.0, 1.0), vec2f(1.0, -1.0), vec2f(1.0, 1.0));
    let local = corners[index];
    let offset = local * rect.zw;
    let c = cos(detail.x);
    let s = sin(detail.x);
    let pixel = rect.xy + vec2f(c * offset.x - s * offset.y, s * offset.x + c * offset.y);
    var out: ParticleVertex;
    out.position = vec4f(pixel / u.screen.xy * vec2f(2.0, -2.0) + vec2f(-1.0, 1.0), 0.0, 1.0);
    out.local = local;
    out.detail = detail;
    out.pixel = pixel;
    return out;
}
@fragment fn particle_fragment(in: ParticleVertex) -> @location(0) vec4f {
    let p = in.local;
    let kind = in.detail.z;
    var alpha = in.detail.y;
    var color = vec3f(0.60, 0.76, 0.87);
    if in.detail.w > 0.5 {
        let r = length((in.pixel - u.disc.xy) / u.disc.z);
        alpha *= (1.0 - smoothstep(0.985, 1.0, r));
    }
    if kind < 0.5 {
        if in.detail.w > 0.5 {
            let r = length(p);
            let rim = exp(-pow((r - 0.75) / 0.12, 2.0));
            let glint = exp(-dot(p + vec2f(0.28, 0.32), p + vec2f(0.28, 0.32)) * 32.0);
            color = mix(vec3f(0.22, 0.37, 0.48), vec3f(0.93, 0.98, 1.0), glint);
            alpha *= (1.0 - smoothstep(0.8, 1.0, r)) * (0.12 + rim * 0.52 + glint * 0.8);
        } else {
            alpha *= (1.0 - smoothstep(0.15, 1.0, abs(p.x))) * (1.0 - abs(p.y)) * 0.55;
        }
    } else if kind < 1.5 {
        let curve = p.y - sin(p.x * 2.5) * 0.35;
        alpha *= exp(-curve * curve * 12.0) * pow(1.0 - abs(p.x), 0.7) * 0.18;
        color = vec3f(0.72, 0.81, 0.86);
    } else if kind < 2.5 {
        let shape = p.x * p.x + p.y * p.y * (1.2 + abs(p.x) * 2.0);
        alpha *= 1.0 - smoothstep(0.72, 1.0, shape);
        let vein = exp(-abs(p.y + p.x * 0.12) * 30.0);
        color = mix(vec3f(0.36, 0.15, 0.025), vec3f(0.76, 0.40, 0.075), p.x * 0.4 + 0.5);
        color = mix(color, vec3f(0.22, 0.11, 0.025), vein * 0.5);
    } else {
        let shape = p.x * p.x + p.y * p.y * (0.9 + p.x * 0.35);
        let notch = exp(-pow(p.y * 8.0, 2.0)) * smoothstep(0.4, 1.0, p.x) * 0.65;
        alpha *= (1.0 - smoothstep(0.72, 1.0, shape)) * (1.0 - notch);
        color = mix(vec3f(0.95, 0.66, 0.76), vec3f(0.65, 0.22, 0.38), (1.0 - p.x) * 0.3);
    }
    return vec4f(color, alpha);
}
