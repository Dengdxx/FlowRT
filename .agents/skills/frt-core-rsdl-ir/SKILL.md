---
name: frt-core-rsdl-ir
description: >
  Use when changing RSDL syntax, Contract IR fields, normalization, validation,
  canonical ordering, schema versions, or FlowRT semantic contracts.
---

# FlowRT Core RSDL / IR

## Overview

RSDL 是源语言，Contract IR 是 normalized 后的合同。这个 skill 防止 agent 只改
parser 或只改 JSON 字段，造成语义链断裂。

## When to Use

- 新增或修改 `.rsdl` 语法。
- 修改 Contract IR 结构、默认值、stable id、canonical ordering。
- 修改 validator 规则、backend capability、deployment 或 message ABI 语义。

不要在以下场景使用：

- 只改用户 app 的 RSDL。
- 只改文档中的示例，不改变工具链语义。

<HARD-GATE>
修改 RSDL/IR 语义前必须确认：

- parser、AST、normalizer、IR model、validator、codegen 是否都受影响。
- 是否需要更新 schema/version/source metadata。
- 是否需要更新 launch manifest、self-description 或 CLI 输出。
- 是否需要 Rust/C++ generated shell 或 runtime 行为同步。

只改单点实现不得宣布完成。
</HARD-GATE>

## Process

1. **定义语义**
   - 先用 FlowRT 术语写出新语义，不直接写字段。
   - 区分用户声明字段、normalized 字段和 derived metadata。

2. **接入语义链**
   - parser 拒绝未知字段和非法值。
   - normalizer 填默认值、稳定排序、分配引用。
   - validator 重新推导派生值并拒绝篡改。
   - codegen public 入口重新验证 IR。

3. **补测试**
   - parser 成功/失败用例。
   - normalizer canonical 输出。
   - validator 防篡改或边界用例。
   - 生成或 CLI 主路径 smoke。

4. **同步文档**
   - 语义变化写 `CHANGELOG.md`。
   - 当前版本背景变化写 `CONTEXT.md`。

## Common Rationalizations

| 借口 | 现实 |
|------|------|
| "TOML 能解析就行" | RSDL 还必须符合 FlowRT schema |
| "IR 是 JSON，直接加字段" | IR 事实源是强类型模型和 validator |
| "codegen 会处理缺省" | defaults 应在 normalization 阶段 canonical 化 |

## Verification

至少选择匹配改动的命令：

```bash
cargo test -p flowrt-rsdl
cargo test -p flowrt-ir
cargo test -p flowrt-validate
cargo test -p flowrt-codegen
```
