---
sidebar_position: 4
title: 图片与 Brush
---

# 图片与 Brush

## Image

```rust
let image = tileink::Image::from_rgba8(width, height, rgba_bytes);
renderer.insert_image(tileink::ImageKey::new(42), image);
```

`Image` 内部保存 premultiplied RGBA8。可用 `rgba8_at`/`rgba8_bytes` 读回，`save` 输出 PNG。

## Direct image draw

`Canvas::push_image` 把 `Rc<Image>` 直接记录进 scene；`push_image_key` 引用 renderer registry 中的 `ImageKey`。Key 方式更适合多个 retained nodes 共用大图和独立更新资源。

## Brush

接受 `impl Into<Brush>` 的 API 可以直接传 `peniko::Color`。其他 brush：

- `Brush::Linear` / `Radial` / `Sweep` / `FourCorner`；
- `Brush::Mesh(MeshGradient)`；
- `Brush::Program(ProgramBrush)`；
- `Brush::Pattern(PatternBrush)`；
- `PatternBrush::from_image_key*` 引用 image registry；
- `Brush::from_gradient*` 从 peniko gradient 生成采样 ramp。

Pattern sampling 由 `PatternSampling` 控制；自然尺寸、显式 source/destination bounds 和 affine transform 应按所用 constructor 选择。

资源更新后 renderer 会失效相关 retained history；无需手动重建无关 scene chunks。

## Mesh Gradient

`MeshGradient` 使用至少 2×2、按行排列的位置与颜色数组。每个网格块是直边凸四边形，控制点可以移动。构造函数拒绝折叠、退化、方向不一致以及边界自交的网格；顺时针和逆时针方向均可。

颜色在预乘 sRGB 中插值，默认 `Smooth` 对两个网格参数使用 cubic smoothstep，也可选择 `MeshInterpolation::Bilinear`。共享边界颜色连续；扭曲网格的空间导数不保证连续。网格边界外透明，需要填满形状时应让 mesh 覆盖整个形状。当前不提供 Bézier 边界控制柄，也不解析 SVG mesh 元素。

```rust
use tileink::{MeshGradient, Radius};
use peniko::{Color, kurbo::Rect};

let mesh = MeshGradient::new(
    3, 3,
    [[0., 0.], [150., 0.], [300., 0.],
     [0., 100.], [190., 70.], [300., 100.],
     [0., 200.], [150., 200.], [300., 200.]],
    [Color::from_rgb8(40, 60, 180), Color::from_rgb8(230, 70, 150), Color::WHITE,
     Color::from_rgb8(20, 190, 210), Color::from_rgb8(130, 70, 240), Color::from_rgb8(255, 100, 100),
     Color::BLACK, Color::from_rgb8(60, 140, 220), Color::from_rgb8(230, 110, 160)],
)?;
canvas.push_rect(Rect::new(0., 0., 300., 200.), Radius::all(20.), mesh);
```

原生 Metal/HLSL 共用同一 Brush 布局与空间分桶；支持 path、SDF、滤镜 Brush、坐标变换和 retained 更新。运行 `cargo run --release --no-default-features --features metal --example mesh_gradient` 可生成示例 PNG。


## 可编程 Brush 与 Filter

`ShaderProgram::new` 接受用户提供的完整 Metal／HLSL 源码及入口，按原文编译。没有表达式 DSL、代码生成器或 GPU 解释器。`brush(uniforms)` 创建程序化画笔，`filter(uniforms)` 创建滤镜；uniform 是原始 `u32` 数据，浮点数用 `f32::to_bits` 编码。画笔用 `with_uniforms` 更新参数；`filter(uniforms)` 返回普通 `ShaderPipeline`，可用 `with_resource(3, ShaderResource::Buffer(words.into()))` 更新参数。`Filter::Program` 和 `ProgramFilter` 已删除。

```rust
use tileink::{ShaderProgram, ShaderSources, ShaderInput};
let program = ShaderProgram::new(
    ShaderSources {
        metal: Some(include_str!("glass.metal").into()),
        hlsl: Some(include_str!("glass.hlsl").into()),
    },
    "glass", ShaderInput::Sampled { radius: 32.0 }, 29,
)?;
let filter = program.filter(uniform_words)?;
canvas.push_backdrop_layer(filter.into(), region);
canvas.pop_layer();
```

只需提供应用支持的后端源码；缺失源码或编译／接口错误会返回渲染错误。完整的 ABI、像素范围检查和采样示例见仓库 `examples/shaders/` 与 `docs/native/program-brush-filter.md`。每组 256 个线程，`ShaderConfig` 提供范围、坐标变换、compact tiles 和采样声明；用户 shader 自行负责越界处理、采样和预乘 sRGB 输出。

`ShaderInput::None` 不读输入，可创建 Brush；`Current` 只读当前输入像素；`Sampled { radius }` 声明最大逻辑采样范围，用于背景捕获和 retained 刷新依赖。声明不会改写 shader。普通 Filter 读取本层内容；Backdrop 读取此前已绘制的背景。读取输入的滤镜使用独立输出纹理和局部 copy-back，支持 chain／graph。

`ShaderProgram::filter()` 是构造普通 `ShaderPipeline` 的便捷方法，使用固定的单 pass ABI；需要额外资源、浮点中间纹理和显式调度时，使用下方的 `ShaderPipeline`。两者均编译用户源码，不使用解释器。原生源码版本为 Metal 2.4／HLSL `cs_6_0`，不会开放所有图形 API 能力。

Brush 先由用户 kernel 在 GPU 上绘制受影响区域，再交给现有路径／SDF／clip 合成流程，因此有额外纹理和预绘制成本。程序源码不上传成 GPU 指令。运行 `program_brush` 示例可查看程序化配色与扫描线；左右都是自定义效果。`program_glass` 左侧为基础玻璃，右侧加入 RGB 色散。

现有 `append_transformed` 对整个 Filter layer 的旋转／shear 尚未同步变换采样区域，可能裁掉内容；可编程 Filter 同样受此限制。Brush 仿射变换、Filter 的 DPI、平移 append 和离屏坐标有回归测试。

### 多 pass 原生 ShaderPipeline

`ShaderPipeline` 让应用提供完整 Metal／HLSL 源码，以及资源、绑定和 pass 描述。支持多张输入图片、sampler、读写 buffer、RGBA8／RGBA16F／RGBA32F 中间纹理、就地读写、任意工作组和多分辨率调度。Tileink 负责资源复用与同步，pass 之间无需 CPU 回读。

所有内置 filter 的 GPU 阶段均通过公开 `ShaderKernel::precompiled`、`ShaderPipeline::new`、`ShaderResource::External` 和 `ShaderPass` 接口构造并执行，包括渐进模糊和液态玻璃。现有 `Filter` API 保留，内置算法继续使用构建时编译的 Metal／HLSL kernel，并直接绑定已有 GPU 资源；稀疏 tile、共享内存和多分辨率优化保留，不增加像素拷贝或逐帧 shader 编译。

`ShaderKernel::new` 声明源码、入口、工作组和绑定；`ShaderPipeline::new` 声明资源与执行顺序；`with_resource` 更新不可变参数或图片。默认追踪全画布输入依赖，`with_input_radius` 可声明整个管线的最大逻辑采样位移，以减少背景捕获与 retained 重算。声明必须覆盖全部 pass 与插值。

`with_output_outset(outset)` 独立声明输出向四周扩大的逻辑距离，用于阴影、模糊、偏移；框架据此处理输出边界、离屏表面和 retained 失效。输入半径和输出外扩均随 DPI 换算。它们只是调度契约，shader 仍由应用完整提供。

`ShaderResource::Input(index)` 是动态图像输入，编号必须从 0 连续且不重复。普通滤镜的 `Input(0)` 读取当前图层／背景。多输入图节点使用 `FilterPrimitiveKind::ShaderPipeline { pipeline, inputs }`，按顺序绑定 `SourceGraphic`、`SourceAlpha` 或任意前序 `Primitive(index)`；允许重复引用同一图像，直接绑定 GPU 纹理，无需 CPU 回读或上传。输入数量须等于 `pipeline.input_count()`，前向／不存在的引用会使渲染失败。此类型使用自己的 `inputs`，忽略公共的 `input`／`input2` 字段；节点的显式 region 始终裁剪输出。

`examples/shader_graph.rs` 展示 SourceAlpha → 偏移 → 模糊，再由用户 shader 合成原图与 GPU 中间结果；完整源码在 `examples/shaders/composite.metal`／`.hlsl`。

`NativeShaderBatch` 公开已有 GPU 资源的调用接口：创建 buffer／纹理／数组／纹理表／sampler，或导入 `NativeTexture`，用 `record` 将句柄按 External index 绑定，最后 `submit`。它与 Canvas 滤镜共用 pipeline 描述、校验和执行器；跨 batch／设备、类型不匹配和读写别名会报错。`ShaderBindingKind::Uniform(size)` 支持任意常量块，`TextureArray`／`TextureTable(count)` 支持数组和纹理表。

资源槽位为 0..=29，纹理表另可用槽位 30，sampler 为 0..=15。可选 `Config` 在槽位 0 使用 `ShaderConfig`。纹理首次写入必须初始化后续读取的像素；buffer 每次执行会恢复初始数据，不保存隐式历史。输出为预乘 sRGB RGBA8；中间浮点纹理保留负数和 HDR。

查看 `examples/shader_pipeline.rs` 和 `examples/common/shader_blur.rs`：四个用户原生 kernel 完成降采样、横向／纵向高斯模糊和上采样，并绑定额外的 tint 图片。
