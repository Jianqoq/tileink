use super::abi::{Interface, Kind};
use std::io;

pub fn validate(text: &str, entry: &str, abi: &Interface) -> io::Result<()> {
    super::abi::validate(abi)?;
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
    let expected = abi.resources_for(entry)?;
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
            let ty = if field.lanes == 1 { "uint" } else { "uint4" };
            let prefix = format!("{ty} {};", field.name);
            let offset = lines[start..=end]
                .iter()
                .find(|l| l.starts_with(&prefix))
                .and_then(|l| l.split("Offset:").nth(1))
                .and_then(|v| v.trim().parse::<u32>().ok());
            require(offset == Some(field.offset), "DXIL uniform offset/type")?;
        }
    }
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
    require(bindings.len() == expected.len(), "DXIL resource count")?;
    for (name, resource) in expected {
        let (prefix, kind) = match resource.kind {
            Kind::Uniform => ("cb", ["cbuffer", "NA", "NA"]),
            Kind::Read => ("t", ["texture", "byte", "r/o"]),
            Kind::Write => ("u", ["UAV", "byte", "r/w"]),
            Kind::Texture => ("t", ["texture", "f32", "2d"]),
        };
        let register = format!("{prefix}{}", resource.binding);
        require(
            bindings
                .iter()
                .any(|b| b[0] == name && b[6] == "1" && b[5] == register && b[1..4] == kind),
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
