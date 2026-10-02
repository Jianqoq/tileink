use super::*;
pub(super) struct Output {
    buffer: Object<dyn MTLBuffer>,
    row: usize,
    pitch: usize,
    rows: usize,
}
impl Output {
    pub fn read(&self) -> Result<Vec<u8>> {
        // SAFETY: Frame is retired only after this command buffer completes;
        // the shared readback allocation has exactly pitch*rows copied bytes.
        let mut result = Vec::with_capacity(self.row * self.rows);
        for index in 0..self.rows {
            // Padding is not written by texture copies. Never construct or read
            // a byte slice spanning those undefined bytes, even temporarily.
            let row = unsafe {
                std::slice::from_raw_parts(
                    self.buffer
                        .contents()
                        .as_ptr()
                        .cast::<u8>()
                        .add(index * self.pitch),
                    self.row,
                )
            };
            result.extend_from_slice(row);
        }
        Ok(result)
    }
}
pub(super) fn record(
    device: &objc2::runtime::ProtocolObject<dyn MTLDevice>,
    command: &objc2::runtime::ProtocolObject<dyn MTLCommandBuffer>,
    batch: &ComputeBatch,
    resources: &[Resource],
) -> Result<Vec<Output>> {
    let encoder = command
        .blitCommandEncoder()
        .ok_or("Metal readback encoder failed")?;
    let encoding = Encoding(objc2::runtime::ProtocolObject::from_ref(&*encoder));
    let mut outputs = Vec::new();
    for id in batch.outputs() {
        let resource = &resources[id.index()];
        let (row, pitch, rows) = match resource {
            Resource::Texture(texture) => {
                let row = texture.width() * 4;
                (
                    row,
                    row.next_multiple_of(256),
                    texture.height() * texture.arrayLength(),
                )
            }
            Resource::Buffer(buffer) => (buffer.length(), buffer.length(), 1),
            _ => return Err("Metal descriptor resources cannot be read back".into()),
        };
        let buffer = memory::buffer(device, pitch * rows, MTLResourceOptions::StorageModeShared)?;
        // SAFETY: allocation dimensions and texture row pitch were computed
        // above from the actual Metal objects, and copies cover defined regions.
        unsafe {
            match resource {
                Resource::Buffer(source) => encoder
                    .copyFromBuffer_sourceOffset_toBuffer_destinationOffset_size(
                        source, 0, &buffer, 0, row,
                    ),
                Resource::Texture(source) => {
                    let image = pitch * source.height();
                    for layer in 0..source.arrayLength() {
                        encoder.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toBuffer_destinationOffset_destinationBytesPerRow_destinationBytesPerImage(source,layer,0,memory::origin([0,0]),MTLSize{width:source.width(),height:source.height(),depth:1},&buffer,layer*image,pitch,image);
                    }
                }
                _ => unreachable!(),
            }
        }
        outputs.push(Output {
            buffer,
            row,
            pitch,
            rows,
        });
    }
    drop(encoding);
    Ok(outputs)
}
