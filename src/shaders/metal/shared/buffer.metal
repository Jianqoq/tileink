// Metal raw pointers carry no runtime length. Slot 29 supplies byte-exact host
// resource lengths, so guarded reads match the other backends even for invalid
// optional records. Offsets are checked before pointer arithmetic or addition.
struct BufferSizes { uint4 a, b, c, d, e, f, g, h; };
uint word_size(constant BufferSizes& sizes, uint slot) {
    uint lane = slot % 4;
    switch (slot / 4) {
        case 0: return sizes.a[lane]; case 1: return sizes.b[lane];
        case 2: return sizes.c[lane]; case 3: return sizes.d[lane];
        case 4: return sizes.e[lane]; case 5: return sizes.f[lane];
        case 6: return sizes.g[lane]; default: return sizes.h[lane];
    }
}
static_assert(sizeof(BufferSizes) == 128, "buffer sizes ABI");
struct Words {
    const device uint* pointer;
    uint length;
    uint operator[](uint index) const { return index < length ? pointer[index] : 0; }
    Words offset(uint base) const {
        uint skip = min(base, length);
        return {pointer + skip, length - skip};
    }
};
