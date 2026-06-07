# FlowRT Skills 套组

本目录是 FlowRT 专有 agent skills 的入库事实源。它不追求适配所有项目，而是服务
两个 FlowRT 场景：

- 开发 FlowRT 本身。
- 使用 FlowRT 开发 app。

当前阶段只落地 `frt-core-*` 和跨 core/app 编排通用的 `frt-subtask`。`frt-app-*`
是 1.0.0 之后的保留命名空间；在 FlowRT schema、runtime、安装包和兼容策略稳定前，
不编写 app 侧 skill。

## 设计原则

1. **FlowRT 范围内通用**：skill 应覆盖一类可复用 FlowRT 工作流，不写成某次任务
   的复盘。
2. **平铺命名空间**：skill 名称通常必须带 FlowRT 前缀；`frt-subtask` 是唯一
   允许的跨 core/app 编排短名。
3. **小而可组合**：每个 skill 只管一个边界清晰的动作，复杂工作由多个 skill 串联。
4. **强门控**：会造成语义漂移、半接入主线、跳过验证或误改生成物的场景必须写
   `<HARD-GATE>`。
5. **证据优先**：skill 要求 agent 给出实际命令、退出结果、关键 diff 或观测输出，
   不能只写“应该可以”。
6. **渐进式披露**：`SKILL.md` 保持短小；长模板、检查表或示例放进 `references/`。

## 命名空间

| 前缀 | 面向对象 | 说明 |
|---|---|---|
| `frt-core-*` | FlowRT 维护者 | 修改 FlowRT 仓库、runtime、工具链、CI 和发布流程。 |
| `frt-app-*` | FlowRT app 开发者 | 1.0.0 之后再落地；当前只保留前缀。 |
| `write-frt-skill` | skill 维护者 | 创建或修改 FlowRT 专有 skill 的元技能。 |
| `frt-subtask` | FlowRT 编排者 | 外部分工提示词、worktree 验收和子任务合入。 |

不要新增 `write-code`、`debug-flow`、`build-run` 这类裸名。裸名在多项目、多 agent
环境下语义不稳定；`frt-subtask` 的短名只因其同时服务 FlowRT core 与 app 子任务
编排而保留。

## `frt-core-*`：开发 FlowRT 本身

| 优先级 | Skill | 触发条件 | 职责 | 硬门控 |
|---|---|---|---|---|
| P0 | `frt-core-change-intake` | 用户提出 FlowRT 新需求、缺陷或架构调整 | 判断影响 parser / IR / validator / codegen / runtime / docs 的范围 | 必须先读 `AGENTS.md`、`CONTEXT.md` 和相关最近提交 |
| P0 | `frt-core-rsdl-ir` | 修改 RSDL 语义、Contract IR 或 validator | 维护 parser、normalizer、IR、validator 的一致链路 | 不允许只改 parser 或只改 IR |
| P0 | `frt-core-codegen` | 修改生成物或 runtime shell | 维护 Rust/C++ codegen、snapshot、smoke 主路径 | 生成物变化必须有可复现验证 |
| P0 | `frt-core-runtime-parity` | 修改 Rust/C++ runtime、C ABI 或跨语言行为 | 保证两侧语义、ABI、错误码和测试对齐 | 不能只验证一种语言 |
| P0 | `frt-core-backend` | 修改 iox2、zenoh 或 backend capability | 维护 capability、resolver、私有依赖和 runtime endpoint | backend SDK 不得泄漏到用户语义 |
| P1 | `frt-core-worktree-review` | 验收外部 worktree 或合入子任务 | 审查 diff、运行验证、cherry-pick、清理状态债务 | 不能信任完成声明，必须亲自复验 |
| P1 | `frt-core-introspection-cli` | 修改 self-description、live socket、CLI 观测 | 对齐静态拓扑、运行态状态和 CLI 输出 | 静态/动态路径必须同时考虑 |
| P1 | `frt-core-ci-release` | 修改 CI、Debian package、release 或 CHANGELOG 抽取 | 保持 amd64/arm64、打包和 release gate 一致 | release notes 必须来自 `CHANGELOG.md` |
| P1 | `frt-core-debug-regression` | FlowRT 自身测试失败、CI 失败或行为回归 | 复现、最小化、定位、修复、补回归测试 | 未复现不得下结论 |
| P1 | `frt-core-docs-context` | 改语义、命令、目录、发布或版本背景 | 同步 README、docs、CONTEXT、CHANGELOG 边界 | 计划和设计草案不得入库 |

## `frt-app-*`：1.0.0 后再写

app skill 需要基于稳定用户体验，而 FlowRT 当前仍在推进 schema、runtime、安装包、
兼容策略和调试主路径。1.0.0 之前不写 app skill，避免把未稳定的使用方式固化成
教程。当前只保留 `frt-app-*` 前缀，防止未来命名冲突。

## 首批落地顺序

优先做能显著减少 FlowRT core 返工的 P0：

1. `frt-core-change-intake`
2. `frt-core-rsdl-ir`
3. `frt-core-codegen`
4. `frt-core-runtime-parity`
5. `frt-core-backend`

`frt-core-worktree-review` 与既有 `frt-subtask` 职责接近，先列为 P1，
等当前编排方式稳定后再决定是否拆出。

## 目录规范

```text
.agents/skills/
  README.md
  write-frt-skill/
    SKILL.md
  frt-subtask/
    SKILL.md
    references/
      ...
  frt-core-rsdl-ir/
    SKILL.md
    references/
      ...
```

新增 skill 时必须：

- 使用 `write-frt-skill`。
- 在 `README.md` 表格中登记。
- 当前必须属于 `frt-core-*` 或既有 `frt-subtask`；`frt-app-*` 等 1.0.0 后再写。
- 写清触发条件、非适用场景、硬门控、验证要求和常见错误。
- 避免复制通用项目开发建议；只保留 FlowRT 范围内会反复用到的判断。

## 参考方法

本套组吸收两类外部通用 skill 的方法，但不复制其通用定位：

- 小技能、短入口、渐进式披露。
- 强门控、反合理化、验证优先。

FlowRT skill 的最终判断标准是：能否让 agent 更稳定地维护 FlowRT 契约、生成物、
runtime 行为和 app 主路径。
