struct SolidVertexInput {
    @builtin(vertex_index) vertex_index: u32,
    @location(0) color: vec4<f32>,
    @location(1) pos: vec2<f32>,
    @location(2) scale: vec2<f32>,
    @location(3) border_color: vec4<f32>,
    @location(4) border_radius: vec4<f32>,
    @location(5) border_width: f32,
    @location(6) shadow_color: vec4<f32>,
    @location(7) shadow_offset: vec2<f32>,
    @location(8) shadow_blur_radius: f32,
    @location(9) snap: u32,
}

struct SolidVertexOutput {
    @builtin(position) position: vec4<f32>,
    // These values are constant for every vertex in an instance.
    @location(0) @interpolate(flat) color: vec4<f32>,
    @location(1) @interpolate(flat) border_color: vec4<f32>,
    @location(2) @interpolate(flat) pos: vec2<f32>,
    @location(3) @interpolate(flat) scale: vec2<f32>,
    @location(4) @interpolate(flat) border_radius: vec4<f32>,
    @location(5) @interpolate(flat) border_width: f32,
    @location(6) @interpolate(flat) shadow_color: vec4<f32>,
    @location(7) @interpolate(flat) shadow_offset: vec2<f32>,
    @location(8) @interpolate(flat) shadow_blur_radius: f32,
}

@vertex
fn solid_vs_main(input: SolidVertexInput) -> SolidVertexOutput {
    var out: SolidVertexOutput;

    var bounds = shadow_expanded_bounds(input.pos, input.scale, input.shadow_offset, input.shadow_blur_radius) * globals.scale;
    var pos: vec2<f32> = bounds.xy;
    var scale: vec2<f32> = bounds.zw;

    var pos_snap = vec2<f32>(0.0, 0.0);
    var scale_snap = vec2<f32>(0.0, 0.0);

    if bool(input.snap) {
        pos_snap = round(pos + nudge) - pos;
        scale_snap = round(pos + scale + nudge) - pos - pos_snap - scale;
    }

    let border_radius = min(input.border_radius, vec4(min(input.scale.x, input.scale.y) / 2.0));

    var transform: mat4x4<f32> = mat4x4<f32>(
        vec4<f32>(scale.x + scale_snap.x + 1.0, 0.0, 0.0, 0.0),
        vec4<f32>(0.0, scale.y + scale_snap.y + 1.0, 0.0, 0.0),
        vec4<f32>(0.0, 0.0, 1.0, 0.0),
        vec4<f32>(pos + pos_snap - vec2<f32>(0.5, 0.5), 0.0, 1.0)
    );

    out.position = globals.transform * transform * vec4<f32>(vertex_position(input.vertex_index), 0.0, 1.0);
    out.color = premultiply(input.color);
    out.border_color = premultiply(input.border_color);
    out.pos = input.pos * globals.scale + pos_snap;
    out.scale = input.scale * globals.scale + scale_snap;
    out.border_radius = border_radius * globals.scale;
    out.border_width = input.border_width * globals.scale;
    out.shadow_color = premultiply(input.shadow_color);
    out.shadow_offset = input.shadow_offset * globals.scale;
    out.shadow_blur_radius = input.shadow_blur_radius * globals.scale;

    return out;
}

@fragment
fn solid_fs_main(
    input: SolidVertexOutput
) -> @location(0) vec4<f32> {
    // Large surfaces need no edge, border, or shadow work in their interior.
    // Leave a conservative inset so fractional edges and rounded corners still
    // use the same distance field and antialiasing as before.
    let radius = max(max(input.border_radius.x, input.border_radius.y),
        max(input.border_radius.z, input.border_radius.w));
    let inset = max(radius, input.border_width) + 0.5;
    let center_distance = abs(input.position.xy - input.pos - input.scale * 0.5);
    if all(center_distance < input.scale * 0.5 - vec2<f32>(inset)) {
        return input.color;
    }

    var mixed_color: vec4<f32> = input.color;

    var dist = rounded_box_sdf(
        -(input.position.xy - input.pos - input.scale * 0.5) * 2.0,
        input.scale,
        input.border_radius * 2.0
    ) / 2.0;

    if (input.border_width > 0.0) {
        mixed_color = mix(
            input.color,
            input.border_color,
            clamp(0.5 + dist + input.border_width, 0.0, 1.0)
        );
    }

    var quad_alpha: f32 = clamp(0.5-dist, 0.0, 1.0);

    let quad_color = mixed_color * quad_alpha;

    if input.shadow_color.a > 0.0 {
        return mix_shadow(
            quad_color,
            quad_alpha,
            input.position.xy,
            input.pos,
            input.scale,
            input.border_radius,
            input.shadow_color,
            input.shadow_offset,
            input.shadow_blur_radius
        );
    }

    return quad_color;
}
