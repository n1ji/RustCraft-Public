#version 450

layout(location = 1) in vec2 v_uv;
layout(location = 3) in float v_fog_dist;
layout(location = 4) in vec4 v_color;
layout(location = 5) in float v_glint;
layout(location = 10) in vec3 v_light_color;
layout(location = 11) in vec2 v_motion;

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

layout(location = 0) out vec4 f_color;
layout(location = 1) out vec2 f_motion;

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
    if (tex.a * v_color.a < 0.1) discard;
    // Minecraft 1.8.9 modulates its base texture, vertex tint, lightmap and
    // fog in display (gamma) space. The atlas is sampled as sRGB and the HDR
    // attachment is linear, so explicitly bridge those spaces around the
    // vanilla arithmetic instead of treating lightmap values as linear light.
    vec3 texture_srgb = linear_to_srgb(tex.rgb);
    vec3 lit_srgb = texture_srgb * v_color.rgb * v_light_color;

    if (v_glint > 0.5) {
        // RenderItem.renderEffect (1.8.9): two passes of the glint texture,
        // rotated -50 and +10 degrees, scrolling on 3000ms / 4873ms timers,
        // tinted 0xFF8040CC and blended with glBlendFunc(GL_SRC_COLOR, GL_ONE)
        // on top of the normally rendered item. fog_color.w carries the
        // shared wall-clock seconds for the scroll timers.
        float t = fog_color.w;
        vec3 glint_tint = vec3(0.501961, 0.25098, 0.8);
        vec2 p = gl_FragCoord.xy * (1.0 / 128.0);
        float s1 = fract(dot(p, vec2(0.64279, -0.76604)) + fract(t / 3.0));
        float s2 = fract(dot(p, vec2(0.98481, 0.17365)) - fract(t / 4.873));
        float streak = pow(0.5 + 0.5 * cos(s1 * 6.2831853), 4.0)
                     + pow(0.5 + 0.5 * cos(s2 * 6.2831853), 4.0);
        lit_srgb += glint_tint * streak * 0.5;
    }

    float fog_start = fog_params.x;
    float fog_end = fog_params.y;
    float fog_factor = clamp(
        (v_fog_dist - fog_start) / max(fog_end - fog_start, 0.001),
        0.0,
        1.0
    );
    vec3 final_srgb = mix(lit_srgb, fog_color.rgb, fog_factor);

    f_color = vec4(srgb_to_linear(clamp(final_srgb, 0.0, 1.0)), tex.a * v_color.a);
    f_motion = v_motion * 0.5;
}
