#version 450

layout(location = 0) in vec2 v_uv;
layout(location = 0) out vec4 f_color;

layout(set = 0, binding = 0) uniform sampler2D tone_source;

void main() {
    vec4 hdr = texture(tone_source, v_uv);
    // Vanilla's lightmap and texture modulation already define the final
    // scene brightness. ACES raises those mid-tones a second time, washing
    // blocks toward white. Keep the HDR intermediate linear and let the sRGB
    // target perform only the final transfer conversion.
    f_color = clamp(hdr, vec4(0.0), vec4(1.0));
}
