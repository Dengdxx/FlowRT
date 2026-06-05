# 变更日志

这里记录 FlowRT 项目的重要变更。

Git 历史使用 Conventional Commits；凡涉及代码、文档、命令、接口或生成物边界的变化，都要同步维护本文件。

## 未发布

### 修复

- 修复 C++ only `flowrt launch` 的 supervisor-only Rust crate 缺少 `rust/src/selfdesc.rs` 的问题，避免生成 crate 引用缺失模块导致 CI demo smoke 失败。
- 修复 CI demo smoke 对持续运行 runtime shell 的假设：generated Rust/C++ shell 支持 `FLOWRT_RUN_TICKS=N` 有界运行，CI 在 smoke step 中设置该环境变量，使 `run` / `launch` 能稳定退出。
- 修复 CI demo smoke 在 clean checkout 下直接运行 `examples/import_demo` 时遗漏 `build` 步骤的问题，避免 `run` / `launch` 依赖本地残留产物。

### 新增

- 增加 FlowRT 静态自描述产物：codegen 会生成 `flowrt/selfdesc/selfdesc.json`，并把同一份 canonical JSON 嵌入 Rust/C++ 生成应用的 `.flowrt.selfdesc` 二进制 section，为后续 `flowrt list`、`status` 和 `echo` 提供部署后可自查的静态拓扑与 Message ABI layout 事实源。
- 增加 `flowrt list` 和 `flowrt nodes` 的静态自描述读取路径，可从生成应用二进制的 `.flowrt.selfdesc` section 或 `flowrt/selfdesc/selfdesc.json` 输出 package、graph、instance、task、channel 和 Message ABI 摘要。
- 增加 Rust runtime introspection Unix socket 协议和 `flowrt status` live discovery：CLI 会扫描当前用户 socket 目录，连接后读取 handshake 与 live state 中的 tick/channel 摘要，socket 文件名只作为发现入口。
- 增加 Rust runtime `IntrospectionState` 与 channel snapshot 请求基础：runtime shell 可以共享更新 tick 计数和 raw ABI payload snapshot，snapshot 展示仍需后续 `flowrt echo` 结合静态 self-description 的 Message ABI layout 接入。
- 增加 C++ runtime introspection socket API 基础：支持与 Rust wire JSON 兼容的 `status`、`channel_snapshot`、未知 channel 结构化错误响应，以及 `$XDG_RUNTIME_DIR/flowrt/<pid>.sock` / `/tmp/flowrt.<uid>/<pid>.sock` 路径规则。
- 增加 `flowrt echo <selfdesc-or-binary> <channel> [--socket <path>]`：CLI 会用静态 self-description 校验 channel 与 Message ABI layout，通过 runtime socket 读取一次 latest raw ABI bytes、`published_count` 和 `published_at_ms`，并在自动发现到多个同 hash 进程时要求显式指定 socket。
- 增加生成 Rust runtime shell 的 channel live stats/snapshot 写入：shell 会为当前 process 的 active channel 注册 canonical channel 名和 message type，并在成功发布输出后记录 latest raw ABI payload 与发布 timestamp。
- 增加生成 C++ runtime shell 的 channel live stats/snapshot 写入：shell 会启动 PID 命名 introspection socket，注册当前 process active channel，并在成功发布输出后记录 latest raw ABI payload 与发布 timestamp。
- 增加 `flowrt echo --follow [--interval-ms <ms>]`：CLI 会持续轮询同一 channel snapshot，首条必输出，后续仅在发布计数、发布时间或 raw payload 变化时输出。
- 增加可入库配套文档：`docs/README.md`、`docs/getting-started.md`、`docs/cli.md`、`docs/examples.md` 和 `docs/development.md`，把快速开始、CLI、示例矩阵和开发维护规则从本地设计文档中拆出。
- 增加 `flowrt --profile <name>` 显式 profile 选择，CLI 会按选定 profile 投影 Contract IR 并生成对应 backend 的产物。
- 增加 `examples/profile_switch_demo`，用于验证同一份 RSDL 通过 `--profile iox2` 在 `inproc` 与 `iox2` 之间切换。
- 增加 `examples/import_demo` 的 `graphs/*.rsdl` 片段拆分，验证 `package.imports` 可以把实例、task 和 bind 挪到独立的 graph 文件中。
- 增加 RSDL `[package.imports]` 文件导入展开，支持相对路径和 `*` 通配导入模块化 `.rsdl` 片段。
- 增加 `examples/import_demo`，用于验证模块化 RSDL package 的 CLI `check` 路径。
- 增加 RSDL 命名规则校验，validator 会拒绝不符合 `snake_case` / `PascalCase` 约定的 package、type、component、instance、process、profile、target、field 和 port 名称。
- 增加 instance 参数 override 类型一致性检查，归一化阶段会拒绝与 component 默认参数类型不兼容的覆盖值。
- 收紧数组参数 override 校验：空默认数组现在只能被空数组覆盖，避免 instance 在没有默认元素类型样本时临时定义数组元素类型。
- 增加 dataflow graph 环路校验，validator 会拒绝 instance 间闭环和 self-loop，避免 codegen 处理未定义反馈语义。
- 增加 C++/Rust 组件接口生成注释：C++ 接口生成中文 Doxygen 注释，Rust trait 生成中文 Rustdoc 注释，明确生命周期、输入视图、输出句柄和返回状态契约。
- 增加 C++ managed runtime shell 骨架、C++ main 入口和 CMake shell/app target，为 Phase 2A C++ inproc demo 提供可构建基础。
- 增加 FlowRT Rust 工具链基建：RSDL 解析、Contract IR 归一化、校验、代码生成、CLI `prepare` / `build` / `run` / `inspect` 闭环、Rust runtime 基础类型，以及 ABI conformance 生成。
- 增加 `flowrt/launch/launch.json` 中按 process 分组的启动元数据。
- 增加生成 Rust 应用的 process 入口，并支持 `--process <name>`。
- 增加生成 Rust supervisor 和 `flowrt launch` 命令，用于 process 分组启动 smoke test。
- 增加 feature-gated 的 Rust `iox2` typed pub/sub runtime 支持。
- 增强 Rust runtime shell 生成：调度步骤按 `TaskIr.inputs` / `TaskIr.outputs` 控制 channel 读取和发布，未参与当前 task 的输入以空 `Latest` 传入完整组件 trait。
- 增加 C++ only inproc runtime shell 生成：生成 `App` 注入接口、组件生命周期调度、latest/FIFO channel 转发和 `flowrt_user::build_app()` 用户工厂入口。
- 增加 `examples/cpp_counter_demo`，用于验证只写 C++ 用户逻辑时的 `flowrt build` + CMake 构建路径。
- 增加 C++ only `flowrt run` 路径：CLI 会构建并直接运行 CMake 产出的 C++ app executable，支持 `--process <name>` 参数。
- 增加 C++ only runtime shell 的 process group 分发：`run_process` 会按 Contract IR 中声明的 process 名称调用对应 step/run 函数。
- 增加 C++ only `flowrt launch` 路径：codegen 会生成 supervisor-only Rust crate，CLI 会先构建 CMake app，再运行 supervisor 启动 C++ process group。
- 增加 GitHub Actions CI 雏形：运行 Rust fmt/test/clippy、C++ runtime CMake/CTest、FlowRT demo smoke，并上传 Linux `flowrt` release binary artifact。
- 增加 C++ runtime 的 latest stale freshness 语义：`StaleConfig` 使用 C++20 `std::chrono::milliseconds` 表达时间窗口，`LatestChannel<T>` 支持 `publish_at` / `view_at`，并与 Rust runtime 的 `warn`、`drop`、`hold_last`、`error` 策略保持一致。
- 增加 mixed contract 语言边界校验：Rust codegen 不再为 C++ component 生成 Rust trait，C++ codegen 不再为 Rust component 生成 C++ interface，语言分离 process group 可以生成各自真实 runtime shell。
- 增强 `flowrt/launch/launch.json`：process group 现在包含 `runtimes` 和 `runtime_kind`，graph instance 也包含 `runtime`，为后续 mixed C++/Rust supervisor 分流打基础。
- 增加 mixed runtime readiness 分类：CLI 会拒绝同进程 C++/Rust 混合和 `inproc` 跨进程混合，并允许 language-separated mixed contract 在 `iox2` backend 下进入运行路径。
- 增强生成的 Rust supervisor：读取 launch manifest 的 `runtime_kind`，为 Rust process 选择 Rust app executable，为 C++ process 选择 C++ app executable，并继续拒绝 mixed process group。
- 收窄 C++/iox2 backend readiness 保护：C++ only `iox2` contract 不再被 CLI 主动拒绝，language-separated mixed `iox2` contract 可通过 supervisor 分流启动 Rust/C++ app。
- 增强 `flowrt/launch/launch.json`：graph 现在包含 `channels` 列表，记录每条 bind 的 backend、channel policy 和 iox2 service name，为 C++/Rust 跨进程通信共享同一 transport 契约打基础。
- 增加 C++ iox2 transport 契约准备：profile 选择 `iox2` 时，生成的 C++ 消息 struct 会带 `IOX2_TYPE_NAME`，生成的 CMake 会声明 `iceoryx2-cxx 0.9.1` 依赖并链接官方 C++ target。
- 增加 C++ runtime 的真实 `flowrt::iox2::Iox2PubSub<T>` binding：定义 `FLOWRT_HAS_ICEORYX2_CXX` 并链接 `iceoryx2-cxx` 时，C++ endpoint 会打开 typed IPC service，通过 `FlowRTIox2Header` user header 携带 runtime timestamp，并支持 loopback publish/receive smoke；默认未启用宏时仍安全返回 `ChannelError::Transport`。
- 增加 iox2 跨语言 type-name 对齐：Rust 消息在 profile 选择 `iox2` 时生成 `#[type_name("...")]`，C++ 和 Rust runtime 共享 `FlowRTIox2Header` user header 名称，transport timestamp 不再包进 payload envelope。
- 增加 `flowrt run --process <name>` 对 mixed contract 的语言分流：Rust process 运行 Rust app，C++ process 运行 CMake app；未指定 process 时提示使用 `flowrt launch` 启动全部 process group。
- 增加 `examples/mixed_iox2_demo`，用于演示 Rust source 与 C++ sink 通过 iox2 分进程连接的 mixed contract。
- 增加 `examples/imu_demo_iox2`，用于演示主 demo 的 Rust source、C++ controller 和 Rust monitor 可以通过 iox2 分进程运行。
- 增强 Rust/C++ message ABI conformance：生成测试现在包含同一份 Contract IR-derived expected byte fixtures，用于在各自语言测试中验证 sample field value 的跨语言字节等价性和 padding zero 语义。
- 增强 Rust/C++ message ABI conformance：生成测试现在会显式断言默认初始化后的整对象 bytes 全零，覆盖 padding bytes 的 deterministic default initialization 契约。
- 增加未来有界变长类型的 Contract IR 表达：RSDL type expression 可解析 `bytes<max=N>`、`string<max=N>` 和 `sequence<T,max=N>`，并要求 `max > 0`，为后续 Variable Frame ABI 保留结构化语义。
- 增加 CI 防回归检查，确认本地规格草案和 FlowRT 生成物没有被误加入 Git 索引。

### 变更

- 统一 deployment satisfaction 判定：normalizer 和 validator 现在共用 `flowrt-ir` 的 typed deployment capability decision，公开 Contract IR JSON schema 保持不变。
- 收敛 process 边界 capability 派生：跨 process group 的 dataflow 现在会推导 `topology:multi_process`，让 validator、normalizer 和 CLI 对 inproc 跨进程边界共享同一套部署判定。
- 统一 Rust/C++ runtime backend capability 报告顺序，使 runtime API 与 Contract IR typed catalog 的全局 canonical 顺序一致，减少后续自描述、诊断和跨语言对比中的漂移。
- 生成的 Rust/C++ runtime shell 改为启动基于 `IntrospectionState` 的 status server，并在 scheduler tick 入口记录 live tick 计数；channel 发布统计与 payload snapshot 会在成功发布输出后写入 live state。
- 收窄 runtime channel snapshot 响应：snapshot 只返回 raw ABI bytes、发布计数和发布时间，channel 名称、message type 与业务字段 layout 继续由静态 self-description 提供。
- 重建 Git 仓库基线，明确设计和规格文档在本地 `docs/` 目录维护但不入库。
- 重写 `README.md` 为更聚焦的项目入口，优先回答 FlowRT 是什么、当前能跑什么、用户需要写什么、生成物边界在哪里，以及哪些配套文档应随代码入库；详细 CLI、示例和开发维护内容分流到 `docs/`。
- 更新项目文档和 agent 指南，使其反映当前 CLI、runtime 和 codegen 状态。
- 明确用户主入口是安装后的 `flowrt` 命令，`cargo run -p flowrt-cli -- ...` 只作为仓库开发者调试方式。
- 同步开发文档中的 FlowRT demo smoke 命令，使其覆盖当前 CI 使用的 `imu_demo` build 和 `profile_switch_demo` profile 切换验证。
- 补充 Rust/C++ runtime 对 `StalePolicy::HoldLast` 和 `OverflowPolicy::Block` 的共享行为测试，继续收紧 Phase 4 的 runtime 行为矩阵。
- 补充 Rust/C++ `iox2` 适配层对 `HoldLast` 与 `Block` 的配置断言，确保 backend 配置不会在生成/运行边界被吞掉。
- 让 Rust `iox2::Iox2PubSub` 暴露可查询的 QoS 配置，和 C++ 侧 `config()` 观察接口保持一致。
- 收紧 task 输入绑定校验：validator 现在会拒绝 task 声明的 active input 没有 incoming `bind.dataflow`，避免 runtime shell 对已声明输入隐式传空视图或在 codegen 阶段 panic。
- 收紧 task 端口集合校验：validator 现在会拒绝 `input` 或 `output` 列表中的重复端口，避免 codegen 与 launch manifest 对重复项产生不同解释。
- 收紧 Contract IR 实体名称唯一性校验：validator 现在会拒绝顶层 type、component、profile、target、graph 和 graph 内 instance 的重复名称，不再依赖 parser 间接保证唯一性。
- 收紧 Contract IR 实体身份校验：validator 现在会拒绝全局重复 `EntityId`，并要求 `EntityRef` 的 `id` 与 `name` 对同一个实体保持一致，避免落盘 IR 被手工篡改后出现引用歧义。
- 收紧 Contract IR 版本兼容性校验：validator 现在会拒绝当前工具链不支持的 `ir_version`、`schema_version` 和 package `rsdl_version`，避免不兼容契约进入 codegen/runtime。
- 收紧 deployment 完整性和派生元数据校验：validator 现在要求每个 graph/profile/target 组合恰好一行，重新推导 bind、target 和 deployment 的 capability 集合，校验 deployment backend 与 profile 一致，并拒绝伪造的 `satisfied` 状态。
- 收紧 Contract IR profile 形状校验：validator 现在会拒绝空 profiles 列表，避免把本应由 normalization 插入的隐式 `default` profile 丢失到落盘 IR 中。
- 收紧 Contract IR 参数校验：validator 现在会拒绝重复参数名、缺失或多余的 instance 参数，以及与 component 默认值类型不兼容的实例覆盖。
- 收紧 Contract IR target 列表校验：validator 现在会拒绝重复的 target runtime 或 backend 条目，避免手工 JSON 破坏 canonical 形状。
- 收紧 Contract IR canonical 字段校验：validator 现在会拒绝非 canonical 的 `source_hash` 和 `EntityId` 形状，避免手工 JSON 破坏稳定 ID 和缓存键。
- 规范化 Contract IR bind ordering：`bind.dataflow` 现在按连接端点稳定排序，仅调整 bind 声明顺序不会再改变 canonical IR 和生成物顺序。
- 规范化 Contract IR target set ordering：`target.runtime` 和 `target.backends` 现在稳定排序，并基于排序后的 backend 列表派生 capability；仅调整列表声明顺序不会再改变 canonical IR。
- 规范化 Contract IR import ordering：`package.imports` 的 pattern 列表现在稳定排序，仅调整 import pattern 声明顺序不会再改变 canonical IR。
- 收紧 Contract IR canonical ordering 校验：validator 现在会拒绝非 canonical 的 `package.imports` pattern、`bind.dataflow`、`target.runtime` 和 `target.backends` 顺序。
- 收紧 Contract IR import 集合校验：validator 现在会拒绝落盘 IR 中重复的 `package.imports` kind 或 pattern，避免手工 JSON 绕过 import 集合语义。
- 收紧 Contract IR import kind 校验：validator 现在会拒绝落盘 IR 中不属于 `types` / `components` / `graphs` / `profiles` / `targets` 的 import kind。
- 收紧 Contract IR 实体集合排序校验：validator 现在会拒绝非 canonical 的顶层实体数组、component/instance 参数数组、graph instance/task 数组和 deployment 数组顺序。
- 收紧 Contract IR 派生 capability 顺序校验：validator 现在要求 `capability_requirements`、`capabilities` 和 `required_capabilities` 与重新推导的能力列表完全一致，而不只比较集合。
- 收紧 Contract IR channel policy 来源元数据校验：validator 现在会拒绝手工改写的 `policy_source` 与默认 profile 策略不一致的未投影或已投影 IR。
- 收紧 codegen 入口边界：`emit_artifacts` 现在会先运行 Contract IR validator，未验证或被手工改坏的 IR 会返回结构化错误，而不是进入生成阶段后触发 panic。
- 收紧 RSDL 保留命名空间校验：validator 现在会以大小写不敏感方式拒绝 `flowrt` 前缀，避免 `FlowrtSample` 这类 PascalCase 名称占用 FlowRT 管理命名空间。
- 收紧 Contract IR v0.1 task 数量约束：validator 现在会拒绝同一 instance 出现多个 task，避免 codegen 在 `instance ~= task` 阶段静默只消费第一条 task。
- 收紧 component kind 支持边界：`external` process component 在外部进程语义落地前会被 validator 拒绝，避免 codegen 伪生成 native 接口。
- 收紧 latest channel depth 校验：`latest` bind 只能省略 `depth` 或显式设置 `depth = 1`，避免 codegen 静默忽略不支持的 latest backlog。
- 收紧 backend 名称校验：validator 现在会直接拒绝 `profile.<name>.backend` 和 `target.<name>.backends` 中的未知 backend 名称，而不是只在 deployment 组合阶段发现 selected backend 错误。
- 明确无显式 profile 时的默认 backend 语义：normalization 会插入隐式 `default` profile，backend 为 `inproc`，使 target/backend deployment 约束仍能被 validator 校验。
- 收紧 periodic task 周期校验：`period_ms = 0` 现在会被 validator 拒绝，避免生成 shell 消费不可执行的零周期声明。
- 收紧 RSDL parser 顶层 section 校验：未知顶层 section 现在会被拒绝，避免 `[components.*]` 这类拼写错误被静默忽略。
- 收紧 RSDL parser 固定 schema 字段校验：`package`、`component`、`instance`、`task`、`bind.dataflow`、`profile` 和 `target` 中的未知字段现在会被拒绝，避免拼写错误被默认值掩盖；message fields 和 `params` 仍保持开放。
- 收紧 Message ABI v0.1 校验：空 message type 现在会被 validator 拒绝，避免 C++/Rust 空 struct size 语义不一致进入 conformance/codegen 路径。
- 收紧 Message ABI v0.1 fixed array 校验：validator 现在会拒绝落盘 Contract IR 中的零长数组，避免 C++ `std::array<T, 0>` 与 Rust `[T; 0]` 布局分裂。
- 收紧 Message ABI v0.1 variable payload 边界：validator 会拒绝 `bytes<max=N>`、`string<max=N>` 和 `sequence<T,max=N>` message 字段并提示需要未来 Variable Frame ABI，conformance helper 和 codegen public 入口也会返回结构化错误而不是继续生成。
- 收紧 Message ABI v0.1 int128 能力边界：消息类型或 component port 使用 `u128` / `i128` 时，deployment 会要求 `abi:int128`，当前 `inproc` / `iox2` backend 不提供该能力，validator 会重新推导并拒绝伪造的 `required_capabilities` 或 `satisfied` 标记。
- 增强生成的 Message ABI conformance：C++ ABI test 会把 Contract IR sample bytes 写入 `flowrt/build/abi-fixtures/cpp/*.bin`，Rust ABI test 在 mixed contract 中会读取这些 C++ fixture 并按 Rust 消息类型重建和比对，补上文件级跨语言 roundtrip 验证。
- 集中 `flowrt-ir` 内部 capability catalog：backend、trigger、channel、stale、overflow 和 message ABI capability 现在由 typed enum 推导成既有 `CapabilityAtom` 字符串，按全局 canonical 顺序去重后输出，保持 IR JSON schema 不变。
- 收紧 task trigger 字段组合校验：非 `periodic` task 现在会拒绝 `period_ms`，避免无效周期字段被 runtime shell 静默忽略。
- 增强 Rust/C++ runtime shell 的 task trigger 语义：`startup` / `shutdown` task 不再退化成每 tick 调用，而是分别在 scheduler 前后各执行一次。
- 增强 Rust/C++ runtime shell 的 `deadline_ms` 语义：带 deadline 的 task 会要求 `timing:deadline_aware` capability，并在用户回调返回 `Ok` 后、发布输出前检查耗时；超出 deadline 时返回 `Error`。
- 增强 `flowrt/launch/launch.json` task metadata：graph-level 和 process-level task 现在都会暴露 `inputs`、`outputs` 和 `priority` scheduler hint，保持与 Contract IR 中已保留的 task 执行端口集合和 priority 字段一致。
- 收紧 Contract IR v0.1 graph 数量约束：validator 和 codegen 现在要求 contract 恰好包含一个 graph，避免 runtime shell 因空 graph panic 或隐式忽略额外 graph。
- 增强生成的 Rust/C++ runtime shell 生命周期清理：只对成功进入 init/start 阶段的组件逆序执行 shutdown/stop，scheduler 或前序 hook 失败后仍完成清理，并保留原始失败状态。
- 增强生成的 Rust/C++ runtime shell `on_message` 触发语义：同步 tick 中只有声明输入至少一个 `present()` 时才调用组件，避免无输入样本时退化成每 tick 调用。
- 收紧 `flowrt launch` 和 `flowrt run --process` 的 backend 边界：`inproc` backend 下的跨 process dataflow 会被拒绝，避免把无法通信的 inproc channel 拆成独立进程或单独 process 运行。
- 明确 C++ only contract 的 `flowrt build` 不应依赖 Cargo，而应通过 CMake 构建 FlowRT 管理的 C++ shell、app 和 ABI test target。
- `flowrt build` 在 contract 含 C++ 组件时会同时调度 generated CMake 工程，构建 C++ managed shell、app 和 ABI test target。
- C++ app target 改为仅在用户提供 `src/cpp/*.cpp` 或显式设置 `FLOWRT_USER_CPP_SOURCES` 时生成，避免没有用户实现时链接出不可用可执行文件。
- 生成的 C++ app main 支持 `--process <name>`，与 CLI `flowrt run --process <name>` 对齐。
- C++ only generated shell 现在会把 bind-level `max_age_ms` / `stale_policy` 编译进 latest channel 初始化，调度步骤使用 timestamped publish/read，并在 `stale_policy = "error"` 时于调用用户组件前返回 `flowrt::Status::Error`。
- CI demo smoke 改为：C++ only demo 执行 build/run/launch，mixed `imu_demo` 只执行 build，Rust-only `import_demo` 执行 run/launch，`mixed_iox2_demo` 与 `imu_demo_iox2` 执行 check，避免在基础 CI 中强制安装可选 `iceoryx2-cxx`。
- GitHub Actions CI 升级到 `actions/checkout@v6` 和 `actions/upload-artifact@v7`，使用原生 Node.js 24 actions，提前规避 hosted runner 上 Node.js 20 deprecation warning。
- 统一项目文档和 `CHANGELOG.md` 的维护语言为中文。
- FlowRT 管理的应用产物继续放在可见的 `flowrt/` 根目录下，同时保持用户代码隔离。

### 修复

- 修正 VSCode clangd 缺少 C++ 编译上下文的问题：仓库新增 `.clangd`，C++ runtime 和生成的 C++ app CMake 都会导出 `compile_commands.json`，示例用户 C++ 文件可通过本示例生成头完成 lint。
- 修正 C++ runtime introspection socket 响应路径在客户端提前关闭连接时可能收到 `SIGPIPE` 并终止进程的问题。
- 修正 C++ only `flowrt launch` 的 supervisor-only Cargo manifest 会被追加未使用 `flowrt` patch 并产生 Cargo warning 的问题。
- 修正多个 `flowrt prepare` / `build` / `run` / `launch` 命令并发写入同一输出目录时可能损坏生成产物的问题；CLI 现在会用 `.flowrt.lock` 对会写生成目录的命令做 fail-fast 保护。
- 修正 Rust/C++ inproc FIFO channel 忽略 bind-level `max_age_ms` / `stale_policy` 的问题；runtime 现在提供 timestamped `push_at` / `pop_at` 读取路径，生成 shell 会把 FIFO stale 配置编入 channel initializer，并在 `stale_policy = "error"` 时继续于用户回调前返回错误。
- 修正生成的 Rust supervisor 只启动 launch manifest 首张 graph 的问题；现在会遍历全部 graph，并在整个 manifest 没有 process group 时明确报错。
- 选择 profile 时，`prepare` / `build` / `run` / `launch` 现在会先投影到对应 profile 再校验和生成，`contract.ir.json` 也只保留该 profile 的 deployment 视图。
- 选择 profile 时，投影后的 Contract IR 现在会重算来自 profile default 的 bind-level `overflow`、`stale_policy` 和 `max_age_ms`，同时保留 bind 上显式声明的 policy，并刷新相关 channel/deployment capability 元数据。
- 修正省略 `--profile` 时未投影默认 profile 的问题；`prepare` / `build` / `run` / `launch` 现在会选择 `default` 或首个 profile，只校验和落盘对应 deployment 视图。
- 修正 Rust runtime shell 拓扑排序，同一 source instance 到同一 target instance 的多条 bind 只计为一条实例依赖。
- 修正生成 supervisor 从自身 binary 名称推导 app binary 名称的逻辑。
- 修正 `.gitignore`，避免 `runtime/cpp/include/flowrt/...` 这类仓库源码路径被通用生成物规则误忽略。
