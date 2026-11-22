struct Globals {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    cam_pos: vec4<f32>,
    fog_color: vec4<f32>,
    sky_top: vec4<f32>,
    sky_horizon: vec4<f32>,
    params: vec4<f32>,
};
@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var sky_tex: texture_2d<f32>;
@group(1) @binding(1) var sky_s: sampler;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VOut {
    var p = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    var o: VOut;
    o.clip = vec4<f32>(p[i], 0.0, 1.0);
    o.ndc = p[i];
    return o;
}

@fragment
fn fs_main(i: VOut) -> @location(0) vec4<f32> {
    let far = g.inv_view_proj * vec4<f32>(i.ndc, 1.0, 1.0);
    let dir = normalize(far.xyz / far.w - g.cam_pos.xyz);
    if (g.params.z > 0.5) {
        let u = atan2(dir.z, dir.x) / 6.2831853 + 0.5;
        let v = 0.5 - asin(clamp(dir.y, -1.0, 1.0)) / 3.14159265;
        return vec4<f32>(textureSampleLevel(sky_tex, sky_s, vec2<f32>(u, v), 0.0).rgb, 1.0);
    }
    let t = pow(clamp(dir.y, 0.0, 1.0), 0.6);
    return vec4<f32>(mix(g.sky_horizon.rgb, g.sky_top.rgb, t), 1.0);
}
