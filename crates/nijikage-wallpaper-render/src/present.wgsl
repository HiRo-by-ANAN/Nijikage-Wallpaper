// 中繼紋理取樣回線性色彩，非 sRGB 輸出表面需在此編碼。
struct Uniforms {
    screen: vec4f, disc: vec4f, light: vec4f,
    background: vec4f, image: vec4f, options: vec4f,
    art_transform: vec4f, art_size: vec4f, desktop_fx: vec4f, audio: vec4f, bands: array<vec4f, 4>,
}
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var image_sampler: sampler;
@group(0) @binding(2) var scene_image: texture_2d<f32>;
struct Vertex {
    @builtin(position) position: vec4f,
    @location(0) uv: vec2f,
}
@vertex fn fullscreen(@builtin(vertex_index) index: u32) -> Vertex {
    let p = vec2f(f32((index << 1u) & 2u), f32(index & 2u));
    var out: Vertex;
    out.position = vec4f(p * vec2f(2.0, -2.0) + vec2f(-1.0, 1.0), 0.0, 1.0);
    out.uv = p;
    return out;
}
@fragment fn present(in: Vertex) -> @location(0) vec4f {
    var color = textureSampleLevel(scene_image, image_sampler, in.uv, 0.0).rgb;
    if u.options.x < 0.5 {
        color = select(1.055 * pow(max(color, vec3f(0.0)), vec3f(1.0 / 2.4)) - 0.055,
            color * 12.92, color <= vec3f(0.0031308));
    }
    return vec4f(color, 1.0);
}
