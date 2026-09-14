#ifndef TILEINK_HLSL_SHARED_INTEGER_HLSLI_INCLUDED
#define TILEINK_HLSL_SHARED_INTEGER_HLSLI_INCLUDED
// period must be positive. HLSL mixed-sign % is undefined; unsigned magnitude
// plus sign correction gives Euclidean coordinates, including INT_MIN.
uint euclidean_remainder_i32(int value, uint period) {
    uint magnitude=value<0 ? 0u-asuint(value) : asuint(value);
    uint remainder=magnitude%period;
    return value<0 && remainder!=0u ? period-remainder : remainder;
}
#endif
