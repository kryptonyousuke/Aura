#version 450
layout(location = 0) in vec2 inUV;
layout(location = 0) out vec4 outColor;
layout(binding = 0) uniform sampler2D videoTexture;

void main() {
    vec4 rgb_color = texture(videoTexture, inUV);
    vec3 linear_color = pow(rgb_color.rgb, vec3(2.2));
    outColor = vec4(linear_color, 1.0);
}