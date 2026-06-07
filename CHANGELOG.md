# 变更日志

这里记录 FlowRT 项目的重要变更。

Git 历史使用 Conventional Commits；凡涉及代码、文档、命令、接口或生成物边界的变化，都要同步维护本文件。

## 未发布

### 新增

- 调度健康策略接入 runtime shell：deadline miss 阻止 late output 发布、stale input 计数记录、backpressure/overflow 事件进入 health counters、lane 饥饿公平性检测；Rust/C++ 生成 shell 行为一致。
- `DeterministicExecutor` 新增 lane 调度追踪：`set_current_tick()`、`lane_starvation_ticks()` 方法，支持跨 lane 公平性饥饿检测。
- 定义 language-neutral 调度健康模型：`IntrospectionTaskHealth`（deadline miss、stale input、backpressure、overflow、run/success/failure 计数、last run/success 时间戳）和 `IntrospectionLaneHealth`（queue depth、dispatched count）。
- selfdesc schema 新增 `SelfDescriptionTaskHealth` 和 `SelfDescriptionLaneHealth` 类型声明。
- Rust/C++ `IntrospectionStatus` 新增 `tasks` 和 `lanes` 字段，`IntrospectionState` 新增 `record_task_health()` 和 `record_lane_health()` 方法。
- `flowrt status` 展示 task 级和 lane 级调度健康指标。
- 所有新增字段使用 `serde(default)` 保证前向兼容，旧版 JSON 不含健康字段时解析为零值。
- supervisor：实现 `process_started`、`runtime_ready`、`service_ready` readiness gate 和 `startup_delay_ms` 错峰启动；`runtime_ready` 通过 introspection socket 握手判断，`service_ready` 额外检查所有 service endpoint 就绪；readiness 超时或进程退出时终止子进程并结构化报错。
- supervisor：`flowrt status` 展示进程当前等待的 readiness gate 类型（`readiness_wait` 字段）。
- supervisor：`process_dependencies_satisfied` 区分 `process_started`（只需 spawned）和 `runtime_ready`/`service_ready`（需通过 readiness）的依赖满足语义。
- supervisor：支持进程资源提示，包括显式 CPU affinity 绑核、`nice` 和可选 Linux RT policy / priority；status 展示 desired/applied 资源状态；权限不足时结构化诊断而非 panic 或静默忽略。
- Rust zenoh 参数控制面 adapter：`params_remote` 模块通过 zenoh query/queryable 实现跨机器远程 `params list/get/set`，复用本机 Unix socket 路径相同的 schema 校验、structured error 和 pending/apply 语义；生成的 Rust runtime shell 会在存在参数时暴露远程参数端点。
- `flowrt params` 支持远程参数控制面：`--remote` 通过 zenoh control-plane 发现匹配 selfdesc hash 的远端 runtime；`--image` 改为命名选项；多个匹配时要求用户用 `--runtime <key_expr>` 显式选择；`target:` 输出明确告知命令打到了哪个 runtime。
- `flowrt params` 远程路径测试覆盖 key expression 解析、远端查询、schema 错误、无匹配和多匹配场景。

### v0.5.0 规划

`v0.5.0` 聚焦三条 runtime 主线：launch-grade supervisor、参数控制面和高频调度硬化；
同时新增 FlowRT core skills 套组，服务 AI 辅助开发和编码。

- supervisor 主线：`process_started` / `runtime_ready` / `service_ready` readiness gate、错峰启动、env 注入、restart/failure policy、显式 CPU affinity 绑核、`nice` + 可选 Linux RT policy / priority。
- 参数控制面主线：参数发现、可校验、可远程操作的 runtime control-plane，跨机器推荐走 zenoh control-plane。
- Rust zenoh 参数控制面 adapter：`params_remote` 模块通过 zenoh query/queryable 实现跨机器远程 `params list/get/set`，复用本机 Unix socket 路径相同的 schema 校验、structured error 和 pending/apply 语义；key expression 格式 `flowrt/params/{package}/{selfdesc_hash}/{pid}` 包含 package、selfdesc hash 和 PID，避免同机多进程冲突；支持 timeout、schema 拒绝、startup-only 拒绝、unknown param 和 unsupported command 等结构化错误。
- 高频调度硬化主线：deadline、stale、backpressure、fairness 进入 runtime 行为和 status 观测。
- FlowRT core skills 套组：`.agents/skills/` 当前落地 `frt-core-*` 和跨 core/app 编排通用的 `frt-subtask`，先服务 FlowRT 仓库维护者；首批包含 change intake、RSDL/IR、codegen、runtime parity 和 backend 五个 P0 skill。`frt-app-*` 暂作为 1.0.0 之后的保留命名空间，`write-frt-skill` 元技能负责约束命名、触发条件、硬门控、验证证据和 FlowRT 专有术语。
- `v0.6.0` 规划 Operation + record/replay + simulated clock + deterministic debug。Operation 是 typed long-running command、generated state machine、explicit policy 和 observable handle；底层编译期 lower 成 Service + Channel，用户只看 Operation，调试时才展开底层拓扑。record/replay、模拟时钟和确定性调试共享同一时间与事件模型。
- `v0.7.0` 规划 external process / driver package 接入边界、ARM64/跨机器部署和离线交付闭环；到 `v0.7.0`，FlowRT 应具备复刻复杂车载机器人应用的系统能力。主项目不做硬件 backend，Web/HTTP/WebSocket/UI 也不进入 runtime core，只消费 typed introspection / params / record API。
- `v0.8.0` 规划多目标部署、交叉编译、多架构安装包和发布硬化；`v0.9.0` 规划 C/Python API 与可选生态互操作扩展；`v1.0.0` 规划 ABI/schema 稳定、兼容策略、故障注入和性能矩阵。
- 参数热更新继续作为 runtime control-plane service-like RPC，可复用 schema、validation、structured error、pending/apply 和 self-description 经验，但不并入 graph 业务 Service 或 Operation 语义。

## v0.4.0 - 2026-06-07

### 新增

- GitHub Actions CI/release 升级为 `amd64` + `arm64` 双架构矩阵：Rust fmt/test/clippy、C++ runtime、Debian package、C++ zenoh runtime、demo smoke、ROS2 Jazzy bridge 和 ROS2 Lyrical bridge 均在对应架构 runner 上执行；package job 分别上传 `flowrt-linux-amd64-deb` 与 `flowrt-linux-arm64-deb`，tag release 同时发布 `flowrt_*_amd64.deb`、`flowrt_*_arm64.deb` 和统一 `SHA256SUMS`。
- Rust/C++ codegen：为 service client 端口生成按 component 复用的 typed handle（`ServiceClient_{component}_{port}`），暴露同步 `call()` 和非阻塞 `start_call()` 路径，并注入用户 `on_tick` 回调。
- Rust codegen：为有 service server 端口的 component trait 生成 `on_{port}_request` handler 方法，返回 `ServiceResult<Resp>`。
- Rust codegen：生成 hidden service task step 函数，调用 `process_pending_requests()` 处理排队请求。
- Rust codegen：scheduler 集成——注册 service task lane 和 dispatch case，service request arrival 通过 `pending_count()` 唤醒 hidden task。
- Rust codegen：service server 组件使用 `Arc<Mutex<Box<dyn ... + Send>>>` 存储，handler 闭包通过受控锁访问组件方法，满足 runtime service registry 的 `Send + Sync` handler 边界，并把 generated service stats 写入 live introspection。
- C++ codegen：为有 service server 端口的 C++ component interface 生成 `on_{port}_request` 虚方法。
- `zenoh` service backend 生成 `Unsupported/NotImplemented`——client handle 返回 `ServiceError::Backend`。
- 读取 IR service policy（`backend`、`timeout_ms`、`queue_depth`、`overflow`、`lane`、`max_in_flight`）生成 `InprocServiceConfig`。
- `docs/cli.md` 更新：Service RSDL 写法、policy 字段说明、Rust/C++ 用户 API 示例。
- `examples/service_demo/`：完整 Service 运行示例，包含 RSDL 声明（service client/server、bind policy、profile、target）、Rust 用户组件实现（server handler + client call）、构建运行和 `flowrt list/status` 健康观测。
- `README.md` Service 章节更新：补充 Service 与 channel、参数热更新的区别，Service policy 字段说明，错误语义，Service 与 Operation 边界，`flowrt list/status` 观测命令。
- `docs/examples.md` 更新：service_demo 章节补充用户源码、构建运行命令和 `flowrt list/status` 用法。
- `InprocServiceServer::pending_count()` / `in_flight_count()` 方法：返回排队和处理中 request 数量，用于 scheduler wake glue 与 `flowrt status` service health。
- Rust zenoh service request/response 运行时：`ZenohServiceClient` 和 `ZenohServiceServer` 基于 zenoh query/queryable 实现跨进程 request/response 语义，复用 canonical service frame 编解码，支持 request id/correlation/timeout、server unavailable（zenoh query timeout 映射为 `ServiceError::Timeout`）、backend error 映射为 `ServiceError::Backend`，handler 业务错误透传 `ServiceError::HandlerError`。client 和 server 接受外部 `Session`（通过 `Session::clone()` 共享），不自行管理 session 生命周期。key expression 命名为 `flowrt/service/{name}/request`，包含 service canonical name，避免同机多应用冲突。
- C++ zenoh service request/response 运行时：`ZenohServiceClient<Req, Resp>` 和 `ZenohServiceServer<Req, Resp>` 与 Rust 同语义，通过 `shared_ptr<::zenoh::Session>` 共享 session，并固定使用 FlowRT 锁定的 zenoh-c/zenoh-cpp 1.9.0 API。
- Rust zenoh service 集成测试：basic request/response、handler error、timeout、unavailable、multiple clients。
- C++ zenoh service smoke 测试：basic request/response、handler error、timeout、multiple clients。
- RSDL `[[bind.service]]` 支持 policy 字段：`backend`（`auto`/`inproc`/`zenoh`）、`timeout_ms`、`queue_depth`、`overflow`（`busy`/`error`）、`lane`、`max_in_flight`；parser 拒绝未知字段和非法值。
- Contract IR `ServiceEdgeIr` 增加 `backend`、`backend_source`、`policy`、`policy_source` 强类型字段，service normalization 实现 auto backend resolver：同进程默认 `inproc`，跨进程/跨 target 默认 `zenoh`；显式 `inproc` 跨进程 fail-fast；显式 `iox2` 或未知 backend fail-fast。
- validator 增加 service backend/policy 校验：拒绝非 `inproc`/`zenoh` 的 service backend，拒绝 `timeout_ms`/`queue_depth`/`max_in_flight` 为零，拒绝显式 `inproc` 跨进程。
- launch manifest 的 service 条目输出 resolved backend 和完整 policy（`timeout_ms`、`queue_depth`、`overflow`、`lane`、`max_in_flight`）。
- service 默认 policy 常量：`timeout_ms = 5000`、`queue_depth = 32`、`overflow = "busy"`、`max_in_flight = 64`。
- self-description schema 增加 `SelfDescriptionServiceEndpoint` 类型和 `SelfDescriptionGraph.services` 字段，记录 service endpoint 静态拓扑（name、canonical_id、client/server instance+port、request/response type、backend、policy）。
- runtime introspection 增加 `IntrospectionServiceStatus` 类型和 `IntrospectionStatus.services` 字段，记录 service 运行态健康状态（ready、in_flight、queued、total_requests、timeout_count、busy_count、unavailable_count、late_drop_count）。
- `flowrt list` 展示 service endpoint 拓扑：service name、client/server instance.port、request/response type。
- `flowrt status` 展示 service 运行态健康：ready、in_flight、queued、total_requests、timeout、busy、unavailable、late_drop。
- Rust/C++ `IntrospectionState` 增加 `register_service()` 和 `record_service_health()` 方法。
- C++ introspection header 增加 `IntrospectionServiceStatus` 结构体和 `service_status_json()` 序列化。
- Rust inproc service 运行时：`ServiceRegistry` 注册 typed handler 与返回 `ServiceResult<T>` 的 fallible handler，`InprocServiceClient` 支持阻塞 `call()` 和非阻塞 `start_call()`，`InprocServiceServer` 由 request arrival 驱动 `process_pending_requests()`；`InprocServiceConfig` 支持 `queue_depth`、`max_in_flight`、overflow 策略和 server `ScheduleWaiter`；支持有界请求队列、overflow 返回 `Busy` 或 `Rejected`、zero timeout 返回 `Timeout`、server 未注册通过 registry 查询、late response 丢弃计数；same-lane 阻塞调用通过 thread-local `ACTIVE_LANES` 检测并返回 `WouldDeadlock`；`LaneGuard` RAII guard 管理 lane 活跃标记；`ServiceCallHandle` 支持 `poll()` 和 `complete()` 非阻塞等待；`ServiceStatsSnapshot` 暴露请求/成功/超时/繁忙/late-drop/死锁计数。
- self-description schema 增加 `SelfDescriptionComponentType`、`SelfDescriptionPortDecl`、`SelfDescriptionServicePortDecl`、`SelfDescriptionParamDecl` 类型和 `SelfDescription.component_types` 字段，记录组件类型声明摘要（name、language、kind、inputs、outputs、service_clients、service_servers、params）。
- `flowrt codegen` 在 self-description 输出中生成 `component_types` 列表，映射 Contract IR 中的组件类型声明。
- `flowrt list` 输出组件视图：summary 行增加 `component_types` 计数；每个 graph 下先展示 component types（language、kind、端口摘要），再按 instance 展示 tasks、channel endpoints、service endpoints 和 params。
- `flowrt nodes` 在 instance 行增加 `kind=` 字段（当 self-description 包含 component type 信息时）。
- `flowrt status` 在 service health 行增加 `client_instance=` 和 `server_instance=` 字段，通过 self-description 关联 service endpoint 与 instance。
- 旧版 JSON（不含 `component_types` 字段）通过 `serde(default)` 兼容加载，不报错。

- `flowrt::service` 模块定义 Service core primitives：`ServiceError`（11 种错误码，u16 ABI 稳定）、`ServiceResult<T>`（不与 `Status` 混用）、`RequestId`（session_id + sequence + service_id 三元组）、`Deadline`（相对超时 + 绝对截止，默认禁止无界等待）、`ServiceFrameHeader`（80 字节固定 header + 变长 tail，含 magic/version/service_id/request_id/correlation/deadline/schema_hash/payload/error_msg）。
- FNV-1a 64-bit hash 函数 `fnv1a64()` 用于从 canonical service name 生成 service_id，跨语言确定性、无外部依赖。
- Rust 和 C++ 完整 frame 编解码 `encode_service_frame()` / `decode_service_frame()`，支持请求帧和错误响应帧。
- C ABI 新增 `flowrt_service_error_t` 常量（0–10）和 `flowrt_service_frame_header_t` POD 结构体（80 字节），字段偏移与 Rust/C++ 完全对齐。
- Rust service frame roundtrip 测试覆盖正常帧、错误帧、空帧、非法 magic/version、deadline 过期和 trailing bytes 拒绝。
- C++ service smoke 测试覆盖同等行为，包含 ABI static_assert、ServiceFrameHeader roundtrip、非法 magic/version 报错和 ServiceResult 语义。
- C++ inproc service request/response 运行时：`InprocServiceServer<Req, Resp>` 注册 typed handler，`InprocServiceClient<Req, Resp>` 支持阻塞 `call()` 和非阻塞 `start_call()`，`InprocServiceHandle<Resp>` 支持 `complete()` / `wait()` 和非消费式 `poll()` ready 查询。`InprocServiceRegistry` 单例管理 service 注册/注销。支持 queue depth、max_in_flight、overflow 返回 `Busy`、server 销毁后返回 `Unavailable`、same-lane 死锁检测返回 `WouldDeadlock`、handler 异常返回 `HandlerError`、业务错误透传。请求到达可选回调通知 runtime（不依赖 tick polling）。默认 timeout 5000ms，不允许无界阻塞。

### 修复

- 修复 Rust runtime 的 zenoh-only service examples 未声明 `required-features`，导致默认 feature 的 `cargo test` 和 tag release CI 编译失败。
- 修复 Debian package smoke 对 CMake repo fallback 的误报检测：只检查 option 默认值是否为 `ON`，不再把错误提示中的开发模式命令当作默认启用。

## v0.3.2 - 2026-06-07

### 变更

- `v0.3.2` 定义为 hardening / architecture repair 版本：不新增用户语义，不推进 v0.4 Service runtime，只修复现有能力缺陷。修复范围包括：codegen 深化、打包 hermetic、arm64 deb 支持、self-description / introspection schema 共享、generated startup 去 panic、supervisor engine 下沉 runtime、parser / normalizer seam 拆分、C++ backend capability 硬化、CMake repo fallback 收口、CI 主路径迁到 `--run-steps`。
- CI demo smoke、快速开始和示例文档的主路径从 `--run-ticks` 迁移到 `--run-steps`；`--run-ticks` 仅在 CLI 兼容测试和兼容说明中保留。生成应用内部的 `--flowrt-run-ticks` 兼容参数不受影响。
- 拆分 `flowrt-codegen` Rust runtime shell 生成逻辑：新增 `rust_shell` 子模块承载 backend、scheduler、lifecycle、params、introspection 和 step 生成 seam，使 `emit_artifacts` public 入口保持不变，同时降低 `lib.rs` 的职责密度。
- 拆分 `flowrt-rsdl` parser 单文件（2042 行）为 `parser/mod.rs` + `workspace.rs` + `imports.rs` + `schema.rs` + `tables.rs` + `values.rs` 六个语义子模块，公共入口 `parse_str`、`parse_file`、`load_file` 保持兼容。
- 拆分 `flowrt-ir` normalize 单文件（2955 行）为 `normalize/mod.rs` + `ids.rs` + `modules.rs` + `resolver.rs` + `profiles.rs` + `targets.rs` + `backends.rs` + `graphs.rs` + `services.rs` + `params.rs` 十个语义子模块，公共入口 `normalize_document`、`normalize_loaded_document`、`project_contract_to_profile`、`hash_source` 保持兼容。
- 新增 split seams 回归测试，覆盖 workspace import、module name resolver、service bind normalization、profile iox2 variable route auto-zenoh fallback 四个 seam。
- 硬化 C++ backend 编译能力：新增 `ChannelError::Unsupported` 和 `BackendHealthState::Unsupported`，当 `FLOWRT_HAS_ICEORYX2_CXX` / `FLOWRT_HAS_ZENOH_CXX` 未定义时，iox2/zenoh endpoint 不再伪装成可恢复的 `Transport` 错误，而是明确返回不可恢复的 `Unsupported` 配置错误；`Iox2Backend` 和 `ZenohBackend` 新增 `static constexpr compiled_with_transport()` 编译期/运行时查询方法。
- C++ runtime smoke 测试更新：disabled 路径断言 `ChannelError::Unsupported` 和 `BackendHealthState::Unsupported`；enabled 路径（`iox2_smoke.cpp` / `zenoh_smoke.cpp`）断言 `compiled_with_transport()` 为 true。
- 发布构建（`scripts/package-deb.sh`）增加依赖锁定校验：新增 `scripts/deps.lock` 锁文件记录 iceoryx2、zenoh-c、zenoh-cpp 的 git tag 对应 commit SHA 以及 zenoh Debian 包的 sha256；脚本拉取后逐项校验，任一不匹配即报错退出，确保发布包构建可复现、可审计。
- `scripts/package-deb.sh` 的 `--architecture` 参数现在真正控制 zenoh Debian 包的下载架构：`amd64` 和 `arm64` 各自下载对应架构的 `libzenohc` / `libzenohc-dev`，`libzenohcpp-dev` 仍为 `all`（架构无关）；不支持的架构会 fail-fast 并列出可用架构列表，`multiarch` 安装路径按目标架构推导。
- 新增 `flowrt-selfdesc` workspace crate：抽取 CLI、codegen 和 runtime 共用的 self-description schema 类型、JSON/binary section 加载/校验、SHA-256 哈希和 Message ABI / variable frame 字段格式化，消除三处复制结构体的 drift 风险。
- `flowrt-codegen` 的 self-description 构建和序列化改为使用 `flowrt-selfdesc` 共享类型，`flowrt-cli` 的 self-description 读取、echo payload 格式化也改为复用该 crate。
- 共享 schema 类型使用 `serde(default)` 兼容旧版 JSON 和未来扩展字段；loader 会明确拒绝不支持的 self-description version。
- supervisor 引擎下沉到 runtime 深模块：`flowrt::supervisor` 包含进程编排、依赖拓扑排序、重启策略、失败传播、zenoh 自动 mesh 和可执行文件解析的运行时逻辑；生成物 supervisor 缩减为 `SupervisorConfig` 常量和 `flowrt::supervisor::launch()` 调用。

### 新增

- `flowrt-selfdesc` crate 包含单元测试，覆盖从 `selfdesc.json` 文件和 `.flowrt.selfdesc` 二进制 section 读取、fixed-size ABI 字段格式化、variable frame 字段格式化、未知字段兼容、不支持版本报错、无效 JSON 报清晰错误、缺失文件报清晰错误和 SHA-256 哈希确定性。
- runtime supervisor 纯函数单元测试覆盖 `RestartPolicy`、`resolve_dependency_order`、`collect_propagated_failures`、`zenoh_launch_env_for_graph` 和 manifest 反序列化默认值。

### 修复

- 收口 CMake repo fallback：生成 CMake 的 `runtime/cpp` 源码树回退不再默认生效，必须显式设置 `-DFLOWRT_ALLOW_REPO_RUNTIME_FALLBACK=ON` 或环境变量 `FLOWRT_ALLOW_REPO_RUNTIME_FALLBACK=1`；CLI 仓库开发模式下会自动传递该标志。默认用户路径必须通过安装包、`CMAKE_PREFIX_PATH` 或 `FLOWRT_CPP_RUNTIME_DIR` 获取 runtime，未配置时错误信息会提示安装 FlowRT 包或设置对应路径。

## v0.3.1 - 2026-06-07

### 变更

- 重构 agent 工作规范：`AGENTS.md` 收敛为长期抽象开发约束，新增入库 `CONTEXT.md` 承载当前仓库状态、CLI/CI/Release 状态和近期版本背景，并强化中文 Conventional Commits、正文要求与原子提交纪律。
- 增加 workspace / module / composition 的主线切片：CLI 通过 workspace root 装载 module 与 composition，Contract IR 记录 module metadata、qualified symbol 和 generated symbol，module 内短名优先解析本 module，root/composition 层短名歧义必须显式写 `module::Name`，Rust/C++ codegen 使用 generated component/type 名避免跨 module 同名碰撞，并新增 `examples/workspace_demo` 覆盖该路径。
- 增加 supervisor 进程编排切片：RSDL 支持 graph 级 `[[process]]` 声明 `depends_on`、`restart`、退避参数和 `failure` 传播策略；Contract IR 会为每个实际 process 生成 canonical orchestration，launch manifest 输出 per-process policy，generated supervisor 按依赖顺序启动、按配置重启并在 `failure = "propagate"` 时终止依赖进程。
- 增加 Service 请求/响应语义切片：component 可声明 `service_client` 和 `service_server` 端口，graph 用 `[[bind.service]]` 绑定 client/server，Contract IR 与 validator 会校验 request/response 类型匹配和 client 端唯一绑定，launch manifest 输出 service 拓扑，为后续 runtime RPC 和 C/Python ABI 做结构准备。
- 增加 C/Python ABI 边界准备：新增 `flowrt/abi.h` C ABI 基础头和 Rust `repr(C)` 镜像类型，稳定 status、backend kind、backend health、borrowed string/bytes view、reconnect policy 和 backend health snapshot 的跨语言 POD 形状；当前不提供 Python binding 或 C runtime wrapper。

## v0.3.0 - 2026-06-07

### 新增

- 增加 Scheduler v2 基础：Rust/C++ runtime 提供 FlowRT 自有 `DeterministicExecutor`、`WorkerPool`、serial lane、task priority、periodic timer、shutdown admission 和轻量 async/coroutine substrate；Rust 侧不引入 tokio，C++ 侧只在 runtime 内部提供 C++20 coroutine adapter。
- RSDL task 支持 `readiness = "any_ready" | "all_ready"` 和 `lane = "<snake_case>"`；`readiness` 只允许用于 `on_message` task，`lane` 用于生成 scheduler lane 计划。
- profile 支持 `worker_threads = N` scheduler 默认值，Contract IR、launch manifest 和 self-description 会记录该值。
- 生成的 Rust/C++ shell 开始按 task 建立 scheduler plan：`periodic` task 由 timer 唤醒，`on_message` task 由输入 channel revision 变化或 FIFO backlog 唤醒，并在同一 scheduler step 的 drain loop 中级联执行依赖 task；`startup` / `shutdown` 仍在 scheduler 前后执行。
- 生成 shell 对 `Status::Retry` 改为非致命 task 结果；只有 `Status::Error` 会停止当前运行序列，backpressure 或用户主动 `Retry` 不再被提升为全局失败。
- inproc/iox2/zenoh channel endpoint 增加接收侧 revision 计数和 cached latest view，用于事件触发检测新到达数据，并避免 transport wake probe 二次消费用户回调要读取的样本；iox2 使用同名 event service 作为 sideband wake，zenoh 使用订阅 callback 唤醒 scheduler。
- Scheduler v2 的阻塞等待使用 drain loop 刷新后的 data-generation barrier，避免同一 step 内部 publish 造成自唤醒空转。

### 变更

- `flowrt run` / `flowrt launch` 增加 `--run-steps <N>` 作为新的外部运行上限名称；`--run-ticks <N>` 保留兼容。CLI 和 supervisor 会向生成应用转发内部 `--flowrt-run-steps`，生成应用仍兼容接受 `--flowrt-run-ticks`。
- 参数 pending apply 从“跟随生成 step 函数”收敛为 Scheduler v2 step 边界按 instance 应用一次，避免同一 instance 多 task 或 task-centric dispatch 下重复应用。

## v0.2.0 - 2026-06-06

### 变更

- 重写 `README.md`：把项目入口调整为面向 FlowRT 应用开发者的概念和用法说明，突出 RSDL、应用目录、构建运行、用户组件、消息、backend 与运行态观测；仓库维护内容收敛为文档入口。
- 拆分 `flowrt-codegen` 的测试、自描述、构建文件和 launch manifest 生成模块，降低单文件维护压力，同时保持 `emit_artifacts` 对外入口不变。
- 将 GitHub Actions CI 从单个串行 Linux job 拆成 `guard-generated`、`rust-fmt`、`rust-test`、`rust-clippy`、`cpp-runtime`、`cpp-zenoh-runtime`、`package`、`demo-smoke`、`ros2-jazzy-bridge`、`ros2-lyrical-bridge` 和 `release`，让格式化、Rust、C++、打包、示例 smoke 和 ROS2 bridge smoke 可以分层并行执行，release 仍等待完整 gate。
- 将 Debian 包调整为 full offline 单包：`flowrt` binary、Rust runtime crate、C++ runtime、Rust crate vendor、`iceoryx2-cxx 0.9.1`、`zenoh-c 1.9.0`、`zenoh-cpp 1.9.0` 和第三方 license material 安装到 `/opt/flowrt/<version>` 私有前缀，`/usr/bin/flowrt` 只作为入口 symlink。
- 生成 Rust app 会写入离线 Cargo config 并优先使用包内 vendor；生成 CMake 会使用 FlowRT 私有前缀解析 C++ runtime 和 backend SDK，不再通过 `FetchContent` 联网拉取 `iceoryx2-cxx`。
- CI 的 deb package、C++ zenoh runtime 和 demo smoke 改为消费同一个 FlowRT deb 包内的私有依赖，避免 CI 与用户安装路径维护两套 backend 依赖解析逻辑。
- 修缮 variable frame 主线：RSDL 变长字段改为 `bytes`、`string` 和 `sequence<T>` 无界语义，生成的 Rust/C++ 用户 API 改用 `Vec<u8>` / `String` / `Vec<T>` 与 `std::vector<std::uint8_t>` / `std::string` / `std::vector<T>`；`iox2` 不再声明或生成变长承载能力，profile 默认 backend 为 `iox2` 且 route 使用 variable frame 时，Contract IR 会把该 route 自动选择到 `zenoh`，fixed-size route 仍继续走 `iox2`。
- 删除旧的 iox2 变长兼容示例和兼容承载层；变长消息跨语言示例收敛到 `examples/mixed_zenoh_demo`。
- 增加单 instance 多 task 支持：RSDL 新增 `[[instance.<name>.task]]` 数组表，task 具备稳定 `name`；旧 `[instance.<name>.task]` 单 task 写法继续可用并归一化为 `main`。
- Rust/C++ runtime shell 现在会按 task 的 input/output 子集分别调度同一个 component 实例，launch manifest 与 self-description 会输出 task name；validator 会拒绝同一 instance 下重复 task name 或非法 task name。
- 继续拆分大文件：将 `flowrt-ir` 的参数 schema、参数覆盖和参数值兼容逻辑从 `normalize.rs` 拆到独立参数归一化模块，使主归一化入口重新低于 2000 行。
- 增加 zenoh-only ROS2 bridge 生成切片：RSDL 支持 `[[bridge.ros2]]`，当前支持 `flowrt_to_ros2` 的 `std_msgs/msg/String` 映射；生成 shell 会把 source output 额外发布到 deterministic zenoh bridge key，生成 C++ ROS2 adapter 订阅该 key 并发布 ROS2 topic，supervisor 会以 `runtime_kind = "ros2_bridge"` 启动该 adapter。ROS2 侧强制 `rmw_zenoh_cpp`，不提供 DDS fallback。
- 生成的 ROS2 bridge CMake 会把 `AMENT_PREFIX_PATH` 映射进 `CMAKE_PREFIX_PATH`，避免只 source ROS2 环境时 plain CMake 找不到 `rclcpp`、`std_msgs` 或 `rmw_zenoh_cpp`。
- 明确 route backend 绑定边界：`backend` 是单条 `[[bind.dataflow]]` 的属性，省略或 `auto` 时由 profile 和 message ABI 自动选择；同一 route 不通过跨 import 重复声明做隐式合并。
- CI 的 Linux job 统一运行在官方 `ros:jazzy-ros-base-noble` 容器上，ROS2 bridge 另有 `ros:jazzy-ros-base-noble` 与 `ros:lyrical-ros-base-resolute` 两个并行强制 smoke；每个 bridge job 都安装对应发行版的 `rmw_zenoh_cpp`，用已打包的 `flowrt` 构建 `ros2_bridge_demo`，确认 adapter 链接 ROS2 `zenoh_cpp_vendor`，并通过 `ros2 topic echo /flowrt/text --once` 验证端到端桥接。

## v0.1.0 - 2026-06-06

### 修复

- 清理 `AGENTS.md` 中过时的仓库阶段、CI artifact、CLI 命令和安装方式说明，使 agent 约束与当前 deb 单包、live introspection、`flowrt echo` 和 `params` 状态一致。
- 修复生成 runtime shell 缺少 SIGINT/SIGTERM 优雅关闭路径的问题：Rust/C++ runtime 增加 `ShutdownToken`，生成 shell 收到信号后会退出 tick loop，并继续执行 `shutdown` task、`on_stop` 和 `on_shutdown`。
- 修复 Rust `IntrospectionState` 在 mutex poison 后全局 panic 的问题；live status、channel snapshot 和参数状态会恢复锁内状态继续服务。
- 修复 runtime introspection socket 启动时无条件删除同路径 socket 的问题；仍可连接的 live socket 会被拒绝覆盖，SIGKILL 后不可连接的 stale socket 文件会被回收。
- 修复 `.flowrt.lock` stale 文件阻塞后续 `prepare` / `build` 的问题；CLI 现在用 OS advisory lock 判断真实占用状态，锁文件可残留，PID 只作为诊断内容。
- 修复静态 self-description 中参数 schema 字段名与 CLI/runtime 不一致的问题：参数类型字段统一输出为 `type`，避免 `flowrt list` / `flowrt params` 解析带参数应用的 `selfdesc.json` 失败。
- 修正安装与 CI smoke 路径：FlowRT CLI 先构建 release binary，再把 CLI、C++ runtime package 和 Rust runtime crate 安装到系统路径后以 `flowrt ...` 运行示例，避免把 `cargo install` 或 `cargo run` 当成用户入口，也避免安装后的 C++/Rust 生成应用依赖构建机源码仓库。
- 修复 C++ only `flowrt launch` 的 supervisor-only Rust crate 缺少 `rust/src/selfdesc.rs` 的问题，避免生成 crate 引用缺失模块导致 CI demo smoke 失败。
- 修复 CI demo smoke 对持续运行 runtime shell 的假设：`flowrt run` / `flowrt launch` 支持显式 `--run-ticks <N>` 运行上限，CLI 会把该上限转发给生成应用；核心 runtime scheduler 不再读取 tick 数环境变量。
- 修复 CI demo smoke 在 clean checkout 下直接运行 `examples/import_demo` 时遗漏 `build` 步骤的问题，避免 `run` / `launch` 依赖本地残留产物。
- 修复 CI demo smoke 在 clean checkout 下直接运行 `examples/profile_switch_demo --profile iox2` 时遗漏 `build` 步骤的问题，避免 `run` 依赖本地残留产物。
- 修复 fixed-size Message ABI 的 echo probe 容量按字段 wire size 相加而忽略 padding 的问题；生成 shell 现在按 conformance 推导的 ABI `size_bytes` 注册 probe 容量，避免带 padding 的消息被误判超长并丢弃。
- 修复 C++ echo 数据面 probe 在带固定容量上限时重建内部 buffer 的问题，避免 variable frame 或固定容量 snapshot 在观察者存在时被误判为 drop。
- 修复 C++ 生成应用优先拾取旧版已安装 `flowrt_runtime` package 的问题；CLI 现在按 `FLOWRT_CPP_RUNTIME_DIR`、系统安装路径、仓库 `runtime/cpp` 的顺序解析 runtime，生成 CMake 中显式 `FLOWRT_CPP_RUNTIME_DIR` 也优先于 `find_package(flowrt_runtime)`。

### 新增

- 增加单包 Debian 打包入口 `scripts/package-deb.sh`：生成的 `flowrt` deb 会同时安装 `/usr/bin/flowrt`、Rust runtime crate、C++ runtime header、multiarch CMake package 和基础文档，使用户项目不需要克隆 FlowRT 仓库即可构建生成应用。
- 增加 deb 包 smoke 脚本：`scripts/test-package-deb.sh` 校验包内标准 Linux 路径，`scripts/test-deb-installed-user-project.sh` 会从 deb 解包后的安装根运行 `flowrt build --launcher`，验证 Rust-only 与 C++ only 用户项目都不引用 FlowRT 源码树。
- 增加 tag release 分发：推送 `v*` tag 时，CI 会在通过完整验证和 deb smoke 后创建 GitHub Release，上传 `flowrt` Debian 包和 `SHA256SUMS`；release 说明从 `CHANGELOG.md` 对应版本段抽取。
- 增加 MIT 许可证文件，并同步 Cargo 与 deb 包元数据，避免正式发布包仍携带占位版权信息。
- 增加运行态参数系统：RSDL component params 支持显式 `type`、`default`、`min`、`max`、`enum` 和 `update` schema，Contract IR 会保留参数类型与更新策略，validator 会拒绝不一致 schema、越界实例覆盖和不可热更新的复合参数。
- 增加 Rust/C++ 参数 codegen：带参数组件会生成 typed `*Params` 结构，`on_tick` 接收参数快照，并提供默认 `on_params_update(old, new, context)` 钩子；生成 runtime shell 会在 tick 边界应用 `on_tick` 参数 pending 值，并在成功提交后更新 live introspection 状态。
- 增加 `flowrt params list|get|set`：CLI 通过静态 self-description 匹配 live runtime socket，可以列出参数、读取当前/pending 值，或提交 JSON pending 更新；`startup` 参数运行时不可修改。
- 增加 `zenoh` backend 的完整实现：Contract IR/backend catalog、validator、Rust runtime 和 C++ runtime 现在都认识 `zenoh`，并把它建模为支持 `topology:multi_process`、`topology:multi_host` 与 `transfer:copy` 的跨主机 backend；生成物会输出 deterministic channel `key_expr`，Rust/C++ runtime shell 会通过真实 zenoh endpoint 发送 canonical wire/frame bytes。
- 收敛 C++ zenoh 依赖策略：生成 CMake 和 `runtime/cpp` 只接受本机预装的 `zenohcxx::zenohc`，让 C++ runtime 绑定到 Rust zenoh 提供的 C ABI；FlowRT 不在生成物中源码拉取 zenoh C++ 依赖。
- 增加本机 `flowrt launch` 的 zenoh 自动 mesh：当没有显式设置 `FLOWRT_ZENOH_MODE` / `FLOWRT_ZENOH_LISTEN` / `FLOWRT_ZENOH_CONNECT` 时，生成 supervisor 会为同一 graph 内的 zenoh process 自动分配本地 TCP listen/connect，便于 mixed demo 在单机上直接启动。
- 增加 variable frame 主线：FlowRT 现在可以把 `bytes`、`string` 和 `sequence<T>` 生成成固定 header + 尾部变长区的 canonical frame codec，Rust/C++ runtime 与 codegen 都支持这套布局，并把它暴露给 `inproc` 和 `zenoh` 路径。
- 增加 C++ iox2 依赖自动获取：生成 CMake 和 `runtime/cpp` 会先查找本机 `iceoryx2-cxx 0.9.1`，找不到时默认通过 `FetchContent` 拉取 `iceoryx2` v0.9.1，并调用 Cargo 构建 upstream Rust FFI。
- 增加 `examples/mixed_zenoh_demo`：示例同时验证 Rust source、C++ sink、无界 variable frame、`zenoh` mixed launch 路径，以及跨主机 session 配置注入。
- 增加 CI 对真实 `zenoh-c` / `zenoh-cpp` 1.9.0 的安装、C++ runtime zenoh smoke，以及 `mixed_zenoh_demo` build/launch 验证。
- 增加 `--run-ticks <N>` 和 `FLOWRT_TICK_SLEEP_MS`：前者由 CLI 显式限制 `run` / `launch` 的 demo tick 数，后者用于把同步 tick 间隔拉长，便于观察 `zenoh` mixed demo 的 live 输出。
- 增加 FlowRT 静态自描述产物：codegen 会生成 `flowrt/selfdesc/selfdesc.json`，并把同一份 canonical JSON 嵌入 Rust/C++ 生成应用的 `.flowrt.selfdesc` 二进制 section，为后续 `flowrt list`、`status` 和 `echo` 提供部署后可自查的静态拓扑与 Message ABI layout 事实源。
- 增加 `flowrt list` 和 `flowrt nodes` 的静态自描述读取路径，可从生成应用二进制的 `.flowrt.selfdesc` section 或 `flowrt/selfdesc/selfdesc.json` 输出 package、graph、instance、task、channel 和 Message ABI 摘要。
- 增加 Rust/C++ runtime introspection Unix socket 控制面：支持与 Rust wire JSON 兼容的 `status`、`self_description`、`channel_snapshot`、`observe_channel` 和结构化错误响应，socket 路径按 PID 命名并只作为发现入口。
- 增加按需 echo 数据面 probe：生成 shell 会注册 active channel 的 canonical channel 名、message type 和有界 probe 容量；只有 `flowrt echo` 建立 `observe_channel` 连接后，发布路径才会在成功发布输出后 best-effort 记录 latest payload，连接断开后自动回收。无观察者时发布热路径只做 channel-local 原子检查，不做 payload 拷贝、frame 编码或 socket 写入。
- 增加 `flowrt echo <channel> [--socket <path>] [--image <selfdesc-or-binary>]` 主路径：未指定 `--image` 时 CLI 从 live runtime 请求 self-description 并自动发现唯一进程；指定 `--image` 或旧式 `flowrt echo <selfdesc-or-binary> <channel>` 时按 self-description hash 匹配 live socket。
- 增强 `flowrt echo` 输出：CLI 会按 self-description 的 Message ABI layout 格式化 fixed-size 字段和 variable frame 字段，同时保留 raw/canonical bytes；`--follow [--interval-ms <ms>]` 会持续轮询同一 channel snapshot，并只在发布计数、时间戳或 payload 变化时输出。
- 增强 `flowrt status` live 摘要：channel 状态现在包含 active echo observer 数量和 probe drop 计数，便于确认观测是否启用以及是否发生数据面观测丢样。
- 增强 generated supervisor health baseline：supervisor 现在暴露自己的 live introspection socket，并轮询子进程 PID socket，把子进程启动、运行、tick stale、退出和失败状态展示到 `flowrt status`。
- 增加 generated supervisor 内置 restart policy：子进程异常退出时 supervisor 会按 `on-failure` 语义最多重启 3 次，退避 100ms 起步、上限 1000ms；正常退出不重启，`flowrt status` 会显示 `restarts` 和 `restarting` 状态。
- 增加 Rust/C++ backend health 和 reconnect 基础抽象：runtime 现在提供 `BackendHealthState`、`BackendHealthSnapshot`、`ReconnectPolicy` 和 `BackendHealthTracker`，为后续 zenoh/iox2 endpoint 自动恢复提供 C ABI 友好的状态与退避模型。
- 增加 `zenoh` endpoint 自动恢复：Rust/C++ endpoint 在本地 session 关闭或 transport 操作失败后会按 `ReconnectPolicy` 重建 session、publisher 和 subscriber；codec/schema 错误不触发重连，backend health 仍保持 ready。
- 增加 `iox2` endpoint 自动恢复：Rust/C++ typed endpoint 在本地 transport 资源丢失或操作失败后会按 `ReconnectPolicy` 重建本地 node、publisher 和 subscriber；backend health 会记录恢复过程。
- 增加 `flowrt hz [channel] [--socket <path>] [--window-ms <ms>]`：CLI 通过 live status 控制面读取 channel 发布计数并按采样窗口计算发布频率，不启用 echo 数据面 probe。
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
- 增加 C++ only `flowrt run` 路径：CLI 会直接运行 CMake 产出的 C++ app executable，支持 `--process <name>` 参数；构建由 `flowrt build` 显式完成。
- 增加 C++ only runtime shell 的 process group 分发：`run_process` 会按 Contract IR 中声明的 process 名称调用对应 step/run 函数。
- 增加 C++ only `flowrt launch` 路径：codegen 会生成 supervisor-only Rust crate；构建由 `flowrt build --launcher` 显式完成，`launch` 只运行已有 supervisor 启动 C++ process group。
- 增加 GitHub Actions CI 雏形：运行 Rust fmt/test/clippy、C++ runtime CMake/CTest、FlowRT demo smoke，并上传 Linux `flowrt` release binary artifact。
- 增加 C++ runtime 的 latest stale freshness 语义：`StaleConfig` 使用 C++20 `std::chrono::milliseconds` 表达时间窗口，`LatestChannel<T>` 支持 `publish_at` / `view_at`，并与 Rust runtime 的 `warn`、`drop`、`hold_last`、`error` 策略保持一致。
- 增加 mixed contract 语言边界校验：Rust codegen 不再为 C++ component 生成 Rust trait，C++ codegen 不再为 Rust component 生成 C++ interface，语言分离 process group 可以生成各自真实 runtime shell。
- 增强 `flowrt/launch/launch.json`：process group 现在包含 `runtimes` 和 `runtime_kind`，graph instance 也包含 `runtime`，为后续 mixed C++/Rust supervisor 分流打基础。
- 增加 mixed runtime readiness 分类：CLI 会拒绝同进程 C++/Rust 混合和 `inproc` 跨进程混合，并允许 language-separated mixed contract 在 `iox2` backend 下进入运行路径。
- 增强生成的 Rust supervisor：读取 launch manifest 的 `runtime_kind`，为 Rust process 选择 Rust app executable，为 C++ process 选择 C++ app executable，并继续拒绝 mixed process group。
- 收窄 C++/iox2 backend readiness 保护：C++ only `iox2` contract 不再被 CLI 主动拒绝，language-separated mixed `iox2` contract 可通过 supervisor 分流启动 Rust/C++ app。
- 增强 `flowrt/launch/launch.json`：graph 现在包含 `channels` 列表，记录每条 bind 的 backend、channel policy 和 iox2 service name，为 C++/Rust 跨进程通信共享同一 transport 契约打基础。
- 增加 C++ iox2 transport 契约准备：profile 选择 `iox2` 时，生成的 C++ 消息 struct 会带 `IOX2_TYPE_NAME`，生成的 CMake 会解析 `iceoryx2-cxx 0.9.1` 依赖并链接官方 C++ target。
- 增加 C++ runtime 的真实 `flowrt::iox2::Iox2PubSub<T>` binding：定义 `FLOWRT_HAS_ICEORYX2_CXX` 并链接 `iceoryx2-cxx` 时，C++ endpoint 会打开 typed IPC service，通过 `FlowRTIox2Header` user header 携带 runtime timestamp，并支持 loopback publish/receive smoke；默认未启用宏时仍安全返回 `ChannelError::Transport`。
- 增加 iox2 跨语言 type-name 对齐：Rust 消息在 profile 选择 `iox2` 时生成 `#[type_name("...")]`，C++ 和 Rust runtime 共享 `FlowRTIox2Header` user header 名称，transport timestamp 不再包进 payload envelope。
- 增加 `flowrt run --process <name>` 对 mixed contract 的语言分流：Rust process 运行 Rust app，C++ process 运行 CMake app；未指定 process 时提示使用 `flowrt launch` 启动全部 process group。
- 增加 `examples/mixed_iox2_demo`，用于演示 Rust source 与 C++ sink 通过 iox2 分进程连接的 mixed contract。
- 增加 `examples/imu_demo_iox2`，用于演示主 demo 的 Rust source、C++ controller 和 Rust monitor 可以通过 iox2 分进程运行。
- 增强 Rust/C++ message ABI conformance：生成测试现在包含同一份 Contract IR-derived expected byte fixtures，用于在各自语言测试中验证 sample field value 的跨语言字节等价性和 padding zero 语义。
- 增强 Rust/C++ message ABI conformance：生成测试现在会显式断言默认初始化后的整对象 bytes 全零，覆盖 padding bytes 的 deterministic default initialization 契约。
- 增加未来有界变长类型的 Contract IR 表达：RSDL type expression 可解析 `bytes<max=N>`、`string<max=N>` 和 `sequence<T,max=N>`，并要求 `max > 0`，为后续 Variable Frame ABI 保留结构化语义。
- 增加 CI 防回归检查，确认本地规格草案和 FlowRT 生成物没有被误加入 Git 索引。
- 增加 iox2/zenoh endpoint peer restart 回归测试，确认对端 endpoint 重建后本端仍可继续接收新样本。

### 测试

- 补强 RSDL parser import 回归测试，覆盖绝对路径、父目录路径、嵌套 import 展开和 loaded source 记录。
- 补强 Rust runtime introspection 回归测试，覆盖 self-description 请求、缺失 self-description 错误、unknown observe channel 和有界 probe 超长 payload 丢样统计。

### 变更

- `examples/imu_demo` 和 `examples/imu_demo_iox2` 的 estimator/controller 参数改为显式 schema，并让 Rust/C++ 用户组件从 typed params 读取 `gravity`、`kp` 和 `kd`。
- 调整 `flowrt run` / `flowrt launch` 职责边界：两者现在只读取已生成、已构建产物，不再执行 `prepare` 或构建；需要 launch supervisor 时由 `flowrt build --launcher` 显式构建，显式 `--profile` 只用于校验已生成产物是否匹配。
- 统一 deployment 和 route satisfaction 判定：normalizer 和 validator 现在共用 `flowrt-ir` 的 typed capability decision，公开 Contract IR JSON schema 保持不变。
- 收敛 process 边界 capability 派生：跨 process group 的 dataflow 现在会在 route capability 中推导 `topology:multi_process`，让 validator、normalizer 和 CLI 对 inproc 跨进程边界共享同一套 route 判定。
- 统一 Rust/C++ runtime backend capability 报告顺序，使 runtime API 与 Contract IR typed catalog 的全局 canonical 顺序一致，减少后续自描述、诊断和跨语言对比中的漂移。
- 生成的 Rust/C++ runtime shell 改为启动基于 `IntrospectionState` 的 status server，并在 scheduler tick 入口记录 live tick 计数；channel 发布统计常驻维护，payload snapshot 只在 echo 数据面 probe 存在观察者时按需写入。
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
- 增加 Message ABI bounded variable frame 主线：validator、codegen、Rust runtime 和 C++ runtime 现在都支持 `bytes<max=N>`、`string<max=N>` 和 `sequence<T,max=N>` 这类 bounded variable 字段，并会生成 canonical frame codec。
- 收紧 Message ABI v0.1 int128 能力边界：channel route 使用 `u128` / `i128` 时，该 route 会要求 `abi:int128`，当前 backend 不提供该能力，validator 会重新推导并拒绝伪造的 route capability 或 satisfied 标记。
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
- 修正多个 `flowrt prepare` / `build` 命令并发写入同一输出目录时可能损坏生成产物的问题；CLI 会用 `.flowrt.lock` 对会写生成目录的命令做 fail-fast 保护，`run` / `launch` 只读取已生成产物。
- 修正 Rust/C++ inproc FIFO channel 忽略 bind-level `max_age_ms` / `stale_policy` 的问题；runtime 现在提供 timestamped `push_at` / `pop_at` 读取路径，生成 shell 会把 FIFO stale 配置编入 channel initializer，并在 `stale_policy = "error"` 时继续于用户回调前返回错误。
- 修正生成的 Rust supervisor 只启动 launch manifest 首张 graph 的问题；现在会遍历全部 graph，并在整个 manifest 没有 process group 时明确报错。
- 选择 profile 时，`prepare` / `build` 会先投影到对应 profile 再校验和生成，`contract.ir.json` 只保留该 profile 的 deployment 视图；`run` / `launch` 只校验已生成产物是否匹配显式 profile。
- 选择 profile 时，投影后的 Contract IR 现在会重算来自 profile default 的 bind-level `overflow`、`stale_policy` 和 `max_age_ms`，同时保留 bind 上显式声明的 policy，并刷新相关 channel/deployment capability 元数据。
- 修正省略 `--profile` 时未投影默认 profile 的问题；`prepare` / `build` 会选择 `default` 或首个 profile，只校验和落盘对应 deployment 视图。
- 修正 Rust runtime shell 拓扑排序，同一 source instance 到同一 target instance 的多条 bind 只计为一条实例依赖。
- 修正生成 supervisor 从自身 binary 名称推导 app binary 名称的逻辑。
- 修正 `.gitignore`，避免 `runtime/cpp/include/flowrt/...` 这类仓库源码路径被通用生成物规则误忽略。
