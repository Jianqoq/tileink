#ifndef TILEINK_HLSL_DISPATCH_HLSLI_INCLUDED
#define TILEINK_HLSL_DISPATCH_HLSLI_INCLUDED

// HLSL has no num_workgroups builtin. The adapter supplies the actual grid.
struct DispatchGrid { uint x; uint y; uint z; uint _pad; };
uint linear_group(uint3 group, uint2 grid) {
    return group.x + group.y * grid.x + group.z * grid.x * grid.y;
}

#endif // TILEINK_HLSL_DISPATCH_HLSLI_INCLUDED
