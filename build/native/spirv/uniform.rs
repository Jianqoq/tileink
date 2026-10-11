//! Reflected byte extents for caller-owned native constant blocks.
use super::{Reflection, invalid, require};
use std::io;

// Use reflected member offsets/array and matrix strides, including nested blocks.
// Bound recursion rejects malformed recursive types rather than exhausting the host.
pub(super) fn size(
    r: &Reflection,
    id: u32,
    matrix: Option<(u32, bool)>,
    depth: usize,
) -> io::Result<u32> {
    require(depth < 64, "SPIR-V uniform type depth")?;
    let (op, args) = r
        .types
        .get(&id)
        .ok_or_else(|| invalid("SPIR-V uniform type"))?;
    let overflow = || invalid("SPIR-V uniform extent overflow");
    match (*op, args.as_slice()) {
        (21 | 22, [bits, ..]) if *bits > 0 && bits.is_multiple_of(8) => Ok(bits / 8),
        (23, [scalar, count]) => size(r, *scalar, None, depth + 1)?
            .checked_mul(*count)
            .ok_or_else(overflow),
        (24, [column, columns]) => {
            let (stride, row_major) = matrix.ok_or_else(|| invalid("SPIR-V matrix stride"))?;
            let rows = r
                .types
                .get(column)
                .and_then(|(op, a)| (*op == 23).then(|| a.get(1).copied()).flatten())
                .ok_or_else(|| invalid("SPIR-V matrix column"))?;
            stride
                .checked_mul(if row_major { rows } else { *columns })
                .ok_or_else(overflow)
        }
        (28, [element, count]) => {
            let stride = r
                .decorations
                .get(&(id, 6))
                .and_then(|a| a.first())
                .copied()
                .ok_or_else(|| invalid("SPIR-V array stride"))?;
            let count = *r
                .constants
                .get(count)
                .ok_or_else(|| invalid("SPIR-V array count"))?;
            require(
                size(r, *element, matrix, depth + 1)? <= stride,
                "SPIR-V array element extent",
            )?;
            stride.checked_mul(count).ok_or_else(overflow)
        }
        (30, members) => {
            let mut end = 0;
            for (i, &ty) in members.iter().enumerate() {
                let i = i as u32;
                let offset = *r
                    .offsets
                    .get(&(id, i))
                    .ok_or_else(|| invalid("SPIR-V uniform member offset"))?;
                let matrix = r
                    .member_decorations
                    .get(&(id, i, 7))
                    .and_then(|a| a.first())
                    .map(|&stride| (stride, r.member_decorations.contains_key(&(id, i, 4))));
                end = end.max(
                    offset
                        .checked_add(size(r, ty, matrix, depth + 1)?)
                        .ok_or_else(overflow)?,
                );
            }
            Ok(end)
        }
        _ => Err(invalid("unsupported SPIR-V uniform type")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reflection() -> Reflection {
        let mut r = Reflection::default();
        r.types.extend([
            (1, (22, vec![32])),
            (2, (23, vec![1, 4])),
            (3, (28, vec![2, 20])),
            (4, (30, vec![2, 3])),
            (5, (24, vec![2, 3])),
        ]);
        r.constants.insert(20, 2);
        r.decorations.insert((3, 6), vec![32]);
        r.offsets.extend([((4, 0), 0), ((4, 1), 32)]);
        r
    }
    #[test]
    fn raw_uniform_extents_include_member_offsets_array_padding_and_matrix_major_order() {
        let r = reflection();
        assert_eq!(size(&r, 2, None, 0).unwrap(), 16);
        assert_eq!(size(&r, 3, None, 0).unwrap(), 64);
        assert_eq!(size(&r, 4, None, 0).unwrap(), 96);
        assert_eq!(size(&r, 5, Some((16, false)), 0).unwrap(), 48);
        assert_eq!(size(&r, 5, Some((16, true)), 0).unwrap(), 64);
    }
    #[test]
    fn raw_uniform_extents_reject_recursive_missing_overflowing_and_short_strides() {
        let mut r = reflection();
        assert!(size(&r, 5, None, 0).is_err());
        r.decorations.insert((3, 6), vec![8]);
        assert!(size(&r, 3, None, 0).is_err());
        r.decorations.insert((3, 6), vec![u32::MAX]);
        assert!(size(&r, 3, None, 0).is_err());
        r.types.insert(4, (30, vec![4]));
        assert!(size(&r, 4, None, 0).is_err());
        assert!(size(&r, 999, None, 0).is_err());
    }
}
