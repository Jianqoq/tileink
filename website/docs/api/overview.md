---
sidebar_position: 1
title: Public API 总览
---

# Public API 总览

本节以 `src/lib.rs` 的 re-export 为准。方法签名随源码同步维护；编译器生成的 trait implementations 和字段级 rustdoc 可通过 `cargo doc --no-deps --open` 查看。

## 顶层常量

| 常量 | 值 | 用途 |
|---|---:|---|
| `TILE_SIZE` | `16` | physical tile 边长 |
| `TILE_SCALE` | `1.0 / TILE_SIZE` | pixel 到 tile 的比例 |
| `BLOCK_SIZE` | `256` | 16×16 block 元素数 |

## 主要入口

| 类型 | 用途 |
|---|---|
| [`Canvas`](canvas.md) | immediate scene recording |
| [`WgpuRenderer`](renderer.md) | WGPU prepare、render、output、resources |
| [`RetainedScene`](retained-scene.md) | 持久事务式 scene graph |
| [`TextContext`](text-and-svg.md) | text layout/outline/glyph cache |
| [`Image`, `Brush`, `Sdf`, `Filter`](paint-and-geometry.md) | paint、geometry、effects |
| [`RenderOptions`, profiles`](debug-and-profile.md) | debug capture 与性能统计 |

## 外部类型 re-export

Tileink re-export cosmic-text 的常用 font 属性：`TextAlign`、`TextAttrs`、`TextCacheKeyFlags`、`TextFamily`、`TextFontSystem`、`TextStretch`、`TextStyle`、`TextWeight`。Path/Rect/Affine/Color 等仍来自 `peniko`/`kurbo`。

## 错误类型

- `RetainedSceneError`：事务验证/提交失败；
- `WgpuTextureRenderError`：目标 texture size/format/usage 不兼容；
- `SvgError`：不支持的 SVG feature；
- `ImageSaveError`：图片编码或文件写入失败。
