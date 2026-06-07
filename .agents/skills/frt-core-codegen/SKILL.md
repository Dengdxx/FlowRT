---
name: frt-core-codegen
description: >
  Use when changing FlowRT codegen, generated Rust or C++ runtime shells,
  launch manifests, self-description output, generated build files, or codegen tests.
---

# FlowRT Core Codegen

## Overview

生成物是可丢弃的 glue，不承载用户业务逻辑。这个 skill 防止 codegen 修改覆盖用户
代码、绕过 IR validator，或只在一种语言生成路径生效。

## When to Use

- 修改 `crates/flowrt-codegen/`。
- 修改 generated Rust/C++ shell、manifest、self-description 或构建文件。
- 生成物新增字段、调度逻辑、backend binding 或用户接口。

不要在以下场景使用：

- 只改 runtime primitive，尚未接 codegen。
- 只改手写用户示例代码。

<HARD-GATE>
修改 codegen 前必须确认：

- 输入 Contract IR 已由 validator 校验。
- 生成物不会覆盖用户 `src/` 业务代码。
- Rust/C++ language boundary 诚实，不为另一种语言伪造接口。
- 生成物变化有 snapshot、unit test、smoke 或编译验证。
</HARD-GATE>

## Process

1. **定位生成面**
   - 消息类型、component trait/interface、runtime shell、backend、manifest、
     self-description、构建文件分别定位。

2. **保持 glue 边界**
   - 用户算法只通过 trait/interface/factory 接入。
   - FlowRT 管理目录只写机器生成产物和必要来源说明。

3. **跨语言检查**
   - 语义跨 Rust/C++ 时，两侧生成路径都要更新或明确拒绝。
   - ABI、消息布局、service/channel policy 不得漂移。

4. **验证**
   - 新增或更新 codegen 测试。
   - 构建至少一个受影响 demo 或 generated app。

## Common Rationalizations

| 借口 | 现实 |
|------|------|
| "只改模板就行" | 生成物还要编译、运行和被 CLI 消费 |
| "Rust 通了 C++ 后面补" | 跨语言语义必须同步或明确拒绝 |
| "用户代码也顺手生成" | 用户算法不能写进 FlowRT 管理产物 |

## Verification

```bash
cargo test -p flowrt-codegen
cargo test -p flowrt-cli codegen
```

C++ 生成路径受影响时补充：

```bash
cmake -S runtime/cpp -B build/cpp
cmake --build build/cpp
ctest --test-dir build/cpp --output-on-failure
```
