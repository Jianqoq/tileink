import hashlib
import json
from pathlib import Path
import tempfile
import unittest
import subprocess
from unittest.mock import patch

import acceptance


class AcceptanceTests(unittest.TestCase):
    def test_execute_rejects_unavailable_validation(self):
        diagnostics = (
            "[WARN wgpu_hal] Failed to get debug interface: unavailable",
            "[WARN wgpu_hal] Failed to get debug interface from device factory: unavailable",
            "[DEBUG wgpu_hal] InstanceFlags::VALIDATION requested, but unable to find layer: VK_LAYER_KHRONOS_validation",
            "[DEBUG wgpu_hal] Unable to find extension: VK_EXT_debug_utils",
        )
        with tempfile.TemporaryDirectory() as temp:
            for index, diagnostic in enumerate(diagnostics):
                def emit(*args, **kwargs):
                    kwargs["stdout"].write(diagnostic)
                with patch.object(acceptance.subprocess, "run", side_effect=emit):
                    with self.assertRaisesRegex(ValueError, "validation unavailable"):
                        acceptance.execute("binary", "test", {}, Path(temp) / f"{index}.log")

    def test_execute_rejects_error_severity_and_process_failure(self):
        with tempfile.TemporaryDirectory() as temp:
            for index, diagnostic in enumerate(("[ERROR wgpu_hal] device failed", "[2026-09-20T12:00:00Z ERROR wgpu_hal] device failed", "test run_selected_corpus ... [2026-09-20T12:00:00Z ERROR wgpu_hal] device failed", "[2026-09-20T12:00:00Z WARN wgpu_hal] adapter warning")):
                def emit(*args, **kwargs):
                    kwargs["stdout"].write(diagnostic)
                with patch.object(acceptance.subprocess, "run", side_effect=emit):
                    log = Path(temp) / f"{index}.log"
                    if " ERROR " in diagnostic or diagnostic.startswith("[ERROR"):
                        with self.assertRaisesRegex(ValueError, "API error"):
                            acceptance.execute("binary", "test", {}, log)
                    else:
                        acceptance.execute("binary", "test", {}, log)
            with patch.object(acceptance.subprocess, "run", side_effect=subprocess.CalledProcessError(1, "binary")):
                with self.assertRaises(subprocess.CalledProcessError):
                    acceptance.execute("binary", "test", {}, Path(temp) / "failed.log")

    def test_unignored_output_cannot_pollute_source_manifest(self):
        with patch.object(acceptance.subprocess, "run") as command:
            command.return_value.returncode = 1
            with self.assertRaisesRegex(ValueError, "git-ignored"):
                acceptance.validate_output(Path("repo"), Path("repo/evidence"))
            command.return_value.returncode = 0
            acceptance.validate_output(Path("repo"), Path("repo/target/evidence"))
            acceptance.validate_output(Path("repo"), Path("outside/evidence"))

    def test_build_source_change_is_rejected(self):
        with patch.object(acceptance, "source_snapshot", return_value={"a": "after"}):
            with self.assertRaisesRegex(ValueError, "sources changed"):
                acceptance.verify_sources(Path("."), {"a": "before"})
            acceptance.verify_sources(Path("."), {"a": "after"})

    def test_exact_bytes_include_transparent_rgb(self):
        row = {"width": 1, "height": 1}
        acceptance.compare_images(b'\0\0\0\0', b'\0\0\0\0', row, row)
        for channel in range(4):
            changed = bytearray(4)
            changed[channel] = 1
            with self.assertRaises(ValueError):
                acceptance.compare_images(bytes(4), changed, row, row)
        with self.assertRaises(ValueError):
            acceptance.compare_images(bytes(4), bytes(4), row, {"width": 2, "height": 1})

    def test_report_requires_complete_unique_outputs_and_intact_bytes(self):
        with tempfile.TemporaryDirectory() as temp, patch.dict(acceptance.SUITES, sample=(1, 1)):
            root = Path(temp)
            (root / "0.rgba").write_bytes(bytes(4))
            row = {"case": "a", "variant": 0, "width": 1, "height": 1, "file": "0.rgba",
                   "sha256": hashlib.sha256(bytes(4)).hexdigest()}
            report = {"schema": 1, "passed": True, "cases": ["a"], "variants": 1, "rows": [row],
                      "manifest": {"route": "r", "suite": "sample", "physical_identity": "gpu"}}
            def check(value):
                (root / "report.json").write_text(json.dumps(value))
                return acceptance.read_report(root, "r", "sample", "gpu")
            check(report)
            for changes in ({"passed": False}, {"rows": []}, {"rows": [row, row]},
                            {"manifest": dict(report["manifest"], physical_identity="other-gpu")},
                            {"cases": []}, {"rows": [dict(row, file="../0.rgba")]},
                            {"rows": [dict(row, width=2)]}):
                with self.assertRaises(ValueError):
                    check(dict(report, **changes))
            (root / "0.rgba").write_bytes(b'\1\0\0\0')
            with self.assertRaises(ValueError):
                check(report)


if __name__ == "__main__":
    unittest.main()
