# 变更日志

这里记录 FlowRT 项目的重要变更。

Git 历史使用 Conventional Commits；凡涉及代码、文档、命令、接口或生成物边界的变化，都要同步维护本文件。

## 未发布

### 新增

- 增加 `flowrt --profile <name>` 显式 profile 选择，CLI 会按选定 profile 投影 Contract IR 并生成对应 backend 的产物。
- 增加 `examples/profile_switch_demo`，用于验证同一份 RSDL 通过 `--profile iox2` 在 `inproc` 与 `iox2` 之间切换。
- 增加 `examples/import_demo` 的 `graphs/*.rsdl` 片段拆分，验证 `package.imports` 可以把实例、task 和 bind 挪到独立的 graph 文件中。
- 增加 RSDL `[package.imports]` 文件导入展开，支持相对路径和 `*` 通配导入模块化 `.rsdl` 片段。
- 增加 `examples/import_demo`，用于验证模块化 RSDL package 的 CLI `check` 路径。
- 增加 RSDL 命名规则校验，validator 会拒绝不符合 `snake_case` / `PascalCase` 约定的 package、type、component、instance、profile、target、field 和 port 名称。
- 增加 instance 参数 override 类型一致性检查，归一化阶段会拒绝与 component 默认参数类型不兼容的覆盖值。
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

### 变更

- 重建 Git 仓库基线，明确设计和规格文档在本地 `docs/` 目录维护但不入库。
- 更新项目文档和 agent 指南，使其反映当前 CLI、runtime 和 codegen 状态。
- 明确用户主入口是安装后的 `flowrt` 命令，`cargo run -p flowrt-cli -- ...` 只作为仓库开发者调试方式。
- 补充 Rust/C++ runtime 对 `StalePolicy::HoldLast` 和 `OverflowPolicy::Block` 的共享行为测试，继续收紧 Phase 4 的 runtime 行为矩阵。
- 明确 C++ only contract 的 `flowrt build` 不应依赖 Cargo，而应通过 CMake 构建 FlowRT 管理的 C++ shell、app 和 ABI test target。
- `flowrt build` 在 contract 含 C++ 组件时会同时调度 generated CMake 工程，构建 C++ managed shell、app 和 ABI test target。
- C++ app target 改为仅在用户提供 `src/cpp/*.cpp` 或显式设置 `FLOWRT_USER_CPP_SOURCES` 时生成，避免没有用户实现时链接出不可用可执行文件。
- 生成的 C++ app main 支持 `--process <name>`，与 CLI `flowrt run --process <name>` 对齐。
- C++ only generated shell 现在会把 bind-level `max_age_ms` / `stale_policy` 编译进 latest channel 初始化，调度步骤使用 timestamped publish/read，并在 `stale_policy = "error"` 时于调用用户组件前返回 `flowrt::Status::Error`。
- CI demo smoke 改为：C++ only demo 执行 build/run，mixed `imu_demo` 只执行 build，Rust-only `import_demo` 执行 run/launch，`mixed_iox2_demo` 与 `imu_demo_iox2` 执行 check，避免在基础 CI 中强制安装可选 `iceoryx2-cxx`。
- GitHub Actions CI 升级到 `actions/checkout@v6` 和 `actions/upload-artifact@v7`，使用原生 Node.js 24 actions，提前规避 hosted runner 上 Node.js 20 deprecation warning。
- 统一项目文档和 `CHANGELOG.md` 的维护语言为中文。
- FlowRT 管理的应用产物继续放在可见的 `flowrt/` 根目录下，同时保持用户代码隔离。

### 修复

- 选择 profile 时，`prepare` / `build` / `run` / `launch` 现在会先投影到对应 profile 再校验和生成，`contract.ir.json` 也只保留该 profile 的 deployment 视图。
- 修正 Rust runtime shell 拓扑排序，同一 source instance 到同一 target instance 的多条 bind 只计为一条实例依赖。
- 修正生成 supervisor 从自身 binary 名称推导 app binary 名称的逻辑。
- 修正 `.gitignore`，避免 `runtime/cpp/include/flowrt/...` 这类仓库源码路径被通用生成物规则误忽略。
