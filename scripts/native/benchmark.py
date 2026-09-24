"""Benchmark the selected native backend with the Criterion benches."""
import argparse
import subprocess

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('backend', choices=('dx12', 'vulkan', 'metal'))
    args = parser.parse_args()
    raise SystemExit(subprocess.call(['cargo', 'bench', '--no-default-features', '--features', args.backend]))
