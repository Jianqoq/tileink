"""Compare native and wgpu completed-frame cost on one explicitly selected GPU."""

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import shutil
import statistics
import subprocess

import acceptance

CASES = ("unchanged", "sparse", "full", "resize", "blur", "text", "images", "image_replace", "clips", "paths", "large")
ROUTES = (("wgpu-dx12", "wgpu", "dx12"), ("native-dx12", "dx12", "dx12"),
          ("wgpu-vulkan", "wgpu", "vulkan"), ("native-vulkan", "vulkan", "vulkan"))


def build(root, output, feature, target="backend_comparison"):
    result = subprocess.run(
        ["cargo", "bench", "--no-default-features", "--features", feature,
         "--bench", target, "--no-run", "--message-format=json"],
        cwd=root, env=dict(os.environ, CARGO_TARGET_DIR=str(root / "target")),
        capture_output=True, text=True, encoding="utf-8")
    (output / f"{feature}-build.log").write_text(result.stdout + result.stderr, encoding="utf-8")
    if result.returncode:
        raise RuntimeError(f"{feature} build failed; see {output / (feature + '-build.log')}")
    records = [json.loads(line) for line in result.stdout.splitlines() if line.startswith("{")]
    executable = next(row["executable"] for row in records
                      if row.get("reason") == "compiler-artifact"
                      and row["target"]["name"] == target and row.get("executable"))
    destination = output / (feature + Path(executable).suffix)
    shutil.copy2(executable, destination)
    return destination


def collect(root, destination, baseline, cases=CASES):
    stats = {}
    for name in cases:
        source = root / "target/criterion/backend_comparison_cycles" / name / baseline
        shutil.copytree(source, destination / ("criterion-" + name))
        estimate = json.loads((source / "estimates.json").read_text())["mean"]
        values = sorted(json.loads((destination / (name + "-latency.json")).read_text()))
        stats[name] = {
            # One Criterion iteration is a 16-frame geometry cycle; replacement contents stay fresh.
            "mean_us": estimate["point_estimate"] / 16000,
            "mean_ci_us": [estimate["confidence_interval"][key] / 16000
                           for key in ("lower_bound", "upper_bound")],
            "latency_mean_us": statistics.mean(values) / 1000,
            "p95_us": values[math.ceil(len(values) * .95) - 1] / 1000,
            "max_us": max(values) / 1000,
        }
    return stats


def compare_pixels(reference, destination, cases=CASES):
    expected = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in reference.glob("*.rgba")}
    actual = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in destination.glob("*.rgba")}
    if len(expected) != len(cases) * 16 or expected != actual:
        raise RuntimeError(f"Pixel mismatch: {reference} versus {destination}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--gpu", required=True)
    parser.add_argument("--dxcompiler", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--clip-matrix", action="store_true", help="Sweep clip count, depth and bounding-box area")
    args = parser.parse_args()
    if args.runs < 1:
        parser.error("--runs must be positive")
    root = Path(__file__).resolve().parents[2]
    output = args.output.resolve()
    acceptance.validate_output(root, output)
    output.mkdir(parents=True, exist_ok=False)
    sources = acceptance.source_snapshot(root)
    binaries = {feature: build(root, output, feature) for feature in ("wgpu", "dx12", "vulkan")}
    env = dict(os.environ, CARGO_TARGET_DIR=str(root / "target"), TILEINK_BENCH_GPU=args.gpu,
               TILEINK_PARITY_DXCOMPILER=str(args.dxcompiler.resolve()))
    for key in ("TILEINK_COMPARE_PROFILE", "TILEINK_COMPARE_CASE", "TILEINK_COMPARE_CLIP_MATRIX"):
        env.pop(key, None)
    if args.clip_matrix:
        env["TILEINK_COMPARE_CLIP_MATRIX"] = "1"
    cases = None if args.clip_matrix else CASES
    receipt = {"clip_matrix": args.clip_matrix, "passed": False, "gpu": args.gpu, "runs": [], "sources": sources,
               "binaries": {key: hashlib.sha256(path.read_bytes()).hexdigest()
                            for key, path in binaries.items()}}
    reference = None
    try:
        for index in range(args.runs):
            # Reverse order between repetitions to expose order/thermal drift.
            for route, feature, api in ROUTES if index % 2 == 0 else reversed(ROUTES):
                acceptance.verify_sources(root, sources)
                destination = output / f"{index + 1}-{route}"
                baseline = f"{output.name}-{index + 1}-{route}"
                env.update(TILEINK_BENCH_API=api, TILEINK_COMPARE_OUTPUT=str(destination))
                print(f"Running {index + 1}/{args.runs}: {route}", flush=True)
                with (output / f"{index + 1}-{route}.log").open("w", encoding="utf-8") as log:
                    subprocess.run([str(binaries[feature]), "--bench", "--save-baseline", baseline],
                                   cwd=root, env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
                acceptance.verify_sources(root, sources)
                actual_cases = tuple(sorted(p.name.removesuffix("-latency.json") for p in destination.glob("*-latency.json")))
                if cases is None:
                    cases = actual_cases
                    if len(cases) != 16:
                        raise RuntimeError("Clip matrix must execute all 16 controlled workloads")
                if set(actual_cases) != set(cases):
                    raise RuntimeError("Workload set changed between routes")
                stats = collect(root, destination, baseline, cases)
                reference = reference or destination
                compare_pixels(reference, destination, cases)
                receipt["runs"].append({"run": index + 1, "route": route, "cases": stats})
                print(json.dumps(stats), flush=True)
                (output / "receipt.json").write_text(json.dumps(receipt, indent=2), encoding="utf-8")
        ratios = {}
        for api in ("dx12", "vulkan"):
            ratios[api] = {}
            for case in cases:
                means = {kind: statistics.median(row["cases"][case]["mean_us"]
                         for row in receipt["runs"] if row["route"] == f"{kind}-{api}")
                         for kind in ("native", "wgpu")}
                ratios[api][case] = means["native"] / means["wgpu"]
        receipt["native_to_wgpu_ratio"] = ratios
        receipt["passed"] = True  # Execution and exact pixels; ratios remain explicit, never waived.
        receipt["mean_parity"] = all(value <= 1 for api in ratios.values() for value in api.values())
    finally:
        (output / "receipt.json").write_text(json.dumps(receipt, indent=2), encoding="utf-8")
    print(json.dumps(ratios, indent=2))


if __name__ == "__main__":
    main()
