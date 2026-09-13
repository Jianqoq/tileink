//! Reflection of the native integer-probe ABI, not a replacement for SPIR-V
//! validation. Numeric opcodes/decorations follow Khronos SPIRV-Headers.
use super::abi::{Interface, Kind};
use std::{collections::BTreeMap, io};

#[derive(Default)]
struct Reflection {
    names: BTreeMap<u32, String>,
    member_names: BTreeMap<(u32, u32), String>,
    decorations: BTreeMap<(u32, u32), Vec<u32>>,
    offsets: BTreeMap<(u32, u32), u32>,
    types: BTreeMap<u32, (u32, Vec<u32>)>,
    variables: BTreeMap<u32, u32>,
    entries: Vec<(u32, String)>,
    groups: BTreeMap<u32, Vec<u32>>,
}

pub fn validate(bytes: &[u8], entry: &str, abi: &Interface) -> io::Result<()> {
    super::abi::validate(abi)?;
    let words: Vec<u32> = bytes
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    require(
        bytes.len().is_multiple_of(4) && words.len() >= 5 && words[0] == 0x07230203,
        "SPIR-V header",
    )?;
    let mut r = Reflection::default();
    let mut cursor = 5;
    while cursor < words.len() {
        let count = (words[cursor] >> 16) as usize;
        let opcode = words[cursor] & 65535;
        require(
            count > 0 && count <= words.len() - cursor,
            "SPIR-V instruction length",
        )?;
        let args = &words[cursor + 1..cursor + count];
        match (opcode, args) {
            (5, [id, name @ ..]) => {
                r.names.insert(*id, string(name)?);
            }
            (6, [id, member, name @ ..]) => {
                r.member_names.insert((*id, *member), string(name)?);
            }
            (15, [5, id, name @ ..]) => {
                r.entries.push((*id, string(name)?));
            }
            (16, [id, 17, group @ ..]) => {
                r.groups.insert(*id, group.to_vec());
            }
            (71, [id, decoration, value @ ..]) => {
                r.decorations.insert((*id, *decoration), value.to_vec());
            }
            (72, [id, member, 35, offset]) => {
                r.offsets.insert((*id, *member), *offset);
            }
            (21 | 22 | 23 | 25 | 29 | 30 | 32, [id, value @ ..]) => {
                r.types.insert(*id, (opcode, value.to_vec()));
            }
            (59, [ty, id, _storage]) => {
                r.variables.insert(*id, *ty);
            }
            _ => (),
        }
        cursor += count;
    }
    require(
        r.entries.len() == 1 && r.entries[0].1 == entry,
        "SPIR-V compute entry",
    )?;
    require(
        r.groups.get(&r.entries[0].0).map(Vec::as_slice) == Some(abi.workgroup.as_slice()),
        "SPIR-V workgroup",
    )?;
    let expected = abi.resources_for(entry)?;
    require(
        r.decorations.keys().filter(|(_, dec)| *dec == 33).count() == expected.len(),
        "SPIR-V binding count",
    )?;
    for &(name, resource) in &expected {
        let binding = resource.binding;
        let id = *r
            .names
            .iter()
            .find(|(_, n)| n.as_str() == name)
            .ok_or_else(|| invalid("SPIR-V resource name"))?
            .0;

        require(
            r.decorations.get(&(id, 33)).map(Vec::as_slice) == Some(&[binding])
                && r.decorations.get(&(id, 34)).map(Vec::as_slice) == Some(&[0]),
            "SPIR-V binding/set",
        )?;
        let pointer = *r
            .variables
            .get(&id)
            .ok_or_else(|| invalid("SPIR-V resource variable"))?;
        let (op, args) = r
            .types
            .get(&pointer)
            .ok_or_else(|| invalid("SPIR-V pointer type"))?;
        if resource.kind == Kind::Texture {
            require(
                *op == 32 && args.len() == 2 && args[0] == 0,
                "SPIR-V image pointer",
            )?;
            let image = r
                .types
                .get(&args[1])
                .ok_or_else(|| invalid("SPIR-V image type"))?;
            require(
                image.0 == 25 && image.1.len() == 7 && image.1[1..] == [1, 2, 0, 0, 1, 0],
                "SPIR-V sampled 2D image",
            )?;
            require(
                r.types
                    .get(&image.1[0])
                    .is_some_and(|ty| ty.0 == 22 && ty.1 == [32]),
                "SPIR-V sampled float32",
            )?;
            continue;
        }
        require(
            *op == 32 && args.len() == 2 && args[0] == 2,
            "SPIR-V uniform storage pointer",
        )?;
        let structure = args[1];
        let (op, members) = r
            .types
            .get(&structure)
            .ok_or_else(|| invalid("SPIR-V resource structure"))?;
        require(*op == 30, "SPIR-V resource structure")?;
        if resource.kind == Kind::Uniform {
            require(
                members.len() == resource.fields.len()
                    && r.decorations.contains_key(&(structure, 2)),
                "SPIR-V uniform block",
            )?;
            for (index, field) in resource.fields.iter().enumerate() {
                require(
                    r.offsets.get(&(structure, index as u32)) == Some(&field.offset)
                        && r.member_names
                            .get(&(structure, index as u32))
                            .map(String::as_str)
                            == Some(field.name.as_str()),
                    "SPIR-V uniform field layout",
                )?;
                let ty = r
                    .types
                    .get(&members[index])
                    .ok_or_else(|| invalid("SPIR-V uniform field type"))?;
                if field.lanes == 1 {
                    require(is_uint(ty), "SPIR-V uint parameter")?;
                } else {
                    require(
                        ty.0 == 23
                            && ty.1.len() == 2
                            && ty.1[1] == field.lanes
                            && r.types.get(&ty.1[0]).is_some_and(is_uint),
                        "SPIR-V uint vector parameter",
                    )?;
                }
            }
        } else {
            require(
                members.len() == 1
                    && r.offsets.get(&(structure, 0)) == Some(&0)
                    && r.decorations.contains_key(&(structure, 3)),
                "SPIR-V byte buffer block",
            )?;
            let array = members[0];
            let ty = r
                .types
                .get(&array)
                .ok_or_else(|| invalid("SPIR-V byte buffer array"))?;
            require(
                ty.0 == 29 && ty.1.len() == 1 && r.types.get(&ty.1[0]).is_some_and(is_uint),
                "SPIR-V byte buffer uint array",
            )?;
            require(
                r.decorations.get(&(array, 6)).map(Vec::as_slice) == Some(&[4]),
                "SPIR-V byte buffer stride",
            )?;
        }
    }
    Ok(())
}

fn is_uint(ty: &(u32, Vec<u32>)) -> bool {
    ty.0 == 21 && ty.1 == [32, 0]
}
fn require(condition: bool, message: &str) -> io::Result<()> {
    if condition {
        Ok(())
    } else {
        Err(invalid(message))
    }
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn string(words: &[u32]) -> io::Result<String> {
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let end = bytes
        .iter()
        .position(|b| *b == 0)
        .ok_or_else(|| invalid("SPIR-V string terminator"))?;
    String::from_utf8(bytes[..end].to_vec()).map_err(|_| invalid("SPIR-V UTF-8 name"))
}
