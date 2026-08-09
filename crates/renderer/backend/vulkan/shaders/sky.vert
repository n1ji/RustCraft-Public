#version 450

layout(location = 0) in vec2 in_pos;

layout(set = 0, binding = 0) uniform SkyUniforms {
    vec4 zenith;
    vec4 horizon;
    vec4 sun_dir;
    vec4 fog_params;
    vec4 custom_sky;
    mat4 inv_view_proj;
};

void main() {
    vec2 pos = in_pos * 2.0 - 1.0;
    gl_Position = vec4(pos, 0.0, 1.0);
}
