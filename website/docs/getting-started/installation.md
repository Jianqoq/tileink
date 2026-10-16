---
title: 安装
---

# 安装

在 Cargo.toml 添加 `tileink = { path = "../tileink" }` 和 `peniko = "0.6.1"`。Windows 默认选择 DX12；Windows/Linux 使用 Vulkan、macOS 使用 Metal 时，关闭默认 feature 并只启用一个后端：`cargo test --release --no-default-features --features vulkan -- --test-threads=1` 或 `--features metal`。文档站需要 Node.js 20 以上；在 `website/` 运行 `npm install` 与 `npm run build`。

macOS 的 Metal 渲染只需要 Xcode 工具链。运行包含 `--include-ignored` 的完整测试时，还需设置固定 GPU 和绝对路径 `TILEINK_NATIVE_DXC_PATH`，供 HLSL 测试实际编译 DXIL 与 SPIR-V。DXC 在 macOS 使用 `libdxcompiler.dylib`；固定版本的构建方法见仓库 `scripts/mac/README.md`。这些编译检查不替代 Windows/Linux 上的 GPU 执行验证。
