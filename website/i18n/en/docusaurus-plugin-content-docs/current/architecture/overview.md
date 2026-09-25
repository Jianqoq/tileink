---
title: Architecture
---

# Architecture

A `Canvas` or a `RetainedScene` produces shared scene records. Preparation builds the execution plan and uploads changed data. The selected native backend then runs path scan, coarse tile binning, fine rasterization, and filters before writing a texture or image. Tiles are 16×16 physical pixels. The backend features `dx12`, `vulkan`, and `metal` are mutually exclusive; DX12 is the default. `src/native` owns device resources and submissions, `src/shared` contains scene and GPU layouts, and `src/svg.rs` lowers usvg trees to canvas operations.
