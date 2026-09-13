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
        lines.contains(
            &format!(
                "NumThreads=({},{},{})",
                abi["workgroup"][0], abi["workgroup"][1], abi["workgroup"][2]
            )
            .as_str(),
        ),
        "DXIL workgroup",
    )?;
    if abi["schema"] == 2 {
        for name in abi["entry_resources"][entry]
            .as_array()
            .ok_or_else(|| io::Error::other("ABI resources"))?
        {
            let name = name
                .as_str()
                .ok_or_else(|| io::Error::other("ABI resource"))?;
            let resource = &abi["resources"][name];
            if resource["kind"] != "uniform" {
                continue;
            }
            let size = resource["size"]
                .as_u64()
                .ok_or_else(|| io::Error::other("ABI uniform size"))?;
            require(
                lines.iter().any(|l| {
                    l.starts_with(&format!("}} {name};"))
                        && l.split_whitespace()
                            .collect::<Vec<_>>()
                            .ends_with(&["Size:", &size.to_string()])
                }),
                "DXIL uniform size",
            )?;
            for field in resource["fields"]
                .as_array()
                .ok_or_else(|| io::Error::other("ABI uniform fields"))?
            {
                let prefix = format!("uint {};", field["name"].as_str().unwrap());
                // Restrict the search to this named buffer; different uniforms
                // may reuse member names with different offsets.
                let start = lines
                    .iter()
                    .position(|l| *l == format!("cbuffer {name}"))
                    .ok_or_else(|| io::Error::other("DXIL uniform declaration"))?;
                let end = lines[start..]
                    .iter()
                    .position(|l| l.starts_with(&format!("}} {name};")))
                    .ok_or_else(|| io::Error::other("DXIL uniform end"))?
                    + start;
                let offset = lines[start..=end]
                    .iter()
                    .find(|l| l.starts_with(&prefix))
                    .and_then(|l| l.split("Offset:").nth(1))
                    .and_then(|v| v.trim().parse::<u64>().ok());
                require(
                    offset.is_some() && offset == field["offset"].as_u64(),
                    "DXIL uniform offset/type",
                )?;
            }
        }
    } else if entry != "range_scatter" {
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
    }
    let dynamic: Vec<(&str, String, u64)> = if abi["schema"] == 2 {
        abi["entry_resources"][entry]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| {
                let name = name.as_str().unwrap();
                let resource = &abi["resources"][name];
                let slot = resource["binding"].as_u64().unwrap();
                let prefix = match resource["kind"].as_str().unwrap() {
                    "uniform" => "cb",
                    "read" => "t",
                    _ => "u",
                };
                (name, format!("{prefix}{slot}"), slot)
            })
            .collect()
    } else {
        Vec::new()
    };
    let expected: Vec<(&str, &str, u64)> = if abi["schema"] == 2 {
        dynamic
            .iter()
            .map(|(name, register, slot)| (*name, register.as_str(), *slot))
            .collect()
    } else if entry == "range_scatter" {
        vec![("destination", "u0", 0), ("source", "t1", 1)]
    } else if entry == "sample_words" {
        vec![
            ("destination", "u0", 0),
            ("source", "t1", 1),
            ("params", "cb2", 2),
            ("texels", "t3", 3),
        ]
    } else if entry == "copy_words" {
        vec![
            ("destination", "u0", 0),
            ("source", "t1", 1),
            ("params", "cb2", 2),
        ]
    } else {
        vec![("destination", "u0", 0), ("params", "cb2", 2)]
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
    for &(name, register, binding) in &expected {
        require(
            (if abi["schema"] == 2 {
                &abi["resources"][name]["binding"]
            } else {
                &abi["bindings"][name]
            })
            .as_u64()
                == Some(binding)
                && bindings.iter().any(|b| {
                    b[0] == name
                        && b[6] == "1"
                        && b[5] == register
                        && match if abi["schema"] == 2 {
                            match abi["resources"][name]["kind"].as_str().unwrap() {
                                "uniform" => "params",
                                "read" => "source",
                                _ => "destination",
                            }
                        } else {
                            name
                        } {
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
