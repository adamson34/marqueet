// Composites a CPU-drawn UI canvas (premultiplied RGBA, sRGB) at 1:1 pixels
// into its rect, with an opacity for fades.

struct U {
    // x, y, w, h of the layer (px)
    rect: vec4<f32>,
    // opacity, manual sRGB encode (0/1), unused, unused
    extra: vec4<f32>,
    // Scrolling strip: offset px, strip width px (0 = a plain layer), tile
    // width px, strip height px. The strip is packed in tiles stacked top to
    // bottom (see `pack_tiles` in gpu.rs).
    scroll: vec4<f32>,
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
    var local = floor(pos.xy - u.rect.xy);
    if (u.scroll.y > 0.5) {
        let sx = (local.x + u.scroll.x) % u.scroll.y;
        let tile = floor(sx / u.scroll.z);
        local = vec2<f32>(sx - tile * u.scroll.z, local.y + tile * u.scroll.w);
    }
    let texel = vec2<i32>(local);
    let size = vec2<i32>(textureDimensions(tex));
    let c = textureLoad(tex, clamp(texel, vec2<i32>(0), size - vec2<i32>(1)), 0);
    var rgb = c.rgb;
    if (u.extra.y > 0.5) {
        rgb = linear_to_srgb(rgb);
    }
    return vec4<f32>(rgb, c.a) * u.extra.x;
}
