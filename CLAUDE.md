# AGENTS.md

本文档为参与 FlowRT 仓库工作的 coding agent 提供长期项目约束。它描述开发
FlowRT 本身时必须遵守的架构原则、语义边界、文档边界和提交纪律。

阶段性事实、已实现能力、当前版本背景、CLI 状态、CI/Release 状态和近期路线
维护在入库的 `CONTEXT.md` 中。修改语义、接口、命令、生成物或发布流程时，应
同步维护 `CONTEXT.md`、`CHANGELOG.md` 和相关入库文档。

## 项目定位

FlowRT 是一个数据流编译型机器人运行时。用户用 `.rsdl` 声明消息、组件端口、
任务周期、数据流连接、部署目标和通信后端；工具链将 RSDL 编译为 Contract IR，
完成校验后生成 C++/Rust 薄 runtime shell、消息类型、启动配置和构建文件。

FlowRT 的核心原则是：

```text
RSDL controls system structure.
Runtime controls execution.
User code controls algorithms.
```

FlowRT 的核心对象是可编译、可校验、可生成的数据流系统契约，而不是运行时动态
拼装的通信对象集合。

## 设计原则

1. **契约优先**：RSDL 是源语言，Contract IR 是 normalized 后的语义合同。
   工具链、validator、codegen 和 runtime 都应面向 Contract IR，而不是直接依赖
   RSDL 文本结构。
2. **生成物可丢弃**：`flowrt/` 下的 FlowRT 管理产物必须可删除、可重建，不得
   承载用户业务逻辑。
3. **用户代码隔离**：用户算法代码通过接口、trait、factory 或 adapter 接入，不
   写进生成文件。
4. **Backend 隔离**：业务层只依赖 FlowRT runtime API，不直接依赖具体通信库。
   iox2、zenoh 等 backend 是传输实现，不是 FlowRT 系统语义本身。
5. **跨语言语义一致**：C++ runtime 和 Rust runtime 可以独立实现，但必须共享
   RSDL 语义、Contract IR、消息 ABI 和 conformance tests。
6. **低学习成本**：普通用户只应关心组件输入、输出和算法实现。通信、调度、生
   命周期和部署由系统生成或管理。
7. **长期模型优先**：阶段性 MVP 只能是长期模型的子集。即使先实现窄切片，也要
   在 IR、validator、codegen 和文档中保留模块化 package、backend capability、
   variable frame、跨语言一致性和运行态观测的扩展边界。

## 非目标

- 不把通信库 publisher/subscriber、session、node 等底层 API 暴露为 FlowRT 用户
  语义。
- 不让用户算法代码直接依赖 iox2、zenoh 或其他 backend SDK。
- 不支持 map、递归消息结构或语言专属 ownership 类型，除非先明确 Message ABI
  和跨语言语义。
- 不在语义未定前堆砌 runtime 代码、大型依赖或半成品目录。

## 仓库边界

- 不要一次性创建空目录树。只有当对应实现、测试或文档已经存在时再落目录。
- FlowRT 管理的应用产物只保留必要的机器标记和来源说明，不维护手写业务逻辑。
- 用户代码属于项目或示例的 `app/` 目录；生成器不得覆盖用户代码。
- 新增 parser、serialization、CLI、template 或测试库时，优先选择成熟、维护良好、
  依赖面可控的库，并在相关文档中记录原因。
- 标准用户入口是安装后的 `flowrt ...` 命令。仓库内的 `cargo run -p flowrt-cli -- ...`
  只作为 FlowRT 开发者调试 CLI 的内部命令，不写成用户主路径。

## RSDL 约定

- RSDL 源文件使用 `.rsdl` 后缀。
- 当前语法载体是 TOML 子集；不要写“像 TOML 但不是 TOML”的含糊语法。`.rsdl`
  文件应能被标准 TOML parser 解析。
- TOML 只是初代语法载体，不是 Contract IR 或长期语义模型。
- 长期语义模型必须区分 `types`、`components`、`graphs`、`profiles`、`targets`
  和 `package`。
- RSDL 不应出现 backend 的底层 API 名称。应描述 channel、policy、target 和
  capability requirement。
- parser 必须拒绝未知顶层 section 和固定 schema 表中的未知字段，不能静默吞掉
  拼写错误。message fields 与参数表等明确开放键空间除外。
- imports 必须展开为统一 package 语义；声明顺序不应影响 canonical Contract IR。
- `component`、`instance` 和 `task` 是不同概念：component 描述可复用组件类型，
  instance 描述图中的组件实例，task 描述 instance 的执行单元。
- task 的输入、输出、触发方式、周期、deadline、lane、readiness 和部署目标必须
  在 RSDL/IR 中显式建模，不得由 codegen 暗自猜测。
- graph 反馈环、delay、初始值或时间同步窗口必须先在 RSDL/IR 中显式建模后才能
  支持；codegen 不得把环路隐式解释成合法反馈。
- graph 级 process orchestration 必须和 instance/task 分层建模。`[[process]]`
  描述进程依赖、启动顺序、restart policy 和故障传播；`instance.<name>.process`
  只选择所属 process group。module 文件不声明 process orchestration，composition
  层统一装配。
- Service 是 request/response 语义，不是 dataflow channel。component service
  client/server 端口必须声明 request 和 response 类型，graph 用 `[[bind.service]]`
  显式绑定 client/server；validator 必须校验端口方向、类型匹配和 client 端唯一
  绑定。用户语义不得暴露底层 RPC、session 或 transport API。

## Contract IR 约定

Contract IR 的真实模型应是 Rust 中的强类型 `struct` / `enum`。落盘格式使用
canonical JSON，默认路径为：

```text
flowrt/contract/contract.ir.json
```

IR 生成前必须完成 normalization：

- 展开 imports。
- 解析 names。
- 分配 stable IDs。
- 填充 defaults。
- 规范化不具备顺序语义的集合 ordering。
- 附加 backend capabilities。

Contract IR 必须内建版本和来源字段，包括 `ir_version`、`schema_version`、
`source_hash` 和 `package_id`。validator 必须拒绝当前工具链不支持的 IR、
schema 或 RSDL 版本，避免旧版或未来版本的契约被现有 codegen/runtime 静默误读。

validator 必须独立校验：

- 同一作用域内的实体名称唯一。
- `EntityId` 在同一 Contract IR 内全局唯一，并保持 canonical 形状。
- 所有 `EntityRef` 的 `id` 和 `name` 指向同一个实体。
- 落盘 IR 的集合顺序、重复项和 import kind 保持 canonical。
- deployment、target、channel policy、backend capability 和 derived metadata
  与重新推导结果一致。
- 参数 schema、component 默认参数和 instance 覆盖参数类型兼容，且集合完整。

不要把 RSDL 原文直接转换成浅层 JSON 后称作 IR。Codegen public 入口必须重新运行
Contract IR validator；即使调用方绕过 CLI，也不得让未验证或手工改坏的 IR 进入
生成阶段后触发 panic 或写出半成品。

## 执行模型约定

- 最小生命周期接口保留 `on_init`、`on_start`、`on_stop`、`on_shutdown`。
- 生成的 Rust/C++ runtime shell 只对成功进入对应阶段的组件执行逆序清理：成功
  start 的组件执行 `on_stop`，成功 init 的组件执行 `on_shutdown`。
- scheduler 或前序 hook 失败后仍必须继续清理。原始非 `Ok` 状态优先；原始状态
  为 `Ok` 时，任一清理 hook 失败统一返回 `Error`。
- 一个 instance 可以有多个 task，但生成 shell 不应在没有明确用户 API 边界前为
  每个 task 造新的用户回调形态。
- 参数 pending apply 必须在明确的 scheduler step 边界按 instance 应用，不能因多
  task 重复应用。
- supervisor 只消费 launch manifest 中的 process 编排合同，不回读 RSDL 文本。进程
  依赖顺序、restart policy、故障传播和健康状态必须在 Contract IR / manifest 中
  可校验、可观测，不能写成 supervisor 内部硬编码常量。
- 默认输入语义是 `latest snapshot`。用户接口必须表达 present/stale 信息，不能把
  缺失输入伪装成有效样本。
- task 声明的每个 active input 必须有且只有一条 incoming dataflow bind；缺失或
  多重绑定由 validator 拒绝，不能让 codegen 隐式传空视图或 panic。
- `periodic` task 由 timer 唤醒，必须声明大于 0 的周期。非周期 task 不得声明无效
  周期字段。
- `on_message` task 由输入 channel 新到达信号或 FIFO backlog 唤醒；transport wake
  probe 只负责刷新 endpoint cache，真正传给用户回调的输入必须从 cached latest
  view 读取，不能二次 receive 消耗样本。
- `startup` task 在组件成功 start 后、scheduler 前调用一次；`shutdown` task 在正
  常停止或 graceful shutdown 后、组件 stop 前调用一次。
- 多 task shell 必须为每个 task 建立独立局部 scope，避免输入、输出、deadline 等
  局部变量重名互相污染。

## 通道语义约定

RSDL 描述 typed data channel。基础 channel policy 为：

```text
latest(depth = 1)
fifo(depth = N)
```

`latest` 只能保留当前 snapshot；如果用户需要 backlog，必须显式使用 `fifo`。
validator 必须拒绝 `latest` 上无意义的多深度配置，避免 codegen 静默忽略策略。

overflow policy 必须显式建模：

```text
drop_oldest
drop_newest
error
block
```

stale data policy 也必须显式建模：

```text
max_age_ms = N
stale_policy = "warn" | "drop" | "hold_last" | "error"
```

overflow 表示队列满，stale 表示数据过期；两者不能混为一个策略。实时路径默认应
避免无界阻塞。

## 消息 ABI 约定

FlowRT Message ABI 的 native ABI 基线支持 fixed-size plain data：

- integers
- floats
- bool
- fixed arrays with `N > 0`
- nested structs

variable frame 使用固定 header + 尾部变长区的 canonical frame codec，承载无界
`bytes`、`string` 和 `sequence<T>`。支持 variable frame 的 backend 可以直接传递
canonical frame；只支持 fixed-size plain data 的 backend 不得重新引入临时 envelope
或兼容承载层。

暂不支持：

- maps
- recursive structures
- language-specific ownership types
- empty message structs

C++/Rust 生成类型必须通过 conformance tests 验证：

- size
- alignment
- field offset
- byte-level roundtrip
- default initialization
- Contract IR-derived expected byte fixtures

跨语言 sample field value 必须保持字节等价，padding 和默认初始化语义不得漂移。

## 跨语言 C ABI 边界

C ABI 是后续 C、Python 和更多语言 runtime/binding 的稳定边界，不是临时 FFI
胶水。新增跨语言 runtime 共享类型时必须遵守：

- C 侧事实源放在 `runtime/cpp/include/flowrt/abi.h`，只定义 POD 类型、整数编码、
  borrowed view 和版本常量，不暴露 C++/Rust 对象、backend SDK 句柄或所有权语义。
- Rust 侧必须提供对应 `#[repr(C)]` 镜像类型和转换函数；C++ 侧必须能直接包含同一
  C header。
- 枚举类语义在 C ABI 中使用固定宽度整数和常量表达，不能依赖 C enum 的实现相关
  大小。
- 可选字段必须使用显式 `has_*` 标志和保留字节，不把 Rust `Option`、C++
  `std::optional` 或语言特定 bool 布局泄漏到 ABI。
- string/bytes 使用借用 view。调用方如果要跨调用保存内容，必须复制；ABI 类型不
  承担分配、释放或生命周期延长责任。
- ABI 版本常量改变前必须说明兼容性影响，并同步 Rust/C++ layout 测试、文档和
  changelog。
- C/Python 支持应复用该边界逐步实现；在语义未定前不要引入 Python binding、
  动态插件加载或新构建系统。

## Backend 约定

Backend capability 必须显式建模。validator 必须拒绝未知 backend 名称，以及 profile
backend 或 route backend 无法满足的 contract。未知 backend 不得因为没有被当前
profile 选中而在 target backend 列表中静默保留。

Backend 相关的 policy source、backend source、capability requirements、target
capabilities、deployment requirements 和 satisfied 状态都是派生或记录字段。
validator 必须重新推导并拒绝不一致值，不能信任落盘 IR 中可被手工改写的派生元数据。

业务层只依赖 FlowRT runtime API。新增 backend 时必须保持：

- RSDL 语义不泄漏底层 SDK。
- Contract IR 记录能力和选择结果。
- Rust/C++ runtime 行为可对齐。
- generated manifest、自描述和诊断能解释 backend 选择。
- 缺失依赖时给出可执行错误，而不是在生成 CMake 或 runtime shell 中隐式联网拉取。

## Codegen 边界

生成代码只能做 glue：

- 消息定义。
- 组件接口。
- runtime shell 入口。
- backend 绑定。
- 启动配置。
- 构建文件。

组件接入类型必须显式区分：

```text
native component
io_boundary component
external process component
```

`native` 表示用户直接实现 FlowRT 生成接口；`io_boundary` 表示进程内自研 I/O、
副作用和资源访问边界；`external process` 表示 FlowRT 启动或连接已有外部进程。
尚未定义完整生命周期、端口绑定和错误传播语义的接入类型必须由 validator 明确拒绝。

Mixed contract 必须保持语言边界诚实：Rust codegen 不得为 C++ component 伪造 Rust
trait，C++ codegen 不得为 Rust component 伪造 C++ interface。同一 process group
内是否允许混合语言、跨 process route 是否允许某 backend，必须由 validator 和
runtime readiness 明确判断。

## 技术选型

- 基础选型：C++20 + Rust 2024 Edition。
- 选择较新的语言版本不是摆设；实现 runtime API 和工具链时应优先使用能提升语义
  清晰度和安全边界的现代特性。
- 性能优先但不过度设计：实现任何组件时，在不过度设计的前提下追求最高性能、最小
  延迟和最现代化特性。过度设计指在完全用不到的地方堆优化，例如只有单读单写的场景
  使用无锁队列。是否上高性能手段先看访问模式、并发基数和是否热路径；并发基数应从
  契约推导，例如 active input 单一 incoming bind 即单生产者。
- pre-1.0 大胆革新，不打兼容胶水：v1.0.0 是 ABI/schema 兼容冻结线，在那之前（且项目
  尚无用户）正确性与设计优雅优先于向后兼容。出现更优方案时应彻底改写旧实现，并连带
  更新所有消费者、golden 和示例，按 Conventional Commits 标 `!` 或 `BREAKING CHANGE:`；
  不在旧设计上叠兼容层制造技术债。仍先尽量一次设计对（见长期模型优先），但发现更优解
  时不被旧实现绑死。v1.0.0 起兼容才成硬约束。
- 工具链和开发者工具使用 Rust。
- C++ 承载 runtime core、backend bindings、FlowRT 管理的 runtime shells 和用户侧
  组件 API。
- C++ build 优先使用 CMake；Rust build 使用 Cargo workspace。
- 生成的混合工程由 `flowrt` CLI 统一调度。
- C++ only contract 不得依赖 Cargo app 路径；含 Rust 用户组件的 contract 才应触发
  Cargo 构建。

## 文档规范

所有项目文档使用中文维护，包括 `README.md`、`docs/*.md`、`AGENTS.md`、
`CONTEXT.md` 和 `CHANGELOG.md`。代码标识符、命令、配置键、协议名和必要专有术语
可以保留英文。代码注释按对应语言代码风格维护，但正文也应优先使用中文。

任何影响语义、接口、目录结构、命令、生成物边界、版本背景或发布流程的变更，都
必须同步更新配套文档，或在最终回复说明不更新原因。

入库维护：

- `README.md`：项目入口和简短定位。
- `CHANGELOG.md`：阶段性变更记录和 GitHub Release notes 事实源。
- `CONTEXT.md`：当前仓库状态、CLI 状态、CI/Release 状态、已实现能力和近期版本背景。
- `docs/README.md`：入库配套文档索引和文档边界。
- `docs/getting-started.md`：面向用户的快速开始。
- `docs/cli.md`：CLI 命令、选项和生成物边界。
- `docs/examples.md`：示例矩阵和运行要求。
- `docs/development.md`：开发验证、文档维护和提交规则。
- 其他入门、操作指南、维护说明等配套文档：随相关代码或行为变化入库。

不入库维护：

- 阶段计划。
- 架构计划。
- RSDL、Contract IR、Message ABI、Backend 等设计草案。
- 临时调研记录和一次性验证记录。

本地阶段计划和设计草案可以放在 `docs/` 下，但不得加入 Git 索引。提交前必须确认
没有把 `.gitignore` 明确排除的本地设计/规格文件加入索引。

文档要写可执行决策，不写空泛愿景。架构取舍必须说明原因。

### CHANGELOG 维护规范

`CHANGELOG.md` 是 GitHub Release notes 的事实源，不能随意改格式。推送 `v*` tag
时，release 流程会从 `CHANGELOG.md` 抽取对应版本段。

必须遵守：

- 每个正式发布版本必须有独立二级标题，格式固定为 `## vX.Y.Z - YYYY-MM-DD`。
- tag 名必须是 `vX.Y.Z`，且版本号必须与根 `Cargo.toml` 中的 workspace version 一致。
- 发布前必须把待发布条目从 `## 未发布` 移入对应版本段。
- 版本段内部优先使用三级标题 `### 新增`、`### 修复`、`### 变更`、`### 测试`。
- 条目必须是面向用户或维护者的具体变化，不写空泛总结。
- 不要把 release notes 写在 `Latest`、`Release`、日期-only heading、四级标题或自由
  文本段落下。
- `## 未发布` 只用于尚未 tag 的后续变化，且应位于最新正式版本段上方。

## 代码风格

根目录的 `.clang-format` 和 `rustfmt.toml` 是格式化的权威来源。若规则与口头约定
冲突，以配置文件为准。

### 通用

- 以自动格式化工具为准，不手工对齐、补空格或维护视觉列宽。
- 源文件默认保持 ASCII；只有在已有语义明确、确实必要时才引入非 ASCII 内容。
- 不要用格式去掩盖设计问题。阅读困难时优先重构命名、函数边界和数据结构。

### Rust

- `cargo fmt` 是权威格式化来源。
- `cargo clippy --all-targets --all-features -- -D warnings` 是主要静态检查。
- 默认遵循 rustfmt 风格，不手动排列 import、字段或链式调用。
- 公共 API 和 crate 级能力边界用中文 `///` 文档注释说明契约、输入、输出、错误、
  所有权和副作用。
- 代码内注释只用于解释不明显的意图、约束、unsafe 边界、后端差异和关键算法选择。
- 注释正文使用中文；代码标识符、类型名、trait 名、协议名和必要专有术语可以保留英文。

### C++

- `clang-format` 是权威格式化来源。
- 使用 4 空格缩进、`BreakBeforeBraces: Attach`、约 100 列目标宽度、禁止 tabs。
- C++ 头文件分组约定为：`flowrt/...` 运行时头文件、`flowrt_app/...` FlowRT 管理的
  应用头文件、其他 quoted 头文件、`<...>` 外部或系统头文件。
- 公共 runtime API 用高质量中文 Doxygen 风格注释说明契约、参数、返回值、错误语义、
  生命周期和所有权。
- 注释只解释意图、约束和边界条件，不重复代码表面动作。
- 注释正文使用中文；代码标识符、类型名、协议名和必要专有术语可以保留英文。

### Markdown / Docs

- 标题短，层级清楚。
- 示例尽量可解析、可运行、可验证。
- 术语保持统一：RSDL、Contract IR、runtime shell、backend、message ABI、component、
  instance、task、channel。
- 文档和 changelog 正文使用中文；只有代码标识符、命令、配置键、协议名、专有名词
  和必要引用保留英文。

### FlowRT 管理代码

- FlowRT 管理的应用产物只保留必要的机器标记和来源说明。
- 不在 FlowRT 管理的应用产物里维护手写解释性注释。
- 可读性通过命名、结构和分层保证，不通过大量注释补救。

## 验证要求

每次修改后运行与改动匹配的最小验证。

纯文档变更至少运行：

```bash
git diff -- AGENTS.md CONTEXT.md
git status --short
```

Rust 代码变更按影响范围运行：

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

C++ runtime 变更按影响范围运行：

```bash
cmake -S runtime/cpp -B build/cpp
cmake --build build/cpp
ctest --test-dir build/cpp --output-on-failure
```

如果无法运行验证，最终回复必须明确说明原因。

## 发布收尾流程

正式发布走 `dev/vX.Y.Z` 开发分支加 tag。在该分支上按顺序收尾，任一步失败先修再继续：

1. 改动已原子提交在 `dev/vX.Y.Z`，工作树干净。
2. 跑本地收尾门禁，全绿才继续：
   - `cargo fmt --check`、`cargo test`、`cargo clippy --all-targets --all-features -- -D warnings`。
   - `scripts/check-architecture-contract.sh`、`scripts/check-architecture-size.sh`。
   - tracked 守卫：确认 `flowrt/**`、`examples/*/flowrt/**` 和 `.gitignore` 排除的本地设计草案未进索引。
   - `scripts/check-release-readiness.sh X.Y.Z`：版本一致性、CHANGELOG 段、release notes、
     focused gate、README/CONTEXT 和 tag 状态的总闸，必须输出全部检查通过。
3. 版本与发布物料同步到 `X.Y.Z`：根 `Cargo.toml`、`runtime/rust/Cargo.toml`、
   `runtime/cpp/CMakeLists.txt`、`Cargo.lock`、`README.md`、`CONTEXT.md`；CHANGELOG 把
   `## 未发布` 定为 `## vX.Y.Z - YYYY-MM-DD`；`scripts/release-gates/registry.toml` 登记该
   版本的 focused smoke（脚本须存在并自身通过），并在 `ci.yml` 接入对应 job。
4. push `dev/vX.Y.Z`，等 push CI 全矩阵和 `Release Evidence Gate` 跑绿。`Release Evidence
   Gate` 只在 `dev/vX.Y.Z` push 上运行，按分支名推版本号，校验后上传 deb 与
   `flowrt-release-evidence`。
5. 在那条已跑绿的 commit 上打 `vX.Y.Z` tag 并 push。`release.yml` 只监听 `v*` tag，按 tag
   指向的 commit SHA 找同 SHA 且 `Release Evidence Gate` 成功的 push CI run，下载并复核
   version/tag/sha/deb/校验和后创建 GitHub Release。tag 必须指向已具备 evidence 的 commit，
   否则 release 失败。
6. 合并 `dev/vX.Y.Z` 到 master 是惯例收尾；发布机制按 SHA 关联 evidence，与是否合并无关。

CI 未绿前不得打 tag；不得跳过 readiness 门禁直接发布。

## Git 提交规范

提交必须原子化。一次提交只做一类相关改动；不要像历史上的超大提交那样把多个不相
关功能、修复、文档和发布准备混入同一次提交。Agent 创建提交时不能只写标题，必须
写正文说明关键动机、边界和验证。

采用 **Conventional Commits** 格式，提交信息使用中文：

```text
<type>(<scope>): <中文标题>

<正文>
```

格式要求：

- `type` 和 `scope` 使用英文。
- 标题和正文使用中文。
- 标题不超过 50 个中文字符，不加句号。
- 正文每行不超过 72 个字符。
- 正文可用 `-` 列要点，说明做了什么、为什么做、如何验证。
- 涉及破坏性变化时按 Conventional Commits 规则使用 `!` 或 `BREAKING CHANGE:`，
  但说明文字仍使用中文。
- `scope` 用小括号标注影响范围，通常是包名或模块名；影响全局时可省略。

类型：

| 类型 | 说明 | 示例 |
|---|---|---|
| `feat` | 新功能 | `feat(rsdl): 增加 module 声明解析` |
| `fix` | Bug 修复 | `fix(cli): 修复 lock 残留后的构建判断` |
| `docs` | 仅文档变更 | `docs(readme): 更新安装后使用说明` |
| `refactor` | 重构，不改变行为 | `refactor(ir): 拆分 backend 能力推导` |
| `test` | 添加或修改测试 | `test(parser): 补充 workspace 边界用例` |
| `chore` | 构建、配置或工具类 | `chore(scripts): 整理 deb smoke 脚本` |
| `perf` | 性能优化 | `perf(runtime): 减少 echo 热路径分支开销` |
| `style` | 格式调整，不影响逻辑 | `style: 统一 C++ include 顺序` |
| `ci` | CI 或发布流水线 | `ci(release): 拆分 deb 发布 gate` |
| `build` | 构建系统或打包 | `build(package): 内嵌 zenoh C++ SDK` |
| `revert` | 回滚提交 | `revert: 撤销错误的模块命名变更` |

完整示例：

```text
docs(agents): 重构 agent 长期规范

- 将阶段性状态迁移到 CONTEXT.md
- 补充中文 Conventional Commits 约束
- 明确入库文档和本地设计草案边界
```

```text
fix(ir): 拒绝非 canonical 的 target backend 顺序

- 在 validator 中重新推导 target capability
- 覆盖手工篡改 IR 的回归用例
- 验证 cargo test -p flowrt-validate
```

禁止：

- 英文提交信息。
- 模糊标题，例如 `update`、`fix bug`、`修改`、`misc`、`fix stuff`。
- 多个不相关改动混入一次提交。
- 没有 Conventional Commits 前缀的裸文字提交。
- Agent 提交只有标题、没有正文。
