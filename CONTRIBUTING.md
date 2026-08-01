# Contributing to Tileink

Thank you for helping improve Tileink. The project is early-stage, so changes should favor clear,
correct semantics and long-term maintainability over compatibility shims or fixture-specific fixes.

## Development workflow

1. Fork the repository and create a focused branch from `main`.
2. Add semantic tests and edge cases for new behavior. Add a regression test before fixing a bug.
3. Document why a behavior or invariant changed and whether a fix addresses the root cause.
4. Keep commits focused and open a pull request with the motivation, user impact, and validation.

Run the required checks locally before opening a pull request:

```powershell
cargo test --release -- --test-threads=1
cargo fmt --all --check
cargo clippy --release --all-targets -- -D warnings
```

Rendering changes must also run the release examples and complete SVG matrix documented in
[`AGENTS.md`](AGENTS.md). Performance changes require a Criterion benchmark for the affected
scenario and evidence that the result improved or remained within noise.

## Pull requests

- Keep unrelated work in separate pull requests.
- Resolve all review conversations and keep required checks green.
- Do not commit generated build output, credentials, private keys, or local configuration.
- Contributions are licensed under the repository's `MIT OR Apache-2.0` terms unless explicitly
  stated otherwise.

Security vulnerabilities must follow [`SECURITY.md`](SECURITY.md) instead of a public issue.
