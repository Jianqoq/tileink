"""Serial Windows GPU acceptance across mutually exclusive Cargo backends.

Compare raw premultiplied RGBA, including transparent RGB. A successful process
alone is insufficient: every expected output and its provenance must be present.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import subprocess

ROUTES = {
    "wgpu": ["wgpu-dx12-native", "wgpu-dx12-portable", "wgpu-dx12-precompiled",
             "wgpu-vulkan-native", "wgpu-vulkan-portable"],
    "dx12": ["native-dx12"],
    "vulkan": ["native-vulkan"],
}
DEFAULT_SUITES = ["svg", "examples", "retained"]
SUITES = {"svg": (1712, 1), "examples": (45, 1), "retained": (29, 6), "rounding": (2, 1)}


def source_snapshot(root):
    listed = subprocess.check_output(["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"], cwd=root)
    return {name: hashlib.sha256((root / name).read_bytes()).hexdigest() if (root / name).is_file() else None
            for name in sorted(set(listed.decode("utf-8").split("\0")) - {""})}


def verify_sources(root, expected):
    if source_snapshot(root) != expected:
        raise ValueError("sources changed during build or acceptance")


def validate_output(root, output):
    if output.is_relative_to(root):
        ignored = subprocess.run(["git", "check-ignore", "-q", str(output)], cwd=root).returncode == 0
        if not ignored:
            raise ValueError("output inside checkout must be git-ignored")


def write_json(path, value):
    with path.open("x", encoding="utf-8") as file:
        json.dump(value, file, indent=2)


def read_report(directory, route, suite, gpu):
    report = json.loads((directory / "report.json").read_text(encoding="utf-8"))
    manifest = report["manifest"]
    count, variants = SUITES[suite]
    cases = report["cases"]
    if (report["schema"] != 1 or report["passed"] is not True
            or len(cases) != count or len(set(cases)) != count
            or report["variants"] != variants
            or manifest["route"] != route or manifest["suite"] != suite
            or manifest["physical_identity"] != gpu):
        raise ValueError(f"invalid acceptance report: {directory}")
    expected = {(case, variant) for case in cases for variant in range(variants)}
    rows = {}
    files = set()
    for row in report["rows"]:
        key = row["case"], row["variant"]
        file = row["file"]
        if key not in expected or key in rows or file in files or Path(file).name != file:
            raise ValueError("duplicate or unexpected output")
        data = (directory / file).read_bytes()
        if (row["width"] <= 0 or row["height"] <= 0
                or len(data) != row["width"] * row["height"] * 4
                or hashlib.sha256(data).hexdigest() != row["sha256"]):
            raise ValueError("invalid raw image extent or digest")
        rows[key] = row
        files.add(file)
    if rows.keys() != expected:
        raise ValueError("missing acceptance outputs")
    return report, rows


def compare_images(expected, actual, expected_row, actual_row):
    if (expected_row["width"], expected_row["height"]) != (actual_row["width"], actual_row["height"]):
        raise ValueError("image dimensions differ")
    if expected != actual:
        raise ValueError("raw RGBA pixels differ")


def compare(reference, candidate, reference_rows, candidate_rows):
    # Retained Auto/ForceFull and all target contracts must also agree.
    comparisons = []
    for (case, variant), row in candidate_rows.items():
        expected_row = reference_rows[(case, 0)]
        try:
            compare_images((reference / expected_row["file"]).read_bytes(),
                           (candidate / row["file"]).read_bytes(), expected_row, row)
        except ValueError as error:
            expected = (reference / expected_row["file"]).read_bytes()
            actual = (candidate / row["file"]).read_bytes()
            equal_extent = (expected_row["width"], expected_row["height"]) == (row["width"], row["height"])
            write_json(candidate / "pixel-failure.json", {
                "case": case, "variant": variant, "error": str(error),
                "expected": str(reference / expected_row["file"]),
                "actual": str(candidate / row["file"]),
                "expected_row": expected_row, "actual_row": row,
                "different_pixels": sum(expected[i:i + 4] != actual[i:i + 4] for i in range(0, len(expected), 4)) if equal_extent else None,
                "max_channel_delta": max((abs(a - b) for a, b in zip(expected, actual)), default=0) if equal_extent else None,
            })
            raise
        comparisons.append({"case": case, "variant": variant, "different_pixels": 0,
                            "max_channel_delta": 0, "expected": str(reference / expected_row["file"]),
                            "actual": row["file"], "sha256": row["sha256"]})
    write_json(candidate / "comparisons.json", comparisons)


def execute(binary, test, env, log):
    with log.open("x", encoding="utf-8") as output:
        subprocess.run([str(binary), test, "--ignored", "--exact", "--test-threads=1", "--nocapture"],
                       env=env, stdout=output, stderr=subprocess.STDOUT, check=True)
    diagnostics = log.read_text(encoding="utf-8")
    # Requesting validation is insufficient: wgpu can continue without it.
    if any(marker in diagnostics for marker in (
            "Failed to get debug interface",
            "InstanceFlags::VALIDATION requested, but unable to find layer",
            "Unable to find extension: VK_EXT_debug_utils")):
        raise ValueError(f"API validation unavailable in {log}")
    # env_logger prefixes severity with an optional timestamp; API errors must
    # fail acceptance even when the process itself exits successfully.
    if (re.search(r"\[(?:\S+\s+)?ERROR(?:\s|\])", diagnostics)
            or any(marker in diagnostics for marker in ("Validation Error", "VUID-", "D3D12 ERROR"))):
        raise ValueError(f"API error diagnostic in {log}")


def build(root, output, backend):
    command = ["cargo", "test", "--release", "--test", "windows_corpus", "--no-default-features",
               "--features", backend, "--no-run", "--message-format=json"]
    with (output / f"build-{backend}.log").open("x", encoding="utf-8") as errors:
        result = subprocess.run(command, cwd=root, stdout=subprocess.PIPE, stderr=errors, text=True, check=True)
    (output / f"build-{backend}.jsonl").write_text(result.stdout, encoding="utf-8")
    binaries = [Path(item["executable"]) for line in result.stdout.splitlines()
                if (item := json.loads(line)).get("reason") == "compiler-artifact"
                and item.get("target", {}).get("name") == "windows_corpus" and item.get("executable")]
    if len(binaries) != 1:
        raise ValueError("expected exactly one acceptance test binary")
    return binaries[0]


def run(args):
    output = args.output.resolve()
    root = Path(__file__).resolve().parents[2]
    validate_output(root, output)
    output.mkdir(parents=True, exist_ok=False)
    sources = source_snapshot(root)
    write_json(output / "sources.json", sources)
    binaries = {}
    for backend in ROUTES:
        binaries[backend] = build(root, output, backend)
        verify_sources(root, sources)
    env = dict(os.environ, RUST_LOG="warn,wgpu_hal=debug", RUST_LOG_STYLE="never")
    env["TILEINK_ACCEPTANCE_OUTPUT"] = str(output / "adapters.json")
    execute(binaries["wgpu"], "list_adapters", env, output / "adapters.log")
    adapters = json.loads((output / "adapters.json").read_text())
    selected = [adapter for adapter in adapters if not args.gpu or adapter["physical_identity"] in args.gpu]
    if not selected or (args.gpu and set(args.gpu) != {a["physical_identity"] for a in selected}):
        raise ValueError("requested physical GPU unavailable")
    receipt = {"schema": 1, "passed": False, "platform": platform.platform(), "adapters": selected,
               "runs": args.runs, "suites": args.suite, "routes": ROUTES, "completed": [],
               "binaries": {key: {"path": str(path), "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
                            for key, path in binaries.items()},
               "scope": "Windows devices listed here only; not an all-platform M6 certification"}
    write_json(output / "started.json", receipt)
    references = {}
    try:
        for adapter in selected:
            gpu = adapter["physical_identity"]
            for repeat in range(args.runs):
                for suite in args.suite:
                    for backend, routes in ROUTES.items():
                        for route in routes:
                            name = f"{gpu}-{repeat + 1}-{suite}-{route}"
                            print(name, flush=True)
                            directory = output / name
                            verify_sources(root, sources)
                            env.update(TILEINK_ACCEPTANCE_OUTPUT=str(directory), TILEINK_ACCEPTANCE_GPU=gpu,
                                       TILEINK_ACCEPTANCE_ROUTE=route, TILEINK_ACCEPTANCE_SUITE=suite)
                            execute(binaries[backend], "run_selected_corpus", env, output / f"{name}.log")
                            report, rows = read_report(directory, route, suite, gpu)
                            manifest = report["manifest"]
                            if manifest["executable_sha256"] != receipt["binaries"][backend]["sha256"]:
                                raise ValueError("executed binary differs from build provenance")
                            if {row["path"].replace("\\", "/"): row["sha256"] for row in manifest["sources"]} != sources:
                                raise ValueError("runtime source provenance does not match build")
                            identity = (gpu, suite)
                            if identity not in references:
                                references[identity] = directory, report, rows
                            reference, reference_report, reference_rows = references[identity]
                            for field in ("sources", "resources", "fonts"):
                                if manifest[field] != reference_report["manifest"][field]:
                                    raise ValueError(f"{field} changed across acceptance processes")
                            if report["cases"] != reference_report["cases"]:
                                raise ValueError("case manifest changed")
                            compare(reference, directory, reference_rows, rows)
                            receipt["completed"].append(name)
        verify_sources(root, sources)
        receipt["passed"] = True
    except Exception as error:
        receipt["error"] = str(error)
        raise
    finally:
        write_json(output / "receipt.json", receipt)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True, help="new evidence directory")
    parser.add_argument("--gpu", action="append", help="exact Windows LUID; default all hardware GPUs")
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--suite", action="append", choices=SUITES)
    args = parser.parse_args()
    if args.runs < 3:
        parser.error("acceptance requires at least three independent runs")
    args.suite = args.suite or DEFAULT_SUITES
    if len(set(args.suite)) != len(args.suite):
        parser.error("duplicate suite")
    run(args)
