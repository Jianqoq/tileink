---
title: Tileink
---

# Tileink

Tileink 是 Rust 的 tile-based GPU-compute 2D renderer。`Canvas` 记录 immediate 场景，`RetainedScene` 以事务追踪大型界面的局部变化。路径、文本、图片、渐变、layer、mask、filter、backdrop 和 SVG 共用原生 DX12、Vulkan、Metal 后端的场景语义。先阅读[安装](getting-started/installation.md)和[快速开始](getting-started/quick-start.md)。


普通子树透明度使用 `Canvas::push_opacity_group(opacity)` 或
`RetainedLayerDescriptor::OpacityGroup { opacity }`：先合成完整子内容，再统一应用透明度。
布局尺寸不构成隐式裁剪，溢出的控件、阴影、文字和图标仍然保留。需要裁剪时显式使用 Clip；
已有带路径的 `push_opacity_layer` / `Opacity` 继续保留路径遮罩语义。
