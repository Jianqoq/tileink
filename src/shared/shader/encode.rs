//! CPU scene serialization preserves complete source bytes, including comments.
//! None of this payload is interpreted on the GPU.
use super::*;
use crate::shared::{brush::ENCODED_BRUSH_HEADER_WORDS, gpu_layout::brush::*};
impl ProgramInstance {
    pub(crate) fn word_len(&self) -> usize {
        ENCODED_BRUSH_HEADER_WORDS
            + 6
            + self.program.entry.len().div_ceil(4)
            + self
                .program
                .sources
                .metal
                .as_ref()
                .map_or(0, |s| s.len().div_ceil(4))
            + self
                .program
                .sources
                .hlsl
                .as_ref()
                .map_or(0, |s| s.len().div_ceil(4))
            + self.uniforms.len()
    }
    fn payload(&self) -> Vec<u32> {
        let p = &self.program;
        let strings = [
            Some(p.entry.as_ref()),
            p.sources.metal.as_deref(),
            p.sources.hlsl.as_deref(),
        ];
        let mut words = vec![
            p.uniform_words as u32,
            match p.input {
                ShaderInput::None => 0,
                ShaderInput::Current => 1,
                ShaderInput::Sampled { .. } => 2,
            },
            p.sample_radius().to_bits(),
        ];
        for string in strings {
            let bytes = string.unwrap_or_default().as_bytes();
            words.push(bytes.len() as u32);
            words.extend(bytes.chunks(4).map(|chunk| {
                let mut b = [0; 4];
                b[..chunk.len()].copy_from_slice(chunk);
                u32::from_le_bytes(b)
            }));
        }
        words.extend(self.uniforms.iter().copied());
        words
    }
    pub(crate) fn encode(&self, blob: &mut Vec<u32>) -> (u32, u32) {
        let offset = blob.len() as u32;
        let payload = self.payload();
        let mut header = [0; ENCODED_BRUSH_HEADER_WORDS];
        header[0] = GPU_BRUSH_PROGRAM;
        header[2] = ENCODED_BRUSH_HEADER_WORDS as u32;
        header[3] = payload.len() as u32;
        for (word, value) in header[9..15].iter_mut().zip(self.transform) {
            *word = value.to_bits();
        }
        blob.extend(header);
        blob.extend(payload);
        (offset, blob.len() as u32 - offset)
    }
    pub(crate) fn decode(payload: &[u32], transform: [f32; 6], allow_input: bool) -> Option<Self> {
        if transform.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let uniform_words = *payload.first()? as usize;
        let radius = f32::from_bits(*payload.get(2)?);
        let input = match *payload.get(1)? {
            0 => ShaderInput::None,
            1 => ShaderInput::Current,
            2 => ShaderInput::Sampled { radius },
            _ => return None,
        };
        if !allow_input && input != ShaderInput::None {
            return None;
        }
        let mut rest = payload.get(3..)?;
        let mut string = || -> Option<String> {
            let len = *rest.first()? as usize;
            let words = len.div_ceil(4);
            let data = rest.get(1..1usize.checked_add(words)?)?;
            let bytes: Vec<_> = data
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .take(len)
                .collect();
            rest = rest.get(1 + words..)?;
            String::from_utf8(bytes).ok()
        };
        let entry = string()?;
        let metal = string()?;
        let hlsl = string()?;
        let program = ShaderProgram::new(
            ShaderSources {
                metal: (!metal.is_empty()).then_some(metal),
                hlsl: (!hlsl.is_empty()).then_some(hlsl),
            },
            &entry,
            input,
            uniform_words,
        )
        .ok()?;
        if rest.len() != uniform_words {
            return None;
        }
        Some(Self {
            program,
            uniforms: rest.into(),
            transform,
        })
    }
}
