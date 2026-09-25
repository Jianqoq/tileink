---
title: Retained example
---

# Retained example

Create a `RetainedScene` with a root node, add child `Canvas` values in a transaction, and commit. `NativeRenderer::render_retained_to_image(&scene)?.readback()?` produces a CPU image. `set_transform` and `replace_scene` update individual nodes without rebuilding the entire tree. See the repository [retained scene guide](https://github.com/Jianqoq/tileink/blob/main/RETAINED_SCENE.md).
