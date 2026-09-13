// HLSL has no num_workgroups builtin. The adapter supplies the actual grid.
struct DispatchGrid { uint x; uint y; uint z; uint _pad; };
ConstantBuffer<DispatchGrid> dispatch_grid : register(b31, space0);
uint linear_group(uint3 group) {
    return group.x + group.y * dispatch_grid.x + group.z * dispatch_grid.x * dispatch_grid.y;
}
