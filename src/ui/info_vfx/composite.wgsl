// Linear upsampling of the diffuse field; the artwork uses its own image draws.
// The pipeline supplies composite_parameters: opacity and padding.
@group(0) @binding(0) var field_sampler: sampler;
@group(0) @binding(1) var field_texture: texture_2d<f32>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vertex_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var output: VertexOutput;
    output.position = vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    output.uv = uv;
    return output;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let parameters = composite_parameters();
    // Iced sets the viewport to the complete art bounds and clips with scissor.
    // Keeping these UVs independent of scissor avoids stretching clipped art.
    let sample = textureSample(field_texture, field_sampler, input.uv);
    // The texture already contains premultiplied color in the target's space.
    // Fading all channels preserves premultiplication through the blend.
    return sample * parameters.x;
}
