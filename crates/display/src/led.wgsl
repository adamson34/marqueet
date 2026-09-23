// LED panel rendering in three steps, all drawn with one fullscreen triangle:
//
// 1. fs_gather:    strip texture (one texel per LED, tiled) -> "grid" texture
//                  holding exactly the LEDs visible right now (plus a
//                  1-LED margin), with scrolling and flicker applied.
// 2. fs_blur:      separable blur of the grid texture -> glow texture.
//                  Both are tiny (about 150x20 texels), so this is cheap.
// 3. fs_composite: per screen pixel: find its LED cell, draw a round dot
//                  (lit color or dim unlit color) and add the glow.
//
// Written against WebGL2 / GLES 3.0 capabilities so it runs on a Pi 4.

struct Params {
    // origin.x, origin.y (px), pitch (px), dot diameter (fraction of pitch)
    origin_pitch: vec4<f32>,
    // cols, rows, strip length (LEDs), strip tile width (texels)
    grid: vec4<f32>,
    // base column, fractional scroll, smooth (0/1), wrap (0/1)
    scroll: vec4<f32>,
    // glow strength, flicker amount, time (s), brightness
    look: vec4<f32>,
    // unlit LED color (linear), a unused
    off_color: vec4<f32>,
    // band background (linear), a = 1 if output needs manual sRGB encoding
    bg: vec4<f32>,
    // blur direction x, y; texel size x, y
    blur: vec4<f32>,
};

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var tex_a: texture_2d<f32>;
@group(0) @binding(2) var tex_b: texture_2d<f32>;
@group(0) @binding(3) var samp: sampler;

@vertex
fn vs_fullscreen(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn fetch_led(col: i32, row: i32) -> vec4<f32> {
    let len = i32(p.grid.z);
    var c = col;
    if (p.scroll.w > 0.5) {
        c = ((col % len) + len) % len;
    } else if (col < 0 || col >= len) {
        return vec4<f32>(0.0);
    }
    let tw = i32(p.grid.w);
    let tile = c / tw;
    return textureLoad(tex_a, vec2<i32>(c - tile * tw, tile * i32(p.grid.y) + row), 0);
}

fn hash3(v: vec3<f32>) -> f32 {
    var q = fract(v * vec3<f32>(0.1031, 0.1030, 0.0973));
    q += dot(q, q.yxz + 33.33);
    return fract((q.x + q.y) * q.z);
}

@fragment
fn fs_gather(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let gx = i32(floor(pos.x)) - 1;
    let gy = i32(floor(pos.y)) - 1;
    if (gy < 0 || gy >= i32(p.grid.y)) {
        return vec4<f32>(0.0);
    }
    let base = i32(p.scroll.x);
    var c = fetch_led(base + gx, gy);
    if (p.scroll.z > 0.5) {
        c = mix(c, fetch_led(base + gx + 1, gy), p.scroll.y);
    }
    // Flicker: a slow global shimmer plus faint per-LED noise, like PWM
    // driven LEDs seen by a camera. Keyed to screen position, not content.
    let t = p.look.z;
    let amount = p.look.y;
    let global = 1.0 - amount * 0.05 * (0.5 + 0.5 * sin(t * 7.3) * sin(t * 2.9 + 1.7));
    let n = hash3(vec3<f32>(f32(gx), f32(gy), floor(t * 20.0)));
    let local = 1.0 - amount * 0.22 * n * n * n;
    return vec4<f32>(c.rgb * global * local, c.a);
}

@fragment
fn fs_blur(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = pos.xy * p.blur.zw;
    let step = p.blur.xy * p.blur.zw * 1.4;
    var sum = textureSample(tex_a, samp, uv) * 0.3829;
    sum += textureSample(tex_a, samp, uv + step) * 0.2417;
    sum += textureSample(tex_a, samp, uv - step) * 0.2417;
    sum += textureSample(tex_a, samp, uv + step * 2.0) * 0.0606;
    sum += textureSample(tex_a, samp, uv - step * 2.0) * 0.0606;
    return sum;
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

@fragment
fn fs_composite(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let pitch = p.origin_pitch.z;
    let cell = (pos.xy - p.origin_pitch.xy) / pitch;
    let ci = vec2<i32>(floor(cell));
    let size = vec2<i32>(i32(p.grid.x), i32(p.grid.y));
    let inside = all(ci >= vec2<i32>(0)) && all(ci < size);

    // Glow is sampled everywhere (uniform control flow), clamped at edges.
    let gsize = vec2<f32>(textureDimensions(tex_b));
    let glow = textureSample(tex_b, samp, (cell + vec2<f32>(1.0)) / gsize).rgb;

    let led = textureLoad(tex_a, clamp(ci + vec2<i32>(1), vec2<i32>(0), size + vec2<i32>(1)), 0);

    // Distance from the cell center: 0 at center, 1 at the cell edge midpoint.
    let d = length(fract(cell) - vec2<f32>(0.5)) * 2.0;
    let r = p.origin_pitch.w;
    let aa = 1.6 / pitch;
    let mask = select(0.0, 1.0 - smoothstep(r - aa, r + aa, d), inside);
    let lit = led.a * mask;

    // Lit LEDs have a hot center; unlit LEDs are faint so the grid shows.
    let hot = 1.0 + 0.65 * (1.0 - smoothstep(0.0, r * 0.9, d));
    let on = led.rgb * hot * lit * p.look.w;
    let off = p.off_color.rgb * mask * (1.0 - led.a);
    let halo = glow * p.look.x * (1.0 - lit * 0.6);

    var col = p.bg.rgb + off + on + halo;
    // Very bright LEDs bleach toward white like a real sensor/eye.
    let peak = max(col.r, max(col.g, col.b));
    col = mix(col, vec3<f32>(peak), clamp(peak - 1.0, 0.0, 0.5));
    col = min(col, vec3<f32>(1.0));
    if (p.bg.a > 0.5) {
        col = linear_to_srgb(col);
    }
    return vec4<f32>(col, 1.0);
}
