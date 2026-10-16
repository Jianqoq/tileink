"""Provisioning semantics; actual DXC compilation is checked separately in CI."""
import hashlib
import importlib.util
import io
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch
from urllib.error import URLError
import zipfile


spec = importlib.util.spec_from_file_location(
    "dxc_installer", Path(__file__).resolve().parents[1] / ".github/actions/setup-dxc/install.py")
installer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(installer)


class DxcInstallerTest(unittest.TestCase):
    def archive(self, platform, compiler):
        data = io.BytesIO()
        members = {compiler: b"compiler", "LICENSE.txt": b"license"}
        if platform == "Windows":
            members["bin/x64/dxcompiler.dll"] = b"compiler library"
            with zipfile.ZipFile(data, "w") as archive:
                for name, content in members.items():
                    archive.writestr(name, content)
        else:
            members["lib/libdxcompiler.so"] = b"compiler library"
            with tarfile.open(fileobj=data, mode="w:gz") as archive:
                for name, content in members.items():
                    entry = tarfile.TarInfo(name)
                    entry.size, entry.mode = len(content), 0o755
                    archive.addfile(entry, io.BytesIO(content))
        return data.getvalue()

    def test_both_native_packages_install_the_selected_compiler_and_library(self):
        for platform, compiler in [("Linux", "bin/dxc"), ("Windows", "bin/x64/dxc.exe")]:
            with self.subTest(platform=platform), tempfile.TemporaryDirectory() as directory:
                name, _, _ = installer.PACKAGES[platform, "X64"]
                data = self.archive(platform, compiler)
                package = name, hashlib.sha256(data).hexdigest(), compiler
                with patch.dict(installer.PACKAGES, {(platform, "X64"): package}), \
                        patch.object(installer, "urlopen", return_value=io.BytesIO(data)) as fetch:
                    path = installer.install(platform, "X64", Path(directory))
                self.assertEqual(path.read_bytes(), b"compiler")
                library = "bin/x64/dxcompiler.dll" if platform == "Windows" else "lib/libdxcompiler.so"
                self.assertEqual((path.parents[2 if platform == "Windows" else 1] / library).read_bytes(),
                                 b"compiler library")
                fetch.assert_called_once_with(
                    f"https://github.com/microsoft/DirectXShaderCompiler/releases/download/v1.8.2502/{name}",
                    timeout=60)

    def test_checksum_failure_never_uses_an_existing_compiler(self):
        with tempfile.TemporaryDirectory() as directory:
            workspace = Path(directory)
            old = workspace / "target/toolchains/dxc-v1.8.2502/bin/dxc"
            old.parent.mkdir(parents=True)
            old.write_bytes(b"old compiler")
            with patch.object(installer, "urlopen", return_value=io.BytesIO(b"corrupt package")):
                with self.assertRaisesRegex(ValueError, "SHA-256"):
                    installer.install("Linux", "X64", workspace)
            self.assertEqual(old.read_bytes(), b"old compiler")

    def test_network_failure_propagates_without_installing_a_package(self):
        with tempfile.TemporaryDirectory() as directory:
            with patch.object(installer, "urlopen", side_effect=URLError("unavailable")):
                with self.assertRaises(URLError):
                    installer.install("Linux", "X64", Path(directory))
            self.assertEqual(list(Path(directory).iterdir()), [])

    def test_unsupported_hosts_never_select_another_platform_package(self):
        with patch.object(installer, "urlopen") as fetch:
            for platform, arch in [("macOS", "ARM64"), ("Linux", "ARM64"), ("Windows", "ARM64")]:
                with self.subTest(platform=platform, arch=arch), self.assertRaises(KeyError):
                    installer.install(platform, arch, Path("unused"))
            fetch.assert_not_called()

    def test_failed_compiler_probe_does_not_publish_a_path(self):
        with tempfile.TemporaryDirectory() as directory:
            env = Path(directory) / "environment"
            env.write_text("")
            with patch.dict(os.environ, RUNNER_OS="Linux", RUNNER_ARCH="X64",
                            GITHUB_WORKSPACE=directory, GITHUB_ENV=str(env)), \
                    patch.object(installer, "install", return_value=Path("compiler")), \
                    patch.object(installer.subprocess, "run", side_effect=subprocess.CalledProcessError(1, "dxc")):
                with self.assertRaises(subprocess.CalledProcessError):
                    installer.main()
            self.assertEqual(env.read_text(), "")

    def test_successful_probe_appends_the_absolute_path_in_a_workspace_with_spaces(self):
        with tempfile.TemporaryDirectory() as directory:
            workspace = Path(directory) / "workspace with spaces"
            env = Path(directory) / "environment"
            env.write_text("EXISTING=1\n")
            name, _, compiler = installer.PACKAGES["Linux", "X64"]
            data = self.archive("Linux", compiler)
            with patch.dict(os.environ, RUNNER_OS="Linux", RUNNER_ARCH="X64",
                            GITHUB_WORKSPACE=str(workspace), GITHUB_ENV=str(env)), \
                    patch.dict(installer.PACKAGES, {("Linux", "X64"):
                        (name, hashlib.sha256(data).hexdigest(), compiler)}), \
                    patch.object(installer, "urlopen", return_value=io.BytesIO(data)), \
                    patch.object(installer.subprocess, "run") as probe:
                installer.main()
            path = (workspace / "target/toolchains/dxc-v1.8.2502/bin/dxc").resolve()
            probe.assert_called_once_with([str(path), "--version"], check=True)
            self.assertEqual(env.read_text(), f"EXISTING=1\nTILEINK_NATIVE_DXC_PATH={path}\n")


if __name__ == "__main__":
    unittest.main()
