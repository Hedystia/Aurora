struct Camera {
    view_projection: mat4x4f,
}
@group(0) @binding(0) var<uniform> camera: Camera;

struct Varyings {
    @builtin(position) position: vec4f,
    @location(0) color: vec3f,
}

@vertex
fn vertex(
    @location(0) position: vec3f,
    @location(1) normal: vec3f,
    @location(2) model_x: vec4f,
    @location(3) model_y: vec4f,
    @location(4) model_z: vec4f,
    @location(5) model_w: vec4f,
    @location(6) tint: vec4f,
) -> Varyings {
    let model = mat4x4f(model_x, model_y, model_z, model_w);
    let world_normal = normalize((model * vec4f(normal, 0.0)).xyz);
    let sun = normalize(vec3f(-0.4, 0.8, 0.5));
    let light = 0.28 + 0.72 * max(dot(world_normal, sun), 0.0);
    return Varyings(camera.view_projection * model * vec4f(position, 1.0), tint.rgb * light);
}

@fragment
fn fragment(input: Varyings) -> @location(0) vec4f {
    return vec4f(input.color, 1.0);
}
