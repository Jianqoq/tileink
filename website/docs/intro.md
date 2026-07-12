---
sidebar_position: 1
title: Tileink 文档
slug: /
---

# Tileink

Tileink 是一个用 Rust 编写、由 WGPU compute pipeline 驱动的 tile-based 2D renderer。它覆盖路径、解析 SDF、文本、图片、渐变、layer、mask、filter、backdrop 和 SVG，并提供两种场景模型：

- `Canvas`：一次性、连续记录的 immediate scene。适合静态内容、小场景和每帧全部重建。
- `RetainedScene`：持久、事务式、增量场景。适合 UI、编辑器和局部变化的大型场景。

## 文档地图

1. [安装与本地运行](getting-started/installation.md)
2. [快速开始](getting-started/quick-start.md)
3. [架构总览](architecture/overview.md)
4. [Retained scene 原理](architecture/retained-scene.md)
5. [公开 API](api/overview.md)
6. [完整示例](examples/immediate.md)

:::tip API 边界
本站只把 `src/lib.rs` re-export 的类型视为稳定的用户 API。源码中 `pub(crate)`、`pub(super)` 或未 re-export 的 `pub` 项属于内部实现。
:::
