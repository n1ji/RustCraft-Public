#version 450

layout(location = 0) in vec3 position;
layout(location = 1) in vec3 normal;
layout(location = 2) in vec2 uv;
layout(location = 3) in vec4 color;

layout(location = 0) out vec3 v_normal;
layout(location = 1) out vec2 v_uv;
layout(location = 2) out vec3 v_light;
layout(location = 3) out float v_fog_dist;
layout(location = 4) out vec4 v_color;
layout(location = 5) out float v_world_y;
layout(location = 6) out vec2 v_motion;

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

float light_table(float level_01) {
    float f1 = 1.0 - level_01;
    return (1.0 - f1) / (f1 * 3.0 + 1.0);
}

vec3 vanilla_lightmap(float sky_level, float block_level) {
    float sun = clamp(fog_params.w, 0.0, 1.0);
    float sky = light_table(clamp(sky_level, 0.0, 15.0) / 15.0) * (sun * 0.95 + 0.05);
    float block = light_table(clamp(block_level, 0.0, 15.0) / 15.0) * 1.5;
    float sky_red = sky * (sun * 0.65 + 0.35);
    float block_green = block * ((block * 0.6 + 0.4) * 0.6 + 0.4);
    float block_blue = block * (block * block * 0.6 + 0.4);
    return clamp(vec3(sky_red + block, sky_red + block_green, sky + block_blue) * 0.96 + 0.03, 0.0, 1.0);
}

float vanilla_face_brightness(vec3 surface_normal) {
    // Tile entities and living models use the same fixed block-face shade
    // convention as the world renderer.  A moving sun vector made faces
    // pulse and could turn a valid lightmap into an almost-black model.
    vec3 n = normalize(surface_normal);
    vec3 axis = abs(n);
    if (axis.y >= axis.x && axis.y >= axis.z) {
        return n.y >= 0.0 ? 1.0 : 0.5;
    }
    return axis.z >= axis.x ? 0.8 : 0.6;
}

void main() {
    // Particle vertices carry their camera-local corner offset in `normal`
    // and use an alpha sentinel in `color.a` (2..3). The center stays in
    // `position`, so rotating the camera no longer rebuilds/uploads the whole
    // particle batch on the CPU.
    bool is_particle = color.a > 1.999 && color.a < 3.001;
    vec3 world_pos = position;
    if (is_particle) {
        world_pos += camera_right.xyz * normal.x + camera_up.xyz * normal.y;
    }
    vec4 current_clip = view_proj * vec4(world_pos, 1.0);
    vec4 current_motion_clip = proj * view * vec4(world_pos, 1.0);
    vec4 previous_clip = prev_view_proj * vec4(world_pos, 1.0);
    gl_Position = current_clip;
    // FSR expects current-to-previous motion, without projection jitter.
    v_motion = previous_clip.xy / max(previous_clip.w, 0.00001)
        - current_motion_clip.xy / max(current_motion_clip.w, 0.00001);
    v_normal = is_particle ? vec3(0.0, 0.0, normal.z) : normal;
    v_uv = uv;
    v_color = vec4(color.rgb, is_particle ? color.a - 2.0 : (color.a > 15.0 ? 1.0 : color.a));

    // A zero normal marks emissive geometry such as flame and portal particles.
    if (is_particle) {
        v_light = normal.z > 0.5
            ? vec3(1.0)
            : vec3(mix(0.15, 1.0, clamp(fog_params.w, 0.0, 1.0)));
    } else if (dot(normal, normal) < 0.0001) {
        v_light = vec3(1.0);
    } else if (color.a > 15.0) {
        float packed = color.a - 16.0;
        float sky_light = floor(packed / 16.0);
        float block_light = mod(packed, 16.0);
        float face_brightness = vanilla_face_brightness(normal);
        v_light = vanilla_lightmap(sky_light, block_light) * face_brightness;
    } else {
        // Non-entity geometry sharing this vertex type keeps its existing
        // daylight approximation and alpha semantics.
        float daylight = clamp(fog_params.w, 0.0, 1.0);
        float face_brightness = vanilla_face_brightness(normal);
        v_light = vec3(mix(0.15, 1.0, daylight) * face_brightness);
    }

    v_world_y = world_pos.y;

    vec4 view_pos = view * vec4(world_pos, 1.0);
    v_fog_dist = length(view_pos.xyz);
}
