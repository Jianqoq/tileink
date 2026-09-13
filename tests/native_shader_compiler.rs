#[allow(dead_code)]
#[path = "../build/native/dxc.rs"]
mod dxc;

#[test]
fn unavailable_compilers_and_unknown_targets_fail_explicitly() {
    assert!(dxc::Dxc::discover("relative/dxc.exe".into()).is_err());
    let missing = std::env::temp_dir()
        .join(format!("tileink-missing-dxc-{}", std::process::id()))
        .join("dxc.exe");
    assert!(dxc::Dxc::discover(missing).is_err());
    assert!(dxc::Dxc::flags("metal", "clear_words").is_err());
    for target in ["dxil", "spirv", "unknown"] {
        assert!(dxc::validate_container(target, b"not a shader").is_err());
    }
}

#[test]
#[ignore = "requires explicitly pinned TILEINK_NATIVE_DXC_PATH"]
fn pinned_dxc_reports_invalid_source_for_both_targets() {
    let compiler =
        dxc::Dxc::discover(std::env::var_os("TILEINK_NATIVE_DXC_PATH").unwrap().into()).unwrap();
    assert!(
        !compiler.identity["version"]
            .as_str()
            .unwrap()
            .trim()
            .is_empty()
    );
    let work = std::env::temp_dir().join(format!("tileink-dxc-negative-{}", std::process::id()));
    for target in ["dxil", "spirv"] {
        let flags = dxc::Dxc::flags(target, "clear_words").unwrap();
        let error = compiler
            .compile(
                "[numthreads(64,1,1)] void clear_words(){ undefined_name = 1; }",
                &flags,
                &work.join(target),
                target,
            )
            .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("undefined_name"), "{message}");
        assert!(message.contains(target), "{message}");
        assert!(!work.join(target).join(format!("shader.{target}")).exists());
    }
    std::fs::remove_dir_all(work).unwrap();
}
