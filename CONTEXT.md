# CONTEXT.md

本文档记录 FlowRT 仓库的当前上下文，供 coding agent 和维护者快速了解现状。它会
随版本演进更新；长期架构原则、语义约定、文档边界和提交纪律维护在 `AGENTS.md`。

## 当前版本背景

当前开发线为 `v0.23.3 Debt Consolidation`。本版本把当前 active debt / defer 收束在
一个 patch 版本内，不再拆成多个 `0.23.Z`；实现按 21 个原子 commit 分，最后本地 gate
通过后集中 push。`v0.23.3` 不纳入 Python binding、Service over `iox2`、PTP/NTP、
cross-host exact sync 或 hard realtime。

本版本目标是清掉已确认的 exposed-but-fake 或显式 defer：global tick determinism、
cross-process FIFO feedback、standby failover、graph health metrics、fault injection
matrix、Operation zenoh / FIFO / retention / policy、FrameDescriptor payload record、
OpenTelemetry / tracing 最小 exporter、C v0 params 子集，以及 v0.23.3 focused smoke /
release readiness / release gate 收尾。

上一发布线为 `v0.23.2 C++ clang-tidy Gate`：C++ runtime headers/tests 和 generated C++
runtime shell 进入 `clang-tidy` focused gate，波次 1 的 C++ 静态质量债闭合。

- 新增仓库级 `.clang-tidy`，采用低噪声 C++20 checks，避免 `modernize-use-trailing-return-type`、
  `readability-magic-numbers`、`modernize-avoid-c-arrays` 等高噪声或 ABI 不适配规则。
- 新增 `v0.23.2 C++ clang-tidy Smoke`，CI 安装 clang-tidy 后通过 release gate registry 运行，
  覆盖 `runtime/cpp` 默认 CMake translation units 与 `cpp_counter_demo` generated C++
  `runtime_shell.cpp`。
- 修复 C++ runtime `OperationCancelToken` 冗余默认成员初始化，保持 C++ runtime ctest 与
  generated shell 编译路径不退化。

上一发布线为 `v0.23.1 Backend Route Health Unification`：dataflow route 的 backend health
进入统一 introspection route facts，`iox2` / `zenoh` endpoint 的恢复状态不再只停留在独立
`BackendHealthTracker` 内部。

- `IntrospectionRouteStatus` 新增 `backend_health_state`、`backend_health_error`、
  `backend_reconnect_attempt`、`backend_next_retry_unix_ms` 和 `backend_recoverable`；
  `flowrt status` route 行同步展示这些字段。
- Rust generated shell 在 `iox2` / `zenoh` dataflow publish 后读取 endpoint
  `BackendHealthSnapshot` 并写入 route facts；publish error 继续记录 `last_error`，但不再
  覆盖 endpoint 已推导出的 reconnect attempt / recoverable 状态。
- C++ runtime introspection JSON 和 route diagnostics 派生镜像新增字段，保持 Rust/C++
  status schema parity。
- 新增 `v0.23.1 Route Health Smoke` focused smoke，覆盖 Rust route facts、CLI 输出、
  Rust codegen transport publish health 接线和 C++ introspection parity。

上一发布线为 `v0.23.0 Zenoh Service Transport`：native Rust/C++ generated Service 不再只
支持 inproc，同一 Contract IR 的 request/response bind 可以跨进程走 zenoh transport，并在
self-description 与 launch manifest 中暴露真实 request key expression。

- Rust codegen 为 `backend = "zenoh"` 的 service client 生成按进程填充的
  `ZenohServiceClient` typed handle；server 进程在组件启动后打开 `ZenohServiceServer`，
  handler 调用用户 `on_<port>_request`，并向 introspection 标记 service ready。
- C++ codegen 镜像 Rust 接线：typed client wrapper 持有 transport slot，server 进程打开
  `flowrt::zenoh::ZenohServiceServer`；mixed contract 仍保持语言边界诚实，不为另一语言
  component 伪造接口。
- validator 要求 zenoh service server component 声明 `concurrency = "parallel"`，因为
  handler 由 transport queryable 回调线程驱动；exclusive server fail-fast。
- `launch/launch.json` 与 self-description 的 service endpoint 对 zenoh backend 暴露
  `key_expr = "flowrt/service/<escaped service-name>/request"`，与 runtime 实际 queryable
  key expression 一致。
- Operation 的 zenoh generated runtime 仍未接线，继续 fail-fast；本版本只放行 native
  Service request/response。
- 新增 `examples/zenoh_service_demo` 和 `v0.23.0 Zenoh Service Smoke` focused smoke，覆盖
  validator 门、codegen 断言、`zenoh_service_{rust,cpp}` golden、CLI `flowrt list`
  service `key_expr` 展示、示例 `check/prepare` 和 service `key_expr`。

### 当前已核对的待收口问题

以下问题是 2026-06-18 核对历史记录与当前代码后确认的真实缺口。后续处理时不要把
这些能力当作已完整支持；应优先选择“实现完整端到端语义”或“validator/CLI 明确
fail-fast 拒绝”，避免 exposed-but-fake 语义继续进入下一个大版本。

`v0.23.3` 还债口径：这些问题全部进入同一版本处理，按 commit 拆分，不按 patch 版本
切碎。每项要么完整端到端实现，要么继续 validator/CLI fail-fast 并在本文档说明原因；
不能生成 placeholder wrapper。

**已收口债务**：

- debt 6 backend route health 统一：已在 `v0.23.1` 收口。route-level publish
  error、backpressure、recovery state 归并进 `introspection/facts.rs` 同一事实源，
  `status` / diagnostics 统一暴露，并由 focused smoke 把关。
- debt 4 C++ `clang-tidy` gate：已在 `v0.23.2` 收口。CI 安装 clang-tidy，通过 focused
  smoke 覆盖 C++ runtime headers/tests 与 generated C++ runtime shell。首版只启用低噪声
  checks，避免把 ABI/POD 风格或 generated generic capture 策略误判为发布阻塞。

**v0.23.3 active scope**：

- debt 2 跨进程 strict determinism / global tick lockstep：先做。需 supervisor/transport
  协调全局 tick。当前跨进程 feedback 是 seeded latest-snapshot，跨进程 fault injection
  determinism 因此不能承诺全局同拍（`crates/flowrt-validate/src/contract.rs` 已是 v1
  单进程口径）。是 debt 3 跨进程片与 debt 1 的前置。
- debt 1 standby failover：后做。建在已完成的 lifecycle 状态机与 graph health 之上，
  缺冗余 instance role、主备选择、运行态 dataflow bind 重定向和健康触发语义；不能在
  语义未定前只堆 runtime 切换代码。受益于 debt 2 的跨进程确定性。图级容错 capstone。
- debt 5 OpenTelemetry / distributed tracing：独立线，无结构依赖。当前仅有
  observability capability/resource 命名或规划，不存在稳定 span/exporter 上报路径。
  FlowRT introspection 优先，tracing 是 additive，可较晚按需排期。
- debt 3 fault injection 矩阵：当前限 test-only、单进程、scheduled task 和合成
  `Status::Error`（`crates/flowrt-codegen/src/cpp_shell/run_emit.rs` 等只合成 error
  outcome）。startup/shutdown task、panic、deadline 超时、backend drop、随机/chaos
  注入与性能矩阵仍未覆盖。`v0.23.3` 覆盖 deterministic matrix 扩展；生产随机/chaos 和
  性能矩阵仍不进本版。
- Operation 的 zenoh generated runtime、`feedback = "fifo"`、显式 `result_retention_ms`、
  `queue` / `cancel_running` / multi in-flight policy 进入本版；未能端到端覆盖的组合继续
  validator 拒绝。
- FrameDescriptor `record_payload = true` 进入本版；payload 只通过 payload capture
  provider 记录 artifact ref/hash，不塞回普通 message channel。
- C v0 只放开固定大小 params readonly snapshot；service、operation、variable frame、
  `io_boundary`、`external`、`pkg_config`、动态加载和 Python binding 继续 fail-fast。

上一发布线为 `v0.22.1 Reserved Keyword Naming`，是 `0.22.x 容错验证` 主题之后的验证
加固 patch：validator 拒绝会被 codegen 直接生成为 Rust/C++ 标识符、且与任一目标语言
保留关键字冲突的 RSDL 名称，避免用户契约在验证阶段通过、却在生成的 Rust/C++ shell
编译时报 `in`、`type`、`class`、`delete` 等关键字错误。同时 validator 拒绝 generated
runtime 尚未实现的显式 opt-in（Operation `feedback = "fifo"`、显式
`result_retention_ms` 和 FrameDescriptor `record_payload = true`），避免 exposed-but-fake
语义继续外露；focused smoke 已接入 release gate registry 与 CI。

上一发布线为 `v0.22.0 Deterministic Fault Injection`，开启 `0.22.x 容错验证` 主题：提供 test-only
确定性故障注入，在 `(instance, task, 第 N 次调用)` 锚点强制 `Status::Error`，让用户无需手写
「按时崩的组件」即可跑遍 0.21.x 全部故障反应策略并验证可复现，也为 v1.0.0 故障注入矩阵去风险。

- 注入是 **test-only codegen-time overlay**（不改 RSDL 契约结构），镜像 `temporary_island`：场景为
  独立 TOML（`[[inject]]`，按名引用 `instance`/`task`，`invocations` 显式集合或 `from_invocation`
  起点），经 `flowrt prepare/build/run --inject <场景>` 投影进 `ContractArtifactIr.fault_injection`，
  与 `temporary_overlay` 并列可叠加；置 `test_only=true`、`clock_source=simulated_replay`，
  `bundle`/`deploy` 默认拒绝（需 `--allow-island`）；
- codegen 为每个注入目标 task 生成 per-task pre-execution 计数器（scheduler 线程自增）+ 注入门：
  命中调用序号时跳过用户回调、合成 `error` outcome（与真实回调返 `Status::Error` 空输出字节等价），
  交既有 0.21.x 故障反应机器处理；Rust/C++ 镜像，gated 于 `fault_injection.is_some()`，非注入产物
  字节不漂移；
- validator 守门：注入只允许命中 **scheduled task**（periodic / on_message / on_synchronized，拒
  startup/shutdown），要求 **≥1 boundary input（island）** 以驱动 simulated_replay 时间线，调用序号
  canonical、EntityRef 一致、单进程（多进程图注入目标拒绝）；
- determinism 验证：注入门纯调用计数驱动（同输入 → 同注入点）由 golden 锁定，底层 record→replay /
  executor 确定性由 v0.17/v0.18 内核测试证明，注入在其上确定性叠加；新 golden
  `fault_injection_{restart,degrade_recover}_{rust,cpp}` + 编译网 + focused smoke 把关。
- **已知限制**：确定性限单进程（无全局 tick lockstep）；注入只合成 `Status::Error`（panic/deadline/
  backend drop 不在范围）；startup/shutdown 注入、跨进程注入 determinism、真实随机/chaos 注入与性能
  矩阵留待后续。

上一发布线为 `v0.21.4 Cross-Process Feedback Loops`，是
`0.21.x 图级容错 / 生命周期` 主题（patch 线）的**最后一片**：放行跨进程反馈边，让控制环可跨
进程闭合，至此本主题 5 切片（生命周期状态机 / 隔离重启 / 降级 / 图级 health / 跨进程反馈）收尾。

- validator 放行**跨进程** `[[bind.dataflow]] feedback = true`（仅 `channel = "latest"`），走
  支持跨进程的 transport backend；环闭合与 init 类型校验保留；跨进程 fifo 反馈拒绝（进程间无
  共享 scheduler tick，N 拍延迟无支撑）；
- 跨进程反馈语义为 **seeded latest-snapshot**：source 进程启动期把 init 播过 transport（复用
  既有反馈播种——按所属进程播种的逻辑对跨进程 zenoh channel 即真实 transport 发布），消费进程
  经既有 latest 缓存接收最近到达样本；codegen/runtime **无需新增**，仅解除 validator 限制；
- 严格 z⁻¹（恒 1 拍、tick-0 present 初值）仍**仅同进程**成立；跨进程延迟由 transport + tick
  skew 决定、tick-0 init 到达前 absent；不新增全局 lockstep / 跨进程 determinism，replay 继承
  既有 per-process 跨进程输入回放；
- 新 golden `cross_process_feedback_{rust,cpp}` 锁定两语言生成输出（source 播种 / 消费不播种 /
  消费端 transport 接收）；其 zenoh shell 不纳入 inproc-only 编译网，以 golden 文本 + smoke 接线
  断言把关；既有同进程 feedback golden 字节不漂移。

上一发布线为 `v0.21.3 Graph Health Aggregation + Controlled Stop`，是 `0.21.x` 主题第四切片：
把每实例 lifecycle 聚合成单一 graph health（worst-of，always-on observable + 图级诊断），并提供
图级受控停机策略（`[graph.health].on_faulted = "stop"` 时终态不可恢复故障 graceful 停机，gated）；
standby failover 诚实 defer（需冗余实例 role + 运行态 bind 重定向语义）。

上一发布线为 `v0.21.2 Degrade Data Semantics`，是 `0.21.x` 主题第三切片：放行 `degrade` 策略，
故障时降级续跑而非停机或重启，下游复用既有 stale policy 老化 last-known-good 数据。纯 codegen/
validate，不改 executor；唯一 runtime 改动是把 `degraded` 诊断派生为 `warn`。



上一发布线为 `v0.21.1 Instance Fault Isolation + Restart`，在 0.21.0 显式状态机之上放行 `isolate`/
`restart` 策略并落地进程内恢复行为：RSDL `[instance.<name>.fault]` 子表声明策略与 restart 退避参数，
runtime `DeterministicExecutor`（Rust 与 C++）新增 `suspend_task`/`resume_task`，生成 shell 在图含
`isolate`/`restart` instance 时按 clock-ms 退避（`min(initial<<consecutive.min(31), max)`）重跑
`on_init`→`on_start`，达 `max_restarts` 终态 `Faulted`；容错机制 gated，既有 golden 字节不漂移。

上一发布线为 `v0.21.0 Lifecycle State Machine`，把 instance 生命周期升为契约一等显式状态机（零恢复
行为改变）：runtime 新增跨语言 `LifecycleState` 枚举，生成 shell 在生命周期段旁路记录 per-instance
状态转移，`flowrt status` 经 `category=lifecycle` diagnostic 暴露状态。

上一发布线为 `v0.20.1 Feedback Loops`，让 graph 支持显式反馈环（cyclic graph）。`[[bind.dataflow]]`
回边标 `feedback = true` 建模为单位延迟 z⁻¹，消费者读上游上一拍输出，runtime 零改动：归一化进
`ChannelEdgeIr.feedback`，validator 无环校验剔除回边并专项校验；codegen 拓扑断环 + 启动期播种；v2
（0.20.1）回边新增 `init`（literal 初值）与 `fifo` + `depth = N`（N 拍延迟），仍限同进程（inproc）。
跨进程延迟环留待多机 / 容错版本。

更早发布线 `v0.19.0 Multi-Sensor Synchronization` 把 N 路
sensor 输入按 event-time（0.18.0 sample-time）对齐成同步集，经新 `on_synchronized` trigger 投递
给融合组件，是 0.18.0 sample-time 一等概念的直接 payoff：

- RSDL 新增 `[[sync]]` 顶层组（`name`/`instance`/`inputs`/`tolerance_ms`）与 task
  `trigger = "on_synchronized"` + `sync = "<组名>"`；归一化进 Contract IR（`SyncGroupIr`、
  `TaskIr.sync_group`），validator 校验组规则（≥2 输入、端口已声明、唯一 incoming bind、消息须声明
  timestamp 源、tolerance>0）与 task/sync 耦合；
- runtime 新增 `flowrt::Synchronizer` 原语（Rust + C++ 各实现，共享语义）：latest-aligned
  approx-window 匹配，只依赖 sample-time，realtime/replay 一致，跨语言对同一事件序列产出位级一致的
  同步集（conformance golden 向量把关）；
- codegen 把 `on_synchronized` 接入两语言生成 shell（复用 on_message 调度机器 + synchronizer gate），
  golden 锁定接线输出、编译网真编译 sync case；示例 `examples/sync_fusion_demo`。

上一发布线为 `v0.18.1 Codegen 验证加固`，是一次纯内部质量/验证版本（零用户语义变更）：

- codegen golden 等价 harness 锁定整份生成输出，配合生成工程真编译网（C++ `g++ -fsyntax-only`、
  Rust `cargo check`）纳入开发回路与 CI，堵 v0.17.0/v0.18.0 连续两版漏发的 codegen 编译错类缺口；
- overflow/stale/trigger 的 enum→string 映射去重收敛到 `runtime_plan` 共享函数（golden 证零行为变更）；
- `[workspace.lints.clippy]` 现代化 forward-guard（uninlined_format_args 等，清理约 40 处）。C++
  clang-tidy 门禁因本地无工具验证暂缓后续。

上一发布线为 `v0.18.0 Sensor Event-Time`，在确定性回放内核之上引入
sensor 采集时刻（event-time）作为回放时间轴。RSDL 消息可用 `[type.<Name>.timestamp]` 子表声明承载
sample 时间戳的字段及时钟语义（`field` + `unit` ns/us/ms + `epoch` monotonic/unix + `clock_domain`），
归一化进 Contract IR（`TypeIr.timestamp`），validator 校验该字段为本消息的 unsigned 整数标量并拒绝未知
枚举值；`timestamp` 仍可作普通字段名（按值类型区分子表与字段，向后兼容）。

回放据此按 sensor 采集时刻而非录制到达时刻确定性步进：`ReplayEvent` / `ReplayTimelineEntry` 承载
sample-time，`effective_time_ms()` 优先取 sample-time、否则回退 receive-time，`ReplayDriver`（Rust 与
C++）按 effective time 排序并逐周期步进。录制侧 `RecordEnvelope.sample_time_ns` 承载采集时刻（无值时
不序列化）；声明了 timestamp 源的 boundary input 被注入时，runtime 经
`register_boundary_input_with_sample_time` 的 typed 提取器（`decode_frame` 后读 stamp 字段 × unit→ns）
写入 envelope，生成 Rust/C++ runtime shell 对此类 boundary 自动改走该注册入口。两语言共享 sample-time
源解析，行为一致。

多传感器同步（`[[sync]]` / synchronizer / `on_synchronized`）、external_stepped 点亮与跨机 drift 是更大
的独立时间语义，各自留待后续版本（`v0.19.0` Multi-Sensor Synchronization 起）。

上一发布线 `v0.17.1 Deterministic Replay C++ Parity` 在 `v0.17.0` 的 Rust 运行时回放内核上补齐 C++
跨语言 parity。运行时原生确定性回放把 simulated_replay 回放从「外部经 introspection socket 由
wall-clock 节奏逐事件注入」改为「runtime 自己拥有回放事件时间线、确定性逐周期步进」，闭合
record→replay 往返，使回放结果只取决于事件序列、与物理快慢无关，且周期 task 在两事件之间逐周期触发
（积分粒度与 realtime 对齐）；`v0.17.1` 把该内核镜像到 C++（`flowrt/replay.hpp` ReplayDriver、boundary
激励录制、生成 C++ shell 走原生回放，与 Rust 字节级对齐），C++ 经 JSONL 回放源消费（CLI 把 MCAP
规范化为 JSONL，单一 MCAP 解析点保留在 Rust）。

上一发布线 `v0.16.0 Clock Model & Deterministic Replay` 把 clock source 提升为 Contract IR 一等
概念（`realtime` / `simulated_replay` / `external_stepped`，由 normalization 派生、validator 重新
推导，`external_stepped` 暂拒），用户算法经 `Context::now_ms()` / `now_secs()` / `dt_ms()` /
`dt_secs()` 取调度时间与积分步长；`simulated_replay` 调度去除 wall-clock 绑定（逻辑时钟由事件
`data_time` 推进、周期 task 调度边界 catch-up）。逐周期回放步进与 runtime 原生确定性回放驱动正是
`v0.17.0` 补齐项。

上一发布线 `v0.15.2 Scheduler Clock Fix` 是针对 generated Rust/C++ scheduler realtime
clock 的缺陷修复版本，修复 realtime 产物误把 boundary / replay 注入的 `published_at_ms`
当作 `scheduler_now_ms` 推进的问题；strict 和普通 island realtime 产物清空 pending 样本
时间戳并使用 scheduler 启动后的 monotonic elapsed time，避免 fixture 或外部样本时间污染
periodic task、record/status timing 和用户 `Context::timing()`。

上一发布线 `v0.15.1 CI Release Evidence` 是针对 CI/release 机制的可靠性版本，不新增
RSDL 用户语义、Contract IR schema、runtime API、C ABI 或时间模型；它把发布前置条件从
手工 `workflow_dispatch` 候选门禁改为发布分支 push 自动产出的 release evidence，避免
tag 发布再次重跑完整矩阵。

`v0.15.1` 的核心 Module 是 release evidence contract：发布分支 `dev/vX.Y.Z` 的 push CI
跑完整矩阵和 deb package 后，`Release Evidence Gate` 校验版本、release notes、
amd64/arm64 deb 元数据和 `SHA256SUMS`，再上传 `flowrt-release-evidence` artifact。
独立的 `.github/workflows/release.yml` 只监听 `v*` tag，解析 tag 指向的 commit SHA，
查询同一 commit SHA 上成功的 push CI 和 `Release Evidence Gate`，下载同一 run 的 deb 与
evidence artifact，复核 version/tag/sha、deb 元数据和校验和后创建 GitHub Release。
`scripts/check-release-candidate.sh` 保留为本地预检和远端 evidence 等待工具，但不再
触发远端 CI；`scripts/release-gates/registry.toml` 已登记
`v0.15.1 CI Release Evidence Smoke`，用于检查 CI contract、release workflow contract
和 helper contract。

上一发布线 `v0.15.0 Architecture Convergence` 是插入的架构收敛版本，不新增 RSDL
用户语义、Contract IR schema、runtime API、C ABI 或时间模型；它把已经落地的发布门禁、
契约派生事实和运行态观测事实收束成可检查的生产路径，避免后续大版本继续在多个模块里
复制同一套派生逻辑。

`v0.15.0` 的第一个核心 Module 是 release gate contract：`flowrt-devtools release-gate`
读取 `scripts/release-gates/registry.toml`，用 registry 校验版本对应的 focused smoke，
`scripts/check-release-readiness.sh` 和 `scripts/check-release-candidate.sh` 都通过该
registry 查询版本对应的 focused smoke。

`v0.15.0` 的第二个核心 Module 是 Contract IR derived facts：`flowrt-ir::derived` 暴露
`derive_contract_facts`、`ContractDerivedFacts` 和 `GraphDerivedFacts`，集中推导 route
backend satisfaction、capability、thread-affinity、resource satisfaction 等派生事实。
validator 的 capability/resource 校验会重新推导这些 facts 并拒绝篡改；codegen 的
runtime plan、launch manifest 和 self-description 通过 adapter 消费同一事实源。

`v0.15.0` 的第三个核心 Module 是 runtime observability facts：Rust runtime introspection 的
`facts` 模块从 live state 派生 status snapshot、diagnostics 和 recorder diagnostic event。
C++ introspection schema parity 已通过测试补强，保证 route、diagnostics 和 selected
backend 等字段与 Rust JSON-line wire schema 对齐。`flowrt status` 文本输出同时展示
`static_selfdesc=loaded|unavailable`，route 行用 `thread_affinity` /
`static_thread_affinity` 表达 static self-description 关联的线程亲和事实；live route 的
`type`、`backend` 和 `selected_reason` 仍以 runtime live status 为准。

架构护栏现在由两层脚本组成：`scripts/check-architecture-size.sh` 检查 tracked Rust、
C/C++ 和 shell 源文件单文件行数阈值；`scripts/check-architecture-contract.sh` 检查
release gate contract、Contract IR derived facts 和 runtime observability facts 的生产消费
路径。`scripts/test-v0150-architecture-convergence-smoke.sh` 串联 registry 校验、脚本语法、
size guard 和 contract guard，作为本版本 focused smoke。

上一发布线 `v0.14.1 Architecture Guard + Codebase Split` 已把 0.14.0 后显著膨胀的
调度、codegen、CLI、introspection 和测试聚合文件拆回清晰边界，并引入
`SchedulerRuntimePlan` 作为 generated scheduler 与 runtime primitive 之间的显式计划层。
`v0.15.0` 不重做这些拆分，只把可持续约束纳入 release gate 和 architecture contract。

此前发布线 `v0.14.0 Realtime Scheduler + Task Timing Context` 把原先调度 admission
同步等待长任务完成的问题，和用户算法无法看到真实 runtime 调度时间的问题合并处理：
scheduler 线程只负责 ready 判定、admission、backend commit 和 introspection，worker
只运行用户 task，并通过 completion queue 或等价完成通知把结果交回 scheduler。用户
task 仍通过现有 `flowrt::Context` 读取调度时间上下文，不改变 handler 签名。

`v0.14.0` 的长期用户语义是 runtime scheduling time，不是传感器事件时间。对每次 task
运行，运行态观测和 `Context` 要区分 `scheduled_time_ms`、`observed_time_ms`、
`lateness_ms`、`missed_periods` 和 `overrun`：`scheduled_time_ms` 是 runtime 计划该次
task 应被调度的时间，`observed_time_ms` 是 scheduler 实际观察并 admission 该次 task
的时间，`lateness_ms` 是两者差值的非负部分，`missed_periods` 只对 periodic task 表示
因迟到跨过的周期数，`overrun` 表示上一轮执行越过本轮周期或 deadline 边界。realtime
路径暴露真实 runtime 观测到的 wall-clock lateness 和相邻运行 `dt`；replay /
temporary island 的 simulated clock 仍保持确定性，fixture `at_ms` / `published_at_ms`
驱动 scheduler、record、Operation 和 status 的同一毫秒时间模型。
当前 Rust/C++ generated scheduler 只在 `artifact.temporary_overlay` 存在时消费
`published_at_ms` 推进 `scheduler_now_ms`；strict 和普通 island realtime 产物会清空外部
样本时间戳并使用 scheduler 启动后的 monotonic elapsed time，避免 fixture 或边界样本时间
污染 runtime scheduling time。
Rust/C++ live status、结构化 diagnostics 和 recorder event 已接入 task/lane timing，
会展示 inflight、scheduled/observed time、lateness、missed periods、overrun、
backpressure、run/success/failure counters 和连续失败计数。
Rust generated scheduler 不再把整个 `App` 捕获进 worker closure：scheduler 线程先从
inproc/iox2/zenoh/boundary 输入读取 owned snapshot，再把用户组件 handle、参数快照和
输入快照送入 worker；output commit 仍回到 scheduler 线程执行。这样 iox2 endpoint
保持 scheduler-local 线程亲和，不需要 unsafe `Send/Sync` 包装。

`v0.14.0` 明确不承诺硬实时；不实现 sensor timestamp、sensor event-time、clock
domain、PTP、NTP、跨机器 exact sync 或 approx sync，也不把多传感器同步策略塞进
runtime scheduler。deadline、lateness、missed period 和 overrun 只解释 FlowRT runtime
看到的调度时序，用于用户算法自适应、status/record 观测和诊断。
`flowrt explain`、App API manifest 和 generated `flowrt/app/implementation.md` 会展示
task context timing 能力：已有 `Context` 参数通过 `context.timing()` 读取，C callback
context 指针通过 `context->has_timing` / `context->timing` 读取；handler 签名不因此改变，
生命周期 context 默认不携带 timing。

`v0.13.0` 完成机器人 runtime completion 收口，补齐参数运行态 apply、抽象 resource
contract 和 variable frame 工程化。参数控制面已
补齐 runtime apply 闭环：RSDL/Contract IR 参数 schema、component default、instance
override、live pending set 和 scheduler 边界 apply 由同一自描述元数据串联；Rust/C++
generated shell 在 apply 边界重新校验 type、`min`、`max`、`enum` 与
`on_params_update(old, new, context)`，只有回调返回 `Ok` 才提交新快照。回调或边界
校验拒绝时旧参数继续生效，pending 被拒绝并记录，不会把无效值先写入再回滚；同一
instance 多 task 每个 scheduler step 只在 instance 边界 apply 一次。`flowrt params
get/list/set` 会展示 `apply_state=applied|pending|startup-only`，App API manifest 与
self-description 均暴露 params schema、update policy 和回调签名；C v0 仍 fail-fast
拒绝 params。

RSDL / Contract IR 已增加抽象 resource requirement 和 provider 语义：component 可声明
resource `capability`、访问方式、必需性、readiness、health 和失败传播，graph 可声明
target、process 或 external package 作用域的 provider。Contract IR 会派生 per-instance
resource satisfaction metadata，optional unsatisfied requirement 保留 diagnostic，validator
会重新推导并拒绝 required unsatisfied、exclusive 冲突、provider 引用错误、非 canonical
satisfaction 和 concrete hardware/protocol 词进入 resource capability。FlowRT core 不建模
串口、TCP、UDP、USB、V4L2、NPU SDK 路径等具体资源字段；这些属于应用、driver package
或 external package 边界。

运行态观测已收束到结构化 diagnostics schema。Rust/C++ live status 会从 clock、channel、
input、route、process、resource、I/O boundary、param、service、operation 和 task 状态
派生 `category`、`entity_kind`、`entity_id`、`state`、`severity`、`reason`、
`suggestion`、时间戳和 metrics；`flowrt status --format json` 输出按 socket 分组的
handshake/status/error 完整 JSON，文本输出保留 `diagnostic=...` 与参数 `apply_state`
行。`flowrt record` 会在显式录制路径写入 `diagnostics_event`，status 查询本身保持无副
作用，避免轮询状态污染录制。

variable frame 工程化已进入生成物和运行态观测主路径。Rust/C++ 生成物在含 `bytes`、
`string` 或 `sequence<T>` 的 message contract 中生成 `message_frame` conformance 测试：
C++ 测试编码 canonical frame 并写出 byte fixture，Rust 测试读取同一 fixture 并断言字节
等价，同时覆盖空变长字段、多元素 sequence、`sequence<fixed struct>`、UTF-8 string、
decode truncation、offset overflow 和 length overflow。Rust/C++ runtime 也保留同一组固定
header + tail byte fixture，避免 frame header、tail offset、length 和 nested fixed struct
sequence 的跨语言布局漂移。`flowrt echo` 使用 self-description 中的完整 Message ABI
metadata 解释 variable frame，可展示 `sequence<Point>` 这类 named fixed struct sequence；
长 sequence 默认输出结构化摘要，`--raw` 输出完整内容。`flowrt pub` / JSON fixture 注入、
`flowrt replay` 和 `flowrt record` 已覆盖 variable frame boundary input 与 event 主路径，
record/replay 继续使用 FlowRT-native 事件格式，不引入 ROS2 schema。backend 能力边界不变：
`iox2` 只承载 fixed-size plain data，variable frame route 必须通过
`abi:variable_payload_frame` 和 `allocation:unbounded_dynamic` capability 选择支持变长消息
的 backend，不生成 iox2 variable envelope。

`v0.12.0` 完成 Contract-driven App Authoring 发布收口。当前用户主路径
已经统一为 `flowrt.toml`、`rsdl/`、`app/` 和可重建的 `flowrt/` 生成目录：
`flowrt.toml` 记录 `[project].main = "rsdl/robot.rsdl"`，
`rsdl/` 放系统契约，`app/` 放用户算法，`flowrt/` 只放 FlowRT 管理产物。CLI 已新增
`flowrt init [path] --lang <rust|cpp|c>`，只生成现代项目入口和最小
`rsdl/robot.rsdl`；默认 Rust，可显式把初始 target runtime 设为 C++ 或 C，但不生成
默认用户实现文件、C++ `build_app()` 或 C callback table factory。`flowrt add
message/module/component` 省略 RSDL 时从 `flowrt.toml` 找主 RSDL，可追加根 message、
当前可解析的 `rsdl/modules/*.rsdl` workspace module 注册，以及 Rust/C++/C native
component、同名 instance 和最小 periodic task；写入前会重新解析、归一化并校验更新后
的 Contract IR，且不创建、追加或覆盖 `app/` 用户代码。`check`、`explain`、`prepare`、
`build`、`run`、`deps` 和 `doctor` 省略 RSDL 路径时会从当前目录向上发现最近的
`flowrt.toml`；
显式 RSDL 路径优先。`launch`、`bundle` 和 `deploy` 仍要求显式路径或 bundle 参数。
`flowrt explain [rsdl] [--format text|json]` 已开放，输出 component 用户实现路径、接口、
task、params、service 和 operation 摘要；C component 会展示 `app/c/<component>.c`
和 `flowrt_app/c_components.h` callback table 接入线索。手写或已有 RSDL 中
`language = "c"` 的 native component 已可由 codegen 生成 C++ adapter：C 用户文件放
`app/c/**` 并实现 generated header 声明的静态 callback table factory，generated C++
runtime shell 通过 C ABI callback table 转发生命周期和 periodic/on_message/startup/shutdown
task callback；C task callback context 会用 `flowrt_c_task_timing_t` 暴露 runtime
scheduling time，生命周期 callback 默认不携带 task timing；callback table 必须同时设置
`FLOWRT_ABI_FEATURE_C_COMPONENT_CALLBACKS_V0` 和
`FLOWRT_ABI_FEATURE_C_COMPONENT_TASK_TIMING_V1`，且不得设置未知 feature bit。
`examples/c_counter_demo` 覆盖 fixed-size `Count` message、
两个 C native component、CMake app build/run 和 supervisor launch；generated supervisor
已将 launch manifest 中的 `runtime_kind = "c"` 显式映射到 CMake app binary。CI 已切到
amd64/arm64 `v0.12.0 Authoring Smoke`：在临时项目中覆盖 `flowrt init` Rust/C++/C、
`flowrt add message/component`、`flowrt check`、`flowrt prepare` 和
`flowrt explain --format text/json`，确认 `init/add/prepare` 不写用户 `app/`，并在临时
`import_demo`、`cpp_counter_demo` 和 `c_counter_demo` 副本上按 runner 架构运行普通
build/run；package/release 依赖链会等待该 gate，`scripts/check-release-readiness.sh`
也会检查该 gate、`CHANGELOG.md` 版本段和 `CONTEXT.md` 当前版本状态。C v0 仍只支持 native component、
fixed-size plain data 输入/输出和 inproc demo，不支持 params、service、operation、
variable frame、`io_boundary`、`external`、`pkg_config`、动态加载、独立 C runtime 或
Python binding。`v0.11.1` 定位为 App SDK / C ABI v0 hardening：不扩展 C 功能，不引入
Python，只收紧 `flowrt init/add/explain` 诊断、C callback table 失败原因、用户接入边界
和发布就绪检查。RSDL / Contract IR / validator 现已承载
v0.10.0 并发
语义基础：component 可声明 `concurrency = "exclusive" | "parallel"`，task 可选声明
同名字段，未声明时默认继承 component 并解析为 `exclusive`；normalized
`ComponentIr` / `TaskIr` 会同时保留 resolved 值和用户显式来源。validator 当前采用
保守规则：task 声明 `parallel` 只有在所属 component 也声明 `parallel` 时才合法，task
不会隐式提升 component；`worker_threads = 1` 仍允许 parallel 声明，只是运行时行为
退化为串行。self-description / launch manifest 现已落盘 task `concurrency`、scheduler
task `lane` / `concurrency` 元数据；Rust/C++ generated runtime shell 已从早期串行
scheduler 主路径切到 `ReadyBatch` / `WorkerPool` admission。默认
`exclusive` component 继续保护同一用户对象串行访问；显式 `parallel` component 才生成
`Send + Sync` 用户 trait / interface，并按显式 lane 让不同 ready task 真正跨 worker
并行执行。Contract IR 现为 dataflow route 派生 `thread_affinity` metadata：`iox2`
为 `scheduler_local_commit`，`inproc` / `zenoh` 为 `send_safe`；validator 会重新推导并拒绝
手工篡改，launch manifest 与 self-description 会暴露该字段。Rust/C++ generated
runtime shell 已采用 two-phase output commit：worker 只执行用户 task 并收集 task-local
output，scheduler 线程在 `Status::Ok` 后按 ready batch canonical order 提交 backend
输出，`Retry`、`Error`、panic 和 exception 都丢弃本次 output。Rust iox2 endpoint 仍由
scheduler/local owner 持有，不做 unsafe `Send/Sync` 包装；含 iox2 route 的 task 仍可跨
worker 并发执行，只是 transport commit 留在 scheduler 线程。`flowrt list` 和 live
`flowrt status` 会展示 route `thread_affinity`，让用户能区分 task 并发执行与 backend
transport commit 线程亲和。`v0.10.1` 收紧了 CLI
工程化细节：
boundary input 注入优先使用 canonical frame layout，zenoh/boundary echo 优先按
canonical frame 解码而 inproc fixed channel 继续按 native Message ABI 解码；显式
`--socket` 指向 FlowRT 管理目录内 stale runtime socket 时，self-description 和静态
image hash 校验路径会删除已确认失效的 socket 并返回明确错误；native C++ build 现在会
应用 host toolchain profile 的编译/链接选项，native pkg-config 通过 `PKG_CONFIG_PATH`
扩展用户路径，不再用 `PKG_CONFIG_LIBDIR` 隔离系统默认搜索路径。`v0.9.0` 是 Island
Mode / Boundary Endpoint 版本，
用于支持单功能单位开发、ROS2 项目逐功能包迁移、边界输入输出和 `flowrt pub`；
`v0.9.1` 在此基础上补齐迁移验证常用工具：canonical frame JSON 注入、`pub --file
--freq`、`params set --file`、多 channel `echo` 和显式空消息。
已接入 RSDL 语法、Contract IR normalization 和 validator 拓扑规则：profile mode canonical 为
`strict` 或 `island`；graph 级 `BoundaryEndpointIr` 记录 stable id、name、direction、
真实 `instance.port` 引用和解析后的 `TypeExpr`，并按方向和名称稳定排序；strict
profile 拒绝 boundary endpoint，island profile 下 typed boundary input 可以满足 task
active input，但同一 input port 不允许同时由 dataflow bind 和 boundary input 满足。
self-description 和 launch manifest 已暴露 profile/graph mode 与 typed
`boundary_endpoints`；`flowrt list` 可以展示 `island_profiles`、`boundary_endpoints` 和
每个 boundary input/output 的绑定端点。Rust/C++ runtime 已提供显式
`BoundaryInput` / `BoundaryOutput` primitive：boundary input 使用 latest snapshot 注入、
revision、stale policy 和 scheduler waiter 唤醒；boundary output 使用 sink guard 自动
回收临时观测。Rust/C++ generated runtime shell 已接入 boundary primitive：island
boundary input 会参与 `on_message` revision/wake 和 task 输入读取，boundary output 会在
用户输出后发布到显式 sink，strict 生成物不携带 boundary 字段。CLI `flowrt pub` 已接入
fixed Message ABI、canonical frame JSON、JSONL/JSON array 文件流和 wall-clock `--freq`
注入，只允许写 boundary input；显式 `empty = true` 空消息使用零长度 wire payload，
可通过 `{}` 或 `null` 注入；`flowrt params set --file` 可用于迁移测试参数批量导入；
`flowrt echo` 支持单 channel 旧格式和多 channel `channel=<name>` 前缀输出；`flowrt status`
和 `flowrt record` 已能围绕 boundary output 做观测/录制；`flowrt bundle` / `flowrt deploy`
默认拒绝 island 脚手架产物，只有显式 `--allow-island` 才允许。ROS2/zenoh boundary
adapter 已进入窄切片：`[[bridge.ros2]].flowrt` 可以引用普通 `instance.port`，也可以
在 island profile 下引用 `boundary.input` / `boundary.output` 名称；Contract IR 会保留
可校验的 boundary endpoint 引用，generated shell 通过 zenoh-only bridge key 把 ROS2
输入注入 boundary input，并把 boundary output 发布给 ROS2 adapter。`examples/island_demo`
已提供无硬件依赖的最小闭环：`flowrt pub` 向 `sample_in` 注入 typed JSON，组件处理后
通过 `result_out` 供 `flowrt echo` / `flowrt record` 观察；`examples/variable_frame_island_demo`
展示 `string` / `sequence<f32>` boundary input、`pub --file --freq` JSONL 注入和 fixed
summary 输出。迁移旧系统或普通单功能单位开发时，外部 live topic、bag 片段或测试
fixture 应先在 FlowRT 外部转换成 RSDL 字段自然 JSONL 或 JSON array，再通过 island
boundary input 注入；FlowRT 不做 ROS2 drop-in，不在 core 中实现 ROS2 message 语义，
也不让用户代码直接读取 rosbag。`flowrt replay` 已作为 FlowRT-native fixture 回放主路径：
CLI 只向 typed boundary input 注入自然 JSON 样本，像真实传感器一样驱动 graph，让用户
组件只看到普通 FlowRT input、`on_message` 和 latest view；普通 dataflow channel、
boundary output、service、operation 或未知 endpoint 都会在选择 live socket 前被拒绝。
strict 生产 self-description 会拒绝 replay 注入，错误会提示使用 island profile 或
temporary island overlay。为了避免临时测试反复修改 `.rsdl`，`prepare` / `build` / `run`
支持 CLI 触发的一次性 temporary island overlay：基础 RSDL 保持 strict，CLI 生成
test-only island projection、manifest 和 self-description，显式声明哪些端口成为
boundary input/output；Contract IR、self-description、launch manifest 和 live status 会
记录 `temporary_overlay`、原 profile mode、生成命令/source、boundary mapping 来源，以及
`source=simulated_replay`、`unit=ms`、`field=tick_time_ms` 的 clock metadata。replay 注入的
`at_ms` 作为 `published_at_ms` 进入 runtime，scheduler tick、record clock event、
Operation event 和 status 使用同一毫秒时间模型。temporary overlay / test-only 产物即使
被手工篡改成 strict mode，`bundle` / `deploy` 也会默认拒绝，除非显式允许 island。
CI 已加入 amd64/arm64 的 `v0.9.0 Island Demo Smoke` 和
`v0.9.1 Island Migration Tooling Smoke`，smoke 会按 CI runner 架构只改写临时 demo
RSDL 的 target platform，避免 arm64 runner 误按示例默认 `linux-amd64` target 构建；
发布就绪脚本也会检查该 focused gate。最终集成 hardening 已收掉 generated Rust fixed
`WireCodec` 的 `cursor` unused warning：生成代码在 encode/decode 末尾断言 cursor
等于 `WIRE_SIZE`，避免 release smoke 输出噪声。

为 `v0.14.0` executor 革命，Rust/C++ runtime executor 的主路径已收敛为显式
admission 与 completion 边界：`DeterministicExecutor` 只负责 ready set 判定、每个 lane
一次 admission 一个 `TaskAdmission`、记录 inflight task，并在 scheduler 线程收到完成结果
后通过 `complete_task` 释放 inflight token。`WorkerPool` 只运行用户 task；worker 结果通过
`WorkerCompletionQueue` 回到 scheduler 线程，scheduler 按 admission canonical order drain
完成队列、提交 output、更新 introspection 和 backend。旧同步 helper 不再作为 generated
Rust/C++ scheduler 的兼容路径，避免为了 deterministic commit order 同步等待长任务完成。
用户组件 API 仍不直接暴露这些 executor primitive。
runtime 已补齐 output transaction primitive：Rust/C++ `Output<T>::write(...)`
用户 API 保持不变，generated shell 可以在 worker 回调内使用 task-local output，回调
返回后把输出转成带 task、port、payload、`published_at_ms` 和 `tick_time_ms` 的 commit
record；只有 `Status::Ok` 的结果保留可提交 output，`Retry`、`Error`、panic 和 exception
都会丢弃本次输出。Rust/C++ generated runtime shell 已接入 two-phase output commit
主路径：scheduler 仍负责 backend output commit，worker 只执行用户 task 并返回可提交
closure。commit 顺序按 admission canonical task order，而不是 worker 完成顺序；`iox2`
路径不再依赖整批本地运行作为并发规避手段。

`flowrt cache status/clean` 已用于解释和安全清理 FlowRT deps cache、项目 build 目录、
incremental cache 和 stale 临时候选。清理命令必须按默认可清、条件可清、仅展示、
永不自动清区分，不得自动删除安装前缀、用户 SDK overlay、`.flowrt/toolchains.toml`、
最终二进制、live socket、MCAP 或日志。用户最终二进制通常不是磁盘大头；GB 级占用
主要来自 Cargo/FRT deps cache、中间产物、多 target、多 feature 和 vendor hash。

CLI 已新增正式 `flowrt explain [rsdl] [--format text|json]`：命令与
`check` 一样只解析、归一化和校验 RSDL，不写生成物；省略 RSDL 路径时复用
`flowrt.toml` 的 `project.main` 发现。`check` 保持简短校验和 generated handler 摘要，
`explain` 负责输出完整 component 实现说明，包括 package / graph / profile mode、
component language/kind、建议用户文件路径、task trigger/readiness/lane/concurrency、
`on_tick` / `on_params_update` 签名、params、输入输出以及 service / operation handle。
JSON 输出由结构体序列化生成，供 agent 和工具稳定消费；`language = "c"` component
会展示 `app/c/<component>.c` 用户文件建议路径和 `flowrt_app/c_components.h` callback
table 接入线索。

当前仍只承诺 `linux-amd64 -> linux-arm64` 交叉编译，不承诺
`linux-arm64 -> linux-amd64`。amd64 deb 继续内嵌完整 `linux-arm64` target SDK，包含
FlowRT C++ runtime、`iceoryx2-cxx`、`zenoh-c`、`zenoh-cpp`、CMake package 和
pkg-config 事实源。RSDL target 描述目标语义，toolchain profile 描述本机如何编译；
交叉编译器、sysroot、CMake toolchain、pkg-config 路径、SDK overlay、C++ compile/link
args 和 runtime dependency policy 都属于 toolchain/profile 配置，不写入 RSDL 或
Contract IR。
部署闭包已进一步收紧：交叉 C++/CMake build 在 bundled runtime dependency policy 下
把 target SDK manifest 记录进 `build-info.json`，`bundle` 会复制到
`runtime-deps/<platform>/` 并在 `bundle.toml` 中记录 version、platform、policy、path 和
sha256。bundle manifest 同时记录 graph resource provider closure；deploy 的 dry-run 和
真实部署都会校验 schema v2 artifact closure、external package multi-platform artifact
closure、external scope resource provider closure、runtime dependency closure、platform
和 hash。temporary overlay / test-only 产物仍默认拒绝；普通 island 产物仍需要显式
`--allow-island`。真实部署上传前会先检查远端 FlowRT 同一 `major.minor` 版本，再通过远端
deploy probe 校验 remote platform 和 runtime dependency 指纹；host、remote dir、ssh/scp
参数继续按参数边界处理，空 host、以 `-` 开头的 host 和不安全 remote dir 会被拒绝。
`doctor` 对 SDK overlay、pkg-config path 和 CMake toolchain file 输出可执行建议，指向
`flowrt toolchain init --target <platform> --sdk-overlay <path>`、
`.flowrt/toolchains.toml` 和 `flowrt doctor <rsdl> --target <platform>`，不引入 apt/sudo
或隐式联网修复路径。

CI/release 侧使用按架构隔离的 Rust/Cargo cache 和外部 `FLOWRT_CACHE_DIR` 降低重复
构建成本。`v0.8.6 Cross UX SDK Smoke` 固定在 amd64 host 上安装 package job 产出的
amd64 deb，准备公开 arm64 SDK overlay，运行 `toolchain init/show`、带 RSDL 的
`doctor`、`deps/build --target linux-arm64`，并检查 AArch64 ELF。deb 成品、release
notes 和 artifact manifest 不缓存，仍每次从源码重建。

`v0.8.1` 是 `v0.8.0` 之后的大 payload descriptor
小升级，聚焦标准 64 字节 FrameDescriptor、I/O boundary descriptor port 绑定、
descriptor-only 观测/录制、`frame_descriptor_demo` 示例、microbench 和 v0.8.1
focused release gate。

`v0.8.4` 是在 `v0.8.3` 交叉编译基础上的板级私有依赖工程化小升级，聚焦 component
build 的 pkg-config 依赖声明、toolchain profile 的 C++ compile/link 选项，以及
`cpp_link_libraries` 对板级私有 SDK 裸库或私有 `.so` 的注入。

`v0.8.5` 是 `v0.8.4` 之后的公开交叉 SDK 示例与验证线：仓库提供
`examples/cross_sdk_deps` 作为显式 prepare 项目，用 CMake 拉取并交叉编译公开真实依赖
`libjpeg-turbo` 和 `Arm KleidiAI` 到 demo-local arm64 SDK overlay；FlowRT 构建阶段只
消费 `component.build.pkg_config` 和 toolchain profile 中的 overlay，不隐式联网。对应
示例 `libjpeg_cross_demo` 覆盖平台无关 C/C++ 库，`kleidiai_cross_demo` 覆盖 Arm 专用
公开 SDK，CI 使用安装后的 amd64 deb 执行 `flowrt doctor/deps/build --target
linux-arm64` 并检查 AArch64 ELF。

`v0.8.0` 已发布，是真实机器人应用接入边界版本，聚焦 I/O boundary component、
variable frame 工程化、FrameDescriptor / side-channel descriptor、ROS2 zenoh 共存
桥接扩展、多目标部署闭环和 v0.8.0 focused release gate。

`v0.7.1` 已发布，是 `v0.7.0` 之后的 hardening 版本，聚焦现有能力的生产边界修复：
deploy/bundle 参数边界、supervisor 子进程生命周期与关闭路径、runtime introspection
控制面、Service / Operation / backend 错误分类、Contract IR 派生元数据防篡改、
C++/Rust runtime parity、安装包离线依赖标记和 `--run-steps` / `record` / `hz` 等
调试主路径。

`v0.7.0` 已发布，核心主题是 external process / driver package typed 接入边界、
ARM64/跨机器部署 baseline 和离线 bundle 交付闭环。FlowRT 主项目不做硬件 backend；
Linux 和外部 driver package 管硬件，FlowRT 管结构、执行、通信、观测、external
process 生命周期和 typed 接入边界。

`v0.6.1` 已发布，是构建/打包可靠性小升级：引入 `flowrt deps` 预热共享底层依赖
cache，`flowrt build` 默认 release 并只构建用户项目，用户二进制统一落在
`flowrt/build/bin/release/`。

`v0.5.0` 已发布，核心主题是 launch-grade
supervisor、参数控制面、高频调度硬化和 FlowRT core skills 套组：补齐 readiness
gate、错峰启动、env 注入、CPU affinity / priority 资源提示、远程参数控制面、
task/lane 调度健康观测和 v0.5.0 focused release gate。

`v0.4.0` 已发布，核心主题是 Service runtime：生成 Rust/C++ service client/server
用户 API，支持 `inproc` 与 `zenoh` service transport，补齐 request/response frame、
错误语义、self-description、`flowrt list` / `flowrt status` 观测和示例文档，并完成
amd64 + arm64 release 安装包闭环。

`v0.3.0` 已发布，核心主题是 Scheduler v2：task-centric scheduler plan、
`startup` / `shutdown` task、`on_message` readiness、serial lane、worker thread
配置、channel revision/cache 和同一 scheduler step 内的 drain loop 级联唤醒。

`v0.3.1` 聚焦复杂系统结构能力：

- workspace / module / composition 机制。
- module name resolver 和跨模块引用。
- supervisor 编排增强，包括 process 依赖顺序、故障传播和可配置 restart policy。
- Service 请求/响应语义切片。
- C/Python ABI 边界准备：先稳定 C ABI 基础类型，不实现 Python binding。

`v0.3.2` 定义为 hardening / architecture repair 版本：不新增用户语义，不推进
v0.4 Service runtime，只修复现有能力缺陷。修复范围：

- codegen 深化。
- 打包 hermetic。
- arm64 deb 支持。
- self-description / introspection schema 共享。
- generated startup 去 panic。
- supervisor engine 下沉 runtime。
- parser / normalizer seam 拆分。
- C++ backend capability 硬化。
- CMake repo fallback 收口。
- CI 主路径迁到 `--run-steps`。

录制回放系统暂不实施，但实现上述能力时应预留 runtime、self-description 或 CLI
边界，避免后续引入时破坏已发布契约。

后续版本路线是当前长期演进主线，不是一次性重写目标。每个版本只推动一个主轴，
但新语义必须按长期边界设计，避免为短期演示留下兼容负担。

| 版本 | 主线 |
| --- | --- |
| `v0.4.0` | Service runtime。 |
| `v0.5.0` | launch-grade supervisor、参数控制面、高频调度硬化和 FlowRT core skills 套组。 |
| `v0.6.0` | Operation + record-only 录制系统 + 时间事件模型基础。 |
| `v0.6.1` | `flowrt deps`、共享依赖 cache、默认 release 构建和 deb smoke 修复。 |
| `v0.7.0` | external process / driver package 接入边界、ARM64/跨机器部署闭环。 |
| `v0.7.1` | v0.7.0 现有能力 hardening 和发布前缺陷修复。 |
| `v0.8.0` | 真实机器人应用接入边界、variable frame 工程化、多目标部署和发布硬化。 |
| `v0.8.1` | 标准 FrameDescriptor、descriptor-only 大 payload 观测/录制和安装后 smoke。 |
| `v0.8.2` | `linux-amd64` host 到 `linux-arm64` target 的交叉编译支持基础。 |
| `v0.8.3` | 完整 `linux-amd64 -> linux-arm64` target SDK、SDK overlay、doctor 预检和真实交叉 smoke。 |
| `v0.8.4` | 板级私有依赖工程化：component build pkg-config、toolchain C++ 选项和私有 SDK 链接配置。 |
| `v0.8.5` | 公开真实交叉 SDK 示例、demo-local overlay prepare 和安装后 cross SDK smoke。 |
| `v0.8.6` | 交叉编译 UX hardening：toolchain init/show、Contract-aware doctor、build diagnostics、cache 治理和 Cross UX SDK smoke。 |
| `v0.9.0` | Island Mode / Boundary Endpoint：支持单功能单位开发、ROS2 项目逐功能包迁移、边界输入输出和 `flowrt pub`。 |
| `v0.9.1` | Island 迁移验证工具补强：canonical frame JSON 注入、批量参数、文件流 pub、多 channel echo 和显式空消息。 |
| `v0.9.2` | Island offline validation：replay/bag 播放、临时 island overlay、fixture 工作流和 strict/island 诊断收口。 |
| `v0.10.2` | 真并发收口：backend thread-affinity、two-phase output commit、scheduler-local transport commit 和并发验收 gate。 |
| `v0.10.3` | 标准 app/ 用户代码布局：废弃旧 `src/` 用户路径，用户实现统一进入 `app/`。 |
| `v0.11.0` | FlowRT App SDK 化与 C ABI v0：项目脚手架、用户 API 可发现性、C component 最小可运行路径。 |
| `v0.11.1` | App SDK / C ABI v0 hardening：诊断、失败路径、用户骨架和 release readiness 收口。 |
| `v0.12.0` | Contract-driven App Authoring：RSDL 先行，`prepare` / `explain` 产出真实用户实现接口。 |
| `v0.13.0` | Robot Runtime Completion：补齐 package/test/clock/抽象资源契约/trace/deployment/control authority 等核心设施。 |
| `v0.13.1` | 预留 v0.13.0 维护修复；scheduler dispatch hardening 已并入 v0.14.0 主线。 |
| `v0.14.0` | Realtime Scheduler + Task Timing Context：解耦 scheduler admission 与 task completion，并暴露 runtime 调度时间上下文。 |
| `v0.15.0` | Architecture Convergence：收束 release gate contract、Contract IR derived facts、runtime observability facts 和 architecture contract guard。 |
| `v0.15.1` | CI Release Evidence：发布分支 push 自动产出 release evidence，tag release 只消费同一 commit SHA 已验证产物。 |
| `v0.15.2` | Scheduler Clock Fix：realtime generated scheduler 使用 monotonic elapsed time，temporary overlay 继续使用 fixture 时间。 |
| `v0.16.0` | Clock Model & Deterministic Replay：clock source 成为 Contract IR 一等概念，用户经 `context.now()/dt()` 取时间，simulated_replay 调度去除 wall-clock 绑定使回放结果与物理快慢无关；逐周期回放步进与 runtime 原生确定性回放驱动留待后续。 |
| `v0.17.0` | Deterministic Replay (Runtime-Native)：runtime 自己拥有回放事件时间线并逐周期确定性步进，闭合 record→replay，回放结果与物理快慢无关；Rust 侧内核，C++ parity 在 `v0.17.1`。 |
| `v0.17.1` | Deterministic Replay C++ Parity：把 v0.17.0 的运行时原生回放内核镜像到 C++（`flowrt/replay.hpp` ReplayDriver、boundary 激励录制、生成 C++ shell 走原生回放），与 Rust 字节级对齐；C++ 经 JSONL 回放源消费（CLI 把 MCAP 规范化为 JSONL）。 |
| `v0.18.0` | Sensor Event-Time：RSDL 声明 sensor sample-time 源（`[type.<Name>.timestamp]`），record→replay 按 sensor 采集时刻（event-time）确定性步进而非到达时刻；生成 Rust/C++ shell 对此类 boundary 注册 typed sample-time 提取器，两语言一致。 |
| `v0.18.1` | Codegen 验证加固（纯内部质量版本，零用户语义变更）：codegen golden 等价 harness 锁定整份生成输出 + 生成工程真编译网（C++ `g++ -fsyntax-only`、Rust `cargo check`）纳入开发回路与 CI，堵 v0.17/v0.18 连续两版漏发的 codegen 编译错类缺口；overflow/stale/trigger 映射去重至 `runtime_plan`；`[workspace.lints.clippy]` 现代化 forward-guard。C++ clang-tidy 门禁暂缓。 |
| `v0.19.0` | Multi-Sensor Synchronization：RSDL `[[sync]]` 组把一个 instance 的 ≥2 路输入按 sample-time（event-time）对齐成同步集，经 `on_synchronized` trigger 投递给融合组件。runtime `flowrt::Synchronizer` 原语（Rust+C++，latest-aligned approx-window v1，DropLate）跨语言位级一致；codegen 两语言接线，golden+编译网真编译把关。最优匹配（ROS2 ApproximateTime 式）、late-policy 变体、跨机 drift 各自另立后续版本。 |
| `v0.20.0` | Feedback Loops / Cyclic Graphs：`[[bind.dataflow]]` 回边标 `feedback = true` 建模为单位延迟 z⁻¹，消费者读上游上一拍输出。codegen 拓扑排序剔除回边断环（图退化为 DAG）+ run 启动期对回边 channel 播种零初值，两语言一致，runtime 零改动；validator 校验 feedback 边仅 latest/同进程/必须真正闭环。golden+编译网真编译把关，示例 `examples/feedback_loop_demo`。v1 仅零初值/单拍/同进程；literal 初值、fifo N 拍、跨进程延迟环各自另立后续版本。 |
| `v0.20.1` | Feedback Loops v2：回边新增 `init`（按源消息类型播种 literal 初值）与 `fifo` + `depth = N`（N 拍延迟）。validator 放宽为允许 latest(1 拍) 或 fifo(N 拍)，按源消息 TypeIr 递归类型校验 init，fifo 反馈要求两端 periodic 等周期；codegen 两语言播种 literal/N 份。golden `feedback_v2_rust/cpp`+编译网真编译。init 仅支持全 primitive 字段消息；跨进程延迟环仍留待多机/容错版本。 |
| `v0.21.x` | 图级容错 / 生命周期（patch 线 5 切片）：生命周期状态机底座 / 进程内隔离重启 / 降级数据语义 / 图级 health 聚合 + 受控停机 / 跨进程反馈环。 |
| `v0.22.0` | Deterministic Fault Injection：test-only 注入 overlay 在 `(instance, task, 第 N 次调用)` 锚点强制 `Status::Error`，跑遍 0.21.x 全部故障反应策略并验证可复现。codegen 两语言 per-task 计数器 + 注入门（gated，非注入字节不漂移）；validator 守 scheduled-only / ≥1 boundary input(island) / 单进程 / canonical；golden + 编译网 + focused smoke。确定性经 golden 锁定的计数驱动门 ∘ v0.17/v0.18 回放内核证明，不另做 CLI MCAP 往返。Error-only、单进程、startup/shutdown 与跨进程注入留待后续。 |
| `v0.22.1` | Reserved Keyword Naming：validator 拒绝 field / port / service port / operation port / instance / task 等生成代码标识符撞 Rust 2024 或 C++ 保留关键字，保留 `profile.default` 等非标识符名称合法。focused smoke 接入 release gate。 |
| `v0.23.0` | Zenoh Service Transport：native Rust/C++ generated Service 支持跨进程 zenoh request/response，生成 typed `ZenohServiceClient` / `ZenohServiceServer` 接线；validator 要求 zenoh server component `concurrency = "parallel"`；manifest/self-description 暴露 service `key_expr`；新增 `zenoh_service_{rust,cpp}` golden、`examples/zenoh_service_demo` 和 focused smoke。Operation zenoh 仍 fail-fast。 |
| `v0.23.1` | Backend Route Health Unification：`iox2` / `zenoh` dataflow route 的 endpoint `BackendHealthSnapshot` 进入 introspection route facts，`flowrt status` route 行展示 `backend_health_state`、错误、重连 attempt、下一次 retry 和 recoverable 状态；Rust generated shell 在 transport publish 后记录 route backend health，C++ runtime JSON / diagnostics 镜像字段；新增 route health focused smoke。 |
| `v0.23.2` | C++ clang-tidy Gate：新增仓库级 `.clang-tidy` 与 focused smoke，CI 安装 clang-tidy 后 lint C++ runtime 默认 translation units 和 generated `cpp_counter_demo` runtime shell；首版启用低噪声 checks，关闭高噪声 ABI/POD/generic generated capture 不适配项。 |
| `v1.0.0` | ABI/schema 稳定、兼容策略、故障注入和性能矩阵。 |

路线边界：

- `v0.4.0` 先把 Service 做成稳定的 request/response runtime 语义。
- `v0.5.0` 优先补齐复杂应用复刻所需的系统编排基础：条件启动、错峰启动、环境变量、
  restart policy、CPU affinity、profile、参数发现/远程控制面，以及高频多 lane
  调度的 deadline、stale、backpressure 和 fairness 语义。同时引入 FlowRT core
  skills 套组，先沉淀开发 FlowRT 本身的 agent 工作流。
- `v0.6.0` 在 Service 之上引入 Operation，并落地只录不放的 record 系统。
  Operation、录制事件和未来 replay / simulated clock / deterministic debug 必须共享
  同一时间与事件模型；本版本只稳定事件和 timestamp 字段，不让模拟时钟驱动 scheduler。
- `v0.7.0` 定义 external process / driver package 的 typed 接入边界，并补齐 ARM64、
  跨机器部署和离线交付闭环。到 `v0.7.0`，FlowRT 应具备复刻一套复杂车载机器人
  应用的系统能力，但不把硬件 backend 做进 FlowRT 主项目。
- `v0.8.0` 把多目标部署、交叉编译、多架构安装包和发布硬化推进到真实机器人应用
  接入边界：I/O boundary component、external package 多平台交付、variable frame
  工程化、FrameDescriptor + side-channel lease、ROS2 共存桥接和运行态诊断深化应
  形成一条可迁移、可部署、可观测的主路径。FlowRT 仍不做硬件 backend，也不做
  ROS2 drop-in 兼容层。
- `v0.8.1` 收紧大 payload 的可工程化路径：本机高频图像/大帧只在 channel 上传递
  标准 fixed descriptor，payload 生命周期仍归 I/O boundary 或 external package 的
  side-channel 负责；echo、status、record 和 smoke 都围绕 descriptor-only 路径验证。
- `v0.9.0` 先解决单功能单位开发和 ROS2 项目迁移不顺手的问题：引入显式
  Island Mode 和 Boundary Endpoint。常规 strict 模式下，缺 active input bind 仍是
  error，不允许 warning 后生成不完整生产代码；island mode 必须在 RSDL/IR/profile
  中显式标记，并把外部输入表达为 typed boundary input，把需要对比的输出表达为
  typed boundary output、record sink 或 ROS2 adapter sink。island 生成物必须在
  self-description、manifest 和 status 中标注，`bundle` / `deploy` 默认拒绝 island
  产物，除非用户显式允许。`flowrt pub` 作为 island 开发和迁移测试工具，只向
  boundary input 注入按 self-description / Message ABI 编码的数据，不默认向任意生产
  channel 写入。迁移完成后应拆掉 boundary endpoint，把 profile 切回 strict，使功能
  单位回到普通 FlowRT graph。
- `v0.9.1` 把 island 从“可隔离运行”推进到“可做行为验证”：外部样本、bag 片段或
  旧系统录制数据先转换成 FlowRT RSDL 字段自然 JSONL / JSON array，再由 `flowrt pub`
  作为 boundary input 注入；参数快照通过 `params set --file` 恢复；输出用多 channel
  `echo`、`record` 或 test sink 对比。FlowRT core 不引入 ROS2 message 类型，用户仍以
  自己的 RSDL message 表达业务数据。
- `v0.9.2` 应补齐 replay/offline validation 与 island 的有机整体：FlowRT 只支持
  “播放数据，像真实传感器一样灌给运行中代码”的模式，不支持用户算法直接读取 bag
  或绕过 FlowRT graph。bag/replay 注入只允许 island 语义；strict 生产 contract
  必须拒绝，避免多来源数据竞争。为了让生产 contract 可临时测试，CLI 应提供
  temporary island overlay：不修改源 `.rsdl`，而是在 build/run/replay 阶段生成
  test-only island projection，并在 self-description、manifest、status 和 artifact
  metadata 中明确标记。初始实现应要求用户显式声明 boundary mapping；自动把所有缺失
  input 变成 boundary 只能作为后续便利功能，且必须打印完整映射并保持 test-only 标记。
- `v0.10.3` 把用户代码目录模型一次性切到长期形态：`app/` 是唯一用户业务代码根，
  `app/rust/mod.rs` 是 Rust 用户入口，`app/cpp/**` 和 `app/c/**` 是 C/C++ 用户实现与
  同目录头文件位置；`flowrt/` 仍只放可删除生成物。
- `v0.11.0` 把 FlowRT 从“可生成可运行”推进到“可作为 app SDK 顺手使用”：新增
  `flowrt.toml` 项目入口、`flowrt init`、`flowrt add`、`flowrt explain` 和 C ABI v0
  最小 demo。`v0.12.0` 已把 app 作者路径纠正为 RSDL 先行：`init` 只创建入口和最小
  RSDL，`add` 只作为 RSDL 编辑助手，`prepare` / `explain` 产出 App API manifest、
  实现清单和参考 stubs；用户再把需要保留的实现放进 `app/`。含 `flowrt.toml` 的项目
  中，`add`、`check`、`explain`、`prepare`、`build`、`run`、`deps` 和 `doctor`
  省略路径时会从当前目录向上发现 manifest；显式 RSDL 路径优先；RSDL 和 Contract IR
  仍是语义事实源。当前 C v0 用户入口通过 `language = "c"` component、App API 参考
  stubs 和 `flowrt_app/c_components.h` callback table 接入。
- `v0.11.0` 同时落地 C ABI v0 基础：C 侧事实源继续放
  `runtime/cpp/include/flowrt/abi.h`，只暴露 POD、固定宽度整数、borrowed
  string/bytes/frame view、状态码、错误码和 callback table；不暴露 C++/Rust 对象、
  backend SDK handle、动态插件 ABI 或所有权语义。C component 先编进 app binary，
  形成最小可运行 demo；Python binding 不进入本版本，后续只能建立在该 C ABI 边界上。
  `language = "c"` 已进入 RSDL/Contract IR/validator/codegen/CLI 用户入口；validator
  会拒绝 C v0 暂不支持的 params、service、operation、variable frame、`io_boundary`、
  `external`、`pkg_config`、动态加载、独立 C runtime 和 Python binding，codegen 只在
  C v0 native / fixed-size message 范围内生成 callback table adapter。generated
  supervisor 已支持 `runtime_kind = "c"` 并启动 CMake app binary。
- `v0.11.1` 不扩展 C ABI v0 的功能面，只做硬化：callback table 校验必须报告明确
  失败原因；`flowrt add` 写入前要尽量完成冲突和 Contract IR 校验；用户骨架要把
  callback table size/version/feature bit 和 borrowed view 所有权边界写清；release
  readiness 必须检查 `CONTEXT.md` 当前版本状态，避免发布后状态文档滞后。
- `v0.12.0` 纠正 FlowRT app 作者主路径：`flowrt init` 只创建项目入口和最小
  RSDL，不再把默认 component 的用户实现写进 `app/`；`flowrt add` 只作为 RSDL
  编辑助手，不再生成或修改用户 `app/` 代码；`prepare` / codegen 从 validated
  Contract IR 生成 App API manifest、实现清单和 `flowrt/app/stubs/` 参考模板，但只能写
  `flowrt/` 可重建生成物，不直接创建、追加或覆盖用户 `app/`。
- `v0.13.0` 的资源模型必须保持抽象。FlowRT core 只建模 component 需要的
  capability / resource contract、访问模式、必需/可选、readiness gate、health、
  故障传播和观测字段；target / deployment / external package 声明哪些 provider
  满足这些抽象需求。FlowRT 不在核心语义里建模串口、TCP、UDP、USB、V4L2、
  RKNN、CUDA、设备路径或端口号等具体硬件和协议细节；这些映射属于 external
  package、driver package、target profile 或部署配置。validator 只校验抽象需求能否
  被目标提供者满足，supervisor 只按抽象 contract 做启动门控、health 汇报和失败传播。
  当前 RSDL / Contract IR 已支持 `[component.<name>.resource.<name>]` 抽象 requirement
  和 `[[resource.provider]]` provider；normalizer 会填充默认 access、readiness、
  health 和 failure policy，按名称 canonical 排序并派生 per-instance satisfaction。
  validator 会重新推导 satisfaction，拒绝 required unsatisfied、exclusive 多实例冲突、
  provider target/process/external package 引用错误、非 canonical metadata 和 concrete
  hardware/protocol 词进入 resource capability；optional unsatisfied requirement 保留
  diagnostic 供 `status` / `doctor` 展示。supervisor 已接入 resource health gate，以
  抽象 provider readiness / health 控制启动和诊断，不读取具体硬件字段。
- `v0.13.1` 不再承载单独的 scheduler 主线；已确认的同步等待阻塞修复并入
  `v0.14.0`，避免把调度 admission 改造和用户可见时间上下文拆成两个互相漂移的版本。
- `v0.14.0` 修复 scheduler 同步执行阻塞问题，并把 runtime scheduling time 暴露为
  长期用户语义。scheduler loop 不应同步等待长任务完成；调度/admission 与 task
  completion 解耦后，仍必须由 scheduler 线程按 deterministic ready / completion order
  提交 output、更新 introspection 和执行 backend commit。`flowrt::Context` 暴露
  `scheduled_time_ms`、`observed_time_ms`、`lateness_ms`、`missed_periods`、
  `overrun` 和相邻运行 `dt`，让用户算法能区分周期迟到、漏周期和执行越界。该版本不
  承诺硬实时，不做 sensor timestamp、clock domain、PTP、NTP、跨机器 exact sync /
  approx sync 或多传感器同步策略；realtime 路径报告 runtime 实际观察时间，replay /
  temporary island 路径继续由 simulated clock 保持确定性。
- `v0.15.0` 不新增用户语义，只把 release gate registry、Contract IR derived facts、
  runtime observability facts 和 architecture contract guard 收束成明确模块与门禁。
  后续改动如果新增派生事实，必须优先接入 typed derived facts 或 observability facts，
  并让 architecture contract guard 能检查生产消费路径；不要重新在 validator、codegen、
  CLI 或 runtime 中手写相同推导。
- `v0.16.0` 是时间模型的底座主线，把 FlowRT 的时间概念从单一 runtime scheduling time
  扩展为可区分、可确定性回放的 clock 模型。clock source（realtime / simulated_replay /
  external_stepped）成为 Contract IR 一等概念，由 normalization 派生、validator 重新推导，
  不再靠 `temporary_overlay.is_some()` 隐式推断（external_stepped 暂不支持，直接拒绝）；
  用户算法经 runtime 时钟（`context.now_ms()` / `now_secs()` / `dt_ms()` / `dt_secs()`）取
  时间与 `dt`，不再直接读 `steady_clock`；simulated_replay 调度去除 wall-clock 绑定——
  只等下一个注入事件或关停，逻辑时钟由事件 `data_time` 推进、周期 task 在调度边界自动
  catch-up，回放物理快慢不影响行为结果；status / diagnostics / record 输出 clock source。
  本版本仍是 runtime scheduling time，不引入 sensor event-time 或多传感器同步。逐周期回放
  步进（与 realtime 完全对齐的积分粒度）和 record/replay 由 runtime 原生确定性驱动（替代
  per-event introspection socket 注入、逐位复现）作为紧随其后的 follow-up，本版本只解除
  wall-clock 绑定并解锁回放结果的物理快慢无关性，已足以支撑 deterministic debug、Rust/C++
  conformance parity 和 ROS2 功能包迁移的标准化行为验证。
- `v0.17.0` 把 v0.16.0 留待后续的逐周期回放步进补齐为 Rust 侧 runtime 原生确定性回放：runtime
  自己拥有回放事件时间线、按「下一个事件时间」与「下一个 periodic 网格点」逐周期确定性步进（积分
  粒度与 realtime 对齐），取代外部经 introspection socket 由 wall-clock 节奏逐事件注入。回放只重放
  录制的外部 boundary 激励、由 runtime 重算下游 channel；`flowrt record` 录制 boundary 激励闭合
  record→replay。
- `v0.17.1` 把该回放内核镜像到 C++ runtime，做到与 Rust 字节级 parity（`flowrt/replay.hpp`
  ReplayDriver、`publish_boundary_input` 录制边界激励、生成 C++ shell 走原生回放），两侧 conformance
  用同一事件序列断言一致；C++ 无 MCAP 解析能力，经 `flowrt-record` 的 JSONL 回放源消费（CLI 启动
  C++ 应用前把 MCAP 规范化为 JSONL，单一 MCAP 解析点保留在 Rust）。把 C++ parity 作为同轴 `.Z`
  补丁紧跟 `v0.17.0`，回放行为不再有跨语言分叉。
- `v0.18.0` 在 `v0.16.0` 的 clock domain 脚手架上引入 sensor event-time 作为回放时间轴：RSDL
  `[type.<Name>.timestamp]` 建模 sample-time 字段、unit、epoch 与 clock domain；record 承载
  `sample_time_ns`，replay 按 sample-time（effective time）确定性步进；生成 Rust/C++ shell 对声明源的
  boundary 注册 typed sample-time 提取器，两语言字节级一致。本版本只做单轴 event-time。
- `v0.19.0` 起承接多传感器同步：`[[sync]]` 同步组（exact/approx、window/tolerance、late sample
  policy）→ codegen synchronizer 与 `on_synchronized` 触发，event-time 作可驱动 clock domain；
  external_stepped 点亮与跨机器 drift capability 各自另立后续版本。validator 不得把未声明 clock domain
  的 sensor timestamp 隐式当成同步依据，也不得把 PTP、NTP 或跨机器同步能力写成 backend 内部假设。
- `v0.20.0` 起承接反馈环：`[[bind.dataflow]]` 回边标 `feedback = true` 作单位延迟 z⁻¹，codegen
  拓扑剔除回边断环 + 启动期播种零初值，runtime 零改动；validator 守住 latest/同进程/必须真正闭环。
  显式 literal 初值、fifo N 拍延迟、跨进程延迟环各自另立后续版本；codegen 不得把未标 feedback 的环
  隐式当合法反馈。
- `v0.20.1` 补齐反馈环 v2：回边 `init`（literal 初值）与 `fifo` + `depth = N`（N 拍延迟），fifo
  反馈强制两端等周期。**跨进程延迟环明确留待多机 / 图级容错版本**（断环 + 播种须过 supervisor /
  transport 边界，与跨进程 determinism、健康传播强耦合）；反馈 `init` 的嵌套 / 数组字段初值也留后续。
  做到那一版时从此处接续，不要重新发明跨进程反馈语义。
- `v1.0.0` 才进入正式稳定线：ABI/schema 冻结、兼容策略、故障注入、性能矩阵和
  长期 release policy。0.x 版本继续承载功能突破和 SDK 体验完善。

`v0.4.0` 的 Service runtime 目标是：生成 Rust/C++ service client/server 用户 API；
Service transport 支持 `inproc` 与 `zenoh`；`iox2` 暂不作为 Service transport，
继续专注 fixed-size shared-memory dataflow；request arrival 直接驱动 server，不靠
tick polling；补齐 request id、correlation、timeout、server unavailable 和
structured error 语义；self-description、`flowrt list` 和 `flowrt status` 展示
service endpoints 与 health。ROS2 Service bridge 先固定语义和 manifest 边界；
external process adapter 先预留 process/service 接入边界；`flowrt pub` 与
record/replay 只预留接口。

Service 与参数热更新有相似的控制面形状，但职责不同：参数热更新是 runtime
control-plane service-like RPC，服务于运行中配置管理；Service 是 graph 业务语义，
服务于用户组件之间的 typed request/response。两者可以复用 schema、validation、
structured error、pending/apply 和 self-description 经验，但不能混成同一个概念。

`v0.5.0` 聚焦三条 runtime 主线：launch-grade supervisor、参数控制面和高频调度
硬化；同时新增 FlowRT core skills 套组，服务 AI 辅助开发和编码。

supervisor 主线：让 generated supervisor 足以替代常见 launch 脚本的核心职责。已拍板
支持 `process_started`、`runtime_ready`、`service_ready` 三级 readiness gate、错峰
启动、env 注入、restart/failure policy、显式 CPU affinity 绑核，以及 CPU priority
采用 `nice` + 可选 Linux RT policy / priority。

参数控制面主线：参数系统从单应用热更新扩展到可发现、可校验、可远程操作的 runtime
control-plane。参数远程控制必须支持跨机器，推荐走 zenoh control-plane。

高频调度硬化主线：deadline、stale、backpressure 和 fairness 要进入 runtime 行为和
status 观测，补齐多 lane 调度的语义硬化。

已知后续项：当前 task health 的 backpressure/overflow 语义主要覆盖 inproc FIFO
写入路径；`iox2` / `zenoh` route 仍主要按 backend publish error 处理。后续深化
backend health / reconnect 时，应把 route-level backend backpressure、publish
error 和恢复状态统一纳入 health/self-description/status，而不是继续只暴露粗粒度
backend error。

FlowRT core skills 套组：入库事实源为 `.agents/skills/`，当前只落地 `frt-core-*`。
`frt-core-*` 面向 FlowRT 仓库维护者，覆盖 RSDL/IR、codegen、runtime、backend、
CLI、CI 和 release。`frt-app-*` 是 1.0.0 之后的保留命名空间；在 schema、runtime、
安装包和兼容策略稳定前，不把 app 开发流程固化成 skill。该套组只追求 FlowRT 范围
内的通用性和抽象性，不写成任意项目都适用的泛用技能。

非目标：Web/HTTP/WebSocket/UI、硬件 backend、新语言 binding、新 backend、ROS2
bridge 扩展。`v0.6.0` 仍不做 `flowrt replay`、simulated clock 驱动 scheduler 或
deterministic replay report。Web、HTTP、WebSocket 和 UI 不进入 FlowRT core，只能
作为应用层或外部工具消费 FlowRT 暴露的 typed introspection / params / record API。

`v0.6.0` 不复制 ROS2 Action。FlowRT 需要的是一等 Operation 语义：typed
long-running command、generated state machine、explicit policy、observable handle，
底层编译期 lower 成 Service + Channel。用户只看 Operation，调试时才展开底层拓扑。
Operation policy 必须显式声明 concurrency、preempt、cancel、timeout 和
result retention；用户不得手写 start/cancel/result/progress 四套底层协议。当前
generated Operation runtime 已收口长期 lifecycle：`idle`、`starting`、`running`、
`cancel_requested`、`cancelled`、`succeeded`、`failed`、`timed_out`。start 建立
invocation id、owner 和 deadline；默认 single-owner control authority 只允许同一 scope
单 owner 控制 active invocation，第二 owner start 返回结构化冲突错误。cancel 只作用于
当前 invocation id，stale id 被拒绝或返回明确说明，不会误取消后续 invocation。
timeout/deadline 由 runtime hidden task 驱动；用户 handler 通过 cancel token 做
cooperative cancel check，并通过 typed progress publisher 发布 progress。当前只支持单
in-flight reject 子集：`concurrency = "reject"`、`preempt = "reject"`、
`max_in_flight = 1`；`queue`、`cancel_running` 和多 in-flight 仍保留为长期 IR 语义，
在 runtime 完整实现前由 validator 拒绝。

Operation 解决的不是“长时间 service call”，而是机器人系统里常见的可取消、
可抢占、可观测、可恢复的长任务。生成器负责把 Operation lowered 成 request、
progress、feedback、cancel、result 和状态观测通道；用户只实现业务 handler 和策略
钩子，不手写底层协议。这样保留 Action 的实用能力，同时避免让用户维护分散的
start/cancel/result/progress glue。

Operation 观测路径沿用 FlowRT 自描述和本机 introspection socket：self-description
记录 operation client/server 端口、goal/feedback/result 类型、policy、backend 和
内部 lowering refs；runtime status 记录 ready/running/queued、当前 operation id、
当前 state、owner、deadline、成功/失败/取消/超时/抢占计数、最近事件、最近错误和
最近状态转换时间；record 输出 state change、progress、result 和 error 事件；
`flowrt op list/status/cancel` 提供本机观测和 cooperative cancel 控制面。`flowrt op start`、
跨机 Operation 控制面和 replay 驱动执行不属于当前范围。

`v0.6.0` 的录制系统只做 record，不做 replay。录制使用 MCAP 作为容器，FlowRT 自有
record envelope 作为 schema，覆盖 channel sample、parameter control-plane event、
service event、operation event、scheduler/time metadata 和 runtime/process metadata。
默认未录制时，runtime 热路径不得持续复制 payload 或 per-sample 分配；开启录制后，
dropped event 必须计数并进入 introspection/status。record event 必须携带
self-description hash、entity id、monotonic timestamp 和 wall-clock timestamp，为
未来 replay / simulated clock / deterministic debug 留稳定输入，但本版本不实现
`flowrt replay`。

`v0.6.0` 已完成 record-only 录制主路径：Rust/C++ runtime 都提供
`IntrospectionStatus.recorder`、recorder start/stop/drain socket 控制面、有界事件
队列和 active filters；生成的 Rust/C++ runtime shell 在 channel publish 路径同时
接入 echo probe 与 recorder tap；C++ recorder drain JSON 与 Rust `RecordEnvelope`
保持同一 schema。`flowrt record` 可以自动发现唯一 live runtime，也可用 `--socket`
显式选择进程，并把 channel / Operation / control-plane / scheduler / runtime 事件写入
MCAP 文件。

FlowRT 主项目不做硬件 backend。Linux 和外部 driver package 管硬件；FlowRT 管结构、
执行、通信、观测、external process 生命周期和 typed 接入边界。

`v0.8.0` 的目标是把 `v0.7.x` 的 external process / bundle / deploy baseline 推进为
真实机器人应用可用的接入边界版本。它不是继续堆 runtime demo，也不是把 FlowRT 做成
ROS2 兼容层；它要解决 fixed ABI 控制岛之外的真实阻塞点：

- **I/O boundary component 正式化**：在 `native component` 与 `external process
  component` 之间补齐进程内 I/O boundary component 语义，用于承载自研串口、SHM、
  UDP、Linux 采样、推理 SDK 等副作用边界。RSDL/IR/manifest 必须能表达其生命周期、
  side effect、resource requirement、health、readiness、restart、graceful shutdown
  和 profile/env 注入；串口 byte frame、网络 wire 和设备私有协议不得暴露成 FlowRT
  dataflow message。
- **Variable frame 工程化**：把 `sequence<fixed struct>`、`string`、`bytes` 和嵌套
  variable frame 做成 Rust/C++ codegen、message ABI conformance、backend resolver、
  self-description、`echo`/`record`/`status` 观测工具的可靠主路径。`iox2` 继续只承载
  fixed-size plain data；涉及无界变长数据的 route 自动选择支持 variable frame 的
  backend，不为 `iox2` 重新引入临时 envelope。
- **FrameDescriptor + side-channel lease**：图像、mask 和其他大 payload 不作为普通
  channel payload 承载。FlowRT channel 传递 descriptor、resource id、slot、generation、
  size、format 和 metadata；attach / acquire / release / lease keepalive 归 I/O boundary
  或 external package 管理。录制系统默认记录 descriptor 和事件，是否记录 payload 必须
  显式建模。
- **ROS2 coexistence bridge 扩展**：迁移期桥接是显式 adapter/profile，不污染 RSDL 核心
  语义。唯一桥梁仍固定为 `zenoh`；不增加 DDS fallback。优先补齐 `FlowRT -> ROS2` 与
  `ROS2 -> FlowRT` 的 typed subset，包括固定 header 映射、常见 pose/scan/image
  descriptor 和必要自定义消息；FlowRT 内部 ABI 不机械复制 ROS2 wire。
- **诊断和调试深化**：`flowrt status` / `echo` / `hz` / `record` 必须更直接回答真实
  调试问题：哪个 input 缺失、哪个 latest stale、哪个 route drop/overflow、backend
  为什么被选中、哪个 boundary 不健康、哪个 resource acquire 失败、哪个 process 正在
  restart 或等待 readiness。
- **多目标部署闭环**：继续推进原 `v0.8.0` 的部署主线。bundle manifest 应成为部署事实
  源，支持 Linux `amd64` / `arm64` 优先的 target platform、external package 多平台
  选择、cross build orchestration、远端 FlowRT 版本校验和 release 安装包 smoke。

当前 `v0.8.0` 已落地边界：

- RSDL / Contract IR 支持 `kind = "io_boundary"` component，并记录 resource
  requirement、side effect、readiness、health 和 shutdown policy；launch manifest 与
  self-description 输出对应摘要。Rust/C++ runtime 已提供 `BoundaryContext` 和
  introspection `io_boundaries` 状态；generated shell 会为 I/O boundary 生命周期钩子传入
  boundary context，并在 `component_started` readiness 下自动标记 ready。
- target platform 输入统一归一化为 `linux-amd64` / `linux-arm64`；`linux-x86_64` 和
  `linux-aarch64` 只作为旧输入别名接受，落盘 IR、自描述和 bundle manifest 均输出
  canonical 字符串。
- `flowrt bundle` 输出 schema v2，保留旧 deploy 字段，并新增 artifact 列表记录
  target、platform、相对路径和 sha256；external package executable 在 bundle 阶段按
  target platform 校验支持矩阵。
- `flowrt deploy` 对 schema v2 bundle 以 artifact 列表为部署事实源，按请求 target
  选择产物，并校验 platform、相对路径、文件存在性和 sha256；schema v1 bundle 继续按
  顶层 target 字段兼容。
- variable frame 的 Rust/C++ generated message API 覆盖 `bytes`、`string`、
  `sequence<primitive>` 和 `sequence<fixed struct>`；runtime frame codec 的 canonical
  tail order 已在 Rust/C++ smoke 中覆盖。`iox2` 仍只承载 fixed-size plain data，变长
  route 自动选择支持 variable frame 的 backend。
- CI 增加 `v0.8.0 Integration Smoke` amd64/arm64 focused gate，并在安装后 demo smoke
  中运行 `scripts/test-v080-installed-smoke.sh`，覆盖 variable frame、I/O boundary
  status、FrameDescriptor self-description、bundle schema v2 和 deploy dry-run。

当前 `v0.8.1` 已落地边界：

- 标准 FrameDescriptor message 是固定 64 字节 plain data，字段为
  `resource_id_hash`、`slot`、`generation`、`size_bytes`、`timestamp_unix_ns`、
  `width`、`height`、`stride_bytes`、`format_id`、`encoding_id` 和 `flags`。
- `io_boundary` resource descriptor 通过 `port` 绑定输出端口；validator 会拒绝缺失
  output port、非标准字段、变长字段或不满足 fixed-size plain data 的 descriptor。
- `iox2` 可以承载标准 FrameDescriptor route，因为 channel 内只传固定 descriptor；
  图像、mask 等真实 payload 不走普通 FlowRT message，也不通过 `bytes` 绕进 iox2。
- Rust/C++ runtime 和 codegen 都提供 `FrameDescriptorFields` helper；生成 message 可
  从 helper 构造，并可还原 fields 供用户逻辑或 boundary event 使用。
- `flowrt echo` 会结构化展示标准 descriptor 字段；`flowrt status` 会展示 resource
  descriptor schema；`flowrt record` 默认记录 descriptor/event，摘要中标注
  `descriptor_payload=descriptor_only`。
- 新增 `examples/frame_descriptor_demo` 和 `scripts/bench-frame-descriptor.sh`；
  CI 增加 `v0.8.1 FrameDescriptor Smoke` amd64/arm64 focused gate，安装后 demo smoke
  增加 `scripts/test-v081-installed-smoke.sh`。

当前 `v0.8.4` 已落地边界：

- `flowrt deps` 和 `flowrt build` 支持 `--target linux-amd64|linux-arm64`。显式
  `--target` 优先于 Contract IR target platform；仍无 platform 时保持 native 构建。
- CLI 内部有 toolchain profile 配置层，维护 target platform 到 Rust target triple、
  Debian multiarch、默认 C/C++ compiler、sysroot、CMake toolchain file、pkg-config
  路径、SDK overlay 和 runtime dependency policy 的映射；profile 配置不写入 RSDL
  或 Contract IR。
- component build 可以声明可移植的 `pkg_config` 依赖名；codegen 会据此生成
  CMake 的 `find_package(PkgConfig)` / `pkg_check_modules(...)` / `PkgConfig::...`
  链接路径，但不会把板端路径写进 RSDL。
- toolchain profile 还支持 `cpp_compile_args`、`cpp_link_args` 和
  `cpp_link_libraries`，用于把板端私有 SDK 需要的编译选项、链接选项、私有库路径
  和 FlowRT 运行时链接约束放在配置层，不污染组件语义。
- Rust/Cargo build 和 deps prewarm 会使用对应 Rust target triple，并把 cache key、
  ready marker、Cargo target dir 和本地二进制输出按 target 隔离。
- C++/CMake cross build 会优先使用 `/opt/flowrt/<version>/targets/<platform>` 的
  target SDK root、CMake wrapper、pkg-config、include 和 lib 事实源；SDK 缺失或
  `complete = false` 时 fail-fast。
- amd64 deb 内嵌 `linux-amd64` 和 `linux-arm64` 两个 complete target SDK；arm64 deb
  当前只保证 `linux-arm64` complete，不承诺反向 `linux-arm64 -> linux-amd64`。
- 新增 `flowrt doctor --target <platform>`，用于预检 Rust target、C/C++ 交叉编译器、
  target SDK、sysroot、CMake toolchain、pkg-config 路径和 SDK overlay。
- `flowrt bundle` 优先读取 `build-info.json` 中的 artifact closure；带 platform 的
  项目二进制复制到 bundle 的 `bin/<platform>/`，`deploy` 会校验 target、platform、
  路径层级和 sha256。
- CI 增加 `v0.8.3 Cross Toolchain Smoke` 和 `v0.8.3 Installed amd64 to arm64 Smoke`；
  package 阶段运行 target SDK layout smoke，安装版 cross smoke 运行
  `scripts/test-v083-installed-smoke.sh`，并用 ELF header 验证 C++ demo 输出为 AArch64。

`v0.7.0` 已落地 external package 主路径：

- RSDL 支持 `language = "external"` / `kind = "external"` 的 component 和 graph 级
  `[[external_process]]`。
- Contract IR 记录 external process 的 package、executable、args、working directory、
  health 和 required backend。
- external route auto resolver 默认选择 `zenoh`；显式 `inproc` 被拒绝，`iox2` 在
  package capability 与 fixed-size 条件未完整建模前默认拒绝。
- `flowrt external check/list` 校验和列出 `flowrt-external.toml` package manifest。
- launch manifest 和 self-description 暴露 external process/package 摘要。
- generated supervisor 能按 `FLOWRT_EXTERNAL_PATH`、`/opt/flowrt/external/<package>`、
  项目 `external/<package>` 查找 package，启动 executable，注入 `FLOWRT_*` 上下文，
  并纳入 restart/readiness/status 机制。
- `flowrt bundle` 输出离线 bundle 目录；`flowrt deploy` 提供 SSH/SCP baseline 和
  `--dry-run`。
- `examples/external_driver_demo` 覆盖 external package、supervisor、bundle 和 deploy
  dry-run。

## 当前仓库状态

仓库已经形成 FlowRT 工具链、Rust/C++ runtime shell、跨进程 backend、ROS2 zenoh
bridge、运行态观测、参数系统和安装包闭环。

主要目录：

```text
Cargo.toml
Cargo.lock
README.md
CHANGELOG.md
CONTEXT.md
AGENTS.md
crates/
  flowrt-cli/
  flowrt-rsdl/
  flowrt-ir/
  flowrt-validate/
  flowrt-codegen/
  flowrt-conformance/
runtime/
  cpp/
  rust/
examples/
docs/
scripts/
```

当前主要示例覆盖：

- `import_demo`：模块化 RSDL import。
- `workspace_demo`：workspace / module / composition、跨模块引用和同名 module symbol 生成命名。
- `imu_demo`：Rust 主 demo。
- `imu_demo_iox2`：Rust source、C++ controller、Rust monitor 通过 iox2 分进程运行。
- `profile_switch_demo`：同一 RSDL 在 inproc 与 iox2 profile 间切换。
- `cpp_counter_demo`：C++ only 用户逻辑。
- `c_counter_demo`：C callback v0、fixed-size `Count` message、CMake app run 和
  supervisor launch。
- `mixed_iox2_demo`：Rust source 与 C++ sink 通过 iox2 分进程连接。
- `mixed_zenoh_demo`：跨主机 copy backend、variable frame 和 mixed launch。
- `ros2_bridge_demo`：Rust source 到 ROS2 `/flowrt/text` 的 zenoh-only bridge。
- `external_driver_demo`：无硬件依赖的 external package、supervisor 和 bundle/deploy
  baseline。

仓库内可以用 `cargo run -p flowrt-cli -- ...` 调试 CLI。README、示例、对外文档和
最终回复默认使用安装后的 `flowrt ...` 命令。

## 已实现能力

工具链已经支持：

- RSDL 解析、import 展开、Contract IR 归一化和 canonical JSON。
- workspace / module / composition 装载、module name resolver 和跨模块
  `module::Name` 引用；module 内短名优先解析本 module，root/composition 层短名
  必须全局唯一。
- Contract IR validator 对名称、ID、版本、canonical ordering、deployment、
  capability、参数、target 和 derived metadata 的防篡改校验。
- Rust/C++ message ABI conformance 测试生成，覆盖 size、alignment、field offset、
  byte-level roundtrip、default initialization 和 IR-derived expected byte fixtures。
- fixed-size plain data 与 variable frame 主线。`bytes`、`string` 和 `sequence<T>`
  使用 canonical frame；`iox2` 只承载 fixed-size plain data，变长 route 自动选择
  支持变长消息的 backend。
- Rust/C++ generated runtime shell 的生命周期、task 调度、latest/FIFO channel、
  bind-level stale freshness、deadline 检查和参数 pending apply。
- Service 请求/响应语义：RSDL component 可声明 `service_client` / `service_server`，
  graph 可用 `[[bind.service]]` 绑定 client/server；Contract IR、validator、
  generated typed API、inproc runtime、self-description、`flowrt list` 和
  `flowrt status` 已覆盖 service 主路径，native generated `zenoh` service 仍 fail-fast。
- C/Python ABI 边界准备：`runtime/cpp/include/flowrt/abi.h` 定义 C ABI 版本、
  status/backend/health 整数编码、borrowed string/bytes view、reconnect policy、
  backend health snapshot，以及 C component context、fixed input view、output slot 和
  callback table；Rust runtime 提供对应 `repr(C)` 镜像类型和转换函数。当前已提供 C
  component v0 adapter、`app/c` 用户接入路径和最小 demo，但不提供完整 C runtime wrapper、
  动态加载或 Python binding。
- C++ only 和 C callback v0 contract 的 CMake app 路径，支持普通 `flowrt build` /
  `flowrt run`，以及 `flowrt build --launcher` 后由 `flowrt launch` 通过 generated
  supervisor 启动。
- language-separated mixed contract over `iox2` 或 `zenoh`，并拒绝同一 process group
  内混合 C++/Rust 以及 mixed `inproc` process boundary。
- Rust/C++ `iox2` typed pub/sub endpoint、QoS 映射、同名 `FlowRTIox2Header` user
  header、canonical service name 和 endpoint 自动恢复。
- Rust/C++ `zenoh` endpoint、deterministic `key_expr`、copy transport、自动恢复和
  本机 launch 自动 mesh。
- zenoh-only ROS2 bridge：生成 source process bridge tap 和 FlowRT 管理的 C++ ROS2
  adapter process，ROS2 侧强制 `rmw_zenoh_cpp`。
- runtime introspection socket、自描述、status、channel snapshot、echo observer、
  `hz` 采样、参数控制面和按需 recorder tap。
- generated supervisor 对 Rust、C++、C callback v0 和 ROS2 bridge process 的分流启动、health
  汇总、PID socket 轮询、tick stale/exit/restart 状态展示、process 依赖顺序、
  可配置 restart policy 和失败传播 baseline。

## CLI 状态

当前已实现的用户入口：

```bash
flowrt check [path/to/robot.rsdl]
flowrt explain [path/to/robot.rsdl] [--format text|json]
flowrt prepare [path/to/robot.rsdl]
flowrt deps [path/to/robot.rsdl]
flowrt deps --backend all
flowrt deps [path/to/robot.rsdl] --target linux-arm64
flowrt doctor --target linux-arm64
flowrt build [path/to/robot.rsdl]
flowrt build [path/to/robot.rsdl] --target linux-arm64
flowrt init [path] --lang rust|cpp|c
flowrt add message <Name> field:type ...
flowrt add module <Name>
flowrt add component <Name> --lang rust|cpp|c [--input name:Type] [--output name:Type]
flowrt run [path/to/robot.rsdl]
flowrt run [path/to/robot.rsdl] --process main
flowrt run [path/to/robot.rsdl] --run-steps 5 --process main
flowrt launch path/to/robot.rsdl
flowrt list path/to/generated-app
flowrt nodes path/to/generated-app
flowrt status
flowrt hz [channel] [--socket path/to/runtime.sock] [--window-ms 1000]
flowrt echo <channel> [--socket path/to/runtime.sock] [--image path/to/generated-app-or-selfdesc.json] [--follow]
flowrt record --output run.mcap [--socket path/to/runtime.sock] [--duration 10s] [--channel name] [--operation name] [--all] [--force]
flowrt params list --image path/to/generated-app-or-selfdesc.json
flowrt params get <instance.param> --image path/to/generated-app-or-selfdesc.json
flowrt params set <instance.param> <json-value> --image path/to/generated-app-or-selfdesc.json
flowrt inspect flowrt/contract/contract.ir.json
```

含 `flowrt.toml` 的项目中，`check`、`explain`、`prepare`、`build`、`run`、`deps` 和 `doctor`
可以省略 RSDL 路径。CLI 会使用最近父目录中的 `[project].main`；显式 RSDL 路径优先。
`deps` 和 `doctor` 找不到 manifest 时仍保留无契约预热或基础环境预检模式。

`prepare` / `build` / `run` / `launch` 支持 `--profile <name>`，用于显式选择 profile
并按该 profile 生成或校验产物。省略参数时会先投影到 `default` profile 或首个
profile。RSDL 未声明任何 profile 时，normalization 会插入隐式 `default` profile，
backend 为 `inproc`。

命令职责边界：

- `deps` 只写全局 FlowRT cache，不生成用户项目产物。cache root 默认
  `~/.cache/flowrt`，可用 `FLOWRT_CACHE_DIR` 覆盖。
- `deps` / `build` 的 `--target <platform>` 当前支持 `linux-amd64` 和 `linux-arm64`，
  Rust/Cargo 路径会使用 toolchain profile 中的 Rust target triple；显式 target
  优先，省略时从 Contract IR target platform 推导，仍无 platform 时保持 native。
- `doctor --target <platform>` 只做环境预检，不生成用户产物；缺失 Rust target、
  交叉编译器、完整 target SDK 或显式 SDK overlay 时返回非零状态。
- `prepare` 和 `build` 会写 `flowrt/` 输出目录，必须持有 OS advisory lock。
- `.flowrt.lock` 文件可残留，PID 只用于诊断，真实占用状态由锁判断。
- `check`、`inspect`、`run`、`launch`、`list`、`nodes`、`status`、`hz`、`echo`、
  `record` 和 `params` 不写生成物，不应获取生成物锁。
- `run` / `launch` 省略 `--run-steps` / `--run-ticks` 时长期运行，直到生成应用返回
  `Error` 或收到 SIGINT/SIGTERM。
- `--run-steps` 是推荐外部名称，`--run-ticks` 是兼容别名；`launch` 下 supervisor
  也会根据 live tick 快照终止达到上限后仍在运行的子进程。
- `flowrt status --live-only` 只输出成功返回 live status 的 runtime，默认 `status`
  仍保留 stale socket 诊断行。
- `flowrt run --process <name>` 运行生成应用中的单个 RSDL process group；mixed
  contract 必须选择一个单语言 process group。
- `launch` 运行 FlowRT 管理的 Rust supervisor。C++ only 和 C callback v0 contract 会生成
  supervisor-only Rust crate，launch 先构建 CMake app 再运行 supervisor；C callback v0
  process 的 `runtime_kind = "c"` 由 supervisor 显式映射到 CMake app binary。
- `inproc` 是单进程 backend；launch 和单独 process run 必须拒绝 inproc dataflow
  跨 RSDL process group。
- `build` 默认 release。Rust app、generated supervisor、C++ app 和 ROS2 bridge
  adapter 在 native 或无 cross target triple 时继续复制到兼容路径
  `flowrt/build/bin/<mode>/`；实际 cross target 构建复制到
  `flowrt/build/bin/<platform>/<mode>/`，避免 host/target 同名二进制互相覆盖。
  `flowrt/build/build-info.json` 记录 build mode、target name、platform、
  target identity、Rust target triple、host triple、deps target 目录和 executable
  相对路径。缺少匹配 deps ready marker 时，`build` 会 fail-fast，提示先运行
  `flowrt deps`。
- `bundle` 会优先使用 `build-info.json` 的 artifact closure，并把带 platform 的本项目
  二进制复制到 `bin/<platform>/<filename>`；manifest schema v2 的 artifact path、
  platform 和 sha256 与实际 bundle 文件保持一致。`deploy` 继续只以 artifact 列表为
  事实源，校验请求 target、platform 别名、路径安全、文件存在性和 sha256，发现目标
  产物缺失、platform 不匹配或 hash 不匹配时提示重新执行对应 platform 的
  `flowrt build --target <platform> --launcher`。
- Rust app、generated supervisor 和 deps prewarm 使用同一个 Rust target triple。
  target triple 会进入 cache key 和 ready marker；Cargo cross target 输出位于
  `CARGO_TARGET_DIR/<triple>/<profile>/`，CLI 会按该路径定位二进制。

## Runtime 和观测状态

`flowrt/launch/launch.json` 当前包含 process group 的 `runtimes`、`runtime_kind`、
`depends_on`、`restart` 和 `failure`，graph instance 的 `runtime`，graph 的
`channels`、`services`，以及 channel 的 backend 元数据。
iox2 channel 暴露 canonical service name，zenoh channel 和 ROS2 bridge 暴露
deterministic `key_expr`。

runtime introspection socket 使用 `$XDG_RUNTIME_DIR/flowrt/<pid>.sock` 或
`/tmp/flowrt.<uid>/<pid>.sock`。socket 路径只用于发现，真实身份来自 handshake。
启动 status server 时不能覆盖仍可连接的 live socket；SIGKILL 后残留且不可连接的
socket 文件可以回收。status server 对活跃连接线程设置上限，超过上限时返回结构化
error。Rust `IntrospectionState` 会在 mutex poison 后恢复访问。

Runtime 已提供 C ABI 基础边界和 Rust/C++ health/reconnect 抽象。C ABI 当前覆盖
`Status`、backend kind、backend health state、borrowed string/bytes view、
borrowed frame view、`ReconnectPolicy`、`BackendHealthSnapshot`、params view/update
result、operation status/progress/result summary view、diagnostic view、resource health
snapshot、C component task timing、C component context、fixed input view、output slot
和 callback table 的稳定 POD 形状；Rust/C++ runtime 内部仍使用各自语言的
高层类型，并通过转换函数或 C header 对齐。operation state 编码使用当前长期 lifecycle
常量：`IDLE`、`STARTING`、`RUNNING`、`CANCEL_REQUESTED`、`SUCCEEDED`、`FAILED`、
`CANCELLED` 和 `TIMED_OUT`，不保留旧 `ACCEPTED` / `CANCELING` / `PREEMPTED`
别名。C component callback ABI 当前为 `0.2`，callback table 必须设置 v0 callback 和
task timing 两个 feature bit；该 callback table 只表达边界，当前 C v0 已开放
adapter、`app/c` 用户接入路径和最小 demo，但不表示完整 C runtime、动态加载或 Python
binding 已开放。`iox2` 和 `zenoh`
endpoint 已接入自动恢复：本地 transport 资源丢失或操作失败会重建本地
publisher/subscriber/session；codec/schema 错误不得触发重连。

## ROS2 Bridge 状态

FlowRT 与 ROS2 bridge 的唯一通信桥梁固定为 `zenoh`。RSDL 使用 `[[bridge.ros2]]`
声明 bridge，当前支持：

- `direction = "flowrt_to_ros2"` 和 `direction = "ros2_to_flowrt"`。
- `ros2_type = "std_msgs/msg/String"` 与 `geometry_msgs/msg/Pose` 的 typed subset。
- `field` 指向 FlowRT message 中的 `string` 字段；Pose 映射要求 FlowRT message 结构与
  ROS2 Pose 的 position/orientation 字段匹配。
- `flowrt = "instance.port"` 继续表示普通 FlowRT 端口；island profile 下也可以写
  `flowrt = "boundary_endpoint_name"`，把 ROS2 topic 显式接到 typed boundary input/output。

normalization 生成的 bridge backend 必须是 `zenoh`；validator 必须拒绝 source target
不支持 `zenoh` 的 contract，不得添加 DDS fallback。codegen 会在 source process 中
生成 zenoh bridge tap，并额外生成 FlowRT 管理的 C++ ROS2 adapter process
`ros2_bridge`。boundary-bound bridge 不把 ROS2 输入伪装成普通 dataflow bind；generated
shell 会先把样本注入对应 `BoundaryInput`，再由正常 island boundary 路径驱动 task。
launch manifest 中该 process 的 `runtime_kind` 为 `ros2_bridge`。

generated supervisor 必须为 ROS2 bridge process 选择 CMake 产出的 adapter executable，
启动时设置 `RMW_IMPLEMENTATION=rmw_zenoh_cpp`；adapter 自身也必须校验 ROS2 侧实际
使用 `rmw_zenoh_cpp`。

## 打包与分发状态

FlowRT 作为标准 Linux 应用分发。当前单个 deb 包同时安装：

- `flowrt` CLI。
- Rust runtime crate。
- C++ runtime header 和 CMake package。
- Rust crate vendor。
- 私有 backend SDK 依赖。
- 基础文档。

包内文件安装到 `/opt/flowrt/<version>` 私有前缀，并通过 `/usr/bin/flowrt` 暴露入口。
`flowrt deps` 会使用包内 vendor 预热全局共享 cache；生成 Rust app 会复用该 cache；
生成 CMake 会使用 FlowRT 私有前缀解析 C++ runtime 和 backend SDK。生成 CMake 不应
通过 `FetchContent` 联网拉取 backend SDK；缺失依赖应要求安装 FlowRT 包或显式设置
`FLOWRT_CPP_RUNTIME_DIR` / `CMAKE_PREFIX_PATH`。
`flowrt bundle` 复制项目二进制后对 ELF 可执行文件 best-effort 执行
`strip --strip-unneeded`；非 ELF 文件跳过，strip 失败只进入命令摘要 warning，不修改
用户工作区原始产物。

## CI 和 Release 状态

当前 `.github/workflows/ci.yml` 将 Linux 验证拆为分层 job，覆盖生成物保护、Rust
格式化/测试/clippy、C++ runtime、v0.5.0 / v0.6.0 / v0.7.0 / v0.8.0 / v0.8.1
focused smoke、v0.8.3 交叉编译 focused smoke、v0.9.x island focused smoke、
v0.10.2 concurrency focused smoke、v0.12.0 authoring focused smoke、v0.13.0 robot
runtime completion focused smoke、v0.14.0 realtime scheduler focused smoke、v0.14.1
architecture focused smoke、v0.15.0 architecture convergence focused smoke、v0.15.1
CI release evidence smoke、v0.15.2 scheduler clock focused smoke、打包、C++ zenoh
runtime、demo smoke、ROS2 bridge smoke 和 release evidence。Linux job 默认
运行在官方 ROS2 Jazzy base 容器上；ROS2 bridge smoke 覆盖 Jazzy 和 Lyrical 两个
发行版，并安装对应的 `rmw_zenoh_cpp`。

CI 的架构相关 job 使用 `amd64` / `arm64` 双矩阵：Rust fmt/test/clippy、C++ runtime、
v0.5.0 / v0.6.0 / v0.7.0 / v0.8.0 / v0.8.1 / v0.9.x / v0.10.2 / v0.12.0 /
v0.13.0 / v0.14.0 / v0.14.1 / v0.15.0 focused smoke、Debian package、C++ zenoh
runtime、demo smoke、ROS2 Jazzy bridge 和 ROS2 Lyrical bridge 都在对应架构 runner 上执行。
单架构 release contract job 包含 v0.15.1 CI release evidence smoke 和 v0.15.2 scheduler
clock focused smoke。
`v0.8.3 Cross Toolchain Smoke` 固定在 amd64 host 上准备
`aarch64-unknown-linux-gnu` Rust target、`aarch64-linux-gnu` C/C++ 交叉编译器和
`pkg-config`，并运行 `flowrt-cli` 的 toolchain、build model、command 和 CMake target
SDK focused tests。`v0.8.3 Installed amd64 to arm64 Smoke` 下载 package job 产出的
amd64 deb，安装后运行 `flowrt doctor --target linux-arm64` 和真实
`flowrt build --target linux-arm64` C++ demo，并用 ELF header 验证输出为 AArch64。
package job 分别上传 `flowrt-linux-amd64-deb` 和 `flowrt-linux-arm64-deb` artifact。
demo smoke 先安装同架构 deb，再用安装后的 `flowrt deps` 预热依赖，然后用
`flowrt ...` 跑示例。正式推送 `v*` tag 前必须先把待发布提交推到 `dev/vX.Y.Z`：
普通 push CI 会运行完整矩阵，最后由 `Release Evidence Gate` 校验版本、release notes、
deb 架构、deb 版本和 `SHA256SUMS`，并上传 `flowrt-release-evidence`。tag release
由独立 `release.yml` 处理，解析 tag 指向的 commit SHA 后只查询同一 commit SHA 上成功的
push CI，要求其中 `Release Evidence Gate` 成功，再下载同一 run 的 deb 与 evidence
artifact 创建 GitHub Release。tag workflow 不重跑完整矩阵，不再依赖手工
`workflow_dispatch`。
tag 版本必须匹配根 `Cargo.toml` 的 workspace version。

`v0.5.0 Runtime Smoke` focused gate 使用 `-j1` 聚焦 supervisor readiness/resource、
远程参数控制面、status/hz 健康展示、scheduler health 和 runtime introspection 相关
测试；`v0.6.0 Runtime Smoke` focused gate 聚焦 Operation RSDL/IR/validator/codegen/
runtime/CLI/status、record format、runtime recorder tap 和 CLI MCAP 写入路径，使
这些新增能力的 CI 失败原因比全量 Rust test 更可定位。`v0.7.0 External/Deploy Smoke`
聚焦 external process、bundle 和 deploy；`v0.8.0 Integration Smoke` 聚焦 I/O
boundary、variable frame、FrameDescriptor、ROS2 typed bridge、diagnostics、bundle
和 deploy；`v0.8.1 FrameDescriptor Smoke` 聚焦标准 descriptor demo、CLI 结构化
echo/status、descriptor-only record 和 microbench；`v0.8.3 Cross Toolchain Smoke`
聚焦安装版交叉编译 toolchain profile、Rust target、C/C++ 交叉编译器和 CMake target
SDK 语义；`v0.8.3 Installed amd64 to arm64 Smoke` 覆盖安装版真实交叉构建。
`v0.10.2 Concurrency Hardening Smoke` 覆盖 codegen 并发 focused tests、Rust iox2
generated shell、backend route、Rust/C++ runtime executor，以及临时 generated Rust/C++
shell 构建，确保 two-phase commit 和 scheduler-local transport commit 进入 release
gate。`v0.13.0 Robot Runtime Completion Smoke` 覆盖 replay、temporary island overlay、
抽象 resource contract、external/boundary health、variable frame、params runtime apply、
Operation lifecycle、diagnostics/status/record、bundle/deploy/doctor/cross 和 C ABI。
`v0.14.0 Realtime Scheduler Smoke` 覆盖 executor admission/completion、Rust/C++
generated scheduler 非阻塞主路径、status/introspection timing 字段和 C ABI task
timing layout。`v0.14.1 Architecture Guard` 覆盖 architecture size guard 与既有大文件
拆分边界。`v0.15.0 Architecture Convergence Smoke` 通过 release gate registry 查询
focused smoke，并串联 architecture size guard 和 `scripts/check-architecture-contract.sh`；
后者检查 release gate contract、Contract IR derived facts 和 runtime observability facts 的
生产消费路径。`v0.15.1 CI Release Evidence Smoke` 检查 CI 不再暴露手工 RC 入口、tag
release 只消费同一 commit SHA 的 push CI release evidence，以及本地 helper 不再触发远端 CI。
`v0.15.2 Scheduler Clock Smoke` 覆盖 realtime generated scheduler 清空 boundary/replay
样本时间戳但不推进 runtime scheduling time，并确认 temporary island overlay 继续使用
fixture 时间驱动 simulated replay clock。`v0.23.0 Zenoh Service Smoke` 覆盖 zenoh
service validator server parallel gate、Rust/C++ generated endpoint 接线、golden、CLI
self-description service `key_expr` 展示、示例 `check/prepare` 和 manifest/self-description
`key_expr`。`v0.23.1 Route Health Smoke` 覆盖 Rust route backend health facts、CLI
route health 输出、Rust codegen transport publish health 接线和 C++ introspection parity。
`v0.23.2 C++ clang-tidy Smoke` 覆盖 C++ runtime headers/tests 和 generated C++
runtime shell lint。
发布前应运行
`scripts/check-architecture-contract.sh`、`scripts/check-release-readiness.sh <version>` 和
`scripts/check-release-candidate.sh <version> --wait --ref dev/v<version>`；
脚本会汇总版本来源、CHANGELOG 段、release notes 抽取、release gate registry、
release evidence 门禁和
v0.5.0 / v0.6.0 / v0.7.0 / v0.8.0 / v0.8.1 / v0.8.3 / v0.8.6 / v0.9.x / v0.10.2 /
v0.12.0 / v0.13.0 / v0.14.0 / v0.14.1 / v0.15.0 / v0.15.1 / v0.15.2 / v0.23.0 /
v0.23.1 / v0.23.2
focused gate 覆盖状态。
v0.12.0 当前覆盖通过既有 Rust/CLI 测试、App API 产物测试和 authoring smoke 收口：
`init`、`add`、`check`、`prepare`、`explain`、`flowrt.toml` 发现、显式 RSDL 优先级、
C ABI layout、C codegen adapter、C reference stub、C v0 fail-fast，以及
`import_demo`、`cpp_counter_demo` 和 `c_counter_demo` 的普通 build/run 路径都有验证入口。
当前发布先由发布分支 push CI 验证分支 HEAD 并产出 release evidence，再由推送对应
`vX.Y.Z` tag 触发 GitHub Release。

workflow 对 Rust 构建产物、FlowRT deps cache 和必要的公开 SDK overlay 做分层缓存；
release artifact、release notes 和 deb 成品在发布分支 push CI 中从源码重建；tag release
只消费同一 run 的产物。多架构 CI 的首要目标是保证发布包能在 amd64 与 arm64 原生
runner 上构建、安装和通过同等 smoke。

## 文档维护提示

`CONTEXT.md` 记录当前事实，不替代 `AGENTS.md` 的长期规范，也不替代
`CHANGELOG.md` 的发布记录。发生以下变化时应同步更新本文档：

- 版本背景或近期路线变化。
- CLI 命令、选项或职责边界变化。
- runtime、supervisor、backend、ROS2 bridge 或观测能力变化。
- 打包、安装路径、依赖解析或 release 流程变化。
- 示例矩阵或 CI smoke 覆盖变化。

阶段计划和设计草案仍然不入库；不要为了更新本文档而新增设计/计划文档。
