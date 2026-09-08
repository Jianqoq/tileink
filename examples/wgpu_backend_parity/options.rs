use std::{
    ffi::OsString,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use super::Result;

pub const HELP: &str =
    "wgpu_backend_parity [--input SVG_FILE_OR_DIR] [--textures native|portable|both]
    [--dxc PATH_TO_DXCOMPILER_DLL] [--output NEW_DIRECTORY] [--luid HEX_LUID]
Without --input, render the built-in semantic probes. Requires hardware DX12 and Vulkan
on the same Windows GPU. Compares all RGBA bytes without tolerance. Output must be new.";

#[derive(Debug, PartialEq, Eq)]
pub struct Options {
    pub input: Option<PathBuf>,
    pub output: PathBuf,
    pub dxc: Option<PathBuf>,
    pub textures: Vec<bool>,
    pub luid: Option<String>,
}

impl Options {
    pub fn resolve_dxc(&mut self) -> Result<()> {
        if let Some(path) = &self.dxc {
            // LoadLibrary searches for relative names. Resolve once so the loaded
            // compiler and the file hashed in the manifest are identical.
            let absolute = path.canonicalize()?;
            if !absolute.is_file() {
                return Err("--dxc must point to a compiler DLL file".into());
            }
            absolute.to_str().ok_or("DXC path is not valid UTF-8")?;
            self.dxc = Some(absolute);
        }
        Ok(())
    }

    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Option<Self>> {
        let mut input = None;
        let mut output = None;
        let mut dxc = None;
        let mut textures = None;
        let mut luid = None;
        let mut seen = std::collections::HashSet::new();
        let mut args = args.into_iter();
        while let Some(flag) = args.next() {
            if flag == "--help" || flag == "-h" {
                return Ok(None);
            }
            let flag = flag.to_str().ok_or("argument name is not UTF-8")?;
            if !seen.insert(flag.to_owned()) {
                return Err(format!("duplicate argument {flag}").into());
            }
            if !matches!(
                flag,
                "--input" | "--output" | "--dxc" | "--textures" | "--luid"
            ) {
                return Err(format!("unknown argument {flag}").into());
            }
            let value = args
                .next()
                .ok_or_else(|| format!("missing value for {flag}"))?;
            if value.is_empty() || value.to_string_lossy().starts_with("--") {
                return Err(format!("missing value for {flag}").into());
            }
            match flag {
                "--input" => input = Some(PathBuf::from(value)),
                "--output" => output = Some(PathBuf::from(value)),
                "--dxc" => dxc = Some(PathBuf::from(value)),
                "--textures" => {
                    textures = Some(match value.to_str() {
                        Some("native") => vec![false],
                        Some("portable") => vec![true],
                        Some("both") => vec![false, true],
                        _ => return Err("--textures must be native, portable, or both".into()),
                    })
                }
                "--luid" => {
                    let value = value.into_string().map_err(|_| "LUID is not UTF-8")?;
                    if value.len() != 16 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                        return Err(
                            "--luid requires 16 hexadecimal digits in reported byte order".into(),
                        );
                    }
                    luid = Some(value.to_ascii_lowercase());
                }
                _ => unreachable!(),
            }
        }
        let output = output.unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target/backend-parity")
                .join(format!(
                    "{}-{}",
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_nanos(),
                    std::process::id()
                ))
        });
        Ok(Some(Self {
            input,
            output,
            dxc,
            textures: textures.unwrap_or_else(|| vec![false, true]),
            luid,
        }))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn dxc_reference_rejects_missing_files_and_directories() -> Result<()> {
        let mut missing = Options::parse([
            OsString::from("--dxc"),
            OsString::from("target/nonexistent-parity-dxc.dll"),
        ])?
        .unwrap();
        assert!(missing.resolve_dxc().is_err());
        let mut directory =
            Options::parse([OsString::from("--dxc"), OsString::from(".")])?.unwrap();
        assert!(directory.resolve_dxc().is_err());
        let mut automatic = Options::parse([])?.unwrap();
        automatic.resolve_dxc()?;
        assert_eq!(automatic.dxc, None);
        Ok(())
    }

    #[test]
    fn dxc_reference_resolves_the_exact_file_before_loading() -> Result<()> {
        let directory = PathBuf::from("target").join(format!(
            "tileink-parity-dxc-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        std::fs::create_dir_all(&directory)?;
        let file = directory.join("dxcompiler.dll");
        std::fs::write(&file, b"compiler identity fixture")?;
        let expected = file.canonicalize()?;
        let mut options =
            Options::parse([OsString::from("--dxc"), file.into_os_string()])?.unwrap();
        options.resolve_dxc()?;
        std::fs::remove_dir_all(directory)?;
        assert_eq!(
            options.dxc,
            Some(expected),
            "runtime loading and the manifest must use the same absolute file"
        );
        Ok(())
    }

    use super::*;
    fn parse(args: &[&str]) -> Result<Option<Options>> {
        Options::parse(args.iter().map(OsString::from))
    }

    #[test]
    fn defaults_cover_both_texture_paths() {
        assert_eq!(parse(&[]).unwrap().unwrap().textures, [false, true]);
        assert!(parse(&["--help"]).unwrap().is_none());
    }

    #[test]
    fn rejects_missing_duplicate_unknown_and_invalid_values() {
        for args in [
            vec!["--input"],
            vec!["--input", "--output"],
            vec!["--input", ""],
            vec!["--unknown", "x"],
            vec!["--textures", "native", "--textures", "both"],
            vec!["--textures", "dx12"],
            vec!["--luid", "4090"],
        ] {
            assert!(parse(&args).is_err(), "{args:?}");
        }
    }

    #[test]
    fn accepts_explicit_paths_mode_and_device() {
        let options = parse(&[
            "--input",
            "a b.svg",
            "--output",
            "result",
            "--textures",
            "portable",
            "--dxc",
            "compiler dir/dxcompiler.dll",
            "--luid",
            "ABCDEF0123456789",
        ])
        .unwrap()
        .unwrap();
        assert_eq!(options.input, Some(PathBuf::from("a b.svg")));
        assert_eq!(options.textures, [true]);
        assert_eq!(options.luid.as_deref(), Some("abcdef0123456789"));
    }
}
