---
title: 安装
---

# 安装

在 Cargo.toml 添加 `tileink = { path = "../tileink" }` 和 `peniko = "0.6.1"`。Windows 默认选择 DX12；Windows/Linux 使用 Vulkan、macOS 使用 Metal 时，关闭默认 feature 并只启用一个后端：`cargo test --release --no-default-features --features vulkan -- --test-threads=1` 或 `--features metal`。文档站需要 Node.js 20 以上；在 `website/` 运行 `npm install` 与 `npm run build`。
