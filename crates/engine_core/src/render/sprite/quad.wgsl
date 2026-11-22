struct VertexOut {
    @builtin(position) position: vec4f,
    @location(0) uv: vec2f,
}

@vertex
fn vertex(@location(0) position: vec2f, @location(1) uv: vec2f) -> VertexOut {
    return VertexOut(vec4f(position, 0.0, 1.0), uv);
}

@group(0) @binding(0) var sprite: texture_2d<f32>;
@group(0) @binding(1) var sprite_sampler: sampler;

@fragment
fn fragment(vertex_in: VertexOut) -> @location(0) vec4f {
    return textureSample(sprite, sprite_sampler, vertex_in.uv);
}
