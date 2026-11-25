struct Camera {
    view_projection: mat4x4f,
    light: vec4f,
    eye: vec4f,
}
@group(0) @binding(0) var<uniform> camera: Camera;

@group(1) @binding(0) var base_color: texture_2d<f32>;
@group(1) @binding(1) var base_color_sampler: sampler;

struct Varyings {
    @builtin(position) position: vec4f,
    @location(0) world: vec3f,
    @location(1) normal: vec3f,
    @location(2) uv: vec2f,
    @location(3) color: vec3f,
    @location(4) params: vec2f,
}

@vertex
fn vertex(
    @location(0) position: vec3f,
    @location(1) normal: vec3f,
    @location(7) uv: vec2f,
    @location(2) model_x: vec4f,
    @location(3) model_y: vec4f,
    @location(4) model_z: vec4f,
    @location(5) model_w: vec4f,
    @location(6) tint: vec4f,
    @location(8) params: vec4f,
) -> Varyings {
    let model = mat4x4f(model_x, model_y, model_z, model_w);
    let world = model * vec4f(position, 1.0);
    let world_normal = normalize(cross(model_y.xyz, model_z.xyz) * normal.x + cross(model_z.xyz, model_x.xyz) * normal.y + cross(model_x.xyz, model_y.xyz) * normal.z);
    return Varyings(camera.view_projection * world, world.xyz, world_normal, uv, tint.rgb, params.xy);
}

@fragment
fn fragment(input: Varyings) -> @location(0) vec4f {
    let albedo = input.color * textureSample(base_color, base_color_sampler, input.uv).rgb;
    let n = normalize(input.normal);
    let l = normalize(camera.light.xyz);
    let v = normalize(camera.eye.xyz - input.world);
    let h = normalize(l + v);
    let metallic = clamp(input.params.x, 0.0, 1.0);
    let roughness = clamp(input.params.y, 0.05, 1.0);
    let ndl = max(dot(n, l), 0.0);
    let diffuse = albedo * (1.0 - metallic) * (0.24 + 0.76 * ndl);
    let f0 = mix(vec3f(0.04), albedo, metallic);
    let shininess = mix(96.0, 8.0, roughness);
    let specular = pow(max(dot(n, h), 0.0), shininess) * (1.0 - roughness);
    return vec4f(diffuse + f0 * specular * (0.4 + 0.6 * ndl), 1.0);
}
