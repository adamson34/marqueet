// Takeover background: team-color diagonal stripes that drift slowly, a faint
// dot texture, up to two rounded boxes (score box, note pill), and an amber
// bar along the bottom. Output is premultiplied alpha so it fades in and out
// over the widget area. GLES 3.0 friendly.

struct Bg {
    // x, y, w, h of the takeover area (px)
    rect: vec4<f32>,
    // stripe color A (linear), a = opacity
    stripe_a: vec4<f32>,
    // stripe color B (linear), a = time (s)
    stripe_b: vec4<f32>,
    // bottom bar color (linear), a = 1 if output needs manual sRGB encoding
    bar: vec4<f32>,
    // box 0: x, y, w, h (px; w = 0 disables)
    box0: vec4<f32>,
    // box 0 color (linear), a = corner radius (px)
    box0_color: vec4<f32>,
    box1: vec4<f32>,
    box1_color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> b: Bg;

@vertex
fn vs_fullscreen(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn rounded_box(p: vec2<f32>, rect: vec4<f32>, radius: f32) -> f32 {
    let half = rect.zw * 0.5;
    let q = abs(p - (rect.xy + half)) - half + vec2<f32>(radius);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

fn paint_box(col: vec3<f32>, p: vec2<f32>, rect: vec4<f32>, color: vec4<f32>) -> vec3<f32> {
    let d = rounded_box(p, rect, color.a);
    let m = select(0.0, 1.0 - smoothstep(-0.75, 0.75, d), rect.z > 0.0);
    return mix(col, color.rgb, m);
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

@fragment
fn fs_bg(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let local = pos.xy - b.rect.xy;
    // Scale the pattern with the screen so it looks the same at any size.
    let s = max(b.rect.z / 1920.0, 0.4);
    let t = b.stripe_b.a;

    // Stripes at about -35 degrees, drifting to the right.
    let u = local.x * 0.819 + local.y * 0.574 - t * 24.0 * s;
    let band = fract(u / (120.0 * s)) < 0.5;
    var col = select(b.stripe_b.rgb, b.stripe_a.rgb, band);

    // Faint LED-style dot grid.
    let cell = fract(local / (14.0 * s)) - vec2<f32>(0.5);
    let dots = 1.0 - smoothstep(0.17, 0.24, length(cell));
    col = mix(col, vec3<f32>(1.0), dots * 0.06);

    col = paint_box(col, pos.xy, b.box0, b.box0_color);
    col = paint_box(col, pos.xy, b.box1, b.box1_color);

    // Amber bar along the bottom edge.
    if (local.y > b.rect.w - 8.0 * s) {
        col = b.bar.rgb;
    }
    if (b.bar.a > 0.5) {
        col = linear_to_srgb(col);
    }
    let o = b.stripe_a.a;
    return vec4<f32>(col * o, o);
}
