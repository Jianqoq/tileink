---
sidebar_position: 1
title: Installation and local docs
---

# Installation

```toml title="Cargo.toml"
[dependencies]
tileink = { path = "../tileink" }
peniko = "0.6.1"
wgpu = "30"
pollster = "0.4"
```

Tileink uses Rust edition 2024. The WGPU renderer is available by default; `directwrite-reference` and `vello-compare` are development comparison features.

## Run this documentation locally

Node.js 20 or newer is required.

```powershell
cd website
npm install
npm run start
```

The development server opens at `http://localhost:3000` with hot reload and a language selector. Validate and preview the production output with:

```powershell
npm run typecheck
npm run build
npm run serve
```

Static files are generated in `website/build/`.
