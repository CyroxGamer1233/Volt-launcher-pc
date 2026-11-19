// Natural Motion Blur, the two passes the mod runs, ported to WGSL.
//
// `fs_blur` is `velocity_pre.fsh` / `velocity_f5.fsh`: the frame is resampled
// along the camera's screen-space velocity with jittered taps, weighted by the
// mod's Blackman-sinc window when the WEIGHTED profile is on, and averaged in
// squared (gamma-2.2) space exactly like the mod does.
//
// `fs_blend` is `accumulation_mix.fsh` / `accumulation_max.fsh`: the smeared
// frame mixed with the previous output, in linear light for the mix and as a
// faded maximum for the max variant.

struct Blur {
    // Viewport width, height, sample count, weighted profile (0 / 1).
    size: vec4<f32>,
    // UV step per tap.
    step: vec4<f32>,
};

struct Blend {
    // Blend factor, algorithm (0 mix, 1 max), history primed, unused.
    params: vec4<f32>,
};

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_samp: sampler;
@group(0) @binding(2) var<uniform> blur_u: Blur;

@group(0) @binding(4) var cur_tex: texture_2d<f32>;
@group(0) @binding(5) var hist_tex: texture_2d<f32>;
@group(0) @binding(6) var blend_samp: sampler;
@group(0) @binding(7) var<uniform> blend_u: Blend;

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> Out {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: Out;
    out.clip = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(p.x, 1.0 - p.y);
    return out;
}

// The mod's interleaved gradient noise, so the taps do not band.
fn noise(pos: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(0.06711056 * pos.x + 0.00583715 * pos.y));
}

// `velocity_pre.fsh`'s Blackman-sinc window.
fn blackman_sinc(u: f32) -> f32 {
    let z = 1.89 * u;
    var sinc = 1.0;
    if (abs(z) >= 0.0001) {
        sinc = sin(3.14159265 * z) / (3.14159265 * z);
    }
    return sinc * (0.42 + 0.5 * cos(3.14159265 * u) + 0.08 * cos(6.2831853 * u));
}

@fragment
fn fs_blur(in: Out) -> @location(0) vec4<f32> {
    let samples = i32(blur_u.size.z);
    let weighted = blur_u.size.w > 0.5;
    let seed = in.uv * blur_u.size.xy;
    let mid = f32(samples) * 0.5;
    var sum = vec3<f32>(0.0);
    var total = 0.0;
    for (var i = 0; i < samples; i = i + 1) {
        let fi = f32(i);
        let jitter = noise(seed + vec2<f32>(fi, fi * 1.4));
        let offset = fi - mid + jitter;
        let uv = in.uv + offset * blur_u.step.xy;
        let c = textureSampleLevel(src_tex, src_samp, uv, 0.0).rgb;
        if (weighted) {
            let w = blackman_sinc(offset * 2.0 / f32(samples));
            sum = sum + pow(max(c, vec3<f32>(0.0)), vec3<f32>(2.2)) * w;
            total = total + w;
        } else {
            sum = sum + c * c;
            total = total + 1.0;
        }
    }
    if (total <= 0.0001) {
        return textureSampleLevel(src_tex, src_samp, in.uv, 0.0);
    }
    if (weighted) {
        let lit = clamp(sum / total, vec3<f32>(0.0), vec3<f32>(1.0));
        return vec4<f32>(pow(lit, vec3<f32>(1.0 / 2.2)), 1.0);
    }
    return vec4<f32>(sqrt(sum / total), 1.0);
}

@fragment
fn fs_blend(in: Out) -> @location(0) vec4<f32> {
    let curr = textureSampleLevel(cur_tex, blend_samp, in.uv, 0.0).rgb;
    if (blend_u.params.z < 0.5) {
        return vec4<f32>(curr, 1.0);
    }
    let prev = textureSampleLevel(hist_tex, blend_samp, in.uv, 0.0).rgb;
    let factor = clamp(blend_u.params.x, 0.0, 1.0);
    if (blend_u.params.y > 0.5) {
        // accumulation_max: the previous frame faded, the brighter pixel kept.
        return vec4<f32>(max(curr, prev * factor), 1.0);
    }
    // accumulation_mix / frame blending, in linear light.
    let mixed = mix(curr * curr, prev * prev, factor);
    return vec4<f32>(sqrt(max(mixed, vec3<f32>(0.0))), 1.0);
}
