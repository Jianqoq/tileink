---
title: Retained 示例
---

# Retained 示例

用根节点创建 `RetainedScene`，在事务里插入子 `Canvas` 并提交。`NativeRenderer::render_retained_to_image(&scene)?.readback()?` 得到 CPU 图像。`set_transform` 和 `replace_scene` 可更新单个节点，无需重建整棵树。更多说明见仓库的 [retained 场景指南](https://github.com/Jianqoq/tileink/blob/main/RETAINED_SCENE.md)。
