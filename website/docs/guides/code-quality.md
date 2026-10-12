---
title: 代码质量检查
sidebar_position: 6
---

# 代码质量检查

仓库通过 `rust-toolchain.toml` 固定 Rust、rustfmt 和 Clippy 版本。
提交前运行 `cargo fmt --all --check`，并选择当前平台的后端：

```sh
# Windows DX12
cargo clippy --release --all-targets -- -D warnings
# Windows Vulkan
cargo clippy --release --all-targets --no-default-features --features vulkan -- -D warnings
# macOS Metal
cargo clippy --release --all-targets --no-default-features --features metal -- -D warnings
```

后端 feature 互斥，不能使用 `--all-features`。Linux 当前检查 Vulkan
shader 编译与共享代码，尚无原生运行适配器；该配置在 `--` 后添加
`-A dead_code -A unused_imports -D warnings`，允许未被运行适配器消费的路径。

`Cargo.toml` 启用 `all`、`pedantic`、`nursery`、`cargo` 和选定的
`restriction` 规则。`all` 包含代码风格、复杂度、性能和正确性检查。
额外规则覆盖 API 错误文档、unsafe 安全说明、冗余返回包装，以及
`unwrap`、`expect`、`panic!`。CI 对测试、示例和构建脚本采用相同策略。

测试允许在初始化和断言失败时终止。生产代码中的内部不变量需要局部
`#[allow(..., reason = "...")]` 解释；外部输入和 I/O 失败应遵循已有错误接口。
把 `unwrap` 改成 `expect` 本身不能消除告警。

渲染器需要的数值量化、精确浮点比较和 GPU ABI 布局在配置或使用处写明例外。
`restriction` 包含互斥规则，CI 会额外使用 `--force-warn` 扫描全部规则组，
上传完整诊断供审查。例外仍出现在完整扫描中。

完整策略、例外理由和本地审查命令见仓库
[Clippy 开发说明](https://github.com/Jianqoq/tileink/blob/main/docs/development/clippy.md)。
语义或所有权修改还需运行单线程 release 测试；渲染修改需核对 SVG、示例画面
和实际应用负载，不能只根据 lint 建议改变算法。
