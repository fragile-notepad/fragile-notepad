// Rasterize the same endpoint-inclusive grid as the software fallback.
// The pipeline supplies field_parameters: time, dark, width, height, and
// LINEAR_TARGET to match the main target's color transfer.

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

fn soft_edge(value: f32) -> f32 {
    let t = clamp(value, 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let parameters = field_parameters();
    let time = parameters.x;
    let dark = parameters.y > 0.5;
    let colors = array<vec3<f32>, 3>(
        select(vec3<f32>(66.0, 145.0, 203.0), vec3<f32>(133.0, 195.0, 241.0), dark),
        select(vec3<f32>(145.0, 116.0, 205.0), vec3<f32>(185.0, 163.0, 235.0), dark),
        select(vec3<f32>(219.0, 146.0, 139.0), vec3<f32>(238.0, 181.0, 172.0), dark),
    );
    // Fragment positions are texel centers; CPU samples include both edges.
    let uv = (input.position.xy - vec2<f32>(0.5)) / (parameters.zw - vec2<f32>(1.0));
    let u = uv.x;
    let v = uv.y;
    let t = clamp(u / 0.80, 0.0, 1.0);
    let arch = sin(t * 3.141592653589793);
    let flow = sin(t * 7.0 - time * 0.65);
    let center = 0.64 - 0.23 * arch + 0.16 * t + 0.035 * flow * arch;
    let taper = soft_edge(u / 0.30) * soft_edge((0.94 - u) / 0.18);
    let envelope = taper * soft_edge(v / 0.20) * soft_edge((1.0 - v) / 0.22);
    var weight = 0.0;
    var rgb = vec3<f32>(0.0);
    for (var strand = 0u; strand < 3u; strand += 1u) {
        let s = f32(strand);
        let offset = (s - 1.0) * 0.105 * arch;
        let drift = 0.025 * sin(time * 0.5 + t * 5.0 + s * 1.8) * arch;
        let distance = v - center - offset - drift;
        let halo_distance = distance / 0.13;
        let ribbon_distance = distance / 0.055;
        let halo = exp(-0.5 * halo_distance * halo_distance) * 0.20;
        let ribbon = exp(-0.5 * ribbon_distance * ribbon_distance) * 0.30;
        let pulse = 0.88 + 0.12 * cos(t * 9.0 - time * 0.8 + s);
        let light = (halo + ribbon) * pulse;
        weight += light;
        rgb += colors[strand] * light;
    }
    let alpha = (1.0 - exp(-weight)) * envelope * select(0.44, 0.50, dark);
    let color = rgb / (max(weight, 0.0001) * 255.0);
    // Match render_field's 8-bit quantization before premultiplication. Filtering
    // premultiplied texels is intentionally approximate to the straight-RGBA
    // software reference, while avoiding colored fringes under low alpha.
    let rgba = floor(clamp(vec4<f32>(color, alpha), vec4<f32>(0.0), vec4<f32>(1.0)) * 255.0 + vec4<f32>(0.5)) / 255.0;
    var target_color = rgba.rgb;
    if LINEAR_TARGET {
        target_color = select(target_color / 12.92, pow((target_color + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4)), target_color > vec3<f32>(0.04045));
    }
    // An sRGB attachment encodes this linear premultiplied value on store and
    // decodes it before filtering. The non-sRGB path retains Iced's web colors.
    return vec4<f32>(target_color * rgba.a, rgba.a);
}
