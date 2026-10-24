"""CI installs its compiler explicitly; renderer builds never download or substitute one."""
import hashlib
import io
import os
from pathlib import Path
import subprocess
import tarfile
from urllib.request import urlopen
import zipfile


PACKAGES = {
    ("Linux", "X64"): (
        "linux_dxc_2025_02_20.x86_64.tar.gz",
        "e0580d90dbf6053a783ddd8d5153285f0606e5deaad17a7a6452f03acdf88c71",
        "bin/dxc",
    ),
    ("Windows", "X64"): (
        "dxc_2025_02_20.zip",
        "70b1913a1bfce4a3e1a5311d16246f4ecdf3a3e613abec8aa529e57668426f85",
        "bin/x64/dxc.exe",
    ),
}


def install(platform, architecture, workspace):
    name, expected, compiler = PACKAGES[platform, architecture]
    url = f"https://github.com/microsoft/DirectXShaderCompiler/releases/download/v1.8.2502/{name}"
    with urlopen(url, timeout=60) as response:
        data = response.read()
    if hashlib.sha256(data).hexdigest() != expected:
        raise ValueError(f"DXC SHA-256 mismatch: {name}")
    root = workspace / "target/toolchains/dxc-v1.8.2502"
    root.mkdir(parents=True, exist_ok=True)
    if platform == "Windows":
        with zipfile.ZipFile(io.BytesIO(data)) as archive:
            archive.extractall(root)
    else:
        with tarfile.open(fileobj=io.BytesIO(data)) as archive:
            archive.extractall(root, filter="data")
    return (root / compiler).resolve()


def main():
    compiler = install(os.environ["RUNNER_OS"], os.environ["RUNNER_ARCH"],
                       Path(os.environ["GITHUB_WORKSPACE"]))
    subprocess.run([str(compiler), "--version"], check=True)
    with Path(os.environ["GITHUB_ENV"]).open("a", encoding="utf-8") as environment:
        environment.write(f"TILEINK_NATIVE_DXC_PATH={compiler}\n")


if __name__ == "__main__":
    main()
