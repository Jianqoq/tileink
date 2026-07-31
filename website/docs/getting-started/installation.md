---
sidebar_position: 1
title: 安装与本地运行
---

# 安装

Tileink 当前从源码使用：

```toml title="Cargo.toml"
[dependencies]
tileink = { path = "../tileink" }
peniko = "0.6.1"
wgpu = "30"
pollster = "0.4"
```

Rust crate 使用 edition 2024。WGPU renderer 默认可用；`directwrite-reference` 与 `vello-compare` 是开发/对照用途的可选 feature。

## 本地运行文档站

文档站要求 Node.js 20 或更新版本。

```powershell
cd website
npm install
npm run start
```

默认在 `http://localhost:3000` 启动并热更新。生产构建：

```powershell
npm run typecheck
npm run build
npm run serve
```

静态输出位于 `website/build/`。

## 验证 Rust 环境

```powershell
cargo test --release -- --test-threads=1
cargo run --release --example wgpu_examples
cargo run --release --example winit_svg_tiger
```
