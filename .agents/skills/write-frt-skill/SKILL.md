---
name: write-frt-skill
description: >
  Create or revise FlowRT core agent skills. Use when user wants to add,
  design, split, rename, or improve skills for FlowRT repository development
  workflows.
---

# 写 FlowRT Skill

## Overview

FlowRT skill 只服务 FlowRT 范围内的可复用工作流。当前阶段只写 `frt-core-*`，
不写 `frt-app-*`。它可以借鉴通用 skill 的短入口、渐进披露和硬门控，但不能写成
适配任何项目的泛用流程。

<HARD-GATE>
写任何 `SKILL.md` 前必须先确认：

1. 目标 workflow 属于 `frt-core-*`，且面向开发 FlowRT 本身。
2. 用户明确说明这个 skill 要减少哪类 FlowRT 返工或错误。
3. 已读 `.agents/skills/README.md` 和同前缀现有 skill，确认没有重复。
4. 已列出该 skill 必须守住的 FlowRT 语义边界和验证证据。
5. 没有创建 `frt-app-*` skill；app skill 等 FlowRT 1.0.0 后再写。

违反任一条，不得开始写文件。
</HARD-GATE>

## 分类

| 前缀 | 用途 | 典型边界 |
|---|---|---|
| `frt-core-*` | 开发 FlowRT 本身 | RSDL、Contract IR、validator、codegen、runtime、backend、CLI、CI、release。 |
| `frt-app-*` | FlowRT app 开发 | 1.0.0 后再写；当前只保留命名空间。 |
| `frt-subtask-orchestration` | FlowRT 编排 | 既有私有 skill；不要复制到其他目录。 |

不要新增裸名 skill。`build-run`、`debug-flow`、`write-code` 这类名称必须改成
带 FlowRT 前缀的稳定名称。当前阶段不得新增这些 app 侧 skill。

## Process

1. **归类**
   - 判断目标是否维护 FlowRT 仓库。
   - 如果目标是开发 FlowRT app，停止并说明该类 skill 等 1.0.0 后再写。

2. **压场景**
   - 写下 2-3 个触发句，例如“我要改 RSDL 语义”或“数据没到 controller”。
   - 写下 agent 最容易犯的错误，例如只改 parser、不跑验证、跳过 status。

3. **定门控**
   - 对会造成语义漂移、半接入主线、跳过验证或误改生成物的步骤写
     `<HARD-GATE>`。
   - 门控要可执行，不能写“认真检查”这类空话。

4. **起草**
   - `description` 只写触发条件，不总结流程。
   - `SKILL.md` 保持短小；长清单放 `references/`。
   - 用 FlowRT 术语，不把 backend、component、instance、task、channel 混成底层
     SDK 概念。

5. **验证**
   - 检查命名、frontmatter、触发条件、硬门控、验证命令和禁用字样。
   - 更新 `.agents/skills/README.md` 中对应表格。

## SKILL.md 模板

```yaml
---
name: frt-core-name
description: >
  Use when [FlowRT-specific trigger], [symptom], or user wants to [goal].
---

# Skill 标题

## Overview

这个 skill 防止哪类 FlowRT 返工或语义错误。

## When to Use

- 用户说 [触发句]。
- 用户遇到 [FlowRT 症状]。

不要在以下场景使用：

- [不属于本 skill 的 FlowRT core 场景]。

<HARD-GATE>
[必须满足的前置检查、禁止事项和验证证据]
</HARD-GATE>

## Process

1. [步骤]
2. [步骤]
3. [验证]

## Common Rationalizations

| 借口 | 现实 |
|------|------|
| "先跑通再说" | 跑通不等于符合 FlowRT 契约 |
| "只改这一侧就够了" | FlowRT 语义通常跨 parser / IR / codegen / runtime |
| "测试绿了" | 没有覆盖主路径或跨语言边界时仍可能是假绿 |

## Quick Reference

| 操作 | 命令或文件 |
|------|------------|
| 查看项目约束 | `AGENTS.md` |
| 查看当前背景 | `CONTEXT.md` |
| 查看变更记录 | `CHANGELOG.md` |
```

## FlowRT 术语表

写 skill 时优先使用以下术语：

| FlowRT 术语 | 不要替换成 |
|---|---|
| RSDL | 配置文件、yaml、launch file |
| Contract IR | 临时 JSON、中间产物 |
| component | 外部框架的执行实体名 |
| instance | 语言对象实例 |
| task | 普通定时器或回调 |
| channel | 外部框架的通信实体名 |
| bind | 连接、连线 |
| backend | middleware、通信层 |
| runtime shell | 生成代码、胶水代码 |
| profile | 配置、profile |

术语替换不是为了咬文嚼字，而是为了让 agent 不把 FlowRT 的契约模型误降级成其他
框架或底层 SDK 的概念。

## Description 规则

`description` 是 agent 判断是否加载 skill 的主要入口。

必须遵守：

- 用 `Use when ...` 描述触发条件。
- 写用户可能会说的话、错误症状或 FlowRT 操作。
- 不写流程摘要，避免 agent 只读 description 后跳过正文。
- 控制在 500 字符以内。

好例子：

```yaml
description: >
  Use when changing RSDL syntax, Contract IR fields, normalizer behavior,
  validator rules, or any FlowRT semantic contract that crosses parser and
  codegen boundaries.
```

坏例子：

```yaml
description: Helps update FlowRT code by checking files and running tests.
```

## 硬门控示例

### `frt-core-*`

```markdown
<HARD-GATE>
修改 RSDL/IR 语义前必须确认：

- parser、normalizer、Contract IR、validator、codegen 是否都受影响。
- 是否需要更新 self-description、launch manifest 或 CLI 输出。
- 是否有 Rust/C++ 或 ABI 一致性验证。

只改单点实现不得宣布完成。
</HARD-GATE>
```

## 验证清单

写完或修改 skill 后检查：

- [ ] name 和目录名一致，且为 `frt-core-*`、`write-frt-skill` 或既有
  `frt-subtask-orchestration`。
- [ ] 没有新增 `frt-app-*` skill。
- [ ] description 只写触发条件，不写流程。
- [ ] `SKILL.md` 够短；长材料已拆到 `references/`。
- [ ] 有明确 When to Use 和不要使用场景。
- [ ] 有硬门控，且门控可执行。
- [ ] 有验证证据要求。
- [ ] 术语符合 FlowRT 语义。
- [ ] `.agents/skills/README.md` 已登记。
- [ ] 没有阶段性计划、一次性任务记录或禁用字样。
