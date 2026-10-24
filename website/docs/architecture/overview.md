---
title: 架构总览
---

# 架构总览

`Canvas` 或 `RetainedScene` 生成共享场景记录。准备阶段创建执行计划并上传变化的数据；所选原生后端依次执行 path scan、coarse tile binning、fine raster 和 filter，最后写入纹理或图像。Tile 固定为 16×16 physical pixels。`dx12`、`vulkan`、`metal` feature 互斥，默认 DX12。`src/native` 管理设备资源与提交，`src/shared` 定义场景和 GPU 数据，`src/svg.rs` 把 usvg 树转换成 Canvas 操作。
