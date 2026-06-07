---
name: frt-core-runtime-parity
description: >
  Use when changing Rust or C++ runtime behavior, C ABI types, scheduler
  semantics, message frames, service/channel behavior, params, or introspection APIs.
---

# FlowRT Core Runtime Parity

## Overview

Rust runtime 和 C++ runtime 可以独立实现，但必须共享 FlowRT 语义、ABI 和主路径
行为。这个 skill 防止只验证一种语言后宣布完成。

## When to Use

- 修改 `crates/flowrt/` 或 `runtime/cpp/`。
- 修改 C ABI、message frame、channel、service、params、scheduler、introspection。
- 修改跨语言 generated shell 依赖的 runtime API。

不要在以下场景使用：

- 只改 CLI 文案。
- 只改 parser/IR 且不影响 runtime 行为。

<HARD-GATE>
修改 runtime 语义前必须确认：

- Rust 和 C++ 是否都应支持；不支持的一侧必须 fail-fast。
- C ABI layout、错误码、状态码和 borrowed view 是否受影响。
- 是否需要 conformance、ABI static assert 或 byte-level fixture。
- 运行态状态是否要进入 self-description、introspection 或 `flowrt status`。
</HARD-GATE>

## Process

1. **写出共享语义**
   - 用 FlowRT 概念描述行为，不从某一语言 API 反推语义。

2. **更新两侧**
   - Rust 与 C++ 公共 API、错误语义、状态转换保持一致。
   - 只有明确单侧能力时，另一侧返回 Unsupported 或 validator 拒绝。

3. **检查 ABI**
   - `runtime/cpp/include/flowrt/abi.h` 是 C ABI 事实源。
   - Rust `repr(C)` 镜像和 C++ layout 测试同步。

4. **验证主路径**
   - 单元测试覆盖状态机。
   - generated app 或 demo 覆盖用户可见路径。

## Common Rationalizations

| 借口 | 现实 |
|------|------|
| "Rust 已经测过" | FlowRT 承诺跨语言语义一致 |
| "C++ 以后补" | 应明确 Unsupported 或同步实现 |
| "ABI 字段只是内部用" | C ABI 是后续语言绑定稳定边界 |

## Verification

```bash
cargo test -p flowrt
cmake -S runtime/cpp -B build/cpp
cmake --build build/cpp
ctest --test-dir build/cpp --output-on-failure
```
