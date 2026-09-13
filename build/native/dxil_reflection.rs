//! Validate DXC's reflection against the explicit minimum probe ABI.
use std::io;

pub fn validate(text: &str, entry: &str, abi: &serde_json::Value) -> io::Result<()> {
    require(
        abi["descriptor_set"].as_u64() == Some(0),
        "DXIL register space ABI",
    )?;
    let lines: Vec<_> = text
        .lines()
        .map(|l| l.trim_start_matches(';').trim())
        .collect();
    require(
        lines.contains(&format!("EntryFunctionName: {entry}").as_str()),
        "DXIL entry",
    )?;
    require(
        lines.contains(&"NumThreads=(64,1,1)") && abi["workgroup"] == serde_json::json!([64, 1, 1]),
        "DXIL workgroup",
    )?;
    require(
        lines.iter().any(|l| {
            l.starts_with("} params;")
                && l.split_whitespace()
                    .collect::<Vec<_>>()
                    .ends_with(&["Size:", "32"])
        }) && abi["parameter_size"].as_u64() == Some(32),
        "DXIL parameter size",
    )?;
    for (field, ty) in [
        ("count", "uint"),
        ("source_offset", "uint"),
        ("destination_offset", "uint"),
        ("stride", "uint"),
        ("value", "uint4"),
    ] {
        let prefix = format!("{ty} {field};");
        let offset = lines
            .iter()
            .find(|l| l.starts_with(&prefix))
            .and_then(|l| l.split("Offset:").nth(1))
            .and_then(|s| s.trim().parse::<u64>().ok());
        require(
            offset.is_some() && offset == abi["parameter_offsets"][field].as_u64(),
            "DXIL parameter offset/type",
        )?;
    }
    let expected: &[(&str, &str, u64)] = if entry == "sample_words" {
        &[
            ("destination", "u0", 0),
            ("source", "t1", 1),
            ("params", "cb2", 2),
            ("texels", "t3", 3),
        ]
    } else if entry == "copy_words" {
        &[
            ("destination", "u0", 0),
            ("source", "t1", 1),
            ("params", "cb2", 2),
        ]
    } else {
        &[("destination", "u0", 0), ("params", "cb2", 2)]
    };
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
        // Parse every resource row before checking count; filtering Count=1
        // first hid extra arrays and accepted an incomplete root signature.
        require(tokens.len() == 7, "DXIL resource row")?;
        bindings.push(tokens);
    }
    require(bindings.len() == expected.len(), "DXIL resource count")?;
    for &(name, register, binding) in expected {
        require(
            abi["bindings"][name].as_u64() == Some(binding)
                && bindings.iter().any(|b| {
                    b[0] == name
                        && b[6] == "1"
                        && b[5] == register
                        && match name {
                            "params" => b[1..4] == ["cbuffer", "NA", "NA"],
                            "texels" => b[1..4] == ["texture", "f32", "2d"],
                            "source" => b[1..4] == ["texture", "byte", "r/o"],
                            "destination" => b[1..4] == ["UAV", "byte", "r/w"],
                            _ => false,
                        }
                }),
            "DXIL register/space",
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
