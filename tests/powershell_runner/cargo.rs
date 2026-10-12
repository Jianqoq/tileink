//! A native child process records what the real PowerShell runner passes to Cargo.
use std::{env, fs};

fn main() {
    fs::write(
        env::var_os("TILEINK_RUNNER_TRACE").unwrap(),
        format!("{:?}", env::args().skip(1).collect::<Vec<_>>()),
    )
    .unwrap();
    fs::write(
        env::var_os("TILEINK_RUNNER_ENV_TRACE").unwrap(),
        env::var("RAYON_NUM_THREADS").unwrap(),
    )
    .unwrap();
    fs::write(
        env::var_os("TILEINK_RUNNER_CWD_TRACE").unwrap(),
        env::current_dir().unwrap().to_str().unwrap(),
    )
    .unwrap();
    fs::write(
        env::var_os("TILEINK_RUNNER_PROGRAM_TRACE").unwrap(),
        env::current_exe().unwrap().to_str().unwrap(),
    )
    .unwrap();
    eprintln!("runner test diagnostic");
    std::process::exit(env::var("TILEINK_RUNNER_EXIT").unwrap().parse().unwrap());
}
