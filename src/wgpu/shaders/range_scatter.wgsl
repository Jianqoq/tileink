@group(0) @binding(0) var<storage, read> upload: array<u32>;
@group(0) @binding(1) var<storage, read_write> destination: array<u32>;

@compute @workgroup_size(256)
fn main(
    @builtin(workgroup_id) workgroup: vec3<u32>,
    @builtin(local_invocation_id) local: vec3<u32>,
) {
    let descriptor = 4u + workgroup.x * 4u;
    let payload_base = upload[0];
    let dst = upload[descriptor];
    let src = upload[descriptor + 1u];
    let len = upload[descriptor + 2u];
    var offset = local.x;
    loop {
        if offset >= len {
            break;
        }
        destination[dst + offset] = upload[payload_base + src + offset];
        offset += 256u;
    }
}
