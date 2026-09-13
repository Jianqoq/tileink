use serde_json::Value;
use std::{collections::BTreeSet, io};
fn error() -> io::Error {
    io::Error::other("invalid native compute ABI")
}
pub fn validate(abi: &Value) -> io::Result<()> {
    if abi["schema"] != 2 || abi["descriptor_set"] != 0 {
        return Err(error());
    }
    let group = abi["workgroup"].as_array().ok_or_else(error)?;
    if group.len() != 3
        || group
            .iter()
            .any(|v| v.as_u64().is_none_or(|n| n == 0 || n > 1024))
        || group.iter().map(|v| v.as_u64().unwrap()).product::<u64>() > 1024
    {
        return Err(error());
    }
    let resources = abi["resources"].as_object().ok_or_else(error)?;
    let mut slots = BTreeSet::new();
    for (name, resource) in resources {
        let slot = resource["binding"].as_u64().ok_or_else(error)?;
        let size = resource["size"].as_u64().ok_or_else(error)?;
        if slot > 31
            || !slots.insert(slot)
            || size == 0
            || size > u32::MAX as u64
            || !size.is_multiple_of(4)
        {
            return Err(error());
        }
        match resource["kind"].as_str() {
            Some("read" | "write") if size == 4 && resource.get("fields").is_none() => {}
            Some("uniform") if size <= 65536 => {
                let fields = resource["fields"].as_array().ok_or_else(error)?;
                if fields.is_empty() {
                    return Err(error());
                }
                let mut end = 0;
                let mut names = BTreeSet::new();
                for field in fields {
                    let name = field["name"].as_str().ok_or_else(error)?;
                    let offset = field["offset"].as_u64().ok_or_else(error)?;
                    if name.is_empty()
                        || !names.insert(name)
                        || field["type"] != "u32"
                        || offset != end
                    {
                        return Err(error());
                    }
                    end += 4;
                }
                if end != size {
                    return Err(error());
                }
            }
            _ => return Err(error()),
        }
        if resource["internal"].as_bool().unwrap_or(false)
            && (name != "dispatch_grid"
                || slot != 31
                || resource["kind"] != "uniform"
                || size != 16)
        {
            return Err(error());
        }
        if slot == 31 && !resource["internal"].as_bool().unwrap_or(false) {
            return Err(error());
        }
    }
    let programs = abi["programs"].as_array().ok_or_else(error)?;
    let mut names = BTreeSet::new();
    for program in programs {
        let name = program.as_str().ok_or_else(error)?;
        if name.is_empty() || !names.insert(name) {
            return Err(error());
        }
        let uses = abi["entry_resources"][name].as_array().ok_or_else(error)?;
        let mut seen = BTreeSet::new();
        if uses.is_empty() {
            return Err(error());
        }
        for name in uses {
            let name = name.as_str().ok_or_else(error)?;
            if !resources.contains_key(name) || !seen.insert(name) {
                return Err(error());
            }
        }
    }
    if programs.is_empty()
        || abi["entry_resources"].as_object().ok_or_else(error)?.len() != programs.len()
    {
        return Err(error());
    }
    Ok(())
}
pub fn declarations(abi: &Value, entry: &str) -> io::Result<String> {
    validate(abi)?;
    let mut bindings = Vec::new();
    for name in abi["entry_resources"][entry].as_array().ok_or_else(error)? {
        let name = name.as_str().ok_or_else(error)?;
        let resource = &abi["resources"][name];
        let kind = match resource["kind"].as_str().unwrap() {
            "read" => "Read",
            "write" => "Write",
            _ => "Uniform",
        };
        bindings.push((
            resource["binding"].as_u64().unwrap(),
            format!(
                "Binding {{ slot: {}, kind: BindingKind::{kind}, size: {}, internal: {} }}",
                resource["binding"],
                resource["size"],
                resource["internal"].as_bool().unwrap_or(false)
            ),
        ));
    }
    bindings.sort_by_key(|b| b.0);
    Ok(format!(
        "&[{}]",
        bindings
            .into_iter()
            .map(|b| b.1)
            .collect::<Vec<_>>()
            .join(",")
    ))
}
