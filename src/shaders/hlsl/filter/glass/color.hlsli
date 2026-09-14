#ifndef TILEINK_FILTER_GLASS_COLOR_HLSLI
#define TILEINK_FILTER_GLASS_COLOR_HLSLI
#include "constants.hlsli"
float liquid_glass_uncompand_srgb(float a) {
    float result = a / 12.92;
    if (a > 0.04045) {
        result = pow((a + 0.055) / 1.055, 2.4);
    }
    return result;
}

float liquid_glass_srgb_to_xyz_y(float r, float g, float b) {
    return liquid_glass_uncompand_srgb(r) * 0.2126 + liquid_glass_uncompand_srgb(g) * 0.7152 + liquid_glass_uncompand_srgb(b) * 0.0722;
}

float liquid_glass_xyz_to_lab_f(float x) {
    float result = 7.787037 * x + 0.13793103;
    if (x > 0.008856452) {
        result = pow(x, 0.33333334);
    }
    return result;
}

float liquid_glass_srgb_to_lch_l(float r, float g, float b) {
    float y = liquid_glass_xyz_to_lab_f(liquid_glass_srgb_to_xyz_y(r, g, b) / LIQUID_GLASS_D65_Y);
    return 116.0 * y - 16.0;
}

float liquid_glass_srgb_to_xyz_x(float r, float g, float b) {
    return liquid_glass_uncompand_srgb(r) * 0.4124 + liquid_glass_uncompand_srgb(g) * 0.3576 + liquid_glass_uncompand_srgb(b) * 0.1805;
}

float liquid_glass_srgb_to_lab_a(float r, float g, float b) {
    float x = liquid_glass_xyz_to_lab_f(liquid_glass_srgb_to_xyz_x(r, g, b) / LIQUID_GLASS_D65_X);
    float y = liquid_glass_xyz_to_lab_f(liquid_glass_srgb_to_xyz_y(r, g, b) / LIQUID_GLASS_D65_Y);
    return 500.0 * (x - y);
}

float liquid_glass_srgb_to_xyz_z(float r, float g, float b) {
    return liquid_glass_uncompand_srgb(r) * 0.0193 + liquid_glass_uncompand_srgb(g) * 0.1192 + liquid_glass_uncompand_srgb(b) * 0.9505;
}

float liquid_glass_srgb_to_lab_b(float r, float g, float b) {
    float y = liquid_glass_xyz_to_lab_f(liquid_glass_srgb_to_xyz_y(r, g, b) / LIQUID_GLASS_D65_Y);
    float z = liquid_glass_xyz_to_lab_f(liquid_glass_srgb_to_xyz_z(r, g, b) / LIQUID_GLASS_D65_Z);
    return 200.0 * (y - z);
}

float liquid_glass_srgb_to_lch_c(float r, float g, float b) {
    float lab_a = liquid_glass_srgb_to_lab_a(r, g, b);
    float lab_b = liquid_glass_srgb_to_lab_b(r, g, b);
    return sqrt(lab_a * lab_a + lab_b * lab_b);
}

float liquid_glass_srgb_to_lch_h(float r, float g, float b) {
    return atan2(liquid_glass_srgb_to_lab_b(r, g, b), liquid_glass_srgb_to_lab_a(r, g, b)) * 57.29578;
}

float liquid_glass_lab_to_xyz_f(float x) {
    float result = 0.12841855 * (x - 0.13793103);
    if (x > 0.206897) {
        result = x * x * x;
    }
    return result;
}

float liquid_glass_lch_to_xyz_x(float l, float c, float h) {
    float hue = h * 0.017453292;
    float lab_a = c * cos(hue);
    float w = (l + 16.0) / 116.0;
    return LIQUID_GLASS_D65_X * liquid_glass_lab_to_xyz_f(w + lab_a / 500.0);
}

float liquid_glass_lch_to_xyz_y(float l) {
    float w = (l + 16.0) / 116.0;
    return LIQUID_GLASS_D65_Y * liquid_glass_lab_to_xyz_f(w);
}

float liquid_glass_lch_to_xyz_z(float l, float c, float h) {
    float hue = h * 0.017453292;
    float lab_b = c * sin(hue);
    float w = (l + 16.0) / 116.0;
    return LIQUID_GLASS_D65_Z * liquid_glass_lab_to_xyz_f(w - lab_b / 200.0);
}

float liquid_glass_compand_rgb(float a) {
    float result = 12.92 * a;
    if (a > 0.0031308) {
        result = 1.055 * pow(a, 0.41666666) - 0.055;
    }
    return result;
}

float liquid_glass_lch_to_srgb_r(float l, float c, float h) {
    float x = liquid_glass_lch_to_xyz_x(l, c, h);
    float y = liquid_glass_lch_to_xyz_y(l);
    float z = liquid_glass_lch_to_xyz_z(l, c, h);
    return liquid_glass_compand_rgb(x * 3.2406255 + y * -1.537208 + z * -0.4986286);
}

float liquid_glass_lch_to_srgb_g(float l, float c, float h) {
    float x = liquid_glass_lch_to_xyz_x(l, c, h);
    float y = liquid_glass_lch_to_xyz_y(l);
    float z = liquid_glass_lch_to_xyz_z(l, c, h);
    return liquid_glass_compand_rgb(x * -0.9689307 + y * 1.8757561 + z * 0.0415175);
}

float liquid_glass_lch_to_srgb_b(float l, float c, float h) {
    float x = liquid_glass_lch_to_xyz_x(l, c, h);
    float y = liquid_glass_lch_to_xyz_y(l);
    float z = liquid_glass_lch_to_xyz_z(l, c, h);
    return liquid_glass_compand_rgb(x * 0.0557101 + y * -0.2040211 + z * 1.0569959);
}
#endif
