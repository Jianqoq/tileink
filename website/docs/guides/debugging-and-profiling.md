---
sidebar_position: 5
title: 调试与性能分析
---

# 调试与性能分析

## Profile 一帧

```rust
renderer.start_profile();
renderer.render_retained(&scene);
let cpu_profile = renderer.end_profile().clone();

while renderer.has_pending_profile_readbacks() {
    renderer.device().poll(wgpu::PollType::wait_indefinitely())?;
    renderer.poll_profile();
}

println!("{}", renderer.profile());
```

CPU stage 在 `end_profile` 后可读；GPU timestamp 可能异步完成，需要 `poll_profile`。`WgpuRenderProfile::summary` 会按同名 stage 合并。

常用 stage：

- `retained.materialize`、`.chunks`、`.plan_sync`、`.frame`；
- `retained.damage`；
- `prepare`、`prepare.lengths`、`prepare.upload_scene`；
- `plan.execute`；
- GPU `scan`、`coarse`、`fine`、`filter.*`。

## Incremental stats

`renderer.incremental_render_stats()` 暴露 full-redraw reason、dirty tile ratio、chunks rebuilt、CPU copied bytes、GPU uploaded bytes、tile pages rewritten、plan fragments rebuilt、surface reuse 和 queue submissions。

## Render debug capture

`render_with_options` 配合 `RenderOptions` / `RenderDebugOptions` 可以输出 tile dump、path/line details、overlay image 和 JSON：

```rust
let options = tileink::RenderOptions {
    debug: Some(tileink::RenderDebugOptions::new("debug-out").with_tile((4, 2))),
    ..Default::default()
};
let capture = renderer.render_with_options(&canvas, &options);
```

使用 `debug_capture_json` 可把 `RenderDebugCapture` 序列化为便于 diff 的 JSON 文本。
