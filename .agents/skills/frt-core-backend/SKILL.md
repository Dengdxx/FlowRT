---
name: frt-core-backend
description: >
  Use when changing FlowRT backend capability, iox2 or zenoh routes, endpoint
  behavior, reconnect policy, private SDK packaging, or backend resolver rules.
---

# FlowRT Core Backend

## Overview

Backend 是传输实现，不是 FlowRT 用户语义。这个 skill 防止底层 SDK 名称、session、
publisher/subscriber 或临时 fallback 泄漏到 RSDL、用户 API 或 generated shell。

## When to Use

- 修改 iox2、zenoh、backend capability 或 route resolver。
- 修改 endpoint reconnect、health、wire/frame、private dependency packaging。
- 修改 profile、target、route-level backend 选择。

不要在以下场景使用：

- 只调用户 app 的 channel policy。
- 只改 CLI 展示，不改 backend 行为或能力。

<HARD-GATE>
修改 backend 前必须确认：

- capability 已在 Contract IR / validator 中显式建模。
- 用户代码不直接依赖 backend SDK。
- generated manifest、自描述和诊断能解释 backend 选择。
- 缺失依赖 fail-fast，不隐式联网拉取。
- 不引入已否决的 fallback 或临时兼容承载层。
</HARD-GATE>

## Process

1. **定义 capability**
   - 明确 topology、transfer、message ABI、fixed/variable frame 支持。
   - route resolver 必须能解释选择原因。

2. **更新工具链**
   - RSDL 不出现底层 SDK API。
   - IR 记录 backend source、policy source、requirements 和 satisfied 状态。
   - validator 重新推导并拒绝伪造派生值。

3. **更新 runtime**
   - Rust/C++ endpoint 行为对齐。
   - health、reconnect、Unsupported/Transport/Codec 错误分类清晰。

4. **更新交付**
   - 私有依赖和 license material 跟 release package 对齐。
   - CI 使用安装包路径验证用户环境。

## Common Rationalizations

| 借口 | 现实 |
|------|------|
| "SDK API 直接暴露更简单" | backend 不是 FlowRT 用户语义 |
| "缺依赖就 fallback" | fallback 会掩盖配置错误和发布缺口 |
| "只改 Rust endpoint" | backend 行为必须考虑 C++ 和 generated manifest |

## Verification

```bash
cargo test -p flowrt-ir backend
cargo test -p flowrt-validate backend
cargo test -p flowrt backend
cargo test -p flowrt-codegen backend
```

C++ backend 受影响时补充：

```bash
cmake -S runtime/cpp -B build/cpp
cmake --build build/cpp
ctest --test-dir build/cpp --output-on-failure
```
