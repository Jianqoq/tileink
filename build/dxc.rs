use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub(crate) const DISCOVERY_ENVIRONMENT_VARIABLES: [&str; 4] =
    ["TILEINK_DXC_PATH", "PATH", "ProgramFiles(x86)", "HOST"];

pub(crate) fn emit_discovery_inputs() {
    for variable in DISCOVERY_ENVIRONMENT_VARIABLES {
        println!("cargo:rerun-if-env-changed={variable}");
    }
    if let Some(path) = windows_sdk_bin_root(env::var_os("ProgramFiles(x86)")) {
        // Tracking the discovery directory fixes the missing-DXC case: installing a newer SDK
        // must invalidate the empty fallback artifact even though no executable existed before.
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

pub(crate) fn emit_toolchain_inputs(dxc: &Path) {
    for path in toolchain_inputs(dxc) {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

pub(crate) fn find() -> Option<PathBuf> {
    if let Some(path) = env::var_os("TILEINK_DXC_PATH").map(PathBuf::from) {
        return path.is_file().then_some(path);
    }
    if let Ok(output) = Command::new("where.exe").arg("dxc.exe").output()
        && output.status.success()
        && let Some(path) = String::from_utf8_lossy(&output.stdout).lines().next()
    {
        let path = PathBuf::from(path.trim());
        if path.is_file() {
            return Some(path);
        }
    }
    let mut versions = fs::read_dir(windows_sdk_bin_root(env::var_os("ProgramFiles(x86)"))?)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    versions.sort_unstable_by(|left, right| right.file_name().cmp(&left.file_name()));
    let host_arch = match env::var("HOST").unwrap_or_default().split('-').next() {
        Some("aarch64") => "arm64",
        Some("i686" | "i586") => "x86",
        _ => "x64",
    };
    versions
        .into_iter()
        .map(|version| version.join(host_arch).join("dxc.exe"))
        .find(|path| path.is_file())
}

fn windows_sdk_bin_root(program_files_x86: Option<OsString>) -> Option<PathBuf> {
    Some(
        PathBuf::from(program_files_x86?)
            .join("Windows Kits")
            .join("10")
            .join("bin"),
    )
}

fn toolchain_inputs(dxc: &Path) -> Vec<PathBuf> {
    let mut inputs = vec![dxc.to_path_buf()];
    if let Some(directory) = dxc.parent() {
        for library in ["dxcompiler.dll", "dxil.dll"] {
            let path = directory.join(library);
            if path.is_file() {
                inputs.push(path);
            }
        }
    }
    inputs
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{DISCOVERY_ENVIRONMENT_VARIABLES, toolchain_inputs, windows_sdk_bin_root};

    #[test]
    fn discovery_tracks_every_environment_input() {
        assert_eq!(
            DISCOVERY_ENVIRONMENT_VARIABLES,
            ["TILEINK_DXC_PATH", "PATH", "ProgramFiles(x86)", "HOST"]
        );
    }

    #[test]
    fn toolchain_inputs_include_the_matching_compiler_libraries() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "tileink-dxc-inputs-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let dxc = root.join("dxc.exe");
        let compiler = root.join("dxcompiler.dll");
        let validator = root.join("dxil.dll");
        for path in [&dxc, &compiler, &validator] {
            fs::write(path, []).unwrap();
        }

        assert_eq!(toolchain_inputs(&dxc), vec![dxc, compiler, validator]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sdk_discovery_root_is_stable() {
        assert_eq!(
            windows_sdk_bin_root(Some("C:/Program Files (x86)".into())).unwrap(),
            std::path::PathBuf::from("C:/Program Files (x86)")
                .join("Windows Kits")
                .join("10")
                .join("bin")
        );
    }
}
