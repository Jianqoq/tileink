---
title: API 总览
---

# API 总览

`Canvas` 适合 immediate 绘图；`RetainedScene` 适合事务更新。`NativeContext` 选择设备，`NativeRenderer` 提交到自有图像、纹理或宿主 render target。`TextFontSystem` 和 `TextContext` 准备文本；`Canvas::push_svg_with_options` 转换 SVG。主要错误类型有 `NativeError`、`RetainedSceneError`、`SvgError`。
