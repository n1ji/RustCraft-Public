#version 450

layout(location = 0) in vec3 position;
layout(location = 1) in vec3 normal;
layout(location = 2) in vec2 uv;
layout(location = 3) in float block_type;
layout(location = 4) in float sky_light;
layout(location = 5) in float block_light;
layout(location = 6) in float ambient_occlusion;

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

layout(push_constant) uniform PushConstants {
    vec3 chunk_offset;
};

layout(location = 1) out vec2 v_uv;
layout(location = 3) out float v_fog_dist;
layout(location = 4) out vec4 v_color;
layout(location = 5) out float v_glint;
layout(location = 10) out vec3 v_light_color;
layout(location = 11) out vec2 v_motion;

float light_table(float level_01, float minimum) {
    float f1 = 1.0 - level_01;
    return (1.0 - f1) / (f1 * 3.0 + 1.0) * (1.0 - minimum) + minimum;
}

// EntityRenderer.updateLightmap from Minecraft 1.8.9. The fixed-function
// lightmap is vertex-driven and interpolated across each face, so calculate it
// once per vertex instead of repeating the same table math for every pixel.
vec3 vanilla_lightmap(float sky_level, float block_level) {
    float sun = clamp(fog_params.w, 0.0, 1.0);
    float dimension = grass_color.w;
    float table_minimum = dimension < -0.5 ? 0.1 : 0.0;
    float sky = light_table(clamp(sky_level, 0.0, 15.0) / 15.0, table_minimum)
        * (sun * 0.95 + 0.05);
    float block = light_table(clamp(block_level, 0.0, 15.0) / 15.0, table_minimum) * 1.5;

    float sky_red = sky * (sun * 0.65 + 0.35);
    float block_green = block * ((block * 0.6 + 0.4) * 0.6 + 0.4);
    float block_blue = block * (block * block * 0.6 + 0.4);
    vec3 color = vec3(sky_red + block, sky_red + block_green, sky + block_blue);
    color = color * 0.96 + 0.03;

    if (dimension > 0.5) {
        color = vec3(
            0.22 + block * 0.75,
            0.28 + block_green * 0.75,
            0.25 + block_blue * 0.75
        );
    }

    color = clamp(color, 0.0, 1.0);
    return clamp(color * 0.96 + 0.03, 0.0, 1.0);
}

float vanilla_face_brightness(vec3 surface_normal) {
    // BlockModelRenderer uses fixed face shading in 1.8.9. Tying block
    // brightness to the moving sun direction makes walls pulse or invert
    // during the day and double-applies directional light on top of lightmap.
    vec3 n = normalize(surface_normal);
    vec3 axis = abs(n);
    if (axis.y >= axis.x && axis.y >= axis.z) {
        return n.y >= 0.0 ? 1.0 : 0.5;
    }
    return axis.z >= axis.x ? 0.8 : 0.6;
}

void main() {
    vec3 world_pos = position + chunk_offset;
    vec4 current_clip = view_proj * vec4(world_pos, 1.0);
    vec4 current_motion_clip = proj * view * vec4(world_pos, 1.0);
    vec4 previous_clip = prev_view_proj * vec4(world_pos, 1.0);
    gl_Position = current_clip;
    // FSR expects motion from the current pixel back to its position in the
    // previous frame.  Keep this unjittered; the CPU supplies render-size
    // scaling to convert the resulting UV-space vector to pixels.
    v_motion = previous_clip.xy / max(previous_clip.w, 0.00001)
        - current_motion_clip.xy / max(current_motion_clip.w, 0.00001);
    v_uv = uv;
    v_glint = 0.0;

    if (block_type > 0.5 && block_type < 1.5) {
        v_color = vec4(grass_color.rgb, 1.0);
    } else if (block_type > 1.5 && block_type < 2.5) {
        v_color = vec4(grass_color.rgb * 0.8, 1.0);
    } else if (block_type > 2.5 && block_type < 3.5) {
        v_color = vec4(vec3(0.4667, 0.6706, 0.1843), 1.0);
    } else if (block_type > 3.5 && block_type < 4.5) {
        v_color = vec4(0.2, 0.4, 0.85, 0.72);
    } else if (block_type > 4.5 && block_type < 5.5) {
        v_color = vec4(1.0, 1.0, 1.0, 1.0);
    } else if (block_type > 5.5 && block_type < 6.5) {
        v_color = vec4(0.49, 0.74, 0.49, 1.0); // item grass tint (default ~0x7CBD7C)
    } else if (block_type > 6.5 && block_type < 7.5) {
        v_color = vec4(0.28, 0.51, 0.18, 1.0); // item foliage tint (default ~0x478247)
    } else if (block_type > 7.5 && block_type < 9.5) {
        // BlockRedstoneWire#colorMultiplier (1.8.9), encoded as 8 + power/15.
        float power = clamp(block_type - 8.0, 0.0, 1.0);
        float red = power * 0.6 + 0.4;
        if (power == 0.0) red = 0.3;
        float green = max(0.0, power * power * 0.7 - 0.5);
        float blue = max(0.0, power * power * 0.6 - 0.7);
        v_color = vec4(red, green, blue, 1.0);
    } else if (block_type > 10.5 && block_type < 11.5) {
        // Enchanted item marker. Vanilla RenderItem.renderEffect draws the
        // item with its own colors first, then adds the scrolling purple
        // glint as an extra pass; the fragment shader reproduces that pass.
        v_color = vec4(1.0);
        v_glint = 1.0;
    } else if (block_type > 11.5 && block_type < 12.5) {
        // Stained-glass textures already carry the dye colour.  Their render
        // layer is translucent, so alpha belongs to the material contract,
        // not to the atlas texture.
        v_color = vec4(1.0, 1.0, 1.0, 0.55);
    } else if (block_type > 12.5 && block_type < 13.5) {
        v_color = vec4(0.86, 0.94, 1.0, 0.62); // ice
    } else if (block_type > 13.5 && block_type < 14.5) {
        v_color = vec4(0.58, 0.92, 0.58, 0.62); // slime
    } else if (block_type > 14.5 && block_type < 15.5) {
        v_color = vec4(0.72, 0.12, 0.82, 0.78); // nether portal
    } else if (block_type > 15.5 && block_type < 16.5) {
        v_color = vec4(1.0, 1.0, 1.0, 0.70); // tripwire
    } else {
        v_color = vec4(1.0);
    }

    float face_brightness = vanilla_face_brightness(normal);
    float ao = clamp(ambient_occlusion, 0.2, 1.0);
    v_light_color = vanilla_lightmap(sky_light, block_light) * face_brightness * ao;

    vec4 view_pos = view * vec4(world_pos, 1.0);
    v_fog_dist = length(view_pos.xyz);
}
