//! Read the deliberately small shared HLSL uint-constant language without a shader compiler.
//! Reject unsupported syntax instead of letting host and shader interpretation silently diverge.
use std::{collections::BTreeMap, env, fs, io, path::Path, sync::OnceLock};

#[path = "hlsl_source.rs"]
pub mod syntax;

pub fn parse(source: &str) -> io::Result<BTreeMap<String, u32>> {
    let mut constants = BTreeMap::<String, u32>::new();
    let (lines, _) = syntax::parse(source)?;
    for (index, line) in lines.iter().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let invalid = || {
            io::Error::other(format!(
                "invalid shared HLSL constant on line {}",
                index + 1
            ))
        };
        let declaration = line
            .strip_prefix("static const uint ")
            .and_then(|line| line.strip_suffix(';'))
            .ok_or_else(invalid)?;
        let (name, expression) = declaration.split_once('=').ok_or_else(invalid)?;
        let name = name.trim();
        if !name.bytes().next().is_some_and(|c| c.is_ascii_uppercase())
            || !name
                .bytes()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
        {
            return Err(invalid());
        }
        let mut value = 1u32;
        for factor in expression.split('*').map(str::trim) {
            let term = if let Some(&value) = constants.get(factor) {
                value
            } else {
                let digits = factor.strip_suffix('u').unwrap_or(factor);
                if digits.is_empty()
                    || (digits.len() > 1 && digits.starts_with('0'))
                    || !digits.bytes().all(|c| c.is_ascii_digit())
                {
                    return Err(invalid());
                }
                digits.parse::<u32>().map_err(|_| invalid())?
            };
            value = value.checked_mul(term).ok_or_else(invalid)?;
        }
        if constants.insert(name.to_owned(), value).is_some() {
            return Err(invalid());
        }
    }
    if constants.is_empty() {
        return Err(io::Error::other("empty shared HLSL constants"));
    }
    Ok(constants)
}

fn definitions() -> &'static BTreeMap<String, u32> {
    static CONSTANTS: OnceLock<BTreeMap<String, u32>> = OnceLock::new();
    CONSTANTS.get_or_init(|| {
        let path = Path::new(&env::var_os("CARGO_MANIFEST_DIR").unwrap())
            .join("src/shaders/hlsl/constants.hlsli");
        let source = fs::read_to_string(&path).expect("read shared HLSL constants");
        parse(&source).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
    })
}

pub fn get(name: &str) -> u32 {
    *definitions()
        .get(name)
        .unwrap_or_else(|| panic!("missing shared HLSL constant {name}"))
}

pub fn write_rust(out: &Path) -> io::Result<()> {
    let mut source =
        String::from("// Generated from src/shaders/hlsl/constants.hlsli. Do not edit.\n");
    for (name, value) in definitions() {
        let visibility = if name == "TILE_SIZE" {
            "pub"
        } else {
            "pub(crate)"
        };
        source.push_str(&format!("{visibility} const {name}: u32 = {value};\n"));
    }
    fs::write(out.join("tileink_gpu_constants.rs"), source)
}
