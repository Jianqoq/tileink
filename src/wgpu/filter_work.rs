use super::buffer::WgpuBuffer;

/// Immutable binding for one compact filter worklist.
///
/// Bind groups retain the cloned `wgpu::Buffer`, so later arena growth cannot
/// invalidate commands that have already been encoded.
#[derive(Clone)]
pub(crate) struct FilterTileWork {
    pub(crate) buffer: ::wgpu::Buffer,
    pub(crate) count: u32,
}

/// Per-frame arena for compact filter worklists.
///
/// Different filter stages can use different coordinate spaces (for example,
/// full-resolution output tiles and projected downsample tiles). Each switch
/// receives a distinct buffer for the current frame, while allocations are
/// retained and reused on following frames.
#[derive(Default)]
pub(crate) struct FilterTileWorkArena {
    buffers: Vec<WgpuBuffer>,
    cursor: usize,
}

impl FilterTileWorkArena {
    pub(crate) fn reset(&mut self) {
        self.cursor = 0;
    }

    pub(crate) fn upload(
        &mut self,
        device: &::wgpu::Device,
        queue: &::wgpu::Queue,
        tiles: &[u32],
    ) -> FilterTileWork {
        if self.cursor == self.buffers.len() {
            self.buffers
                .push(WgpuBuffer::new(device, "tileink wgpu filter active tiles"));
        }
        let slot = self.cursor;
        self.cursor += 1;
        self.buffers[slot].upload(device, queue, "tileink wgpu filter active tiles", tiles);
        FilterTileWork {
            buffer: self.buffers[slot].buffer().clone(),
            count: tiles.len() as u32,
        }
    }
}
