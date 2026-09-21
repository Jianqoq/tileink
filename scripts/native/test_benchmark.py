"""Coverage and byte-exact evidence checks for the cross-backend benchmark."""

import tempfile
import unittest
from pathlib import Path

import benchmark


class PixelEvidenceTests(unittest.TestCase):
    def test_every_case_and_phase_must_match(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            reference, actual = root / "reference", root / "actual"
            reference.mkdir()
            actual.mkdir()
            for case in benchmark.CASES:
                for phase in range(16):
                    name = f"{case}-{phase}-1x1.rgba"
                    for directory in (reference, actual):
                        (directory / name).write_bytes(bytes([phase, 0, 0, 255]))
            benchmark.compare_pixels(reference, actual)
            image = actual / "text-0-1x1.rgba"
            image.write_bytes(bytes([1, 0, 0, 255]))
            with self.assertRaises(RuntimeError):
                benchmark.compare_pixels(reference, actual)
            image.unlink()
            with self.assertRaises(RuntimeError):
                benchmark.compare_pixels(reference, actual)


if __name__ == "__main__":
    unittest.main()
