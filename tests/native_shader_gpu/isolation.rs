use super::Result;
pub fn run(name: &str) -> Result<bool> {
    if std::env::var("TILEINK_NATIVE_FAULT_WORKER").ok().as_deref() == Some(name) {
        return Ok(false);
    }
    // D3D12 may share device/InfoQueue objects between contexts. Deliberate
    // diagnostic injection and quarantined owners must die with a child process
    // before ordinary validation runs, without clearing any validation messages.
    let status = std::process::Command::new(std::env::current_exe()?)
        .args([
            "--ignored",
            "--exact",
            name,
            "--test-threads=1",
            "--nocapture",
        ])
        .env("TILEINK_NATIVE_FAULT_WORKER", name)
        .status()?;
    if !status.success() {
        return Err(format!("isolated GPU fault test failed: {name}: {status}").into());
    }
    Ok(true)
}
