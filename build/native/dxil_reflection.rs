use super::abi::{Interface, Kind};
use std::io;

pub fn validate(text: &str, entry: &str, abi: &Interface) -> io::Result<()> {
    validate_inner(text, entry, abi, false)
}
#[allow(dead_code)] // Also reused by the runtime application-shader compiler.
pub fn validate_user(text: &str, entry: &str, abi: &Interface) -> io::Result<()> {
    validate_inner(text, entry, abi, true)
}
fn validate_inner(text: &str, entry: &str, abi: &Interface, optional: bool) -> io::Result<()> {
    if optional {
        super::abi::validate_user(abi)?;
    } else {
        super::abi::validate(abi)?;
    }
    let lines: Vec<_> = text
        .lines()
        .map(|l| l.trim_start_matches(';').trim())
        .collect();
    require(
        lines.contains(&format!("EntryFunctionName: {entry}").as_str()),
        "DXIL entry",
    )?;
    require(
        lines.contains(
            &format!(
                "NumThreads=({},{},{})",
                abi.workgroup[0], abi.workgroup[1], abi.workgroup[2]
            )
            .as_str(),
        ),
        "DXIL workgroup",
    )?;
    let start = lines
        .iter()
        .position(|l| *l == "Resource Bindings:")
        .ok_or_else(|| io::Error::other("missing DXIL bindings"))?;
    let mut bindings = Vec::new();
    for line in &lines[start + 1..] {
        if line.starts_with("target datalayout") || (line.is_empty() && !bindings.is_empty()) {
            break;
        }
        if line.is_empty()
            || line.starts_with("Name ")
            || line.chars().all(|c| c == '-' || c.is_whitespace())
        {
            continue;
        }
        let tokens: Vec<_> = line.split_whitespace().collect();
        // Count every row, including arrays; filtering Count=1 would hide extra resources.
        require(tokens.len() == 7, "DXIL resource row")?;
        bindings.push(tokens);
    }
    let expected: Vec<_> = abi
        .resources_for(entry)?
        .into_iter()
        .filter(|(name, _)| !optional || bindings.iter().any(|b| b[0] == *name))
        .collect();
    if optional {
        require(
            abi.resources_for(entry)?
                .iter()
                .filter(|(_, r)| matches!(r.kind, Kind::Write | Kind::TextureWrite))
                .all(|(name, _)| expected.iter().any(|(active, _)| active == name)),
            "DXIL user shader output missing",
        )?;
    }
    for &(name, resource) in &expected {
        if resource.kind != Kind::Uniform {
            continue;
        }
        // Scope fields to their buffer, since different uniforms may reuse member names.
        let start = lines
            .iter()
            .position(|l| *l == format!("cbuffer {name}"))
            .ok_or_else(|| io::Error::other("DXIL uniform declaration"))?;
        let end = start
            + lines[start..]
                .iter()
                .position(|l| l.starts_with(&format!("}} {name};")) && l.contains("Size:"))
                .ok_or_else(|| io::Error::other("DXIL uniform end"))?;
        require(
            lines[end]
                .split_whitespace()
                .collect::<Vec<_>>()
                .ends_with(&["Size:", &resource.size.to_string()]),
            "DXIL uniform size",
        )?;
        for field in &resource.fields {
            let scalar = match field.scalar {
                super::abi::Scalar::U32 => "uint",
                super::abi::Scalar::I32 => "int",
                super::abi::Scalar::F32 => "float",
            };
            let ty = if field.lanes == 1 {
                scalar.to_owned()
            } else {
                format!("{scalar}4")
            };
            let prefix = format!("{ty} {};", field.name);
            let offset = lines[start..=end]
                .iter()
                .find(|l| l.starts_with(&prefix))
                .and_then(|l| l.split("Offset:").nth(1))
                .and_then(|v| v.trim().parse::<u32>().ok());
            require(offset == Some(field.offset), "DXIL uniform offset/type")?;
        }
    }
    require(bindings.len() == expected.len(), "DXIL resource count")?;
    for (name, resource) in expected {
        let (prefix, kind) = match resource.kind {
            Kind::Uniform => ("cb", ["cbuffer", "NA", "NA"]),
            Kind::Read => ("t", ["texture", "byte", "r/o"]),
            Kind::Write => ("u", ["UAV", "byte", "r/w"]),
            Kind::Texture | Kind::TextureTable => ("t", ["texture", "f32", "2d"]),
            Kind::TextureWrite => ("u", ["UAV", "f32", "2d"]),
            Kind::TextureArray => ("t", ["texture", "f32", "2darray"]),
            Kind::Sampler => ("s", ["sampler", "NA", "NA"]),
        };
        let register = format!("{prefix}{}", resource.binding);
        require(
            bindings.iter().any(|b| {
                b[0] == name
                    && b[6] == resource.count.to_string()
                    && b[5] == register
                    && b[1..4] == kind
            }),
            "DXIL register/space/type",
        )?;
    }
    Ok(())
}
fn require(condition: bool, message: &str) -> io::Result<()> {
    if condition {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::InvalidData, message))
    }
}
