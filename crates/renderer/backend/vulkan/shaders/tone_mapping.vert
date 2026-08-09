#version 450

layout(location = 0) in vec2 position;
layout(location = 0) out vec2 v_uv;

void main() {
    // The CPU supplies [0,0], [2,0], [0,2]. The clip transform below maps
    // those vertices to [-1,-1], [3,-1], [-1,3]; over the visible viewport,
    // `position` therefore interpolates through exactly [0,1]. Multiplying it
    // by 0.5 samples only the source texture's top-left quarter and stretches
    // that quarter across the entire screen.
    v_uv = position;
    gl_Position = vec4(position * 2.0 - 1.0, 0.0, 1.0);
}
