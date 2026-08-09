#version 450

layout(location = 0) in vec3 v_normal;
layout(location = 1) in vec2 v_uv;
layout(location = 2) in vec3 v_light;
layout(location = 3) in float v_fog_dist;
layout(location = 4) in vec4 v_color;
layout(location = 5) in float v_world_y;
layout(location = 6) in vec2 v_motion;

layout(location = 0) out vec4 f_color;
layout(location = 1) out vec2 f_motion;

layout(set = 0, binding = 0) uniform Uniforms {
    mat4 view;
    mat4 proj;
    mat4 view_proj;
    mat4 prev_view_proj;
    vec4 light_dir;
    vec4 fog_color;
    vec4 fog_params;
    vec4 grass_color;
    vec4 camera_right;
    vec4 camera_up;
};

layout(set = 0, binding = 1) uniform sampler2D tex_sampler;

vec3 linear_to_srgb(vec3 color) {
    vec3 low = color * 12.92;
    vec3 high = 1.055 * pow(max(color, vec3(0.0)), vec3(1.0 / 2.4)) - 0.055;
    return mix(low, high, step(vec3(0.0031308), color));
}

vec3 srgb_to_linear(vec3 color) {
    vec3 low = color / 12.92;
    vec3 high = pow(max((color + 0.055) / 1.055, vec3(0.0)), vec3(2.4));
    return mix(low, high, step(vec3(0.04045), color));
}

void main() {
    vec4 tex = texture(tex_sampler, v_uv);

    // Alpha test
    if (tex.a < 0.1) discard;

    // Match the same gamma-space texture/lightmap modulation used by vanilla
    // entity rendering, then return to linear for the HDR attachment.
    vec3 texture_srgb = linear_to_srgb(tex.rgb);
    vec3 lit_srgb = texture_srgb * v_color.rgb * v_light;

    // Fog
    float fog_start = fog_params.x;
    float fog_end = fog_params.y;
    float camera_y = fog_params.z;
    float height_delta = v_world_y - camera_y;
    float height_density = clamp(-height_delta / 96.0, 0.0, 0.06);

    float dist_factor = clamp((v_fog_dist - fog_start) / (fog_end - fog_start), 0.0, 1.0);
    float fog_factor = max(dist_factor, height_density);

    // Aerial perspective — only at extreme distance
    float sky_bleed = max(0.0, fog_factor - 0.9) * 10.0;
    vec3 final_srgb = mix(lit_srgb, fog_color.rgb, sky_bleed);

    // Standard distance fog
    final_srgb = mix(final_srgb, fog_color.rgb, fog_factor);

    f_color = vec4(srgb_to_linear(clamp(final_srgb, 0.0, 1.0)), tex.a * v_color.a);
    f_motion = v_motion * 0.5;
}
