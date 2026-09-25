// Only defined membership bits affect tile selection; retained high bits are ignored.
uint tile_kind(uint flags) { return (flags & 4) ? 0 : (flags & 2) ? ((flags & 1) ? 4 : 3) : (flags & 1) ? 2 : 1; }
