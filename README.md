# Tileink

Tile-based 2D vector renderer for Rust and WGPU, with immediate `Canvas` and transactional incremental `RetainedScene` APIs.

## Documentation

The TypeScript/React documentation site lives in [`website`](website). The default command builds and serves both Chinese and English, so the language selector works locally:

```powershell
cd website
npm install
npm run start
```

For faster single-language development with hot reload, use `npm run dev` for Chinese or `npm run dev:en` for English. Docusaurus development mode serves only one locale at a time, so its language selector cannot switch to the other locale; use `npm run start` when testing localization.

Production validation:

```powershell
npm run typecheck
npm run build
```

Rust API rustdoc can be generated with `cargo doc --no-deps --open`.

## Development

```powershell
cargo test --release -- --test-threads=1
```

See [`AGENTS.md`](AGENTS.md) for the repository's complete test and review policy.
