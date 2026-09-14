//! Typed host interface contracts. HLSL declares its own resources; reflection
//! verifies the compiled interface against these contracts, without JSON or injection.
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Read,
    Write,
    Uniform,
    Texture,
    TextureWrite,
    TextureArray,
    Sampler,
}
#[derive(Clone, Debug)]
pub struct Field {
    pub name: String,
    pub offset: u32,
    pub lanes: u32,
}
#[derive(Clone, Debug)]
pub struct Resource {
    pub binding: u32,
    pub kind: Kind,
    pub size: u32,
    pub fields: Vec<Field>,
    pub internal: bool,
}
#[derive(Clone, Debug)]
pub struct Interface {
    pub workgroup: [u32; 3],
    pub descriptor_set: u32,
    pub resources: BTreeMap<String, Resource>,
    pub entries: BTreeMap<String, Vec<String>>,
}
impl Interface {
    pub fn resources_for(&self, entry: &str) -> io::Result<Vec<(&str, &Resource)>> {
        self.entries
            .get(entry)
            .ok_or_else(|| invalid("unknown shader entry"))?
            .iter()
            .map(|name| {
                self.resources
                    .get(name)
                    .map(|r| (name.as_str(), r))
                    .ok_or_else(|| invalid("unknown entry resource"))
            })
            .collect()
    }
    // A stable, versioned binary encoding keeps layout changes in shader cache keys.
    pub fn cache_bytes(&self) -> Vec<u8> {
        fn word(out: &mut Vec<u8>, n: u32) {
            out.extend(n.to_le_bytes());
        }
        fn text(out: &mut Vec<u8>, s: &str) {
            out.extend(s.as_bytes());
            out.push(0);
        }
        let mut out = b"tileink-interface-v1\0".to_vec();
        for n in self.workgroup {
            word(&mut out, n);
        }
        word(&mut out, self.descriptor_set);
        word(&mut out, self.resources.len() as u32);
        for (name, r) in &self.resources {
            text(&mut out, name);
            word(&mut out, r.binding);
            word(
                &mut out,
                match r.kind {
                    Kind::Read => 0,
                    Kind::Write => 1,
                    Kind::Uniform => 2,
                    Kind::Texture => 3,
                    Kind::TextureWrite => 4,
                    Kind::TextureArray => 5,
                    Kind::Sampler => 6,
                },
            );
            word(&mut out, r.size);
            word(&mut out, u32::from(r.internal));
            word(&mut out, r.fields.len() as u32);
            for f in &r.fields {
                text(&mut out, &f.name);
                word(&mut out, f.offset);
                word(&mut out, f.lanes);
            }
        }
        word(&mut out, self.entries.len() as u32);
        for (name, resources) in &self.entries {
            text(&mut out, name);
            word(&mut out, resources.len() as u32);
            for resource in resources {
                text(&mut out, resource);
            }
        }
        out
    }
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn identifier(name: &str) -> bool {
    !name.is_empty()
        && !name.as_bytes()[0].is_ascii_digit()
        && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
}
pub fn validate(abi: &Interface) -> io::Result<()> {
    if abi.descriptor_set != 0
        || abi.workgroup.iter().any(|&n| n == 0 || n > 1024)
        || abi.workgroup.iter().map(|&n| u64::from(n)).product::<u64>() > 1024
    {
        return Err(invalid("invalid descriptor set or workgroup"));
    }
    let mut slots = BTreeSet::new();
    for (name, r) in &abi.resources {
        if !identifier(name) || r.binding > 31 || !slots.insert(r.binding) {
            return Err(invalid("invalid resource binding"));
        }
        if r.kind == Kind::Uniform {
            let mut end = 0;
            let mut names = BTreeSet::new();
            if r.fields.is_empty() || r.size > 65536 {
                return Err(invalid("invalid uniform size"));
            }
            for f in &r.fields {
                if !identifier(&f.name)
                    || !names.insert(&f.name)
                    || !matches!(f.lanes, 1 | 4)
                    || f.offset != end
                    || (f.lanes == 4 && !f.offset.is_multiple_of(16))
                {
                    return Err(invalid("invalid uniform field"));
                }
                end += f.lanes * 4;
            }
            if end != r.size {
                return Err(invalid("invalid uniform size"));
            }
        } else if r.size != if r.kind == Kind::Sampler { 0 } else { 4 } || !r.fields.is_empty() {
            return Err(invalid("invalid resource size"));
        }
        if r.internal
            && (name != "dispatch_grid"
                || r.binding != 31
                || r.kind != Kind::Uniform
                || r.size != 16)
            || r.binding == 31 && !r.internal
        {
            return Err(invalid("invalid internal dispatch binding"));
        }
    }
    if abi.entries.is_empty() {
        return Err(invalid("empty shader inventory"));
    }
    for (entry, names) in &abi.entries {
        let mut seen = BTreeSet::new();
        if !identifier(entry)
            || names.is_empty()
            || names
                .iter()
                .any(|name| !abi.resources.contains_key(name) || !seen.insert(name))
        {
            return Err(invalid("invalid shader resource inventory"));
        }
    }
    Ok(())
}
pub fn binding_declarations(abi: &Interface, entry: &str) -> io::Result<String> {
    validate(abi)?;
    let mut resources = abi.resources_for(entry)?;
    resources.sort_by_key(|(_, r)| r.binding);
    let bindings = resources
        .into_iter()
        .map(|(_, r)| {
            let kind = match r.kind {
                Kind::Read => "Read",
                Kind::Write => "Write",
                Kind::Uniform => "Uniform",
                Kind::Texture => "Texture",
                Kind::TextureWrite => "TextureWrite",
                Kind::TextureArray => "TextureArray",
                Kind::Sampler => "Sampler",
            };
            Ok(format!(
                "Binding {{ slot: {}, kind: BindingKind::{kind}, size: {}, internal: {} }}",
                r.binding, r.size, r.internal
            ))
        })
        .collect::<io::Result<Vec<_>>>()?;
    Ok(format!("&[{}]", bindings.join(",")))
}
