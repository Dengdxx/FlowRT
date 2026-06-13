# FlowRT 子任务提示词模板

把下面模板复制给子智能体。不要只发一句任务名；子智能体缺少上下文时，会把问题
留给主编排者收尾。

主编排者默认只生成提示词，由用户手动复制给外部子智能体。除非用户明确要求
“由你直接派发 / 启动 subagent”，不要调用 subagent 工具。

当用户要求主编排者直接启动子智能体时，只能使用 Codex，并选择本机可用的最高能力模型
和最高可用推理强度。当前推荐命令形态为
`codex exec -m gpt-5.5 -c 'model_reasoning_effort="xhigh"' ...`。如果 Codex 配置或
可用模型变化，主编排者必须先确认实际最高可用项，并在派发记录中写明模型、推理强度、
worktree 和分支名。

最终分发文档中的每个任务段都必须是完整自包含的提示词。可以在分发文档顶部写总览
和执行拓扑，但不能要求用户先复制“通用头部”再复制某个任务。用户只复制任意一个
任务段时，外部子智能体也必须拿到完整背景、基线、范围、禁止事项、验证、提交和
交付格式。

主编排者必须在产出或交付提示词前创建或确认主开发分支存在，并在分发文档和每个
任务提示词中写明基线分支、创建时 seed commit，以及本任务应基于派发时的最新
`git rev-parse <branch>`。不要让第一个子任务负责创建主开发分支。

````markdown
你是 FlowRT 仓库的子任务智能体。你只负责本任务，不负责合并主开发分支。

## 任务

一句话目标：[写清楚要完成的行为变化、修复点或调研输出]

本提示词是完整自包含任务包；不要假设用户还会额外转交分发文档顶部的公共头部。

## FlowRT 背景

- FlowRT 是数据流编译型机器人运行时。
- RSDL 控制系统结构，runtime 控制执行，用户代码控制算法。
- Contract IR 是工具链、validator、codegen 和 runtime 的稳定语义合同。
- 用户业务代码不得写进 FlowRT 生成物。
- backend 是传输实现，不是用户语义；不要把底层 SDK API 泄漏给用户。
- C++ runtime 与 Rust runtime 可以独立实现，但 RSDL 语义、Contract IR、消息 ABI 和测试基线必须一致。

## 当前基线

- 主开发分支：[例如 dev/v0.4.0]
- 主开发分支状态：[主编排者已创建 / 已确认存在]
- 主开发分支 seed HEAD：[创建主开发分支时的 git rev-parse <branch> 结果]
- 本任务实际基线：[派发本任务时最新 git rev-parse <branch>；若前置任务已合入，可不同于 seed HEAD]
- 本任务基线 commit：[填写主编排者确认的 commit hash]
- 近期相关提交：
  - [提交 hash + 一句话说明，列出可能影响本任务的相邻改动]
- 本任务对应版本目标：[例如 v0.4.0 Service runtime]
- 已确定的长期决策：
  - [列出和本任务有关的设计决策]
  - [列出不能回退的实现边界]
- 禁止回退项：
  - [例如不得恢复旧 fallback、旧版本号、旧兼容层、旧 CLI 隐藏职责]
  - [例如不得把只应在 codegen 的职责塞回 runtime 或 CLI]

## 执行拓扑位置

- 批次：[Batch N]
- 前置任务：[无 / T01, T02]
- 可并行任务：[无 / T03]
- 启动门槛：[例如必须等 T01、T02 由主编排者验收并合入主开发分支]
- 禁止提前启动的下游任务：[例如 T05、T06]
- 本任务完成后会阻塞：[无 / T07 / 集成任务]

## Worktree

- 基线分支：[base branch]
- worktree 路径：[例如 .worktrees/v0.4.0-service-runtime]
- 分支名：[例如 feat/v0.4.0-service-runtime]
- 推荐创建命令：

```bash
git fetch --all --prune
git worktree add -b [branch] [worktree-path] [base-branch]
```

要求：

- 开工前报告 `git status --short --branch`。
- 开工前报告 `git rev-parse HEAD`。
- 不要改其他 worktree。
- 不要合并、rebase 或 force push 主开发分支。
- 不要清理主编排者或用户要求保留的 worktree。
- 本提示词由用户手动转交；主编排者不直接启动 subagent。
- 若主编排者直接派发本任务，必须使用 Codex 子智能体和最高可用模型/推理强度；
  当前推荐为 `codex exec -m gpt-5.5 -c 'model_reasoning_effort="xhigh"' ...`。
  不得改用 Claude、通用 shell agent 或未说明模型能力的外部执行器。

如果发现基线分支不是最新、worktree 已存在且不干净、前置任务未合入，先停止并报告。

## 范围

允许修改：

- [目录或文件 1]
- [目录或文件 2]

禁止修改：

- 版本号，除非任务明确要求 release bump。
- 正式发布段，除非任务明确要求发布。
- 阶段计划、设计草案或 `.gitignore` 明确排除的本地草稿。
- 用户代码和生成物目录，除非任务明确要求。
- 与本任务无关的格式化、重命名或大重构。
- 已被用户要求保留的 worktree 或本地状态。

## 完成定义

本任务不是“写了底层 primitive 就算完”。完成必须达到以下用户可见或维护者可验收状态：

- [完成定义 1：例如 parser/IR/validator/codegen/runtime/CLI 中哪些主路径必须接通]
- [完成定义 2：例如 Rust/C++ 两侧是否都必须支持]
- [完成定义 3：例如文档、CHANGELOG、self-description 或 CI 是否必须同步]

明确不做的事项：

- [不做事项 1，并说明原因或后续任务编号]
- [不做事项 2]

如果实现过程中发现完成定义和范围冲突，先停止并报告，不要自行扩大范围。

## 实现要求

- [行为要求 1，写成可验收句子]
- [行为要求 2，写成可验收句子]
- 错误必须结构化返回或清晰报错，不要 panic、unwrap 或静默忽略。
- 不要引入临时兼容层、旧 fallback、隐式默认行为或被否决依赖。
- 涉及 runtime 热路径时，避免未观测场景下增加持续开销。
- 涉及 backend 时，必须写清 capability、fallback 禁止项、依赖查找路径和失败报错。
- 涉及 RSDL/IR 时，parser、normalization、validator 和 codegen public 入口都要保持一致。
- 涉及 CLI/runtime 时，CLI 只做工具职责，不承担核心 runtime 行为。
- 涉及生成物时，用户业务代码不得写入 FlowRT 管理文件。
- 如果影响 CLI、RSDL、Contract IR、runtime shell、backend、安装包或 CI，必须更新对应文档和 CHANGELOG。
- 文档和 CHANGELOG 使用中文；命令、标识符和配置键可以保留英文。

## 跨边界要求

如果任务触及以下边界，必须同步检查对应另一侧：

- Rust/C++：检查两侧 runtime、生成 API、ABI 或 conformance 是否一致。
- parser/IR/validator/codegen：不能只改 parser 或只改 codegen。
- CLI/runtime：不能让 CLI 隐式决定 runtime 行为。
- backend/wire：检查 wire 格式、超时、correlation、重连和错误传播。
- static self-description/live introspection：检查编译期拓扑和运行态状态是否能对齐。
- docs/tests/CI：用户可见行为变化必须有文档或 changelog；高风险路径必须进测试。

如果本任务明确只做单侧，必须在交付的“风险和未覆盖”里写清另一侧状态。

## 依赖和环境

- 使用项目锁定的依赖和私有前缀；不要让构建系统隐式联网拉取未声明依赖。
- 如果缺依赖，先检查仓库脚本、CI 配置、vendor 目录、安装包路径和环境变量。
- 报告依赖问题时写出完整命令、完整错误、查过的路径和建议处理方式。
- 不要用“本机没有所以没测”结束任务；必须提供替代验证或明确阻塞点。

## 负向检查

根据本任务填写必须不存在的字符串、旧版本号、旧 fallback、旧概念或旧路径。

必须运行并报告结果：

```bash
rg -n "[forbidden string 1]" [paths]
rg -n "[forbidden string 2]" [paths]
```

这些命令期望无匹配；如果有匹配，必须解释是否是合法历史说明、测试 fixture，或需要删除。

## 验收标准

本任务完成时必须满足：

- [验收点 1]
- [验收点 2]
- 负向检查无未解释匹配。
- 测试输出没有未解释的 panic、warning、timeout、fallback message 或核心路径 skip。
- 没有引入无关 diff。
- 没有未提交修改。

## 必跑验证

运行这些命令，并在交付中写出结果：

```bash
[命令 1]
[命令 2]
[命令 3]
git diff --check
git status --short --branch
```

要求：

- 写出每条命令的退出结果。
- 如果测试退出码为 0 但输出里有 panic、warning、timeout、fallback 或 skip，不能直接算通过。
- 如果某个命令无法运行，说明原因、完整错误和替代验证。不要写“应该通过”。
- 不要只跑新增测试；必须覆盖任务影响的主路径。

## 提交要求

- 使用中文 Conventional Commits。
- 每个独立行为一个提交。
- 标题不超过 50 字，不加句号。
- 正文用 `-` 列出关键改动和验证。
- 不要一次提交混入不相关代码、文档、CI 和格式化。

示例：

```text
fix(codegen): 结构化处理 transport 初始化失败

- generated App 记录 startup_status
- backend open 失败返回 Error，不再 panic
- 补充 codegen 回归测试和 runtime feature 测试
```

## 交付格式

请按这个格式回复：

```markdown
## 基线
- base: `[branch]`
- head: `[hash]`
- worktree: `[path]`

## 完成内容
- ...

## 修改文件
- ...

## 负向检查
- `命令`: 结果

## 验证
- `命令`: 结果
- 异常输出处理：无 / 已解释如下

## 风险和未覆盖
- ...

## 提交
- `<hash> <subject>`
```
````

## 主编排者验收清单

收到子任务后，主编排者按以下顺序处理：

1. `git -C <worktree> status --short --branch`，确认工作树干净或理解所有未提交修改。
2. `git -C <worktree> rev-parse HEAD`，确认子任务报告的 head 一致。
3. `git -C <worktree> log --oneline <base>..HEAD`，确认提交粒度合理。
4. `git -C <worktree> diff --stat <base>...HEAD`，确认没有越界文件。
5. 阅读关键 diff，不只看摘要。
6. 对照长期决策做语义审查，先查旧 fallback、旧依赖、旧版本号和概念混淆。
7. 运行任务包要求的负向 `rg` 检查。
8. 重新运行任务要求的验证；必要时补全更高层验证。
9. 检查测试输出中的 panic、warning、timeout、fallback message 和 skip。
10. 如果发现问题，主编排者亲自修复并重新验证。
11. 对照执行拓扑，确认下游任务是否仍然需要等待本任务合入。
12. 合入主开发分支时保持原子提交；冲突按长期决策解决。
13. 合入后在主开发分支运行集成验证。
14. 再次检查 `git status --short --branch`。
15. 确认 feature branch 已是主开发分支祖先后，清理已合入 worktree 和临时分支；用户要求保留的 worktree 不删。
16. 不保留 stash。确有必须暂存的内容，应转成提交、补丁文件或明确交给用户决策。
17. 如果收尾发现提示词漏项，把经验回灌到后续任务包或本 skill。

## 提示词质量原则

- 背景要足够多，尤其是已经做过的相邻任务、长期设计决策和不能回退的边界。
- 验收标准要可执行，不写“处理好”“完善一下”。
- 禁止事项要直接写明，例如“不 bump 版本”“不新增发布段”“不改用户代码”。
- 验证命令要具体到 package、test filter 或脚本路径。
- 交付格式要固定，否则主编排者很难批量验收。
- 提示词要明确由用户手动转交；主编排者不直接启动 subagent。
- 任务包要写清楚执行拓扑位置，避免下游子智能体基于未合入接口提前开工。
- 如果任务风险高，要求子智能体在改代码前先列实现方案和风险点。
- 如果任务涉及跨语言或 backend，必须要求 Rust/C++ 两侧和对应 conformance 或 smoke 测试。
- 如果任务涉及性能或实时路径，必须写清未观测场景的开销约束和验证方式。
