"""Guard units and nearest-rank quantiles using persisted benchmark evidence."""
import json
from pathlib import Path
import tempfile
import unittest

import benchmark
import retained_benchmark


class StatisticsTests(unittest.TestCase):
    def test_completed_frame_units_and_bounded_latency_samples(self):
        for samples, expected in [(4, None), (64, 61), (255, 243)]:
            with self.subTest(samples=samples), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                output = root / "output"
                output.mkdir()
                estimate = root / "target/criterion/retained_backend_cycles/case/baseline"
                estimate.mkdir(parents=True)
                (estimate / "estimates.json").write_text(json.dumps({"mean": {
                    "point_estimate": 20000, "confidence_interval": {
                        "lower_bound": 18000, "upper_bound": 22000}}}))
                (output / "case.json").write_text(json.dumps({
                    "name": "case", "frames_per_iteration": 2,
                    "latency_ns": list(range(samples * 1000, 0, -1000)), "pixels": []}))
                actual = retained_benchmark.collect(root, output, "baseline")["case"]
                self.assertEqual(actual["mean_us"], 10)
                self.assertEqual(actual["mean_ci_us"], [9, 11])
                self.assertEqual(actual["p95_us"], expected)
                self.assertEqual(actual["max_us"], samples)
                self.assertEqual(actual["latency_samples"], samples)

    def test_original_matrix_uses_nearest_rank_for_128_samples(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / "output"
            output.mkdir()
            estimate = root / "target/criterion/backend_comparison_cycles/case/baseline"
            estimate.mkdir(parents=True)
            (estimate / "estimates.json").write_text(json.dumps({"mean": {
                "point_estimate": 160000, "confidence_interval": {
                    "lower_bound": 144000, "upper_bound": 176000}}}))
            (output / "case-latency.json").write_text(json.dumps(list(range(1000, 129000, 1000))))
            actual = benchmark.collect(root, output, "baseline", ["case"])["case"]
            self.assertEqual(actual["mean_us"], 10)
            self.assertEqual(actual["p95_us"], 122)


if __name__ == "__main__":
    unittest.main()
