//! Borrow upload payloads until they can be copied directly into mapped staging memory.
//! Avoid a frame-sized intermediate Vec and its reallocations/copies during resize.
pub(super) struct Upload<'a> {
    spans: Vec<(usize, &'a [u8])>,
    len: usize,
}

impl<'a> Upload<'a> {
    pub fn new() -> Self {
        Self {
            spans: Vec::new(),
            len: 0,
        }
    }

    pub fn push(&mut self, bytes: &'a [u8], alignment: usize) -> Result<usize, &'static str> {
        let mask = alignment
            .checked_sub(1)
            .filter(|_| alignment.is_power_of_two())
            .ok_or("invalid upload alignment")?;
        let offset = self.len.checked_add(mask).ok_or("upload size overflow")? & !mask;
        let end = offset
            .checked_add(bytes.len())
            .ok_or("upload size overflow")?;
        self.spans.push((offset, bytes));
        self.len = end;
        Ok(offset)
    }

    pub fn len(&self) -> usize {
        self.len
    }

    /// # Safety
    /// `destination` must be writable for `self.len()` bytes and not overlap any source span.
    pub unsafe fn copy_to(&self, destination: *mut u8) {
        let mut end = 0;
        for &(offset, bytes) in &self.spans {
            unsafe {
                destination.add(end).write_bytes(0, offset - end);
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), destination.add(offset), bytes.len());
            }
            end = offset + bytes.len();
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn direct_upload_matches_concatenation_including_padding_and_empty_spans() {
        let payloads = [vec![1, 2, 3, 4], vec![], vec![7; 1025], vec![8; 16], vec![]];
        let mut upload = super::Upload::new();
        let mut reference = Vec::new();
        for (bytes, alignment) in payloads.iter().zip([1, 256, 4, 256, 256]) {
            let offset = (reference.len() + alignment - 1) & !(alignment - 1);
            reference.resize(offset, 0);
            assert_eq!(upload.push(bytes, alignment).unwrap(), offset);
            reference.extend_from_slice(bytes);
        }
        let mut output = vec![255; upload.len()];
        unsafe {
            upload.copy_to(output.as_mut_ptr());
        }
        assert_eq!(output, reference);
    }

    #[test]
    fn invalid_layout_does_not_mutate_the_upload() {
        let mut upload = super::Upload::new();
        assert!(upload.push(&[1], 0).is_err());
        assert!(upload.push(&[1], 3).is_err());
        assert_eq!(upload.len(), 0);
        upload.len = usize::MAX;
        assert!(upload.push(&[], 4).is_err());
        assert!(upload.push(&[1], 1).is_err());
        assert!(upload.spans.is_empty());
    }
}
