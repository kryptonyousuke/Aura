#version 450
layout(location = 0) in vec2 inUV;
layout(location = 0) out vec4 outColor;
layout(binding = 0) uniform sampler2D videoTexture;

float srgb_to_linear(float x)
{
    if (x <= 0.04045)
        return x / 12.92;

    return pow((x + 0.055) / 1.055, 2.4);
}

void main() {
    // the ycbcr sampler sends texels in srgb
    vec4 video_tex = texture(videoTexture, inUV);

    // we need to normalize this and then the shader will automatically apply the srgb curve again
    vec3 linear = vec3(
        srgb_to_linear(video_tex.r),
        srgb_to_linear(video_tex.g),
        srgb_to_linear(video_tex.b)
    );
    outColor = vec4(linear, 1.0);
}
