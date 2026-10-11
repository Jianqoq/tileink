//! Controlled subprocess protocol for runtime compiler/cache error tests.
//! This does not compile HLSL or stand in for native DXC conformance tests.
use std::{env, fs, io::Write, path::PathBuf};

fn main() {
    let args: Vec<_> = env::args().skip(1).collect();
    let root = PathBuf::from(env::var_os("TILEINK_DXC_PROCESS_TEST_ROOT").unwrap());
    let operation = if args == ["--version"] {
        "version"
    } else if args[0] == "-dumpbin" {
        "reflect"
    } else {
        "compile"
    };
    writeln!(
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(root.join("calls"))
            .unwrap(),
        "{operation}"
    )
    .unwrap();
    let mode = fs::read_to_string(root.join("mode")).unwrap();
    match operation {
        "version" => println!("controlled DXC process fixture"),
        "reflect" => match mode.as_str() {
            "reflection-failure" => {
                eprintln!("deliberate reflection process failure");
                std::process::exit(17);
            }
            "invalid-encoding" => std::io::stdout().write_all(&[0xff]).unwrap(),
            _ => println!(
                "; EntryFunctionName: paint\n; NumThreads=({},1,1)\n; Resource Bindings:\n; destination UAV byte r/w U0 u3 1\ntarget datalayout",
                if mode == "wrong-abi" { 128 } else { 256 }
            ),
        },
        "compile" => {
            if mode == "compile-failure" {
                eprintln!("deliberate compile process failure");
                std::process::exit(18);
            }
            let output = &args[args.iter().position(|arg| arg == "-Fo").unwrap() + 1];
            let mut bytes = [0u8; 32];
            bytes[..4].copy_from_slice(b"DXBC");
            bytes[24..28].copy_from_slice(&32u32.to_le_bytes());
            fs::write(output, bytes).unwrap();
        }
        _ => unreachable!(),
    }
}
