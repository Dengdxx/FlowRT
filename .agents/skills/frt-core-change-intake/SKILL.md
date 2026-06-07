---
name: frt-core-change-intake
description: >
  Use when a user asks for a FlowRT core feature, bug fix, architecture change,
  release change, or asks what should be changed inside the FlowRT repository.
---

# FlowRT Core Change Intake

## Overview

把需求先映射到 FlowRT 的真实边界，再动代码。这个 skill 防止 agent 只改最显眼的
文件，遗漏 Contract IR、validator、codegen、runtime、CLI、文档或发布流程。

## When to Use

- 用户提出 FlowRT 新功能、缺陷、架构调整或版本任务。
- 用户问“这个该怎么接进来”“还缺什么”“下一步做什么”。
- 用户给出外部 agent 的 worktree 结果，但还没明确影响面。

不要在以下场景使用：

- 只开发 FlowRT app。
- 已经有明确实现计划，且当前只是在执行单个原子任务。

<HARD-GATE>
开始修改前必须确认：

- 已读 `AGENTS.md` 的相关约束。
- 已读 `CONTEXT.md` 中当前版本背景。
- 已查看 `CHANGELOG.md` 未发布段。
- 已列出影响面：RSDL、Contract IR、validator、codegen、runtime、CLI、docs、CI。
- 已决定是否需要更新入库文档；计划和设计草案不得入库。
</HARD-GATE>

## Process

1. **归类请求**
   - 标记为语义、runtime、backend、CLI、CI/release、docs 或测试缺口。
   - 判断是否属于当前版本目标；不属于时先说明风险。

2. **画影响面**
   - 写出会被触及的 crate、runtime 目录、docs 和示例。
   - 对跨语言行为明确 Rust/C++ 是否都受影响。

3. **定完成定义**
   - 写清用户可见行为。
   - 写清必须跑的最小验证。
   - 写清哪些不做，避免偷带范围。

4. **执行或分发**
   - 简单任务亲自做。
   - 需要外部 agent 时再用任务编排 skill 写完整提示词。

## Common Rationalizations

| 借口 | 现实 |
|------|------|
| "只是个小改动" | 小改动也可能破坏 IR、生成物或 CLI 契约 |
| "先写代码再补文档" | FlowRT 语义变化必须同步文档和 changelog |
| "测试跑一个就够" | 跨 parser/codegen/runtime 的变化必须覆盖主路径 |

## Quick Reference

| 目标 | 先看 |
|------|------|
| 项目长期约束 | `AGENTS.md` |
| 当前版本背景 | `CONTEXT.md` |
| release 事实源 | `CHANGELOG.md` |
| RSDL/IR 语义 | `crates/flowrt-rsdl/`、`crates/flowrt-ir/`、`crates/flowrt-validate/` |
| 生成路径 | `crates/flowrt-codegen/` |
| 运行时 | `crates/flowrt/`、`runtime/cpp/` |
