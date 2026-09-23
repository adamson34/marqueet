// Composites a CPU-drawn UI canvas (premultiplied RGBA, sRGB) at 1:1 pixels
// into its rect, with an opacity for fades.

struct U {
    // x, y, w, h of the layer (px)
    rect: vec4<f32>,
    // opacity, manual sRGB encode (0/1), unused, unused
    extra: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var tex: texture_2d<f32>;

@vertex
fn vs_fullscreen(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

@fragment
fn fs_ui(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let texel = vec2<i32>(floor(pos.xy - u.rect.xy));
    let size = vec2<i32>(textureDimensions(tex));
    let c = textureLoad(tex, clamp(texel, vec2<i32>(0), size - vec2<i32>(1)), 0);
    var rgb = c.rgb;
    if (u.extra.y > 0.5) {
        rgb = linear_to_srgb(rgb);
    }
    return vec4<f32>(rgb, c.a) * u.extra.x;
}
