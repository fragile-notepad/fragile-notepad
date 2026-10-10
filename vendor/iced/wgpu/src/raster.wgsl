@group(0) @binding(0) var cached_pixels: texture_2d<f32>;
@group(0) @binding(1) var nearest_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    // A single triangle covers the viewport without an internal diagonal edge.
    let corners = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(2.0, 0.0),
        vec2<f32>(0.0, 2.0),
    );
    let uv = corners[vertex_index];

    var output: VertexOutput;
    output.position = vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    output.uv = uv;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // Captured pixels are already premultiplied; preserve their color and alpha.
    return textureSample(cached_pixels, nearest_sampler, input.uv);
}
