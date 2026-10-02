#ifndef TILEINK_HLSL_FINE_TEXT_GAMMA_INCLUDED
#define TILEINK_HLSL_FINE_TEXT_GAMMA_INCLUDED


float srgb_to_linear(float value) {
    float v = clamp(value, 0.0, 1.0);
    float result = v / 12.92;
    if (v > 0.04045) {
        result = pow((v + 0.055) / 1.055, 2.4);
    }
    return result;
}

float linear_to_srgb(float value) {
    float v = clamp(value, 0.0, 1.0);
    float result = v * 12.92;
    if (v > 0.0031308) {
        result = 1.055 * pow(v, 1.0 / 2.4) - 0.055;
    }
    return result;
}

float linear_to_srgb_derivative(float value) {
    float v = clamp(value, 0.0, 1.0);
    float result = 12.92;
    if (v > 0.0031308) {
        result = (1.055 / 2.4) * pow(v, 1.0 / 2.4 - 1.0);
    }
    return result;
}

float linear_premul_from_srgb8(uint value, float alpha) {
    float result = 0.0;
    if (alpha > 0.0) {
        result = srgb_to_linear((float(value) * (1.0 / 255.0)) / alpha) * alpha;
    }
    return result;
}

uint linear_premul_channel_to_srgb8(float value, float alpha) {
    return uint(linear_to_srgb(clamp(value / alpha, 0.0, 1.0)) * alpha * 255.0 + 0.5);
}

uint pack_linear_premul_to_srgb8(float r, float g, float b, float a) {
    uint result = 0u;
    if (a > 0.0) {
        float alpha = clamp(a, 0.0, 1.0);
        uint pr = linear_premul_channel_to_srgb8(r, alpha);
        uint pg = linear_premul_channel_to_srgb8(g, alpha);
        uint pb = linear_premul_channel_to_srgb8(b, alpha);
        uint pa = uint(alpha * 255.0 + 0.5);
        result = pr | (pg << 8u) | (pb << 16u) | (pa << 24u);
    }
    return result;
}

#endif
