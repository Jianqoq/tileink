"""Compare the shared retained scale, dirty-area and stress workloads on four APIs."""

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import statistics
import subprocess

import acceptance
import benchmark


def collect(root, destination, baseline, group="retained_backend_cycles"):
    cases = {}
    for path in sorted(destination.glob("*.json")):
        record = json.loads(path.read_text())
        name = record["name"]
        estimates = root / "target/criterion" / group / name / baseline / "estimates.json"
        estimate = json.loads(estimates.read_text())["mean"]
        values = sorted(record["latency_ns"])
        divisor = record["frames_per_iteration"] * 1000
        cases[name] = dict(
            mean_us=estimate["point_estimate"] / divisor,
            mean_ci_us=[estimate["confidence_interval"][key] / divisor
                        for key in ("lower_bound", "upper_bound")],
            p95_us=values[math.ceil(len(values) * .95) - 1] / 1000 if len(values) >= 20 else None,
            max_us=values[-1] / 1000,
            latency_samples=len(values), latency_kind=record.get("latency_kind", "completed-frame"),
            pixels=record["pixels"])
    return cases


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--gpu", required=True)
    parser.add_argument("--dxcompiler", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--suite", choices=("retained", "immediate", "pipelined"), default="retained")
    parser.add_argument("--runs", type=int, default=1)
    parser.add_argument("--case", help="Comma-separated case-name prefixes; receipt records partial coverage")
    args = parser.parse_args()
    if args.runs < 1:
        parser.error("--runs must be positive")
    root = Path(__file__).resolve().parents[2]
    output = args.output.resolve()
    acceptance.validate_output(root, output)
    output.mkdir(parents=True, exist_ok=False)
    snapshot = acceptance.source_snapshot(root)
    binaries = {feature: benchmark.build(root, output, feature, args.suite + "_backend_comparison")
                for feature in ("wgpu", "dx12", "vulkan")}
    env = dict(os.environ, TILEINK_BENCH_GPU=args.gpu,
               TILEINK_PARITY_DXCOMPILER=str(args.dxcompiler.resolve()))
    for key in ("TILEINK_COMPARE_CASE", "TILEINK_COMPARE_REFERENCE"):
        env.pop(key, None)
    if args.case:
        env["TILEINK_COMPARE_CASE"] = args.case
    receipt = dict(passed=False, suite=args.suite, gpu=args.gpu, filter=args.case, sources=snapshot, runs=[],
                   binaries={key: hashlib.sha256(path.read_bytes()).hexdigest()
                             for key, path in binaries.items()})
    reference = None
    expected = None
    try:
        for repetition in range(args.runs):
            for route, feature, api in benchmark.ROUTES if repetition % 2 == 0 else reversed(benchmark.ROUTES):
                acceptance.verify_sources(root, snapshot)
                name = f"{repetition + 1}-{route}"
                destination = output / name
                baseline = f"{output.name}-{name}"
                env.update(TILEINK_BENCH_API=api, TILEINK_COMPARE_OUTPUT=str(destination))
                if reference:
                    env["TILEINK_COMPARE_REFERENCE"] = str(reference)
                print(f"Running {name}", flush=True)
                with (output / f"{name}.log").open("w", encoding="utf8") as log:
                    subprocess.run([str(binaries[feature]), "--bench", "--save-baseline", baseline],
                                   cwd=root, env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
                acceptance.verify_sources(root, snapshot)
                cases = collect(root, destination, baseline, args.suite + "_backend_cycles")
                if not cases or (not args.case and len(cases) != {"retained": 181, "immediate": 62, "pipelined": 44}[args.suite]):
                    raise RuntimeError(f"Incomplete workload inventory: {len(cases)}")
                pixels = {key: value["pixels"] for key, value in cases.items()}
                expected = expected or pixels
                if expected != pixels:
                    raise RuntimeError("Case inventory or exact pixels differ")
                reference = reference or destination
                receipt["runs"].append(dict(run=repetition + 1, route=route, cases=cases))
                (output / "receipt.json").write_text(json.dumps(receipt, indent=2), encoding="utf8")
        receipt["native_to_wgpu_ratio"] = {
            api: {case: statistics.median(row["cases"][case]["mean_us"] for row in receipt["runs"]
                                         if row["route"] == f"native-{api}") /
                        statistics.median(row["cases"][case]["mean_us"] for row in receipt["runs"]
                                          if row["route"] == f"wgpu-{api}") for case in expected}
            for api in ("dx12", "vulkan")}
        receipt["passed"] = True
        receipt["mean_parity"] = all(value <= 1 for ratios in receipt["native_to_wgpu_ratio"].values()
                                     for value in ratios.values())
    finally:
        (output / "receipt.json").write_text(json.dumps(receipt, indent=2), encoding="utf8")


if __name__ == "__main__":
    main()
