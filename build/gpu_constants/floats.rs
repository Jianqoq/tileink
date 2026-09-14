use std::{collections::BTreeSet, io};

/// Preserve the maintained literal verbatim so WGSL and HLSL round the same decimal.
/// Expressions and nonfinite values are rejected instead of silently approximated.
pub fn parse_wgsl(source: &str) -> io::Result<String> {
    let (lines, _) = super::syntax::parse(source)?;
    let mut names = BTreeSet::new();
    let mut result = String::new();
    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let invalid = || io::Error::other("invalid shared HLSL float constant");
        let declaration = line
            .strip_prefix("static const float ")
            .and_then(|s| s.strip_suffix(';'))
            .ok_or_else(invalid)?;
        let (name, value) = declaration.split_once('=').ok_or_else(invalid)?;
        let (name, value) = (name.trim(), value.trim());
        if !name.bytes().next().is_some_and(|c| c.is_ascii_uppercase())
            || !name
                .bytes()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
            || !names.insert(name.to_owned())
            || value.starts_with('+')
            || !value
                .bytes()
                .all(|c| c.is_ascii_digit() || b".-+eE".contains(&c))
            || !value.contains(['.', 'e', 'E'])
            || !value.parse::<f32>().is_ok_and(f32::is_finite)
        {
            return Err(invalid());
        }
        result.push_str(&format!("const {name}: f32 = {value};\n"));
    }
    if names.is_empty() {
        return Err(io::Error::other("empty shared HLSL float constants"));
    }
    Ok(result)
}
