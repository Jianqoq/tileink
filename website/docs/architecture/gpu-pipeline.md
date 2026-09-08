---
sidebar_position: 4
title: GPU Pipeline
---

# GPU Pipeline

```mermaid
flowchart LR
  Records[Scene records] --> ScanCount[Scan count]
  ScanCount --> Prefix[Prefix sums]
  Prefix --> ScanEmit[Emit path segments]
  ScanEmit --> CoarseCount[Coarse count per tile]
  CoarseCount --> CoarseEmit[Emit particles/pages]
  CoarseEmit --> Fine[Fine raster 16x16]
  Fine --> Composite[Layer stacks]
  Composite --> Offscreen[Filters / masks / backdrop]
  Offscreen --> Target[RGBA8 target]
```

## Scan

Path geometry 被 flatten 为 line records。scan shaders 计算每条 line 穿过哪些 tile/row，并通过 prefix/cumsum 分配 backdrop 与 segment 输出。解析 SDF 不需要 CPU tessellation；transform 作为 affine 数据进入 GPU。

## 原子操作的边界

原子操作只用于 scan count 的并发 winding/segment 计数和 scan emit 的并发 cursor 分配。
clear、scan prefix、cursor 初始化、cumsum、coarse 和 filter 使用普通整数访问；只读的
consumer 同时使用只读 storage binding。各阶段由有序 dispatch 建立依赖，完整与增量
scan plan 的记录/chunk 不重叠，因此清零和前缀回写每个位置只有一个 writer。这从根源上
移除了沿用共享 buffer 原子类型造成的多余操作，不是临时 workaround，也不保证 FPS 提升。

## Coarse

Coarse 阶段只遍历该 tile 的 draw references，而不是扫描整个 draw table。它产生 fine particles、glyph work 和 layer-stack events。Retained scenes 用稳定 `BatchId`，物理 draw slot 可以在 arena 中不连续。

Particle 和 glyph 的 tile 计数使用同一条整数前缀链：`coarse_prefix_chunks` 同时计算
两种局部范围，`coarse_chunk_offsets` 并行扫描 chunk totals，`coarse_apply_chunk_offsets`
把全局偏移加回两种范围。这样从根源上消除了两条独立分配链的重复 dispatch：每批 6 次减为
3 次。chunk offsets 按 256 个一组扫描并传递 carry，尾部线程参与 barrier 但不写越界记录。

Dense coarse 按 tile 顺序分配；compact 增量 coarse 按 active tile 列表顺序分配，inactive tile 的记录保持
不变。正常、chunked emit 和 profiling 路径遵循相同的 count → prefix → emit 依赖；绘制
顺序与粒子/glyph 数据格式不变。dense/compact 成本估算同步计入共享链的实际 workgroup 数。

## Fine

Fine shader 每 workgroup 处理 tile pixels，组合 path coverage、SDF、text coverage、brush sampling、clip/opacity/blend stack。native backend 可直接写 storage texture；portable backend 使用兼容的中间表示与 texture copy。

Fine 始终使用 direct 派发，每 tile 一个 workgroup。每批只有一次 fine dispatch，删除了
原先的参数清零、分类压缩和三条间接渲染链，从根源上去掉多批次场景的重复调度开销。
不存在按场景切换的阈值或 `TILEINK_FINE_DIRECT` 开关；复杂矢量场景也使用相同路径。

Coarse 仍生成每 tile 的类型，供单个 fine shader 内的 color／SDF／完整解释器分支使用。
专用的三份分类 tile 列表、indirect 参数 buffer 和 compact shader 均已移除；active tile
列表紧接类型数组存储。每 tile 少占用三个 u32，即 12 字节临时存储。

超过 device 单维工作组上限时使用二维 direct dispatch。FineConfig 携带实际 X 宽度，
shader 先恢复线性的 dispatch index，再查 active tile 列表；末行越界工作组立即返回。
完整重绘、局部更新、resize 和 offscreen target 都遵循同一规则，必须保持相同的 RGBA
像素、绘制顺序和 inactive pixel history。

`cargo bench --bench root_batches` 覆盖 dense/sparse 批次、不同尺寸及单层/多层 Tiger。
性能对照使用修改前后的独立 release binary；冷启动编译和预热不计入稳定帧耗时。
减少 dispatch 有利于多批次场景，但复杂大图可能较慢，不能把微基准收益直接当作 FPS。

线性 filter 同样按 device 上限分成二维派发，避免 4096×4096 清屏超过 65,535 个工作组。
FilterConfig 的原 padding 字段保存 X 派发宽度；普通 kernel 恢复线性 invocation index，
compact shared blur 则恢复 workgroup index，并在读取 active tile 前排除末行填充组。
这是对超限派发的根因修复，dense shared blur 保留原有二维 tile 网格。

## Filters 与 offscreen surfaces

不能 fuse 的 filter/mask/backdrop 变成 ExecPlan offscreen ops。持久 scene 用 `RetainedSurfaceId` 复用兼容 surface；revision、bounds、resource 或 dependency damage 决定是否重新渲染。

## Native 与 portable

两条 WGPU 路径共享场景数据和绝大多数 shader 语义。仓库的 examples/SVG scripts 会执行 native 与 portable pixel compare，确保 backend 一致。

portable fine 对仅含 fused root layer 的执行计划使用整帧 ping-pong：full redraw 只复制一次进入中间纹理、一次复制回输出；partial redraw 额外初始化第二张中间纹理以保留 inactive pixels。递归 offscreen/filter 计划继续使用逐目标的保守路径。`IncrementalRenderStats::portable_texture_copies` 用于观测实际复制次数。

## DX12 构建期 DXIL

Windows 构建会把 portable fine 的唯一 compute entry point 预编译为 Shader Model 6.0
DXIL，并把 blob 嵌入库中。Cargo 以 WGSL、shader patch、binding manifest、构建脚本、DXC
发现环境、Windows SDK bin 目录、DXC 可执行文件及同目录的 `dxcompiler.dll`/`dxil.dll`
作为失效输入，因此安装或更新工具链也会重新生成产物。DX12 运行时只有在真实 D3D12
device 支持 Shader Model 6.0，且 `PASSTHROUGH_SHADERS`、portable texture path、64-entry
image texture table 和 entry point 全部匹配时才使用预编译模块；其他 device、backend 或
layout 自动回退到现有 WGSL 路径。

DXIL 与 GPU 厂商/型号无关，但驱动从 DXIL 生成的 PSO/机器码仍与 adapter 和 driver
相关。`Renderer::precompiled_dxil_pipeline_count` 可证明实际初始化了多少条 DXIL pipeline，
避免把输出等价的 WGSL 回退误判为缓存命中。`TILEINK_DXC_PATH` 可指定构建期 DXC；
`TILEINK_DXIL_PRECOMPILE=0` 可用于验证回退路径。

## 原生完整帧的提前提交

较大的完整重绘会把首次 queue submission 提前到约四分之一的根绘制批次完成之后，让
GPU 的 coarse/fine 工作与 CPU 后续编码及命令缓冲区完成重叠。这解决了整帧等待 CPU
完成全部命令后才开始 GPU 工作的调度问题；没有改变 shader 或绘制顺序。

当前采用保守条件：native texture path、完整重绘、至少 1024×1024 个目标像素、至少
16 个非空根批次。首次提交预算为非空根批次数量除以 4，向下取整；例如 33 个批次时
预算为 8。它是可通过 benchmark 调整的工作量策略，不是固定的第 8 批规则或通用最优值。
小帧、局部更新和 portable ping-pong 继续使用原有提交时序。

根批次包括 backdrop 内仍然绘制到主目标的前景；绘制到 scratch 的子层和空范围 backdrop 不计入。
只有遇到下一个真实的根批次才提交前缀；空批次不消耗预算。整帧最多增加一次用于重叠的
提交；如果 uniform arena 已经触发过提交，则不再额外切分。所有提交保持在同一个 queue
上，uniform 写入、绘制、offscreen/filter/backdrop 和最终 history copy 保持依赖顺序。
`IncrementalRenderStats::queue_submissions` 记录实际提交数。

`cargo bench --bench root_batches` 比较不同尺寸和批次数量的 native/portable CPU+GPU
完成时间。真实应用的 FPS、p95 和最长帧必须另外测量，不能用微基准耗时替代。

### Filter input resource states

Both texture paths bind each filter source/aux input once as a sampled texture (SRV), shared by integer `textureLoad` and linear sampling. Only the output uses storage access; the portable path reads a separate previous target. A simultaneous read-only storage alias would require the illegal DX12 UAV | SRV resource state, causing command-list close failure and device invalidation. This fixes the resource declaration itself without a wgpu-hal patch, global wait, or pixel conversion change.

## Pattern transform cancellation

Pattern 坐标的两个乘积可能严格抵消。普通乘加的 contraction 或重排会留下微小残差，
负残差经过 `floor` 后让 nearest/repeat 采到图像另一侧，导致很大的颜色差异。
`shared/pattern_transform.wgsl` 用显式 `fma` 补回第二个乘积的舍入误差，再应用平移，
修复这一数值根因；没有 epsilon、坐标吸附、fixture 分支或输出后处理。
该修正不承诺所有浮点表达式在不同 API 上一致。参考运行器仍要求全部 RGBA 字节相等，
独立数值测试和完整旋转 pattern 回归分别检查运算语义与实际渲染。
