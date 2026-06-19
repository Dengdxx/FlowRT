# 变更日志

这里记录 FlowRT 项目的重要变更。

Git 历史使用 Conventional Commits；凡涉及代码、文档、命令、接口或生成物边界的变化，都要同步维护本文件。

## 未发布

## v0.23.3 - 2026-06-19

### 新增

- 新增 `profile.<name>.determinism` 的 `global_tick` 契约建模、validator 守门、
  launch manifest 投影和 supervisor tick coordinator。
- Rust/C++ runtime 与 generated shell 新增外部步进基础：`ExternalTick`、
  `ExternalTickReport` 和 `flowrt_run_tick(grant)`，`global_tick` profile 可由外部 grant
  推进单 tick，避免在该路径中依赖 wall-clock 轮询。
- 新增 `examples/global_tick_demo` 与 `scripts/test-v0233-global-tick-determinism-smoke.sh`，
  覆盖 global tick IR、validator、codegen、supervisor 和示例 prepare。
- 在 `global_tick` profile 下放开跨进程 FIFO feedback：validator 要求共享 global tick，
  codegen 为 zenoh/iox2 FIFO transport 生成 depth 配置，并按 depth 用 transport publish
  播种反馈初值。
- 新增 `[[redundancy.group]]` standby 冗余组契约建模，归一化进 Contract IR，并由
  validator 校验 `global_tick` profile、成员 component/port shape、standby 输出直连和
  实例多组 membership 边界。
- Rust/C++ runtime introspection 新增 `failovers` 事件快照；generated shell 为 standby
  redundancy group 生成 active route 状态，inactive standby 输出被抑制，primary critical
  fault 后在 tick 边界切到首个 standby 并记录 old/new active、tick_id 和 reason。
- `[graph.health]` 新增 `critical = ["instance"]`，归一化为 canonical instance refs；
  Rust/C++ introspection status 新增 `graph_critical_health`、`critical_instances`、
  per-instance `restart_count`、`last_fault_reason`、`last_fault_tick` 和
  `last_transition_tick`，`graph_health` 保持全实例 worst-of 语义。
- fault injection overlay 的每个 `[[inject]]` 新增 `kind`，默认 `status_error`，并支持
  `startup_error`、`shutdown_error`、`panic`、`deadline_miss` 和 `backend_drop`；`deadline_miss`
  复用 deadline 守门累计 `deadline_missed`，`backend_drop` 写入 route backend health degraded、
  drop 计数和 `fault_injection_backend_drop` 诊断 reason。
- Rust/C++ generated Operation 支持 `backend = "zenoh"` 的 start/cancel/status control path；
  用户侧仍使用 Operation typed API，生成物内部复用 zenoh service transport，不再生成
  placeholder 或 codegen fail-fast。
- Rust/C++ Operation runtime 支持 `queue`、`cancel_running`、`max_in_flight > 0` 和
  `result_retention_ms`：active slot 满后按 `queue_depth` 有界排队，抢占会先发出
  cooperative cancel，终态 status 按 retention deadline 保留。
- FrameDescriptor `record_payload = true` 支持显式 payload capture provider：
  `payload_capture = "boundary"` 时，I/O boundary 可把 payload artifact ref、content hash
  和大小写入 descriptor event；真实 payload 不进入普通 channel sample。

### 变更

- 开启 `v0.23.3 技术债收束`：本版本集中处理当前 debt / defer，并明确不纳入
  Python binding、Service over `iox2`、PTP/NTP、cross-host exact sync 或 hard realtime。
- `flowrt status` live summary 展示 graph health、critical subset 和 per-instance
  restart/fault 指标；generated shell 在 restart 尝试和 task error 路径写入实例级
  restart/fault metrics。
- Rust/C++ generated shell 为 `startup_error` / `shutdown_error` 在对应 lifecycle task
  调用前返回 `Status::Error`，为 `panic` 注入触发 worker panic/exception 路径；既有
  `status_error` 保持 per-task pre-execution 计数器语义。

### 测试

- 新增 standby failover codegen 断言和 Rust/C++ introspection smoke，覆盖 active route 生成、
  failover event 记录与跨语言 status 字段。
- 新增 graph health critical subset 的 RSDL/IR/validator、Rust/C++ runtime、CLI 和 codegen
  覆盖；同步受影响 generated shell golden，避免发布 hash 漂移。
- 新增 fault injection kind 的 IR/validator/CLI/codegen 覆盖，锁定 startup/shutdown 注入、
  panic、deadline miss、backend drop 注入，以及跨 process 注入的 `global_tick` 守门。
- 新增 Rust/C++ `zenoh_operation` golden 和 codegen 断言，覆盖 Operation over zenoh 的 typed
  client/server 接线、Operation control state 保留与内部 zenoh service envelope。
- 新增 Rust/C++ Operation policy 状态机测试，覆盖 queue promotion、cancel-running preempt、
  multi in-flight admission 和 result retention eviction；codegen 测试覆盖 self-description
  暴露真实 Operation policy。
- 新增 FrameDescriptor payload artifact 的 record schema、Rust/C++ runtime、validator、
  CLI summary 和 codegen self-description 覆盖。

## v0.23.2 - 2026-06-19

### 变更

- 新增 C++ `clang-tidy` focused gate，覆盖 C++ runtime headers/tests 和 generated C++
  runtime shell，使用低噪声 C++20 checks 并把 warnings 升为错误。

### 测试

- 新增 `v0.23.2 C++ clang-tidy Smoke`，CI 安装 clang-tidy 后通过 release gate registry
  运行 runtime/generated C++ lint。

## v0.23.1 - 2026-06-19

### 变更

- route backend health 统一进入 introspection route facts：`IntrospectionRouteStatus` 新增
  `backend_health_state`、`backend_health_error`、`backend_reconnect_attempt`、
  `backend_next_retry_unix_ms` 和 `backend_recoverable`，`flowrt status` route 行同步展示
  `iox2` / `zenoh` endpoint 当前恢复状态。
- Rust generated shell 在 `iox2` / `zenoh` dataflow publish 后记录 endpoint
  `BackendHealthSnapshot`，publish error 不再只留下粗粒度 `last_error`；C++ runtime
  introspection JSON 和 route diagnostics 派生保持同字段 parity。

### 测试

- 新增 `v0.23.1 Route Health Smoke` focused smoke，覆盖 Rust route facts、CLI route health
  输出、Rust codegen transport publish health 接线和 C++ introspection parity。

## v0.23.0 - 2026-06-18

Generated Service over zenoh：native Rust/C++ generated Service 不再只支持 inproc，同一
Contract IR 的 request/response bind 现在可以跨进程走 zenoh transport，并在
self-description 与 launch manifest 中暴露真实 request key expression。

### 新增

- Rust codegen 为 `backend = "zenoh"` 的 service client 生成按进程填充的
  `ZenohServiceClient` typed handle；server 进程在组件启动后打开
  `ZenohServiceServer`，handler 调用用户 `on_<port>_request`，并在 introspection 中
  `register_service` / `mark_service_ready`。
- C++ codegen 镜像 Rust 接线：生成 typed client wrapper 的 transport slot，server 进程
  打开 `flowrt::zenoh::ZenohServiceServer`，保持 mixed/language boundary 诚实，不为另一语言
  component 伪造接口。
- `launch/launch.json` 与 self-description 的 service endpoint 对 zenoh backend 暴露
  `key_expr = "flowrt/service/<escaped service-name>/request"`，与 runtime 实际 queryable
  key expression 一致。
- 新增 `examples/zenoh_service_demo`，覆盖 Rust client/server 分进程、generated Service over
  zenoh、typed API 和 service key expression 观测。

### 变更

- validator 要求 zenoh service server component 声明 `concurrency = "parallel"`，因为 server
  handler 由 transport queryable 回调线程驱动；exclusive server 会在 Contract IR validator
  阶段 fail-fast。
- Operation 的 zenoh generated runtime 仍未接线，继续 fail-fast；本版本只放行 native
  Service request/response。

### 测试

- 新增 `zenoh_service_{rust,cpp}` codegen golden，锁定两语言 generated endpoint 构造、
  manifest/selfdesc `key_expr` 和 service ready 标记。
- 新增 `v0.23.0 Zenoh Service Smoke` focused smoke，覆盖 validator 门、codegen 断言、
  golden、CLI `flowrt list` service `key_expr` 展示、示例 `check/prepare` 和 service
  `key_expr`。

## v0.22.1 - 2026-06-18

RSDL identifier 命名加固：validator 拒绝会被 codegen 直接生成为 Rust/C++ 标识符、且与任一目标语言保留关键字冲突的名称，避免合法契约在生成阶段产出无法编译的 shell。

### 修复

- `field`、data port、service port、operation port、instance 和 task 名称新增跨语言关键字检查，拒绝 `in`、`type`、`match`、`class`、`new`、`delete` 等 Rust 2024 或 C++ 保留关键字；错误在 Contract IR validator 阶段给出，而不是等到 Rust/C++ 编译报错。
- 保持 `profile.default`、target、process、lane、bridge、package 和 component source name 等非标识符用途名称的原有 `snake_case` 规则，避免把 manifest 字符串或 profile key 误判为生成代码标识符。
- Operation generated runtime 尚未实现 FIFO feedback channel 和 result retention；validator 现在拒绝 `feedback = "fifo"` 与显式 `result_retention_ms`，self-description 不再输出未生效的 result retention 时间。
- FrameDescriptor 真实 payload 录制尚未建模；validator 现在拒绝 `record_payload = true`，避免 `flowrt record` 仍按 descriptor-only 写入时 self-description 宣称可录真实 payload。

### 测试

- 覆盖 field / data-port / service-port / operation-port / instance / task 的关键字拒绝，以及 `profile.default` 继续合法。
- 覆盖 Operation 未实现 policy 和 FrameDescriptor payload 录制 opt-in 的 validator 拒绝。
- 新增 `v0.22.1 Reserved Keyword Naming` focused smoke，并接入 CI 与 release gate registry。

## v0.22.0 - 2026-06-18

确定性故障注入 + determinism 验证：提供一种 test-only 机制，在确定的 `(instance, task, 第 N 次调用)` 锚点强制 `Status::Error`，让用户无需手写「按时崩的组件」即可跑遍 0.21.x 全部故障反应策略（fail_fast / isolate / restart / degrade / 受控停机）并验证可复现。这是 0.21.x 图级容错主题的直接兑现，也为 v1.0.0 故障注入矩阵去风险。

### 新增

- RSDL/契约之外的 **test-only 故障注入 overlay**：场景为独立 TOML 文件（`[[inject]]` 表数组，按名引用 `instance`/`task`，支持 `invocations` 显式调用序号集合与 `from_invocation` 起点），经 CLI `flowrt prepare/build/run --inject <场景>` 投影进 `ContractArtifactIr.fault_injection`，与既有 `temporary_overlay` 并列、可叠加；投影置 `test_only = true`、`clock_source = simulated_replay`，`bundle`/`deploy` 默认拒绝（与 island 脚手架一致，需显式 `--allow-island`）。
- codegen 为每个注入目标 task 生成 per-task pre-execution 调用计数器（scheduler 线程自增）与注入门：命中调用序号时跳过用户回调、直接合成 `error` outcome（与回调真实返回 `Status::Error` 的空输出字节等价），交既有 0.21.x 故障反应机器处理。Rust 与 C++ 生成 shell 镜像，门 gated 于 `fault_injection.is_some()`，非注入产物字节不漂移。

### 变更

- 注入只在 simulated_replay 时钟下运行（确定性回放）；该时钟由 boundary 激励时间线步进，故 validator 要求注入契约 **≥1 boundary input（island）**，否则给出可执行错误而非生成不可驱动/不可编译产物。
- 注入只允许命中 **scheduled task**（`periodic` / `on_message` / `on_synchronized`）——0.21.x 故障反应机器只对这些 task 的回调 `Status::Error` 反应；`startup` / `shutdown` 不走该错误分发路径，被拒。

### 测试

- 覆盖 IR overlay 解析/canonical、validator 门（scheduled-only / 单进程 / 需 boundary input / canonical 调用序号 / EntityRef 一致）、CLI `--inject` 场景解析与接线。
- 新 golden `fault_injection_{restart,degrade_recover}_{rust,cpp}`（island 契约 + 同源 `inject.toml`）锁定两语言注入 shell 整份输出；纳入编译网真编译（C++ `g++ -fsyntax-only`、Rust `cargo check`）。
- 新增 `v0.22.0 Fault Injection Determinism` focused smoke 并接入 CI 与 release gate registry。
- determinism 验证策略：注入门纯调用计数驱动（同输入 → 同注入点）由 golden 锁定，底层 record→replay / executor 确定性由 v0.17/v0.18 内核测试证明，注入在其上确定性叠加；既有 golden 因版本钉版 0.21→0.22 机械重生，无行为漂移。

### 已知限制

- 确定性 v1 限**单进程**（无全局 tick lockstep，继承 v0.21.4）；多进程图的注入目标被拒，跨进程注入 determinism 留待后续。
- 注入只合成 `Status::Error`：panic（0.21.1 起不捕获）、deadline 超时、backend drop 不在范围（触发语义不同）；startup/shutdown task 注入留待后续。
- 注入事件不写 replay log（由确定性调用计数 + 既有回放重推导）；真实随机 / chaos 生产注入与性能矩阵不在本版。

## v0.21.4 - 2026-06-18

跨进程反馈环（0.21.x 图级容错主题第五/最后一片）：放行跨进程反馈边，让控制环可跨进程闭合。语义为 seeded latest-snapshot——source 进程启动期把 init 播过 transport，消费进程读最近到达样本，非严格 z⁻¹。

### 新增

- validator 放行**跨进程** `[[bind.dataflow]] feedback = true`（`channel = "latest"`），走支持跨进程的 transport backend（zenoh/iox2）；环闭合与 init 类型校验保留。
- 跨进程反馈边语义为 seeded latest-snapshot：source 进程启动期对反馈 channel 播 init 过 transport（复用既有反馈播种 + 跨进程 route 发布），消费进程经既有 latest 缓存接收；codegen/runtime 无需新增——既有按所属进程播种的逻辑对跨进程 zenoh channel 即真实 transport 发布。

### 变更

- 跨进程反馈边仅允许 `channel = "latest"`：进程间无共享 scheduler tick，fifo 的 N 拍延迟无支撑，跨进程 fifo 反馈拒绝（同进程 fifo 反馈不变）。`validate_feedback_fifo_periods`（两端同周期）仅适用于同进程 fifo 反馈。

### 测试

- 覆盖 validator 放行跨进程 latest feedback、拒绝跨进程 fifo feedback；同进程 latest/fifo 反馈仍正常。
- 新 golden `cross_process_feedback_{rust,cpp}`（controller/plant 分进程 + zenoh + init）锁定两语言生成输出：source 进程播种 init、消费进程不播种、消费端经 transport 接收。
- focused smoke 断言生成 shell 的源进程播种 / 消费进程不播种接线。既有同进程 feedback golden 字节不漂移。

### 已知限制

- 严格 z⁻¹（恒 1 拍、tick-0 读到 present 的初值）仍**仅同进程**成立（依赖共享 scheduler tick）。跨进程为 seeded latest-snapshot：延迟由 transport + tick skew 决定，tick-0 在 init 到达前为 absent（与一切跨进程输入冷启动一致）。
- 跨进程反馈不新增全局 tick lockstep 或跨进程 determinism 机制；replay determinism 继承既有 per-process 跨进程输入回放。
- 跨进程反馈走 transport（zenoh/iox2），其生成 shell 不纳入 inproc-only 编译网（需 transport SDK）；以 golden 文本与 smoke 接线断言把关。

## v0.21.3 - 2026-06-18

图级 health 聚合 + 受控停机（0.21.x 图级容错主题第四切片）：把每实例 lifecycle 聚合成单一 graph health 作为图级一等观测，并在其上提供一个图级反应策略——图内出现终态不可恢复故障时受控停机。

### 新增

- runtime introspection 新增图级 health 聚合：`IntrospectionStatus.graph_health` 为每实例 lifecycle 的 worst-of（`faulted` > `degraded` > `healthy`），并派生一条 `category = graph_health`、`entity_kind = graph` 的图级诊断（faulted→error / degraded→warn / healthy→info）；always-on，Rust 与 C++ 位级镜像。
- RSDL 新增顶层可选 `[graph]` 段及 `[graph.health].on_faulted`（`continue` 默认 / `stop`），归一化进 `GraphIr.health`（默认 continue 跳过 canonical JSON）。
- 生成 runtime shell（Rust 与 C++）在 `on_faulted = "stop"` 且图含 `isolate`/`restart` 实例时，于 tick 边界对终态不可恢复故障（`isolate` 故障即终态、`restart` 耗尽 `max_restarts`）触发图受控停机：置 `_graph_terminal_fault` 并 `shutdown.request()`，复用既有 graceful 逆序清理路径（`status` 仍 `Ok`，停机原因由 `graph_health = faulted` 诊断承载）。

### 变更

- validator 校验 `on_faulted` 取值，并拒绝无 `isolate`/`restart` 实例的 `stop`（`fail_fast` 故障已直接停图、`degrade` 永不终态，无可达终态时 `stop` 无意义）；单图单契约，composition/import 合并出现重复 `[graph]` 拒绝。
- 受控停机机制 gated：仅 `on_faulted = "stop"` 且存在终态可达实例时 emit，非 stop 图与既有 golden shell 字节不漂移。

### 测试

- 覆盖 RSDL `[graph].health` 解析与重复拒绝、IR normalize 图级反应枚举（含未知值拒绝）、validator 放行 `stop` 并拒绝无 isolate/restart 的 `stop`、runtime graph_health worst-of 聚合与诊断（Rust facts + C++ lifecycle ctest）。
- 新 golden `graph_health_stop_{rust,cpp}`（`restart` + `isolate` 实例 + `on_faulted = "stop"`）锁定两语言受控停机输出并纳入编译网真编译（C++ `g++ -fsyntax-only`、Rust `cargo check`）。

### 已知限制

- standby failover 本切片不实现：需要冗余实例 role 语义与运行态 dataflow bind 重定向，二者均未定义，半成品违反「语义未定前不堆 runtime 代码」；记录为显式 defer，留待后续切片或正式版本。
- 图受控停机当前对任一终态故障即触发，不区分 critical 实例子集（future 可加 `critical` 标记）；graph health 转移由确定性实例状态重新推导（不写入 replay log）；跨进程健康传播按 0.21.4 跨进程反馈环实际需要再补；数值健康 metric 仍未提供。

## v0.21.2 - 2026-06-18

降级数据语义（0.21.x 图级容错主题第三切片）：在 0.21.1 进程内隔离 / 重启之上放行 `degrade` 策略，让 instance 在故障时降级续跑而非停机或重启，下游复用既有 stale policy 老化 last-known-good 数据。

### 新增

- validator 放行第四故障策略 `degrade`（`failure_policy = "degrade"` 或 `[instance.<name>.fault] policy = "degrade"`）：故障触发面仍仅 task 回调返回 `Status::Error`。
- 生成 runtime shell（Rust 与 C++）为 `degrade` instance 注入降级续跑：命中 `Error` 时不挂起 task、不停图，仅把 instance 记为 `Degraded` 并继续调度；该 instance 后续 task 返回 `Ok` 时翻回 `Running`。
- `flowrt status` 把 `degraded` 生命周期态派生为 `warn` 诊断（介于健康 `info` 与停机 `error` 之间），Rust 与 C++ 一致。

### 变更

- `degrade` 失败那拍输出为空，下游 latest 快照保持上一份并随时间老化，由下游既有 channel stale policy（`hold_last`/`warn`/`drop`/`error` + `max_age_ms`）决定 absent vs hold-last——复用而非新增传输或 ABI。
- `recoverable_instances` 收录谓词加入 `degrade`；生成 shell 用单个 `_degraded` bool 做边沿跟踪，`suspend_task` 收紧到 `isolate`/`restart`。容错机制仍 gated，既有 golden shell 字节不漂移。
- 本切片不改 runtime/executor（`degrade` 无需 suspend/resume），属纯 codegen/validate 变更。

### 测试

- 覆盖 validator 放行 degrade、codegen `recoverable_instances` 收录 degrade、新 golden `instance_degrade_{rust,cpp}`（下游 `hold_last` stale bind），并把新 golden case 纳入编译网真编译（C++ `g++ -fsyntax-only`、Rust `cargo check`）。

### 已知限制

- 沿用 0.21.1 观测姿态：仅以 `record_lifecycle_state` 的 `Degraded`/`Running` 状态循环作为降级可观测证据；降级转移由确定性 task 结果重新推导（不写入 replay log），数值 metric 仍未提供。
- 多 task degrade instance 的态翻转按确定性 commit 顺序取该拍最后完成 task 的结果（单 task 为语料常态，多 task 为已知粗粒度边角）；`degrade` 与 feedback / sync 的交互未额外约束，与 0.21.1 的 `isolate`/`restart` 保持一致，留待 0.21.3 图级 health 统一处理。

## v0.21.1 - 2026-06-18

进程内 instance 故障隔离 + 重启（0.21.x 图级容错主题第二切片）：在 0.21.0 显式生命周期状态机之上，放行 `isolate`/`restart` 策略并落地运行时恢复行为。

### 新增

- RSDL `[instance.<name>.fault]` 子表：`policy` 取 `fail_fast`/`isolate`/`restart`，`restart` 可带 `max_restarts`/`initial_delay_ms`/`max_delay_ms`（省略填默认 `3`/`100`/`1000`）；保留扁平糖 `failure_policy = "..."`，两者互斥。
- validator 放行 `isolate`/`restart`，校验 restart 退避范围（`0 < initial_delay_ms <= max_delay_ms`）；`degrade` 仍建模保留并被拒绝（deferred 到 0.21.2）。
- runtime `DeterministicExecutor`（Rust 与 C++）新增 `suspend_task`/`resume_task`：隔离 task 退出调度且不被 `wake`/periodic due 准入，重启成功后恢复。
- 生成 runtime shell 在图含 `isolate`/`restart` instance 时注入故障处理：`isolate` 挂起故障 instance 的 task、图内其余继续；`restart` 额外按 clock-ms 退避重跑 `on_init`→`on_start`，连续失败达 `max_restarts` 进入终态 `Faulted`。

### 变更

- 故障触发面收敛为 task 回调返回 `Status::Error`；panic 不捕获、deadline miss 不视为故障。
- 退避按 `min(initial << consecutive.min(31), max)` 以 scheduler clock-ms 计量，保证 simulated_replay 重启时序确定。
- 生成 Rust trait / C++ interface 的 `on_init` 文档补可重入契约提示（restart 会在同一对象上重新调用）。
- 容错机制仅在图含 `isolate`/`restart` instance 时 emit，既有 golden shell 字节不漂移。

### 测试

- 覆盖 RSDL fault 解析、IR normalize（默认填充 / 双写拒绝 / 非 restart 带参拒绝）、validator 放行与越界拒绝、executor suspend/resume 与重启时序确定性、codegen `recoverable_instances` 与新 golden `instance_fault_restart_{rust,cpp}`、C++ runtime 隔离 ctest，并把新 golden case 纳入编译网真编译。

### 已知限制

- 本切片仅以 `record_lifecycle_state` 的 `Faulted`/`Running` 状态循环作为故障可观测证据；`restart_count`、`last_fault_reason` 等数值 metric 暂未提供，后续切片补齐。

## v0.21.0 - 2026-06-17

生命周期状态机底座（0.21.x 图级容错 / 生命周期主题首切片）：instance 生命周期升为契约一等显式状态机，零恢复行为改变。

### 新增

- runtime 跨语言 `LifecycleState` 枚举（`Uninitialized`/`Initialized`/`Running`/`Stopped`/`ShutDown`/`Faulted`，`Degraded` 保留），Rust 与 C++ 离散值逐一镜像。
- RSDL `instance.<name>.failure_policy` 字段与 IR `InstanceFailurePolicy` 枚举；0.21.0 仅放行 `fail_fast`（= 既有逆序清理语义），`isolate`/`restart`/`degrade` 建模保留并由 validator 拒绝（deferred 到 0.21.x）。
- 生成 runtime shell 在 on_init/start/stop/shutdown 与失败路径记录 per-instance 生命周期状态转移。
- `flowrt status` 暴露 per-instance 生命周期状态（`category=lifecycle`、`entity_kind=instance` diagnostic，`faulted` 为 error）。

### 变更

- 生成 shell 在生命周期段新增 `record_lifecycle_state` 旁路记录；调用条件、逆序清理与状态聚合逻辑不变。

### 测试

- 覆盖 failure_policy 解析/normalize/validator、`LifecycleState` 离散值、introspection 记录与 diagnostic 派生、两语言 codegen 记录断言、golden 锁定、C++ `lifecycle_smoke` ctest 与生成 shell 真编译。

## v0.20.1 - 2026-06-17

反馈环 v2：在 0.20.0 的零初值单拍反馈之上，回边支持 literal 初值与 fifo N 拍延迟。

### 新增

- RSDL `[[bind.dataflow]]` 反馈回边新增 `init`（字段名→字面值表）：按源消息类型
  播种初值，省略 `init` 仍播零初值。详见 `docs/examples.md` 的 `feedback_loop_demo` 段。
- 反馈回边支持 `channel = "fifo"` + `depth = N`：表达 N 拍延迟（回边 fifo 缓冲播种
  N 个初值，每拍 pop 上游 N 拍前输出）。
- 生成验证网新增 feedback v2 case（`feedback_v2_rust` / `feedback_v2_cpp`，fifo
  depth=2 + literal init）：golden 锁定两语言播种输出，编译网真编译。

### 变更

- validator 放宽反馈边规则：允许 `latest`（1 拍）或 `fifo`（depth≥1，N 拍）；`init`
  按源消息 `TypeIr` 递归类型校验（全 primitive 字段、字段齐、类型兼容）；fifo 反馈
  要求两端 task `periodic` 且 `period_ms` 相等，避免 N 拍延迟随节奏漂移。
- codegen 播种支持 literal 初值（Rust 结构字面量、C++ 默认构造后逐字段赋值）与 fifo
  按 depth 播种 N 份，两语言一致；fifo 反馈读复用既有 fifo pop 路径。

### 限制

- 反馈 `init` 仅支持全 primitive 字段的消息（嵌套/数组字段初值留后续）；跨进程
  延迟环仍未支持，留待多机/容错版本。

## v0.20.0 - 2026-06-17

反馈环（Feedback Loops / Cyclic Graphs）：graph 允许显式回边构成闭环。回边标
`feedback = true` 建模为单位延迟 z⁻¹——消费者读上游上一拍输出，runtime 无需新原语。

### 新增

- RSDL `[[bind.dataflow]]` 增 `feedback`（bool）：标记一条回边为单位延迟。配合前向边
  即可声明 controller↔plant 等闭环。详见 `docs/examples.md` 的 `feedback_loop_demo` 段。
- 示例 `examples/feedback_loop_demo`：controller 读状态算控制量、plant 读控制量更新
  状态，`plant.state→controller.state` 标 `feedback = true` 闭合反馈环。
- 生成验证网新增 feedback case（`feedback_loop_rust` / `feedback_loop_cpp`）：golden
  锁定两语言断环与播种输出，编译网真编译（Rust `cargo check`、C++ `g++ -fsyntax-only`）。

### 变更

- Contract IR 增 `ChannelEdgeIr.feedback`；validator 在无环校验中剔除 feedback 边，并新增
  feedback 边专项校验：仅 `latest` channel、仅同进程（inproc）、且必须真正闭合一个环路
  （剔除后源仍经前向边可达终点；自环天然闭合）。
- codegen 把 feedback 边纳入单位延迟语义：拓扑排序剔除回边断环（图退化为 DAG），并在
  run 启动期对回边 channel 播种零初值（消息 `default` at tick 0），两语言一致。

### 限制

- v1 仅零初值、`latest` 单拍延迟、同进程反馈；显式 literal 初值、fifo N 拍延迟、跨进程
  延迟环留后续版本。

## v0.19.0 - 2026-06-17

多传感器同步（Multi-Sensor Synchronization）：把 N 路 sensor 输入按 event-time（0.18.0
sample-time）对齐成同步集，经新 `on_synchronized` trigger 投递给融合组件，是 0.18.0
sample-time 一等概念的直接 payoff。

### 新增

- RSDL `[[sync]]` 顶层组：声明一个 instance 的 ≥2 路输入按 sample-time 对齐（`name` /
  `instance` / `inputs` / `tolerance_ms`），配合 `instance.<name>.task` 的
  `trigger = "on_synchronized"` 与 `sync = "<组名>"`。详见 `docs/examples.md` 的
  `sync_fusion_demo` 段。
- runtime 同步原语 `flowrt::Synchronizer`（Rust 与 C++ 各实现，共享语义）：latest-aligned
  approx-window 匹配——所有输入最新样本落入 `tolerance` 窗口即发射对齐集并排空，否则丢弃
  落后输入的陈旧 backlog，迟到样本丢弃。只依赖 sample-time，realtime 与 `--replay` 行为
  一致；跨语言对同一事件序列产出位级一致的同步集序列（conformance golden 向量把关）。
- 示例 `examples/sync_fusion_demo`：imu/odom 周期产样，`[[sync]]` 对齐后 `on_synchronized`
  触发 fusion，结果投递 sink。
- 生成验证网新增 sync case（`sync_fusion_rust` / `sync_fusion_cpp`）：golden 锁定两语言
  同步器接线输出，编译网真编译（Rust `cargo check`、C++ `g++ -fsyntax-only`）。

### 变更

- Contract IR 增 `SyncGroupIr` 与 `TaskIr.sync_group`；validator 校验 sync 组（instance
  存在、inputs≥2、端口已声明、唯一 incoming bind、消息类型须声明 timestamp 源、
  `tolerance_ms > 0`），并校验 `on_synchronized` task 必须引用同实例 sync 组、不列 input，
  非 `on_synchronized` task 不得带 `sync`。
- codegen 把 `on_synchronized` 接入两语言生成 shell：复用 on_message 的 wake/snapshot/调度
  机器，task body 增 synchronizer gate（按 sample-time 推样本、poll 出对齐集才调用户回调），
  同步算法在 runtime 原语内、codegen 只实例化接线。

## v0.18.1 - 2026-06-16

纯内部质量/验证版本，零用户语义变更（RSDL / Contract IR / runtime 行为不变）。

### 新增

- codegen golden 等价 harness（`crates/flowrt-codegen/tests/golden/`）：每个 corpus case 跑
  `emit_artifacts` 后逐 artifact 与基线比字节，`FLOWRT_UPDATE_GOLDEN=1` 重生。锁定整份生成输出作
  codegen 改动的回归 oracle（字符串断言只查片段，golden 锁全文）。6 个 case 覆盖两语言 shell、
  on_message/periodic、latest/fifo + overflow/stale、service、sample-time boundary。
- 生成工程真编译网 `scripts/test-codegen-compile.sh`（接入 CI 与 release-gate registry）：对 corpus
  子集真编译生成 shell——C++ 经 `g++ -fsyntax-only`、Rust 经 `cargo check`（每 case 独立
  `CARGO_TARGET_DIR`，避免仓库共享 target 命中旧 fingerprint 造成假绿）。把「生成代码真编译」纳入
  开发回路与 CI，堵 v0.17.0/v0.18.0 连续两版漏发的 codegen 编译错类缺口；取代按版本散落的
  per-version smoke，作分支完整、单一、可扩展的编译门禁。
- 现代化 lint 门禁 `[workspace.lints.clippy]`（uninlined_format_args、manual_string_new、
  explicit_iter_loop、redundant_else）：forward-guard，防未来写法退化。

### 变更

- codegen 去重：overflow/stale policy 路径与 trigger 诊断名的 enum→string 映射收敛到 `runtime_plan`
  共享函数，删除 rust/cpp shell 两侧 6 处孪生（输出字符串两语言本就一致）。golden 逐字节一致证明
  零行为变更。
- 清理约 40 处 `uninlined_format_args`（`format!("{}", x)` → `format!("{x}")`），纯机械内联。

### 测试

- golden harness 自证能捕获生成输出漂移；编译网自证能捕获注入的 codegen 编译错（emitter trait 名
  改为不存在符号即转 RED）。

## v0.18.0 - 2026-06-16

### 新增

- Sensor event-time 源声明：RSDL 消息可用 `[type.<Name>.timestamp]` 子表声明承载 sample 时间戳
  的字段及时钟语义（`field` + `unit` ns/us/ms + `epoch` monotonic/unix + `clock_domain`），归一化进
  Contract IR（`TypeIr.timestamp`）。validator 校验 `field` 指向本消息的 unsigned 整数标量字段，并
  拒绝未知 unit/epoch 枚举值；`timestamp` 仍可作普通字段名（按值类型区分子表与字段）。
- 回放按 sensor sample-time（event-time）确定性步进：`ReplayEvent` / `ReplayTimelineEntry` 承载
  `sample_time`，`effective_time_ms()` 优先取 sample-time、否则回退 receive-time；`ReplayDriver`
  按 effective time 排序与逐周期步进（Rust 与 C++ 一致）。回放结果取决于 sensor 采集时刻而非
  录制时的到达时刻。
- 录制承载 sample-time：`RecordEnvelope.sample_time_ns`（可选字段，无值时不序列化）；
  `read_replay_timeline` 由 envelope 填充 `sample_time_ms` 并按 effective time 排序。声明了
  timestamp 源的 boundary input 被注入时，runtime 从 payload 提取 sample-time 写入 envelope。
- runtime 新增 `register_boundary_input_with_sample_time`（Rust 与 C++）：注册带 typed sample-time
  提取器的 boundary input；`publish_boundary_input` 录制激励时填入 `sample_time_ns`，供 event-time
  回放使用。

### 变更

- 生成 Rust/C++ runtime shell 对声明了 timestamp 源的 boundary input，改用
  `register_boundary_input_with_sample_time` 注册并附带 typed 提取器闭包（`decode_frame` 后读 stamp
  字段 × unit→ns）；未声明源的 boundary 仍走原 `register_boundary_input`。两语言共享同一 sample-time
  源解析，行为一致。

### 测试

- 新增 `scripts/test-v0180-event-time-smoke.sh` focused smoke（接入 release-gate registry 与 CI）：除
  IR/validator、record→replay、codegen 字符串断言外，真编译生成工程的 event-time 分支——C++ shell
  经 `g++` 语法校验、Rust island 经 `flowrt build` 出终产物，关闭「字符串断言不编译生成代码」缺口。
- 新增 codegen 双语言发射断言、Rust 录制 sample_time_ns 单测、C++ `introspection_smoke` 录制
  sample-time ctest。

## v0.17.1 - 2026-06-16

### 新增

- 运行时原生确定性回放的 C++ parity：新增 `flowrt/replay.hpp`，C++ runtime 自己拥有回放事件
  时间线并按确定性网格逐周期步进（`flowrt::ReplayDriver` / `ReplayEvent` / `Step`），语义与字节级
  步进行为与 Rust `flowrt::ReplayDriver` 对齐，由两侧 conformance 测试用同一事件序列断言一致。
- C++ `publish_boundary_input` 在 recorder 覆盖该 endpoint 时，把注入作为 canonical frame
  channel sample 记录（`try_record_channel_sample_frame_bytes`），闭合 C++ record→replay。
- `flowrt-record` 新增 JSONL 回放时间线读写（`write` / `read_replay_timeline_jsonl`）：作为 C++
  runtime 可解析的回放源格式（C++ 无 MCAP 解析能力），reader 忽略未知字段、为 sensor event-time
  预留向前兼容。

### 变更

- simulated_replay 生成 runtime shell（Rust 与 C++）采用回放双子模式，由 `FLOWRT_REPLAY_SOURCE`
  选择：设置时走 runtime 原生确定性回放（从回放源装配 `ReplayDriver`、逐周期步进、命中事件经
  `publish_boundary_input` 进程内注入）；未设置时回退到外部 socket 注入（`flowrt replay` /
  temporary island 交互式回放），按注入样本 data_time 推进逻辑时钟。realtime 调度路径不变。
- `flowrt run --replay <mcap>` 启动 C++ 应用时，先把 MCAP 规范化为 JSONL 时间线再注入；Rust
  应用仍直读 MCAP。单一 MCAP 解析点保留在 Rust（`flowrt-record`）。

### 修复

- 修复 v0.17.0 生成 Rust simulated_replay shell 无法编译：调度改调 `ReplayDriver` 的 inherent
  `step()` 而非未导入的 `TimeDriver::next_step` trait 方法，关闭「生成工程端到端编译」缺口
  （由 `v0.9.2` island offline 回放 smoke 在 push CI 中捕获）。
- 修复 v0.17.0 让 simulated_replay 强制要求 `FLOWRT_REPLAY_SOURCE`、缺失即报错的回归——该回归
  破坏 temporary island「`flowrt run` 空转 + `flowrt replay` socket 注入」交互式工作流；改为缺省
  回退到 socket 注入，两语言一致。

### 测试

- 新增 `flowrt_replay_smoke` ctest：用与 Rust 同一事件序列断言 C++ ReplayDriver 逐周期步进、
  同时刻多事件、shutdown 短路、boundary 过滤与 JSONL reader 一致；新增 `v0.17.1 C++ Replay
  Parity Smoke` focused gate。发射的 C++ 回放片段经编译校验，关闭 v0.17.0 的 C++ 发射编译缺口。

### 文档

- `v0.17.1` 补齐 `v0.17.0` 运行时原生确定性回放的 C++ parity；external_stepped 点亮与 sensor
  event-time（含多传感器同步）留待 `v0.18.0` C++/Rust 同步实现。更新 roadmap 与当前版本背景。

## v0.17.0 - 2026-06-16

### 新增

- 运行时原生确定性回放（Rust）：runtime 自己拥有回放事件时间线并按确定性网格逐周期步进，
  取代 v0.16.0 经 introspection socket 由外部 wall-clock 节奏逐事件注入的回放。新增
  `flowrt::ReplayDriver`（在「下一个事件时间」与「下一个 periodic 网格点」间取较早者推进
  逻辑时钟）、`TimeDriver` 抽象、`SteppedDriver` + `StepController`（external_stepped 预备）、
  `flowrt-record` 的 MCAP→回放时间线 reader，以及 runtime `replay_driver_from_mcap` 装配桥。
- `flowrt record` 录制 boundary input 激励：`publish_boundary_input` 在 recorder 覆盖该
  endpoint 时把注入作为 canonical frame channel sample 记录，作为确定性回放的边界激励来源——
  回放只重放外部边界激励，由 runtime 重算下游 channel。
- `flowrt run --replay <mcap>`：以 `FLOWRT_REPLAY_SOURCE` 注入生成运行时，配合
  `--temporary-island` 触发 simulated_replay 时钟源的 runtime 原生确定回放；realtime 忽略并告警。

### 变更

- simulated_replay 生成 Rust runtime shell 增加 runtime 原生回放：设置 `FLOWRT_REPLAY_SOURCE`
  时循环前装配 `ReplayDriver`（只含本图 boundary 激励），调度逐周期步进并在命中事件时经
  `publish_boundary_input` 进程内注入；未设置时回退到外部 socket 注入（见 `v0.17.1` 修复）。
  realtime 调度路径不变。

### 测试

- 新增 ReplayDriver 逐周期步进/同时刻多事件、TimeDriver+SteppedDriver 预算步进、MCAP 时间线
  reader、boundary 激励录制、runtime MCAP→driver 装配、Rust simulated_replay codegen 走原生
  回放等单测；新增 `v0.17.0 Deterministic Replay Smoke` focused gate。

### 文档

- `v0.17.0 Deterministic Replay` 聚焦 Rust 侧 runtime 原生确定回放；**C++ runtime 暂仍走
  v0.16.0 外部注入回放路径，跨语言 parity 在 `v0.17.1` 补齐；external_stepped 点亮与 sensor
  event-time 在 `v0.18.0` 完成（C++/Rust 同步实现）**。更新 roadmap 与 `docs/cli.md`（`flowrt run --replay`）。

## v0.16.0 - 2026-06-15

### 新增

- clock source 成为 Contract IR 一等概念：新增 `ClockSourceKind`
  (`realtime` / `simulated_replay` / `external_stepped`) 与
  `artifact.clock_source` 字段，由 normalization 派生（temporary island overlay
  为 `simulated_replay`，常规合同为 `realtime`），validator 重新推导并拒绝不一致
  取值；`external_stepped` 保留长期边界但暂不支持，直接拒绝。codegen 不再散落
  `temporary_overlay.is_some()` 推断 clock source。
- Rust/C++ `Context` 新增 `now_ms` / `now_secs` / `dt_ms` / `dt_secs`，作为用户
  算法获取时间与积分步长的规范入口；时间一律来自 runtime 时钟，用户不再直读
  `steady_clock` / `Instant::now` / `SystemTime`。生命周期上下文无 timing 时返回
  `None` / `std::nullopt`，不伪装有效时间。

### 变更

- `simulated_replay` 调度去除 wall-clock 绑定：generated scheduler 在该时钟来源下
  只等待下一个注入事件或关停，不再计算 `Instant::now` / `steady_clock` deadline，
  逻辑时钟由注入事件 `data_time` 推进，周期 task 在调度边界按 `missed_periods`
  自动 catch-up。回放结果只取决于事件序列，与回放物理快慢无关。`realtime` 路径
  仍按 wall-clock deadline 唤醒，行为不变。
- 逐周期回放步进（与 realtime 对齐的积分粒度）和 record/replay 由 runtime 原生
  确定性驱动（替代外部 per-event introspection socket 注入）留待后续 runtime 原生
  确定性回放驱动补齐；本版本只解除 wall-clock 绑定并解锁回放结果的物理快慢无关性。

### 测试

- 新增 IR canonical JSON 跳过默认 `realtime` 与 round-trip `simulated_replay`、
  validator clock source accept/reject、Rust/C++ `context.now()/dt()` 有无 timing
  两路单测，以及 rust/cpp `simulated_replay` 调度唤醒无 wall-clock deadline 的
  codegen 用例。

### 文档

- 将原 `v0.16.0 Time Model / Clock Semantics` 路线拆分为两条主线，遵循"一版一主轴"
  纪律：`v0.16.0 Clock Model & Deterministic Replay` 作为底座，区分 runtime monotonic /
  wall-clock / simulated replay 时间基准，让 logical clock 确定性驱动调度器、用户经
  runtime 时钟取 `dt`、回放升级为 runtime 原生确定性驱动；`v0.17.0 Sensor Time &
  Synchronization` 在其上引入 sensor event time、跨机器同步能力声明和多传感器同步策略。
- 更新 `docs/cli.md`、`docs/getting-started.md` 的时间模型说明：补充 clock source
  一等概念、`context.now()/dt()` 规范入口和 `simulated_replay` 回放的确定性边界。

## v0.15.2 - 2026-06-15

### 修复

- 修复 Rust/C++ generated scheduler 在 realtime 产物中误把 boundary / replay 注入的
  `published_at_ms` 当作调度时钟推进的问题；strict / 普通 island 产物现在使用
  scheduler 启动后的 monotonic elapsed time，temporary island overlay 仍使用 fixture
  时间保持 simulated replay 确定性。

### 测试

- 新增 `v0.15.2 Scheduler Clock Smoke`，覆盖 realtime 生成物清空样本时间戳但不推进
  scheduler clock，以及 temporary island overlay 继续用 fixture 时间推进 simulated
  replay clock。

## v0.15.1 - 2026-06-15

### 新增

- 新增独立 `release.yml` tag release workflow：tag 会解析其指向的 commit SHA，并只查询
  同一 commit SHA 上成功的 push CI 和 `Release Evidence Gate`，下载同一 run 的 deb 与
  release evidence artifact 后创建 GitHub Release。
- 新增 `v0.15.1 CI Release Evidence Smoke`，检查 CI contract、release workflow
  contract 和本地发布 helper contract。

### 变更

- `ci.yml` 不再提供手工 `workflow_dispatch` 发布候选入口，tag push 也不再触发完整 CI
  矩阵；发布分支 `dev/vX.Y.Z` 的普通 push CI 负责产出 `flowrt-release-evidence`。
- `scripts/check-release-candidate.sh` 改为本地预检和远端 push CI evidence 等待工具，
  不再触发远端 workflow。
- 发布就绪检查改为校验 release evidence 流程，确认 tag release 只消费同一 commit SHA
  已验证 artifact，并同步登记 `0.15.1` focused smoke。
- README 安装示例、`CONTEXT.md` 当前版本背景和发布文档同步到 `v0.15.1`。

### 测试

- release readiness 增加 `v0.15.1` 专项 adapter，覆盖 `Release Evidence Gate`、独立
  tag release workflow、release gate registry 和 smoke 脚本可执行性。
- `scripts/test-v0151-ci-release-evidence-smoke.sh` 覆盖脚本语法、CI workflow contract、
  tag release workflow contract 和 helper 不再手工触发 CI 的约束。

## v0.15.0 - 2026-06-15

### 新增

- 新增 release gate contract：`flowrt-devtools release-gate` 读取
  `scripts/release-gates/registry.toml`，为每个发布版本登记 focused smoke，并提供
  registry 校验和脚本查询入口。
- 新增 `v0.15.0 Architecture Convergence Smoke`，通过 release gate registry 找到
  focused smoke，串联脚本语法检查、architecture size guard 和 architecture contract
  guard。
- 新增 `scripts/check-architecture-contract.sh`，检查 release gate contract、
  Contract IR derived facts 和 runtime observability facts 是否已经进入生产消费路径。
- 新增 `flowrt-ir::derived` 模块，集中派生 `ContractDerivedFacts` /
  `GraphDerivedFacts`，作为 Contract IR 派生事实的 typed 入口。
- 新增 Rust runtime observability facts 模块，将 live status、diagnostics 和 recorder
  diagnostic event 的派生事实收束到同一生产路径。

### 变更

- `scripts/check-release-readiness.sh` 和 `scripts/check-release-candidate.sh` 改为通过
  release gate registry 校验 `v0.15.0` focused smoke，避免发布脚本继续手写版本分支。
- validator 的 capability/resource 校验和 codegen 的 runtime plan、launch manifest、
  self-description 生成改为消费 Contract IR derived facts，不再各自复制派生逻辑。
- C++ runtime introspection schema parity 补强到观测字段测试，确保 C++ live status 与
  Rust JSON-line wire schema 在 route、diagnostics 和 selected backend 字段上保持一致。
- `flowrt status` 文本输出明确区分 static self-description 和 live status：
  `static_selfdesc=loaded|unavailable` 表示静态自描述是否可关联，route 行中的
  `thread_affinity` / `static_thread_affinity` 来自 static self-description；live route 的
  `type`、`backend` 和 `selected_reason` 仍以 runtime live status 为准。
- 在后续路线中记录 `v0.16.0 Time Model / Clock Semantics`：将 runtime scheduling
  time 之后的 clock domain、sensor event time、跨机器同步能力声明和多传感器同步策略
  作为独立大版本主线，不塞入 `v0.15.0` 架构收敛范围。
- README 安装示例同步到 `v0.15.0`，保持 release readiness 检查的用户入口版本一致。

### 修复

- validator 重新对显式 dataflow backend 运行 backend resolver，拒绝手工伪造
  external route 使用非 `zenoh` backend 的 Contract IR，即使派生的 thread affinity
  元数据也被同步篡改。
- `v0.15.0 Architecture Convergence Smoke` 在 CI 中显式安装 `ripgrep`，避免
  architecture contract guard 因 runner 缺少 `rg` 而失败。

### 测试

- release readiness 增加 `v0.15.0` 专项 adapter，校验 CI 中的 architecture convergence
  smoke job、release gate registry、focused smoke 脚本和 architecture contract guard。
- release candidate 本地预检会先运行 release readiness，再通过 registry 执行
  `v0.15.0 Architecture Convergence Smoke`；远端 RC 仍需要同 SHA 的
  `Release Candidate Gate` 成功后才允许 tag release。
- 补强 runtime 观测事实跨语言一致性测试，覆盖 C++ introspection schema 与 Rust status /
  diagnostics 事实的字段对齐。

## v0.14.1 - 2026-06-15

### 新增

- 为 `0.14.1` 架构修缮新增 `architecture guard`：检查 tracked Rust、C/C++ 和
  shell 源文件单文件行数阈值；本版本完成既有大文件拆分后不保留 legacy allowlist，
  后续新增或回涨的超阈值文件会直接使 CI 失败。
- 新增 `v0.14.1 Architecture Guard` focused smoke 入口，先校验相关脚本语法，再运行
  architecture guard，作为 SchedulerRuntimePlan、大文件拆分和后续架构 smoke 的统一入口。

### 变更

- 将 release candidate 本地预检改为版本 registry：`0.14.0` 继续运行 realtime scheduler
  smoke，`0.14.1` 运行 architecture smoke，后续版本可显式登记自己的 focused gate。
- 将 `0.14.1` 定位为架构修缮版本，重点收敛 `SchedulerRuntimePlan`、大文件拆分和
  architecture guard。
- 将 Rust runtime introspection 从单个大文件拆分为 `model`、`state`、`probe`、
  `server`、`client`、`params`、`diagnostics` 和 `paths` 模块，保持
  `flowrt::introspection::*` public API、JSON 协议和 socket 规则不变。
- 将 C++ runtime `flowrt/introspection.hpp` 拆分为 `model`、`probe`、`state`、
  `json`、`request_parser`、`socket` 和 `server` 子头文件，保留原 public include
  façade 和运行态 introspection 语义不变。

### 测试

- 将 `flowrt-cli` 的 `echo_params_tests` 和 `workspace_tests` 大测试文件拆分为目录模块，
  保持原有 CLI 测试覆盖和断言不变。
- CI 在 package、release candidate 和 tag release 前运行 `v0.14.1 Architecture Guard`，
  发布就绪检查同步覆盖 CI job、smoke 脚本、architecture guard、release candidate 泛化和
  本版本 CHANGELOG 条目。
- 稳定 supervisor restart 竞态测试：不再用固定 sleep 猜测依赖进程已退出，改为显式等待
  测试子进程退出，避免 amd64 CI runner 调度抖动导致 v0.5.0 focused gate 偶发失败。

## v0.14.0 - 2026-06-15

### 新增

- 新增 task timing context，用户算法通过现有 `flowrt::Context`
  读取 runtime scheduling time，不改变 Rust/C++ handler 签名；上下文会区分
  `scheduled_time_ms`、`observed_time_ms`、`lateness_ms`、`missed_periods` 和
  `overrun`。
- C component callback ABI 升级到 `0.2`，新增 `flowrt_c_task_timing_t` 和
  `flowrt_c_component_context_t.has_timing`，让 C task callback 可读取 runtime
  scheduling time、周期/deadline 标记、迟到、missed period、overrun 和 clock source。
- Rust/C++ live status、结构化 diagnostics 和 recorder event 现在会暴露 task/lane
  timing 观测字段，包括 inflight、scheduled/observed time、lateness、missed
  periods、overrun、backpressure 和连续失败计数，便于 `flowrt status` 与录制结果
  直接解释 scheduler 运行态时序。
- `flowrt explain`、App API manifest 和 `flowrt/app/implementation.md` 现在展示 task
  context timing 能力，说明 `context.timing()`、C callback context timing、realtime /
  replay 时间来源和生命周期 context 默认无 timing 的边界。

### 变更

- 调度主线将 scheduler admission 与长任务 completion 解耦：runtime 公开
  `TaskAdmission`、`WorkerCompletionQueue` 和显式 `complete_task` primitive，
  scheduler 线程继续负责 ready 判定、completion drain、backend commit、introspection
  和 deterministic output commit，worker 只运行用户 task；Rust/C++ generated scheduler
  走 completion queue 主路径，不保留旧同步 helper 兼容路径。
- realtime 路径会暴露 runtime 实际观测到的调度迟到和相邻运行 `dt`；replay /
  temporary island 路径继续使用 simulated clock，使 `published_at_ms`、scheduler tick、
  record、Operation 和 status 的时间字段保持确定性。
- C component callback table 现在必须同时设置
  `FLOWRT_ABI_FEATURE_C_COMPONENT_CALLBACKS_V0` 和
  `FLOWRT_ABI_FEATURE_C_COMPONENT_TASK_TIMING_V1`；generated C adapter 会从
  `flowrt::Context::timing()` 填充 C task timing，生命周期 callback 默认
  `has_timing=0`。
- 明确不承诺硬实时，不做 sensor timestamp、sensor event-time、clock
  domain、PTP、NTP、跨机器 exact sync 或 approx sync，也不定义多传感器同步策略。

### 修复

- 修复 Rust generated scheduler 在 worker closure 中捕获整个 `App` 的问题；含 iox2
  route 的 task 现在由 scheduler 线程先读取 owned input snapshot，再把用户组件 handle
  与输入快照送入 worker，避免 iox2 endpoint 的 scheduler-local 线程亲和阻断
  `frame_descriptor_demo` 等安装后 demo smoke 编译。
- 修复 generated supervisor 无法解析 launch manifest 中 `artifact.clock` 与
  `artifact.temporary_overlay` 元数据的问题；`flowrt launch`、demo smoke 和 ROS2
  bridge smoke 会继续保留严格未知字段校验。
- 修正 v0.8.0 / v0.8.1 installed smoke 对 I/O boundary resource status 的旧
  `kind=shm` 断言，改为抽象 `payload.frame_buffer` capability，并在失败时打印
  live status。

### 测试

- 新增 `v0.14.0 Realtime Scheduler Smoke` focused gate，覆盖 executor admission /
  completion、Rust/C++ generated scheduler 非阻塞主路径、status/introspection timing
  字段和 C ABI task timing layout。
- 新增 tag 前 release candidate 门禁：`workflow_dispatch` 可按版本触发完整发布同款
  gate 但不创建 GitHub Release；tag release 会拒绝没有同 SHA 成功 RC 记录的发布，
  `scripts/check-release-candidate.sh` 负责本地预检、触发和等待远端 RC。

## v0.13.0 - 2026-06-14

### 新增

- C ABI 基础边界升级到 `0.2`，新增 borrowed frame view、params view/update result、
  operation status/progress/result summary view、diagnostic view 和 resource health
  snapshot 的 POD 形状，供后续 Python 和更多语言复用；C component callback ABI 仍保持
  `0.1` 且布局不变。
- RSDL / Contract IR 增加抽象 resource requirement 和 provider 语义，component 可声明
  `capability`、访问方式、必需性、readiness、health 和失败传播，graph 可声明 target、
  process 或 external package 作用域的 provider。
- Contract IR 现在派生 per-instance resource satisfaction metadata，optional unsatisfied
  requirement 会保留诊断字段，供后续 `status` / `doctor` 使用。
- variable frame 生成物现在为 Rust/C++ 同步生成 `message_frame` conformance 测试，覆盖
  `bytes`、`string`、`sequence<fixed struct>`、空变长字段、多元素 sequence、UTF-8 字符串
  和 malformed frame decode 错误路径；C++ 测试会写出 byte fixture，Rust 测试读取并比对。
- `flowrt record` / `flowrt replay` 与 boundary input JSON 注入路径补齐 variable frame
  event 主路径，继续使用 FlowRT-native schema，不引入 ROS2 message schema。
- Operation runtime 明确长期 lifecycle：`idle`、`starting`、`running`、
  `cancel_requested`、`cancelled`、`succeeded`、`failed` 和 `timed_out`；start 会记录
  invocation id、owner 和 deadline，status/record 会输出 state change、progress、
  result 和 error。
- `flowrt replay` 会在选择 live socket 前校验 fixture 的 `boundary`，只允许写 typed
  boundary input；普通 dataflow channel、boundary output、service、operation 或未知
  endpoint 会返回明确错误。
- Temporary island overlay 产物现在在 Contract IR、self-description 和 launch manifest
  中记录 `temporary_overlay` metadata，包括原 profile mode、生成命令/source 和 boundary
  mapping 来源，并暴露 replay clock metadata。
- Rust/C++ live status 新增结构化 diagnostics schema，覆盖 clock、channel、input、
  route、process、resource、I/O boundary、param、service、operation 和 task 状态。
- `flowrt status --format json` 输出按 socket 分组的完整机器可读 live status，并保留
  handshake/status/error，便于脚本直接消费 `status.diagnostics`。
- `flowrt record` 新增 `diagnostics_event` 通道，记录结构化诊断项、entity id、
  self-description hash 和事件时间。
- bundle manifest 现在记录 resource provider closure 和 runtime dependency 指纹；交叉
  C++/CMake build 使用 bundled runtime dependency policy 时，会把 target SDK manifest
  纳入 `runtime-deps/<platform>/` 部署闭包。

### 变更

- 参数 runtime apply 语义补齐为 `applied -> pending -> applied/rejected` 状态机：
  Rust/C++ generated shell 在 scheduler 边界非破坏性读取 pending，完成 type、`min`、
  `max`、`enum` 和 `on_params_update` 校验后才提交；校验或回调拒绝时保留旧参数，且不
  因参数拒绝停止 scheduler。
- `flowrt params get/list/set` 输出新增 `apply_state=applied|pending|startup-only`，
  明确区分当前值、待应用值和启动期参数。
- App API manifest 的 params schema 现在包含 `min`、`max` 和 `choices`，与
  self-description 中的运行态校验元数据保持一致。
- `component.resource` 不再使用 `serial`、`udp`、`shm` 等具体 `kind` 作为核心语义；
  validator 会重新推导 satisfaction，并拒绝 required unsatisfied、exclusive 冲突、
  provider 引用错误和 concrete hardware/protocol 词进入 resource capability。
- C ABI operation state 常量同步为长期 lifecycle：`IDLE`、`STARTING`、`RUNNING`、
  `CANCEL_REQUESTED`、`SUCCEEDED`、`FAILED`、`CANCELLED` 和 `TIMED_OUT`，不再暴露旧
  `ACCEPTED` / `CANCELING` / `PREEMPTED` 命名。
- Rust/C++ runtime 和 generated scheduler 统一 replay 时间模型：boundary replay 的
  `published_at_ms` 会驱动 scheduler tick，record clock event、Operation event 和 live
  `flowrt status` 使用同一 `tick_time_ms` 毫秒字段和 `time_source` / `clock_source`。
- `flowrt list` / `flowrt status` 输出会展示 temporary overlay 和 clock source 字段，便于区分
  strict 生产 runtime 与 temporary island replay runtime。
- `flowrt status` 文本输出会展示 `diagnostic=...` 与参数 `apply_state` 行；普通 status
  查询保持无副作用，diagnostics event 只在显式录制路径写入。
- `flowrt bundle` / `flowrt deploy` 的 schema v2 校验收紧为部署闭包校验：dry-run 和真实
  部署都会校验 external package 多 platform artifact、external scope resource provider、
  runtime dependency version/platform/hash/path，以及 target platform 和 artifact hash。
- `flowrt doctor --target <platform>` 对 SDK overlay、pkg-config path 和 CMake toolchain file
  输出可执行修复建议，指向 `flowrt toolchain init`、`.flowrt/toolchains.toml` 和带 RSDL 的
  `flowrt doctor`。

### 修复

- 修复 C++ generated task output fan-out 同时提交 dataflow bind、bridge 或 boundary output
  时重复定义 `payload` 局部变量的问题；每个 commit closure 现在使用唯一 payload 名称，避
  免 C++ 编译失败。
- `flowrt echo` 现在可以借助 self-description 中的 Message ABI metadata 展示
  `sequence<fixed struct>`，长 sequence 默认输出结构化摘要，`--raw` 保留完整内容。
- Rust/C++ generated Operation hidden task 现在由 runtime/scheduler 驱动 deadline
  timeout，active invocation 每个 scheduler tick 至少检查一次，不再依赖用户 handler
  自觉退出。
- Operation control authority 现在默认按同一 scope 单 owner 执行：第二 owner start 返回
  结构化冲突错误，stale invocation cancel 被拒绝或返回明确说明，不会误取消后续 invocation。
- `flowrt bundle` / `flowrt deploy` 现在会根据 bundle manifest 的 `temporary_overlay` 和
  `test_only` 标记拒绝 temporary overlay 产物，避免手工篡改 `artifact_mode` 后绕过 island
  安全门。
- `flowrt deploy` 真实上传前会通过远端 FlowRT probe 校验 remote platform 和 runtime
  dependency 指纹，远端版本、platform 或 hash 不匹配时给出安装匹配 FlowRT / target SDK
  的提示。
- `flowrt deploy` 继续拒绝空 host、以 `-` 开头的 host 和不安全 remote dir，`ssh` / `scp`
  参数按边界传递，避免 host 或路径被解释成本地 option。

### 测试

- 补充 Rust/C++ runtime、codegen、CLI 和 App API focused 测试，覆盖 pending 可观测、
  callback reject 保留旧值、新 pending 不被旧 apply 清掉、多 task instance 不重复 apply，
  以及 `apply_state` 输出。
- 补充 RSDL、IR 和 validator focused 测试，覆盖 target/process/external provider
  满足、optional unsatisfied 可观测、canonical JSON roundtrip、required unsatisfied、
  exclusive 冲突和 concrete 字段拒绝。
- 补充 Rust/C++ runtime variable frame byte fixture，覆盖固定 header + tail canonical layout、
  empty string/bytes/sequence、nested fixed struct sequence、UTF-8 string、截断、offset overflow
  和 length overflow。
- 补充 Rust/C++ runtime、codegen、CLI status/record 和 C++ smoke 覆盖 Operation
  成功、cancel、timeout、第二 owner 冲突、stale cancel 和 handler error 进入 failed。
- 补充 CLI、codegen 和 runtime 回归测试，覆盖多 boundary replay、strict replay 拒绝、
  非 boundary endpoint 拒绝、temporary overlay metadata、bundle/deploy 拒绝以及 replay
  clock record/status 语义。
- 补充 CLI focused 测试，覆盖完整 multi-platform external package bundle closure、
  resource provider 缺失、external package artifact 缺失或 platform mismatch、runtime
  dependency hash mismatch、远端 deploy probe platform/hash mismatch 和 cross SDK doctor
  actionable diagnostics。
- CI 新增 `v0.13.0 Robot Runtime Completion Smoke` amd64/arm64 focused gate，覆盖
  replay、temporary island overlay、抽象 resource contract、external/boundary health、
  variable frame、params apply、Operation lifecycle、diagnostics/status/record、
  bundle/deploy/doctor/cross 和 C ABI 主路径，并纳入 package/release 依赖链。

## v0.12.0 - 2026-06-14

### 新增

- `flowrt prepare` 现在会生成稳定 App API 产物：`flowrt/app/app_api.json`、
  `flowrt/app/implementation.md` 和 `flowrt/app/stubs/`，从已校验 Contract IR 派生
  Rust/C++/C 组件实现接口、task、params、service、operation 和 C callback table 线索。

### 修复

- C component generated adapter 现在会在 callback table 校验失败时输出明确原因，包括
  空指针、size/version 不匹配、缺少 v0 feature bit、未知 feature bit 和缺少对应 task
  callback，避免 C v0 用户只能看到笼统 `Status::Error`。
- C component output slot 校验失败时会输出 instance、port、status、written_len 和期望
  size，便于定位 C callback 写入长度或状态错误。
- `CONTEXT.md` 当前 workspace 版本状态已纳入 release readiness 检查，避免发布后状态文档
  仍停留在旧版本。

### 变更

- `flowrt init` 现在只创建 `flowrt.toml` 和最小 `rsdl/robot.rsdl`，不再生成默认
  Rust、C++ 或 C 用户实现文件；`--lang` 只影响初始 target runtime 和下一步提示。
- `flowrt explain` 的 text/json 输出现在复用 App API manifest 模型派生，避免与
  `flowrt prepare` 生成的 `app_api.json` / `implementation.md` 分别维护签名逻辑。
- `flowrt add component` 现在只编辑 RSDL，不再创建、追加或覆盖用户代码文件；
  成功输出会提示通过 `flowrt prepare` 或 `flowrt explain` 查看真实实现接口。
- 用户主路径文档统一为 Contract-driven App Authoring：`init` 创建入口，`add` 只编辑
  RSDL，`prepare` 生成 `flowrt/app/` 下 App API 参考产物，`explain` 展示同一事实源的
  用户实现清单，用户再把实现手写或复制到 `app/` 后执行 `build` / `run`。
- 发布文档中的版本示例改为占位变量，不再把历史版本号写成默认发布路径。

### 测试

- 补充 CLI 测试，覆盖 Rust、C++ 和 C 的 `flowrt init` 只创建项目入口和最小 RSDL，
  不生成默认用户实现文件。
- 补充 `flowrt explain` focused 断言，覆盖 App API JSON 核心字段复用，以及 Rust/C++/C
  text 输出中的用户实现路径、handler、params、service、operation 和 C callback table。
- 补充 C adapter codegen 断言，覆盖 callback table 失败原因和 output slot 失败诊断。
- 补充 CLI 测试，覆盖 Rust、C++ 和 C 的 `flowrt add component` 只编辑 RSDL 且不改写
  `app/` 用户代码文件。
- CI 新增 `v0.12.0 Authoring Smoke` amd64/arm64 focused gate，覆盖 Rust、C++ 和 C 的
  `init` / `add` / `check` / `prepare` / `explain` 主路径，以及 Rust/C++/C 示例的
  普通 `build` / `run`。

## v0.11.0 - 2026-06-14

### 新增

- 新增 FlowRT app 项目入口 `flowrt.toml`，`check`、`prepare`、`build`、`run`、`deps`、
  `doctor` 和 `explain` 省略 RSDL 路径时会从当前目录向上发现 `project.main`；显式 RSDL
  路径仍保持优先。
- 新增 `flowrt init [path] --lang <rust|cpp|c>`，可生成现代 FlowRT app 项目骨架：
  `flowrt.toml`、`rsdl/robot.rsdl` 和 `app/rust/mod.rs`、`app/cpp/components.cpp` 或
  `app/c/controller.c`。
- 新增 `flowrt add message/module/component`，可基于 `flowrt.toml` 默认主 RSDL 追加 message、
  workspace module 文件、Rust/C++/C native component RSDL 片段和现代 `app/` 用户代码骨架；
  写入前会重新解析、归一化并校验更新后的 Contract IR，C component 骨架生成
  `app/c/<component>.c`。
- 新增正式 `flowrt explain [rsdl] [--format text|json]`，在校验后输出 component 用户实现
  路径、task 触发与并发语义、params、输入输出以及 service / operation handle 摘要；省略
  RSDL 路径时同样支持 `flowrt.toml` 项目入口发现。
- RSDL / Contract IR 增加 `language = "c"` component 基础语义，target runtime canonical
  ordering 同步纳入 `c`；当前作为 C ABI v0 codegen adapter 的语义入口。
- C ABI v0 新增 C component callback table 基础边界：定义 context、fixed input view、
  output slot、生命周期 callback 和 task callback 的 POD 形状；事实源为
  `runtime/cpp/include/flowrt/abi.h`，Rust runtime 提供对应 `repr(C)` 镜像类型。
- C codegen 增加 C component adapter 最小生成路径：generated C++ runtime shell 会通过
  静态链接的 callback table factory 调用 C 生命周期和 task callback，并把 fixed input/output
  映射为 C ABI borrowed view / output slot；adapter 会要求 callback table 设置 v0 feature
  bit 并拒绝未知 feature bit。
- 新增 `examples/c_counter_demo`，覆盖 fixed-size `Count` message、两个 C native component、
  CMake app build/run 和 generated supervisor launch。

### 变更

- FlowRT app 用户主路径统一为 `flowrt.toml`、`rsdl/`、`app/` 和可重建 `flowrt/` 生成目录；
  `flowrt init`、`flowrt add`、`flowrt explain` 和普通 build/run 文档均按该布局说明。
- validator 对 C v0 component 采用 fail-fast 边界：暂不允许 `pkg_config`、参数、
  service、operation、variable frame、`io_boundary` 或 `external` kind；codegen 只在已校验的
  C v0 native / fixed-size message 范围生成 adapter 和 callback table 脚手架。

### 修复

- generated supervisor 现在识别 launch manifest 中的 `runtime_kind = "c"`，并按 CMake
  app binary 启动 C callback v0 进程，避免 `flowrt build --launcher` 成功但
  `flowrt launch` 无法解析 C 进程可执行文件。

### 测试

- 补充 CLI 覆盖 `init`、`add message/module/component`、`explain`、`flowrt.toml`
  向上发现和显式 RSDL 优先级。
- 补充 C ABI / C codegen 覆盖 callback table layout、feature bit 校验、C skeleton 生成、
  fixed input/output adapter 和 C v0 fail-fast 限制。
- 补充 `examples/c_counter_demo` smoke，覆盖普通 `flowrt build` / `flowrt run`、以及
  `flowrt build --launcher` / `flowrt launch` 的 C callback v0 路径。
- CI 新增 `v0.11.0 App SDK Smoke` amd64/arm64 focused gate，覆盖
  `flowrt init` Rust/C++/C、`flowrt add message/component`、`flowrt explain`
  text/json，以及 `examples/c_counter_demo` 的普通 build/run 和 launcher build/launch；
  该 init/add/explain gate 已纳入 package/release 依赖链和发布就绪检查。

## v0.10.3 - 2026-06-14

### 新增

- Generated Rust/C++ app glue 现在只使用现代 `app/` 用户代码布局：Rust 用户入口为
  `app/rust/mod.rs`，C/C++ 用户源码从 `app/cpp/**` 与 `app/c/**` 递归发现。
- Generated CMake app 工程启用 C/C++ 语言并把 `app/cpp`、`app/c` 加入用户 include
  root，支持用户把 `.h/.hpp/.c/.cpp` 按 module/component 同目录组织。

### 修复

- Rust generated shell 在含 `iox2` route 时把 thread-affine endpoint 提交留在
  scheduler/local owner 线程，保留 two-phase output commit，但不再把 iox2 endpoint
  捕获进 worker closure。

### 变更

- 仓库示例和安装后 smoke 临时项目全部迁移到 `app/` 用户代码布局；旧 `src/rust` /
  `src/cpp` 不再作为 FlowRT 用户代码路径。
- 文档更新为 `rsdl/`、`app/`、`flowrt/` 分层：RSDL 声明系统结构，`app/` 承载用户实现，
  `flowrt/` 只保留可删除生成物。

## v0.10.2 - 2026-06-14

### 新增

- Contract IR 现在为 dataflow route 派生 `thread_affinity` metadata：`iox2` 标记为
  `scheduler_local_commit`，`inproc` / `zenoh` 标记为 `send_safe`；validator 会重新推导并拒绝
  手工篡改，launch manifest 与 self-description 也会暴露该字段。
- Rust/C++ runtime 增加 output transaction primitive：`Output<T>::write(...)` 用户 API
  保持不变，generated shell 可把 task-local output 转成带 task、port、payload 和调度时间
  的 commit record，并只在 `Status::Ok` 后由 scheduler 线程统一提交。
- Rust/C++ executor 增加 worker 结果收集能力，用于 worker 执行用户回调后按原 ready
  顺序返回 status 与可提交 output；非 `Ok`、panic 或 exception 路径都会丢弃本次
  output。
- Rust/C++ generated runtime shell 现在使用 two-phase output commit：worker 执行用户
  task 并收集 commit closure，scheduler 线程按 ready batch canonical order 提交 backend
  输出；`iox2` 不再通过整批 `run_local` 把同一批 task 串行化。
- `flowrt list` / `flowrt status` 现在会展示 dataflow route 的 `thread_affinity`，用于解释
  `iox2` route 为何由 scheduler/local owner 提交 transport output，而用户 task 仍可并发运行。

### 测试

- 补充 Rust/C++ runtime 覆盖：task-local output 成功后形成 commit record、非 `Ok`
  与 panic/exception 后不提交、local ready-batch 不要求跨线程输出约束，以及 worker 路径
  收集 output 时保持结果顺序。
- 补充 codegen 覆盖：Rust/C++ generated shell 的输出提交块位于 scheduler loop，
  非 `Ok` 状态不会提交本次输出。
- 新增 `v0.10.2 Concurrency Hardening Smoke` CI gate，在 amd64/arm64 上覆盖 codegen
  并发 focused tests、Rust iox2 generated shell、backend route、Rust/C++ runtime
  executor，以及临时 generated Rust/C++ shell 构建。
- 补充并发语义文档，明确 `exclusive` / `parallel`、lane、worker、two-phase commit、
  `Ok` 后提交和非 `Ok` 丢弃 output 的行为。

## v0.10.1 - 2026-06-13

### 修复

- `flowrt pub` 现在优先使用 self-description 中的 canonical frame layout 编码 boundary
  input，即使 fixed Message ABI 同时存在，也不会把带 native padding 的结构误按 ABI
  字节布局注入；嵌套 frame 字段也走同一 frame codec。
- `flowrt echo` 现在会按 backend 选择正确的 payload 解释路径：zenoh channel 和
  boundary output 优先使用 canonical frame，inproc fixed channel 继续使用 native
  Message ABI，避免含 padding 的 fixed 类型显示错字段。
- 显式 `--socket` 指向 FlowRT 管理目录下的 stale runtime socket 时，`echo` 的
  self-description 读取和静态 image hash 校验路径都会删除已确认失效的 socket 并给出
  明确错误，不再要求用户手动清理残留文件。
- native C++ 构建现在也会应用 host toolchain profile 中的 C++ 编译/链接选项；native
  pkg-config 只把 profile/overlay 路径加入 `PKG_CONFIG_PATH`，不再用
  `PKG_CONFIG_LIBDIR` 隔离系统默认搜索路径。

### 测试

- 补充 CLI 回归覆盖：canonical frame 与 native ABI 并存时的 `pub` / `echo` 行为、
  显式和自动 stale socket 清理、native CMake profile 参数和 pkg-config 环境。

## v0.10.0 - 2026-06-13

### 新增

- RSDL 与 Contract IR 增加 component/task `concurrency = "exclusive" | "parallel"` 语义：
  component 默认 `exclusive`，task 默认继承 component；normalized IR 会保留 resolved
  值以及用户是否显式声明该字段，供 validator 和后续 codegen 判断是否真的放开同一
  instance 内并行。
- Rust/C++ runtime executor 增加面向后续 generated shell 的真并发 dispatch primitive：
  `DeterministicExecutor` 现可按 ready set 稳定取出 `ReadyBatch`，保证同一 lane 一次只
  dispatch 一个 task，并通过 `WorkerPool` 在不同 lane 间并行执行。
- C++ generated runtime shell 现已接入 `ReadyBatch` / `WorkerPool` 主路径：scheduler
  step 不再走串行 `run_ready`，而是按 lane admission 后把 ready task batch 提交到
  worker，并在 shell 内部聚合 per-task `Status` 与 health。
- Rust generated runtime shell 现已接入同一 `ReadyBatch` / `WorkerPool` 主路径；默认
  `exclusive` component 使用 `Arc<Mutex<...>>` 串行保护用户对象，显式 `parallel`
  component 才生成 `Send + Sync` trait 与共享只读回调接收者。

### 修复

- validator 现在会拒绝 `exclusive` component 下的 `task concurrency = "parallel"`，
  防止 task 局部声明隐式提升整个 instance 的并发许可；`worker_threads = 1` 下保留
  parallel 声明合法性，只让运行时行为自然退化为串行。
- Rust/C++ `WorkerPool` 现在显式支持 `close_admission` / `drain`，并在 job
  panic/exception 时统一聚合为 `Status::Error`、稳定释放 active worker 计数，避免并发
  dispatch 关闭路径挂住。
- Rust/C++ codegen 现在会把未显式声明 lane 的 task 保持在 `<instance>_serial`，由 IR
  显式 lane 决定是否跨 lane 并行；inproc service / operation hidden task 也改为稳定 lane 名，
  避免 worker 数变化时 lane 语义漂移。
- 修复 Rust generated shell 并发接线的所有权问题：run scope 使用 `Arc<App>` 进入 worker
  closure，channel、backend bridge、introspection probe 和参数快照通过内部锁共享，避免
  生成物出现非法 `self` shadow 或 move-after-use。
- 修复 Rust generated shell 在 introspection probe 外层重复增加 `Arc<Mutex<...>>` 的
  并发退化：probe 改为启动时通过 `OnceLock` 注册，发布热路径直接使用其内部线程安全
  handle，不再为每次 channel 发布获取额外互斥锁。
- 修复 C++ generated shell 把 `ReadyBatch` 声明为 `const auto` 后再 `std::move` 调用
  右值限定 `run(...)` 的问题；arm64 Debian package 用户项目 smoke 现在可编译并发调度
  shell。
- 修复 generated launch manifest 新增 `artifact` 元数据后 supervisor runtime 解析结构
  未同步的问题；安装包后的 demo smoke、ROS2 bridge smoke 现在不会在启动 supervisor
  时被 manifest 兼容性检查拒绝。
- 修复 Rust generated shell 在 iox2 channel 场景下把非 `Send/Sync` 的 iox2 endpoint
  隐式捕获进 worker closure 的问题；含 iox2 的 Rust app 现在明确走 scheduler 线程亲和
  的 local ready-batch 路径，避免 unsafe 包装 backend handle。
- 修复 v0.9.2 island replay CI smoke 把 bundle 的 FlowRT 版本和 amd64 平台写死的问题；
  fixture 现在跟随当前 workspace version 和 CI matrix platform，后续版本及 arm64 gate
  不会在真正校验 island deploy 拒绝语义前被兼容性检查截断。
- self-description 现会暴露 task 级 `concurrency` 与 scheduler task `lane`/`concurrency`
  元数据，便于验证 generated shell 是否按 IR 并发语义落盘。

### 测试

- 补充 `flowrt-rsdl` / `flowrt-ir` / `flowrt-validate` 的并发语义覆盖：验证 parser
  接受 component/task concurrency、拒绝非法值、normalizer 默认填充 `exclusive`、
  canonical JSON 保留显式 `parallel`，以及 validator 对 component/task 并发关系和
  `worker_threads = 1` 退化场景的判定。
- 补充 Rust/C++ runtime executor 并发覆盖：验证跨 lane overlap、同 lane 不 overlap、
  `worker_threads=1` 退化串行、panic/exception 转 `Error`、关闭准入后拒绝新 job 以及
  shutdown/drain 稳定性。
- 补充 Rust/C++ shell codegen 覆盖：验证生成物不再包含 `scheduler.run_ready(...)` 主路径、
  默认 `exclusive` 多 task 仍共用串行 lane、显式 `parallel` + 不同 lane 会生成并发
  dispatch 路径、`worker_threads = 1` 退化仍生成合法 shell，以及 self-description
  暴露 task concurrency 元数据；新增 service / operation demo 的 generated Rust
  release build 和短步运行验证。

## v0.9.2 - 2026-06-13

### 新增

- `flowrt check` 现在会在校验通过后输出 generated user API 摘要，提前展示 Rust/C++
  component 需要实现的 `on_tick` 和参数更新签名，避免用户只能通过编译错误发现
  typed params、输入视图、输出句柄或 service/operation handle 参数。
- `flowrt echo` 对长 `sequence<T>` 默认输出稳定摘要，numeric sequence 展示
  `count`、`min`、`max`、`mean`、`first` 和 `last`；新增 `--raw` 可恢复完整数组输出，
  多 channel 模式仍保留 `channel=<name>` 前缀。
- 新增 `flowrt replay`，支持 FlowRT-native JSONL / JSON array fixture 按时间顺序驱动
  多个 island boundary input；strict 产物仍被拒绝，ROS2 bag / MCAP 互通不进入 core。
- `flowrt prepare`、`flowrt build` 和 `flowrt run` 支持 `--temporary-island` 以及显式
  `--boundary-input name=instance.port` / `--boundary-output name=instance.port`，可在不修改源
  `.rsdl` 的前提下生成 test-only island projection；生成的 Contract IR、
  self-description 和 launch manifest 会标记 `temporary_island` / `test_only`。

### 修复

- `flowrt echo`、`status`、`hz`、`params`、`op`、`record` 和 `pub` 的自动 runtime
  socket 发现路径现在会跳过并安全删除 FlowRT 管理目录下确认 stale 的 socket，避免
  上一次异常退出残留 socket 后阻断后续调试命令。显式 `--socket` 仍只针对用户指定
  目标报错，不会自动切换到其他 runtime。
- 生成的 Rust/C++ runtime shell 现在会在参数 pending apply 边界重新校验 `min`、`max`
  和 `enum` 约束；即使绕过 CLI 写入非法 pending 值，也不会进入用户
  `on_params_update` 或覆盖旧的合法参数快照。

## v0.9.1 - 2026-06-12

### 新增

- `flowrt pub` 支持按 self-description 的 canonical frame metadata 将自然 JSON 编码为
  typed boundary input payload，覆盖 `string`、`bytes`、`sequence<primitive>`、
  `sequence<fixed struct>` 和嵌套 fixed struct header 字段。
- `flowrt params set` 增加 `--file` 批量导入能力，支持用 JSON object 或 JSON array
  逐项设置运行态参数；本机 introspection socket 和远程 zenoh params control-plane
  路径都复用现有单项 set 语义，不引入 atomic 事务。
- `flowrt pub` 增加 `--file` / `--freq` 批量注入能力，支持 JSONL 流式输入、JSON
  array 便利输入和单个 JSON value 文件；`--freq` 只控制 wall-clock 发送节奏，不解释
  或修改消息字段时间戳。
- `flowrt echo` 支持一次观察多个 channel 或 boundary output；多 channel 模式每条
  输出带 `channel=<name>` 前缀，单 channel 输出保持旧格式。
- 新增 `examples/variable_frame_island_demo`，展示 `sequence<f32>` / `string`
  canonical frame boundary input、`flowrt pub --file --freq` JSONL 注入和 fixed summary
  输出观测。
- RSDL 消息类型支持显式 `empty = true` 空消息，生成 Rust/C++ 零长度 wire payload、
  self-description `empty` 标志和空 payload ABI conformance；未显式声明的空
  `[type.X]` 仍会被 validator 拒绝。
- 补充 v0.9.1 island 迁移验证文档与 `frt-app-ros2-migration` skill：明确外部样本
  转 JSONL、`params set --file`、`pub --file --freq`、多 channel `echo`、`record`
  对比和拆 boundary 回 `strict` 的主路径。

### 修复

- `flowrt pub` 不再把 canonical frame boundary input 一律拒绝为 fixed-only JSON；`bytes`
  现在支持 base64 字符串，并额外接受 `0..=255` 整数数组作为调试输入。
- `flowrt pub` 的 canonical frame 错误路径现在会明确报告未知字段、缺字段、sequence
  非数组、primitive 越界、非法 base64 和缺失嵌套 fixed metadata。
- `flowrt pub` 支持用 `{}` 或 `null` 注入显式空消息 boundary input，避免迁移脚手架
  为无载荷信号消息添加无意义占位字段。

### 测试

- 补充 `flowrt-cli` 的 boundary pub canonical frame 编码覆盖，验证成功注入、`bytes`
  两种 JSON 入口，以及关键错误路径。
- 补充 `flowrt-cli` 的 boundary pub 文件输入覆盖，验证 JSONL、JSON array、`--freq`
  解析、输入互斥和坏 JSONL 行号错误。
- 补充 `flowrt-cli` 多 channel echo 覆盖，验证 `--image` 多 channel 解析、逐行
  `channel=<name>` 前缀和旧单 channel echo 兼容。
- 新增 `scripts/test-v091-variable-frame-island-demo.sh`，验证 variable frame island demo
  的 deps/build/run/pub/echo 闭环。
- CI 新增 `v0.9.1 Island Migration Tooling Smoke` amd64/arm64 focused gate，覆盖
  `params set --file`、`flowrt pub`、显式空消息和 variable frame island demo 的
  `pub --file --freq` / `echo` 闭环，并纳入 package / release 依赖链。
- 补充 `flowrt-cli` 参数控制面覆盖：验证 `params set --file` 的命令解析、本机批量
  导入、JSON object / array 两种输入形状，以及部分失败时仍继续执行并输出逐项
  `ok` / `error` 摘要。
- 补充 RSDL parser、Contract IR normalization、validator、codegen 和 CLI 的显式空
  消息覆盖，验证 `empty = true`、隐式空消息拒绝、`empty + fields` 拒绝、生成
  self-description 和 `flowrt pub` 空 payload 注入。

## v0.9.0 - 2026-06-12

### 新增

- 为 `v0.9.0` Island Mode / Boundary Endpoint 增加 RSDL 与 Contract IR 基础承载：
  profile 可声明 `mode = "strict" | "island"`，RSDL 可声明 typed
  `boundary.input` / `boundary.output`，归一化后的 IR 会输出 `GraphMode` 和
  canonical `BoundaryEndpointIr`。
- Contract IR validator 增加 island/boundary 拓扑规则：strict profile 拒绝 boundary
  endpoint；island profile 下 typed boundary input 可以满足 task active input；同一
  input port 不允许同时由 dataflow bind 和 boundary input 满足。
- self-description 和 launch manifest 增加 island/boundary 静态描述：profile 和 graph
  会标注 `mode`，graph 会列出 typed `boundary_endpoints`；`flowrt list` 摘要会展示
  `island_profiles` 和 `boundary_endpoints` 计数，并列出 boundary input/output 绑定端点。
- Rust/C++ runtime 增加显式 boundary primitive：`BoundaryInput` 支持按 latest snapshot
  注入、revision 跟踪、stale policy 和 scheduler 唤醒；`BoundaryOutput` 支持按 guard
  生命周期注册输出 sink，临时观测断开后自动回收。
- Rust/C++ generated runtime shell 接入 island boundary：island profile 下会为 typed
  boundary input/output 生成 runtime primitive 字段，boundary input 可驱动 `on_message`
  唤醒，boundary output 会发布到显式 sink；strict 生成物不携带 boundary primitive。
- 新增 `flowrt pub`：CLI 会从 live runtime 或 `--image` self-description 查找 boundary
  input，按 fixed Message ABI 将 JSON 编码为 canonical payload，并通过 runtime
  introspection socket 注入；strict 产物、普通 channel 和 boundary output 均默认拒绝。
- `flowrt echo`、`flowrt status` 和 `flowrt record` 接入 island boundary 观测路径：boundary
  output 以 endpoint 名称注册为可观察 channel，status 会展示 graph mode 与 boundary
  endpoint 绑定信息，record 可按 channel 过滤录制 boundary output snapshot。
- `flowrt bundle` / `flowrt deploy` 增加 island artifact 安全门：bundle manifest 记录
  `artifact_mode = "strict" | "island"`，默认拒绝打包或部署 island 脚手架产物，只有显式
  传入 `--allow-island` 才允许。
- ROS2 bridge 可显式绑定 Island Boundary Endpoint：`[[bridge.ros2]].flowrt` 既可以继续
  使用普通 `instance.port`，也可以在 island profile 下引用 `boundary.input` /
  `boundary.output` 名称；generated shell 会通过 zenoh-only bridge key 将 ROS2 输入注入
  boundary input，并把 boundary output 发布到 ROS2 adapter，FlowRT 与 ROS2 仍保持各自
  zenoh 命名空间隔离。
- 新增 `examples/island_demo`：用 `boundary.input sample_in -> processor -> boundary.output
  result_out` 展示单功能单位 typed IO 测试；文档补充 `flowrt pub` 注入、`flowrt echo`
  观察和 `flowrt record` 录制 boundary output 的用户路径。

### 修复

- 修复 `v0.9.0 Island Demo Smoke` 在 arm64 runner 上仍按示例默认
  `linux-amd64` target 构建的问题；CI 现在会按矩阵架构把临时 demo RSDL 切到
  native target，避免 arm64 发布门禁误找 x86_64 Rust target。
- 修复生成的 Rust fixed `WireCodec` 在多字段消息中最后一次 `cursor` 推进后未读取导致的
  `unused_assignments` warning；生成代码现在会在 encode/decode 结束时断言 cursor 等于
  `WIRE_SIZE`，同时校验 wire size 推导。

### 测试

- 补充 `flowrt-rsdl` 与 `flowrt-ir` 的 island/boundary 解析、归一化和 canonical ordering
  覆盖，避免 boundary endpoint 声明顺序影响 Contract IR JSON。
- 补充 `flowrt-validate` 的 island/boundary 校验覆盖，包括 strict 拒绝、boundary input
  满足 task 输入、重复满足拒绝、boundary output 方向校验和 IR 防篡改 canonical 检查。
- 补充 `flowrt-selfdesc`、`flowrt-codegen`、`flowrt-cli` 和 runtime supervisor 的
  island/boundary schema 与 manifest 解析测试，保证新版静态字段可被安装后工具和
  generated supervisor 消费。
- 补充 Rust/C++ runtime boundary primitive 覆盖，验证 boundary input 注入会唤醒 waiter、
  stale drop 生效，以及 boundary output sink guard 析构后不再收到样本。
- 补充 `flowrt-codegen` 的 island boundary 生成覆盖，验证 Rust/C++ shell 接线和 strict
  shell 负向路径。
- 补充 `flowrt-cli` 的 `pub`、boundary output echo、live status enrich、bundle/deploy
  island gate 和命令解析测试，保证 CLI 不支持普通生产 channel 注入。
- 补充 runtime introspection `boundary_publish` 请求测试和 C++ smoke 覆盖，验证 socket
  请求会调用已注册 boundary input handler，并能返回结构化错误。
- 补充 ROS2 bridge / boundary endpoint 的 IR、validator 和 codegen 覆盖，验证 boundary
  引用进入 Contract IR、篡改后的方向/端口会被 validator 拒绝，并确保 generated shell
  不把 ROS2 输入绕过 boundary 直接接进 task。
- 新增 `scripts/test-v090-island-demo.sh` 和 CI `v0.9.0 Island Demo Smoke`，在 amd64 与
  arm64 上构建 island demo、运行 runtime、用 `flowrt pub` 注入 boundary input，并用
  `flowrt echo` 校验 boundary output 字段。
- 发布就绪脚本新增 v0.9.0 focused gate，检查 island smoke job、双架构矩阵、FlowRT
  island cache、smoke 脚本和 `pub` / `echo` 闭环是否进入 CI。

## v0.8.6 - 2026-06-11

### 新增

- 新增 `flowrt toolchain show --target <platform>`，展示合并后的 toolchain profile，
  包含每个字段的来源标注（builtin/system/user/workspace）和配置层优先级说明。
- 新增 `flowrt toolchain init --target <platform> [--sdk-overlay <path>] [--force]`，
  在当前 workspace 下生成 `.flowrt/toolchains.toml`；默认不覆盖已有配置，`--force`
  允许重写。生成的 TOML 保持最小可读格式，可后续手动编辑。
- 新增 `flowrt cache status` 和 `flowrt cache clean`，用于展示 FlowRT deps cache、
  当前项目 `flowrt/build`、stale 临时候选和 SDK overlay 占用，并按显式范围安全清理
  FlowRT 管理的可重建目录，避免误删用户 SDK overlay、安装前缀、live socket、`.mcap`
  和日志。
- 新增 `flowrt doctor [<rsdl>] --target <platform>` 的 Contract IR 依赖预检：传入 RSDL
  后会校验 selected profile 的 Contract IR，并逐项检查 selected target 下 C++
  component `build.pkg_config` 模块的 pkg-config 可见性，输出所属组件、模块状态、
  `.pc` 路径、include/libdir 摘要和 SDK overlay 修复提示。

### 变更

- `flowrt build --target <platform>` 成功后会输出简短 build summary，展示 target
  platform、build mode、Rust target triple、C/C++ compiler、runtime dependency
  policy、SDK overlay、selected `pkg_config` 模块和最终二进制路径；当 target SDK
  或 `pkg-config` 依赖缺失时，会 fail-fast 输出当前 `PKG_CONFIG_LIBDIR`、缺失项和
  `flowrt doctor <rsdl> --target <platform>` 修复提示，同时保留底层 CMake/Cargo
  原始错误。
- 公开交叉 SDK smoke 迁移到 `v0.8.6 Cross UX SDK Smoke`：通过
  `flowrt toolchain init/show` 生成并展示最小 workspace profile，通过带 RSDL 的
  `flowrt doctor <rsdl> --target linux-arm64` 检查 `pkg_config` 依赖，再执行
  `flowrt deps/build --target linux-arm64` 和 AArch64 ELF 检查。
- `flowrt cache` 的 status/clean 实现从 CLI `main.rs` 拆入独立 module，保持命令、
  输出和删除安全语义不变，降低后续磁盘治理改动的维护成本。

### 测试

- 发布就绪检查新增 `v0.8.6` focused gate 校验，确认 release 依赖公开交叉 SDK smoke，
  且 smoke 脚本覆盖 `toolchain init/show`、带 RSDL 的 `doctor`、pkg-config 模块命中和
  build summary。

## v0.8.5 - 2026-06-11

### 新增

- 新增 `examples/cross_sdk_deps`，用显式 CMake prepare 步骤拉取并交叉编译公开真实
  依赖 `libjpeg-turbo` 与 `Arm KleidiAI`，生成可被 `flowrt build --target
  linux-arm64` 消费的 demo-local SDK overlay；FlowRT 用户构建阶段不隐式联网拉取依赖。
- 新增 `examples/libjpeg_cross_demo`，验证 C++ component 通过 RSDL
  `component.build.pkg_config = ["libjpeg"]` 使用可移植 C/C++ 库，并交叉编译为
  AArch64 二进制。
- 新增 `examples/kleidiai_cross_demo`，验证 Arm 专用公开 SDK 通过 pkg-config overlay
  接入 FlowRT C++ component，并在 arm64 目标机运行真实 NEON kernel。

### 测试

- 新增 `scripts/test-v085-cross-sdk-demos.sh` 和 CI `v0.8.5 Public Cross SDK Smoke`，
  在安装 amd64 deb 后准备公开 arm64 SDK overlay，执行 `flowrt doctor/deps/build
  --target linux-arm64`，并检查产物 ELF 为 AArch64。

## v0.8.4 - 2026-06-11

### 新增

- `component.<name>.build.pkg_config` 进入 RSDL / Contract IR：C++ 组件可以声明可移植
  的 pkg-config 模块名，codegen 会生成对应的 `find_package(PkgConfig)` 和
  `pkg_check_modules(...)` 链接逻辑，但不会把板端路径写进契约。
- toolchain profile 新增 `cpp_compile_args`、`cpp_link_args` 和
  `cpp_link_libraries`，用于把板级私有 SDK 需要的 C++ 编译选项、链接选项和私有
  `.so` 路径放在配置层，保持 RSDL 只描述语义，不描述本机路径。

### 修复

- 修复 `flowrt deps/build --target linux-arm64` 构建 Rust app、generated supervisor 和
  runtime 依赖时未向 Cargo 传递目标 linker 的问题，避免交叉编译阶段误用 host linker。
- 修复 `flowrt launch` 启动 generated supervisor 时没有注入 FlowRT 私有 `lib` 目录的
  问题，使安装包内嵌的 `zenoh-c` 等动态库能被 C++ 子进程直接加载。
- 修复 Rust generated shell 在 iox2/zenoh 输入观测记录中先返回借用 view、再读取
  endpoint revision 导致的借用冲突。
- 修复 supervisor 不能解析 launch manifest 中 I/O boundary resource descriptor schema
  的问题，使 frame descriptor demo 能通过 `flowrt launch` 正常运行。
- 修复 ROS2 bridge demo 在安装包环境中继承 FlowRT 私有 `libzenohc.so` 后污染
  `rmw_zenoh_cpp` 进程的问题；generated ROS2 bridge 现在优先使用 ROS2
  `zenoh_cpp_vendor`，supervisor 会为 `ros2_bridge` 子进程隔离动态库路径。

## v0.8.3 - 2026-06-10

### 新增

- `linux-amd64` 安装包内嵌完整 `linux-arm64` target SDK，包含 FlowRT C++ runtime、
  `iceoryx2-cxx`、`zenoh-c`、`zenoh-cpp`、CMake package 和 pkg-config 事实源；amd64
  host 可直接构建 arm64 FlowRT 用户程序，不需要目标板编译或从目标板拉取目录。
- toolchain profile 增加 SDK overlay、额外 CMake prefix、多个 pkg-config libdir 和
  runtime dependency policy 字段，用于接入板级私有 SDK，同时保持 RSDL / Contract IR
  不暴露 vendor 路径。
- 新增 `flowrt doctor --target <platform>` 预检命令，检查 Rust target、C/C++ 交叉
  编译器、完整 target SDK、pkg-config/CMake 查找路径和 SDK overlay，并给出可执行
  修复提示。
- CI 增加 v0.8.3 完整交叉编译 gate：在 amd64 runner 安装 amd64 deb 后实际执行
  `flowrt build --target linux-arm64`，并用 ELF 架构检查验证输出为 AArch64。

### 变更

- 新增 `scripts/test-v083-installed-smoke.sh` 作为 v0.8.3 安装后交叉编译 smoke，不再把
  `linux-arm64` target SDK incomplete 当作期望行为。
- `flowrt build` 不再在每次 Cargo build 前清理 generated package 产物，避免把可复用
  的用户侧增量编译 cache 当作中间垃圾删除；最终运行二进制仍复制到项目自己的
  `flowrt/build/bin/...` 路径。
- C++ generated app 的 CMake 临时 build dir 按 target platform 分层，避免 native 与
  cross target 来回构建时互相污染，同时保留 CMake 增量构建能力。
- release readiness 脚本改为校验 v0.8.3 完整交叉编译 gate、安装后 smoke 和 target
  SDK layout smoke，避免发布流程继续认可 0.8.2 的占位 SDK 语义。
- deb 包补齐 Rust runtime examples，并在包 smoke 中校验安装后 runtime crate 可被
  Cargo 解析，避免 `flowrt deps` 在离线预热时因缺少 example 源文件失败。
- workspace、Rust runtime crate、C++ runtime package 版本升级到 `0.8.3`。

### 修复

- 修复 release package job 中 amd64 deb 未可靠安装 `aarch64-linux-gnu` 交叉编译器的
  问题，确保 amd64 安装包可以内嵌完整 `linux-arm64` target SDK。

## 未来规划

- **v0.12.0：Contract-driven App Authoring**。纠正 FlowRT app 作者主路径：
  用户先写 RSDL，再通过 `flowrt check` / `flowrt prepare` / `flowrt explain`
  获取 codegen 生成的真实实现接口，最后在 `app/` 中填写算法。`flowrt add`
  只作为 RSDL 编辑助手，不再生成或修改用户业务代码；用户实现清单、签名、
  推荐文件路径和 agent/IDE 可消费的 App API manifest 由 validated Contract IR
  与 codegen 统一产出。`prepare` 可以在 `flowrt/app/stubs/` 生成可删除、可重建的
  参考模板，但不得直接创建、追加或覆盖用户 `app/` 代码。
- **v0.13.0：Robot Runtime Completion**。除 Python 之外，把已存在但仍需打磨的
  机器人运行时核心能力一次性收口：variable frame 工程化、FlowRT-native replay
  与 simulated clock、I/O boundary / external package health 与 restart、抽象
  capability / resource contract、参数 schema / instance override / pending apply、
  Operation 长任务语义、运行态观测诊断、多目标部署与交叉编译体验、以及可承接
  Python 的 C ABI 稳定边界。资源模型只表达抽象需求、provider、访问模式、
  readiness 和 health，不把串口、TCP、UDP、设备路径或板级 SDK 名称做进
  FlowRT 核心语义。
  本版本目标不是再开新方向，而是把现有能力升级到真实机器人系统可工程化使用。
- **v0.13.1：维护修复预留**。已确认的 scheduler 同步等待阻塞问题不再作为单独
  v0.13.1 主线拆出，而是并入 v0.14.0，避免调度 admission 改造和用户可见 timing
  context 在两个版本里漂移。
- **v0.14.0：Realtime Scheduler + Task Timing Context**。修复 scheduler 同步等待
  长任务导致周期 tick 被阻塞的问题，同时把 runtime scheduling time 暴露给用户算法。
  scheduler 线程负责 ready 判定、admission、backend commit、introspection 和
  deterministic output commit；worker 只运行用户 task，并把 completion 交回 scheduler。
  task timing 通过现有 `flowrt::Context` 提供，不改变 handler 签名。该版本只处理
  runtime 调度时间：realtime 报告 observed time、lateness 和 `dt`，replay / temporary
  island 保持 simulated clock 确定性；不承诺硬实时，不做 PTP、NTP、exact sync、
  approx sync、sensor event-time 或多传感器同步策略。
- **v1.0.0：稳定性与兼容策略**。冻结 RSDL schema、Contract IR、Message ABI、
  self-description、launch manifest、bundle schema 和 C ABI 的兼容规则，补齐
  故障注入、性能矩阵、版本协商、升级/降级测试和长期 release policy。0.x 阶段
  继续允许破坏性修正长期模型错误，不为 pre-1.0 历史债保留兼容层。

## v0.8.2 - 2026-06-10

### 新增

- CLI 内部新增交叉编译 toolchain profile 配置层，集中维护 `linux-amd64` /
  `linux-arm64` 到 Rust target triple、Deb multiarch 和默认 C/C++ compiler 的映射，
  并支持按 system、user、workspace、CLI override 优先级合并可选配置字段。
- `flowrt deps` 和 `flowrt build` 新增 `--target <platform>`，Rust app、generated
  supervisor 和依赖预热会使用同一个 Rust target triple，并把 cache key、ready marker
  和 Cargo 输出路径按 target triple 隔离；未显式指定时会从选定 Contract IR target
  platform 推导，仍无 platform 时保持 native 构建。
- `flowrt build --target <platform>` 接通 C++/CMake 交叉编译主路径：CMake 会消费
  toolchain profile 的 toolchain file、compiler、sysroot 和 pkg-config 设置，并优先
  使用完整 target SDK 的 CMake/pkg-config/include/lib 事实源。
- CI 新增 `v0.8.2 amd64 to arm64 Cross Compile Smoke` focused gate，在 amd64 host 上
  准备 `aarch64-unknown-linux-gnu` Rust target、`aarch64-linux-gnu` C/C++ 交叉编译器
  和 `pkg-config`，并运行 `flowrt-cli` 的 toolchain、build model、command 与 CMake
  target SDK 交叉编译相关测试。
- 新增 `scripts/test-v082-installed-smoke.sh`，安装后验证 target SDK manifest、
  `flowrt deps --target ... --check` 提示和 incomplete target SDK 的 C++ cross build
  fail-fast 行为。

### 修复

- 修复 Debian 包安装后用户项目 smoke 在 arm64 环境中仍按示例默认 `linux-amd64`
  target 构建的问题；smoke 会按当前包架构重写临时示例 RSDL，并让依赖预热和构建
  使用同一个 FlowRT platform。

### 变更

- Debian 包新增 `/opt/flowrt/<version>/targets/<platform>` target SDK 布局，当前原生架构
  目录提供 C++ runtime、后端 SDK、CMake 和 pkg-config 查找事实源；未内嵌的另一架构
  目录以 `flowrt-target-sdk.toml` 标记为 `complete = false`。
- C++/CMake 交叉构建遇到 target SDK manifest 缺失或 `complete = false` 时会 fail-fast，
  不再把 host 私有前缀当作目标架构 SDK 继续配置。
- `flowrt build` 的用户项目二进制在实际 cross target 构建时改为复制到
  `flowrt/build/bin/<platform>/<mode>/`，native 或无 cross target triple 时保留
  `flowrt/build/bin/<mode>/` 兼容路径；`build-info.json` 同步记录 target identity、
  Rust target triple、host triple 和 executable 相对路径，供 `run` / `launch` /
  `bundle` 定位产物。
- package 与 release job 纳入 v0.8.2 交叉编译 focused gate，package 阶段同时运行
  target SDK layout smoke，避免 tag release 绕过交叉编译和 SDK 布局检查。
- `flowrt bundle` 改为优先使用 `build-info.json` 的 artifact closure；带 platform 的
  本项目二进制在 bundle 内复制到 `bin/<platform>/<filename>`，manifest schema v2 的
  artifact path、platform 和 sha256 与实际复制文件一致，避免多 target 同名二进制
  覆盖或混淆。
- `flowrt deploy` 继续以 schema v2 artifact 列表为事实源，并补充 target/platform
  artifact closure 校验：目标产物缺失、platform 与路径层级不匹配或 sha256 不一致时，
  会提示重新执行对应 platform 的 `flowrt build --target <platform> --launcher` 后再
  bundle。
- workspace、Rust runtime crate、C++ runtime package 版本升级到 `0.8.2`。

### 测试

- CI 为 Rust/Cargo job 增加架构隔离的构建缓存，并为 package、demo smoke 和 ROS2
  bridge smoke 增加 `FLOWRT_CACHE_DIR` 缓存；deb 成品、release notes 和 artifact
  manifest 仍每次从源码重建。

## v0.8.1 - 2026-06-10

### 新增

- 新增标准 `FrameDescriptor` 输出端口绑定：`io_boundary` resource descriptor 必须通过
  `port = "<output>"` 绑定一个固定 64 字节 descriptor message，validator 会校验字段
  名称、顺序、类型和 fixed-size plain data 形状。
- Rust/C++ runtime 和 codegen 增加 `FrameDescriptorFields` helper，生成的标准
  descriptor message 可直接从 helper 构造，也可还原为 runtime descriptor fields。
- `flowrt echo` 会把标准 FrameDescriptor payload 按结构化字段展示；`flowrt status`
  会展示 I/O boundary resource descriptor schema；`flowrt record` 默认保持
  descriptor-only，不把真实图像 payload 当作普通 channel sample 复制进 MCAP。
- 新增 `examples/frame_descriptor_demo`，演示本机 `iox2` route 只传固定 descriptor，
  真实 payload 由 I/O boundary / side-channel 管理。
- 新增 `scripts/bench-frame-descriptor.sh`，用于本机快速对比 64 字节 descriptor
  encode/decode 和 payload memcpy 的量级。
- CI 增加 `v0.8.1 FrameDescriptor Smoke` amd64/arm64 focused gate，并在安装后
  demo smoke 中运行 `scripts/test-v081-installed-smoke.sh`。

### 修复

- 修复 `scripts/test-v080-installed-smoke.sh` 中旧 FrameDescriptor 示例字段，改为当前
  标准 64 字节形状和 descriptor port 绑定，避免旧 smoke 被新 validator 拒绝。
- 修复 `scripts/test-v080-installed-smoke.sh` 的 variable frame 自描述断言：改用当前
  `message_frames` / `canonical_frame_v1` / `variable=true` schema，避免 CI demo smoke
  在 build 成功后因过时 `abi_kind` 字段无声失败。

### 变更

- workspace、Rust runtime crate、C++ runtime package 版本升级到 `0.8.1`。
- `scripts/check-release-readiness.sh` 增加 v0.8.1 focused gate、安装后 smoke 和
  microbench 覆盖检查。

## v0.8.0 - 2026-06-10

### 新增

- 新增 `io_boundary` component 静态合同：RSDL/Contract IR 可以声明进程内 I/O boundary
  组件、资源需求、side effect、readiness、health 和 shutdown policy；launch manifest
  与 self-description 会输出对应摘要，供后续 runtime health 和诊断路径消费。
- 新增 target platform 规范化模型：RSDL target 和 external package 平台输入统一归一为
  `linux-amd64` / `linux-arm64`，旧写法 `linux-x86_64` / `linux-aarch64` 仅作为输入别名。
- `flowrt bundle` 输出 bundle schema v2：manifest 保留旧 deploy 字段，同时新增
  `artifacts` 列表，记录每个产物的 target、platform、相对路径和 sha256；external
  package executable 会按 target platform 做支持性校验。
- `io_boundary` component 接入 Rust/C++ runtime 主路径：生成的 runtime shell 会注册
  boundary 资源状态，并在生命周期钩子中传入 `BoundaryContext`，用户可上报 readiness、
  resource health 和 last error；`flowrt status` 的 live status schema 增加
  `io_boundaries` 字段。
- variable frame 主路径补强：Rust/C++ runtime 都覆盖 canonical tail order 测试，
  codegen 覆盖 `sequence<fixed struct>`、`string` 和 `bytes` 的 frame codec 生成。
- `flowrt deploy` 读取 bundle schema v2 的 artifact 列表，按请求 target 选择产物，
  并校验 artifact platform、相对路径、文件存在性和 sha256；schema v1 bundle 仍按
  顶层 target 字段兼容。
- CI 增加 `v0.8.0 Integration Smoke` amd64/arm64 focused gate，并在安装后 demo smoke
  中验证 variable frame、I/O boundary、FrameDescriptor 自描述、bundle v2 和 deploy
  dry-run 主路径。

### 修复

- 修复 ROS2 bridge adapter 头文件依赖边界：纯 `std_msgs/msg/String` bridge 不再生成
  `geometry_msgs/msg/Pose` include，避免安装后 smoke 在未安装 `geometry_msgs` 的 ROS2
  环境中构建失败。
- 修复 C++ Message ABI fixture 对嵌套 fixed struct array 的类型限定：生成在
  `flowrt_app` 命名空间外的测试辅助代码时，数组元素类型会使用 `flowrt_app::Type`
  限定名，避免 `std::array<PathPoint, N>` 这类未限定类型导致 C++ 构建失败。
- 修复 mixed fixed/variable contract 的 ABI fixture 生成边界：fixed ABI 测试只为
  fixed-size plain data 类型生成 sample helper，不再遍历 variable frame 类型后触发
  codegen panic。

### 变更

- `scripts/check-release-readiness.sh` 增加 v0.8.0 focused CI gate 和安装后 smoke 覆盖
  检查，发布前会同时校验版本来源、CHANGELOG 版本段和 release notes 抽取。

## v0.7.1 - 2026-06-09

### 修复

- 加固 `flowrt deploy` 的 SSH/SCP 参数边界：拒绝空 host 和 `-` 开头的 host，避免远端主机名被底层工具解释为命令行选项。
- 加固 generated supervisor 的子进程生命周期：启动失败、readiness timeout、失败传播和 SIGINT/SIGTERM 关闭路径都会清理已启动子进程，并在重启前重新校验依赖与 readiness。
- 修复参数规范化边界：拒绝 `NaN` / `Inf` 参数值，避免宽整数与浮点约束比较时因 `f64` 舍入漏检，并允许空数组默认值被数组覆盖。
- 修复 C++ runtime shell 参数热更新整数解码：无符号参数使用无符号解析路径，并在写入 `u8`、`u64` 等目标类型前做范围检查。
- 修复 C++ 消息生成中的非标准 `__int128`：改用 FlowRT C ABI 中的 128-bit POD，并补齐 C++ wire codec 读写支持。
- 放宽 Service frame 的未知错误码处理：header 解码保留 raw `u16`，上层调用按未知错误降级处理，避免未来错误码导致旧 runtime 直接拒帧。
- 修复 launch manifest 生成中的 service 类型校验：不再依赖 debug-only assert，手工损坏的 IR 在 release 路径也会返回结构化 codegen 错误。
- 放宽 `flowrt deploy` 的 bundle 版本策略：同一 `major.minor` 的 patch 版本差异允许部署并输出 warning，跨 minor 或格式非法仍拒绝。
- 修复 supervisor 自动 zenoh mesh 的端口抢占窗口：不再通过 bind/drop 伪预留端口，而是在 supervisor 生命周期内持有 FlowRT 本机端口租约。
- 修复 supervisor 启动阶段的关闭处理：在启动子进程前安装 SIGINT/SIGTERM handler，readiness 和错峰启动等待期间收到关闭请求会终止已启动子进程。
- 修复 launch manifest 中 process `env` 被 supervisor 忽略的问题：普通进程、external process 和 restart 路径都会注入用户声明的环境变量，并保持 FlowRT 保留变量优先。
- 修复 supervisor 对当前 launch manifest 的严格解析漂移：process 的 `target`、`runtimes`、`tasks` 和 service 的 `lane`、`max_in_flight` 字段会被显式接收，未知字段仍会被拒绝。
- 修复 `flowrt launch --run-steps` 只转发给子进程的问题：supervisor 现在也会根据 live tick 快照主动终止达到上限后仍在运行的子进程。
- 限制 runtime introspection socket 的并发连接线程数，超过上限时返回结构化 error，避免异常客户端耗尽线程资源。
- 修复 Service / Operation / external backend 合同校验：validator 会检查 route backend 是否被 endpoint target 支持，并拒绝手工篡改为跨进程 `inproc` 的 IR；launch manifest 和生成依赖也会把 Service / Operation 的 `zenoh` backend 计入进程 backend。
- 修复 service readiness 过早放行：runtime 预注册 service 时默认 `ready=false`，生成的 Rust shell 只在组件启动和 startup task 成功后标记 service ready，C++ introspection 同步该语义。
- 修复 supervisor readiness 轮询可能被异常 introspection socket 卡住的问题：runtime/service readiness 查询会限制单次 socket 读写等待，不再越过外层 readiness deadline。
- 修复 iox2 publish 路径的重复发送风险：payload 已发送后若 wake notify 失败，只记录 backend 健康退化，不再重发同一 payload。
- 修复 iox2 wake listener 启动失败被吞掉的问题：listener 打开失败、线程创建失败或 ready 信号异常会返回结构化 backend 错误，启动失败路径不再丢弃 worker join handle，也不再让 on-message 唤醒假装可用。
- 补齐 Rust C ABI 与 C++ runtime 的 backend health / 128-bit POD parity：增加 `Unsupported` 健康状态和 `FlowrtU128` / `FlowrtI128` layout 测试。
- 加固 bundle/deploy 边界：bundle 复制拒绝 symbolic link，`flowrt deploy` 拒绝空远端目录，并解析远端 `flowrt --version` 确认同一 `major.minor` 后才上传。
- `flowrt bundle` 复制项目二进制后会对 ELF 可执行文件 best-effort 执行 `strip --strip-unneeded`，并在命令摘要中报告 `stripped_executables` 和 `strip_warnings`。
- 修复 `flowrt record` 的输出文件边界：显式 `--socket` 时不再先扫描 live runtime，录制启动失败时会清理临时文件且不留下空 MCAP 输出。
- 修复 `flowrt hz` 多 runtime 扫描鲁棒性：单个 socket 返回非 Status 响应时按 stale 记录，不再拖垮整体 hz 输出。
- 加固 Rust zenoh service runtime：server handler 执行改为有界 in-flight，client/server service 错误会同步更新 endpoint health。
- 修复 C++ runtime recorder schema 漂移：operation、task 和 lane 事件使用与 Rust 相同的 `flowrt.*` payload schema，并补齐调度/Operation 健康字段。
- 修复 C++ inproc service 与 Rust runtime 的语义漂移：显式零超时返回 `Timeout`，`queue_depth=0` 拒绝新请求，统计中补齐 handler error 计数，生成的 C++ Service / Operation wrapper 默认使用 RSDL policy timeout。
- 加固 Contract IR JSON 读取：`ContractIr::from_json_str` 会拒绝任意层级未知字段，避免手工篡改或未来 schema 被旧工具链静默吞掉。
- 修复 native `zenoh` Service / Operation codegen 的假可用状态：在真实 transport 接线完成前改为 fail-fast，避免生成只返回 `Backend` 的 placeholder；external endpoint 的 manifest 表达保持可用。
- 修复 generated Operation 的并发策略漂移：当前 runtime 只支持单 in-flight `reject` 语义，第二个 start 会返回 `Busy`；`queue`、`cancel_running` 和多 in-flight 策略在完整实现前由 validator 拒绝。
- 修复参数声明类型范围校验：`u8`、`i16`、`f32` 等窄类型的默认值和 instance 覆盖值会在 IR 归一化阶段拒绝越界值，避免生成代码出现截断或不可移植浮点 literal。
- 新增 `flowrt status --live-only`，用于隐藏 stale socket 诊断行，只显示仍能返回 live status 的 FlowRT runtime。
- 修复 Rust inproc Service 的 pending request 生命周期：超时后未处理的排队请求在 server/registry drop 时会释放 in-flight 计数和 endpoint 引用，避免泄漏 service endpoint。
- 加固 runtime introspection 控制面：idle client 会在初始请求超时后释放连接 slot，Rust/C++ 的 `echo`/observe 长连接使用独立 observer 配额，不再耗尽 `status` / `params` 控制面。
- 修复 C++ runtime observe 长连接空闲超时问题：稀疏 channel 的 `flowrt echo --follow` 不会在 1 秒无 keepalive 后被服务端误关闭。
- 修复 Rust/C++ zenoh Service 对未来错误码的分类：未知 response error code 映射为 `Protocol` 并保留 raw code，不再误记为 backend 故障。
- 收窄 iox2 backend overflow capability：validator 现在拒绝 iox2 route 上无法由 runtime 精确表达的 `drop_newest` 和 `error`，只保留 `drop_oldest` 与 `block`。
- 修复 Contract IR 派生元数据校验边界：显式 external dataflow `backend = "zenoh"` 保留 explicit backend source；validator 会重新运行 route backend resolver，拒绝手工伪造 external route backend 或缺少 target 时的 selected backend capability；参数 enum choice 也必须满足声明的 min/max 约束。
- 加固 `flowrt deploy` 远端目录参数：dry-run 和真实部署都会拒绝相对路径、`.`/`..` 路径段和非 POSIX-safe 字符，避免把 remote_dir 拼接进远端 shell 命令时形成注入风险。
- 安装包内嵌 cargo vendor hash marker，安装版 `flowrt deps/build` 缺少 marker 时 fail-fast，避免从系统安装路径静默回退到开发机源码仓库路径。
- 修复 `WorkerPool` job panic 后 active slot 不释放的问题：worker 会捕获 job panic、标记 executor error，并保证 shutdown 不因 active 计数残留而挂住。
- 修复发布就绪脚本的版本参数归一化：`scripts/check-release-readiness.sh` 现在同时接受 `0.7.1` 和 `v0.7.1`，不会误拼出 `vv0.7.1`。

### 后续规划

- `v0.8.0` 规划真实机器人应用接入边界、variable frame 工程化、多目标部署和发布硬化；`v0.9.0` 规划 C/Python API 与可选生态互操作扩展；`v1.0.0` 规划 ABI/schema 稳定、兼容策略、故障注入和性能矩阵。
- 参数热更新继续作为 runtime control-plane service-like RPC，可复用 schema、validation、structured error、pending/apply 和 self-description 经验，但不并入 graph 业务 Service 或 Operation 语义。

## v0.7.0 - 2026-06-08

### 新增

- RSDL/Contract IR 增加 external component 和 graph 级 `[[external_process]]`：可以声明由外部 package/executable 提供的 typed component，并记录 package、executable、args、working directory、health 和 required backend。
- backend resolver / validator 增加 external route 规则：涉及 external component 的 dataflow、Service 和 Operation route 自动选择 `zenoh`；显式 `inproc` 被拒绝，`iox2` 在 external package 能力与固定大小约束未完整建模前默认拒绝。
- 新增 `flowrt external check/list`，用于校验和列出 external package manifest。manifest 文件为 `flowrt-external.toml`，包含 package metadata、executable path、platform、backend、health 和 license 字段。
- launch manifest 和 self-description 输出 external process/package 摘要；`runtime_kind = "external"` 的 process 可被 generated supervisor 统一编排。
- generated supervisor 支持 external process：按 `FLOWRT_EXTERNAL_PATH`、`/opt/flowrt/external/<package>`、项目 `external/<package>` 查找 package，启动 external executable，注入 `FLOWRT_PROCESS`、`FLOWRT_BACKEND`、`FLOWRT_EXTERNAL_PACKAGE`、`FLOWRT_EXTERNAL_PACKAGE_ROOT`、`FLOWRT_WORKSPACE_ROOT` 和 `FLOWRT_RUN_STEPS` 等环境变量，并把 external process 纳入 restart/readiness/status 机制。
- 新增 `flowrt bundle`：把已构建项目打包为离线 bundle 目录，包含本项目二进制、Contract IR、launch manifest、self-description、build-info 和本地 external package 副本。
- 新增 `flowrt deploy` baseline：读取 bundle manifest，校验 target，非 dry-run 时通过 SSH/SCP 做远端 FlowRT 版本检查和 bundle 上传。
- 新增 `examples/external_driver_demo`，提供无硬件依赖的 external package 示例，覆盖 external manifest、supervisor 启动、环境变量契约、bundle 和 deploy dry-run。
- CI 增加 `v0.7.0 External/Deploy Smoke` amd64/arm64 focused gate，并在安装后 demo smoke 中验证 external package、bundle 和 deploy dry-run 主路径。

### 变更

- external-only contract 也会生成 supervisor-only Rust crate；不会为 external component 生成 Rust trait 或 C++ interface。
- external executable 不接收生成 app 的内部 `--process` 参数；上下文通过环境变量传递，避免把 FlowRT managed shell 的 CLI 约定泄漏给外部包。
- `scripts/check-release-readiness.sh` 增加 v0.7.0 focused CI gate 和安装后 smoke 覆盖检查。

## v0.6.1 - 2026-06-08

### 新增

- 新增 `flowrt deps` 命令，用于按 RSDL/profile 或显式 backend 预热 FlowRT 底层依赖
  缓存。默认使用 release 构建，缓存目录按 FlowRT 版本、Rust toolchain、target
  triple、vendor hash、backend feature 组合和 build mode 分组。
- `flowrt build` 默认改为 release 构建，并把 Rust app、generated supervisor、C++
  app 和 ROS2 bridge adapter 统一复制到用户项目的 `flowrt/build/bin/<mode>/`。
  `flowrt run` / `flowrt launch` 通过 `flowrt/build/build-info.json` 读取已构建产物。

### 修复

- 修复 `v0.6.0` Debian package CI 失败的根因：deb 包漏装 `flowrt-record` crate，导致
  安装后 Rust 用户项目无法解析私有 runtime 依赖。
- 安装包 smoke 增加 `flowrt-record` 路径断言，并在安装后用户项目、demo 和 ROS2
  bridge smoke 中先执行 `flowrt deps`，再验证 release 本地 bin 输出路径。
- 修复 supervisor 在新 build bin 布局下启动 C++ app 和 ROS2 bridge adapter 的路径解析；
  现在优先查找与 supervisor 同目录的 sibling executable，并保留旧 CMake 输出路径回退。
- 修复 supervisor Zenoh 环境变量单元测试的并发污染，避免 CI 中随机失败。

### 变更

- `flowrt build` 不再把底层依赖预热和用户项目构建混在一个隐式步骤中；缺少匹配的
  deps ready marker 时会 fail-fast，并提示先运行 `flowrt deps`。
- 生成项目的用户二进制保留在用户自己的工作空间内；全局 cache 只作为构建加速和
  依赖复用机制，不作为部署事实源。

## v0.6.0 - 2026-06-08

### 新增

- RSDL/Contract IR/validator 增加 Operation 合同层语义：组件可声明 typed operation client/server 端口，`[[bind.operation]]` 绑定 client/server，normalization 生成 stable ID、canonical ordering、auto backend 选择和显式 policy metadata。
- Operation policy 支持 `timeout_ms`、`concurrency`、`preempt`、`queue_depth`、`max_in_flight`、`feedback` 和 `result_retention_ms`；validator 校验端口方向、goal/feedback/result 类型匹配、client 唯一绑定、backend 支持范围和关键数值字段合法性。
- Rust/C++ runtime 增加 Operation primitive：`OperationId`、状态机、policy、cooperative cancel token、progress carrier、状态快照和健康计数，为后续 codegen lowering 与观测/录制接入提供统一基础。
- Rust/C++ codegen 为 Operation client/server 端口生成 typed 用户 API：client 暴露 `start`/`cancel`/`status`，server handler 接收 typed goal、cooperative cancel token 和 progress publisher，并把 inproc Operation 编译期 lower 成稳定命名的内部 start/cancel/status service 与 feedback/result endpoint。
- self-description、runtime introspection 和 CLI 接入 Operation 观测：`flowrt list` 展示 Operation endpoint 与 lowering refs，`flowrt status` 展示 live health，`flowrt op list/status/cancel` 提供本机 Operation 观测和 cancel 控制面。
- 新增 `flowrt-record` crate：定义 FlowRT `RecordEnvelope` v1、record event kind、payload encoding 和 MCAP writer 基础封装；录制文件使用 MCAP 容器、JSON schema 和 FlowRT 自有 envelope，不依赖外部消息 schema，也不实现 replay。
- Rust/C++ runtime 增加按需 recorder tap：默认关闭时 channel 热路径只做轻量开关判断；开启后以有界队列采集 channel sample、参数、Operation、scheduler、clock 和 runtime metadata 事件，`IntrospectionStatus.recorder` 暴露 enabled、output、dropped count、accepted payload bytes 和 active filters；生成的 Rust/C++ runtime shell 会在 publish 路径同时服务 echo probe 和 recorder tap。
- 新增 `flowrt record`：可扫描唯一 live runtime 或通过 `--socket` 指定进程，按 `--channel` / `--operation` / `--all` 过滤 FlowRT 事件并写入 MCAP 文件；无 `--duration` 时运行到 SIGINT/SIGTERM，并在退出前停止 recorder、drain 剩余事件和输出 `event_count` / `dropped_count` / `bytes_written` 摘要。
- 新增 `examples/operation_demo`：提供 Rust inproc Operation RSDL、用户 server handler、
  可运行 generated app 和 `flowrt op list` 自描述 smoke。
- CI 新增 `v0.6.0 Runtime Smoke` amd64/arm64 focused gate，覆盖 Operation
  RSDL/IR/validator/codegen/runtime/CLI/status，以及 record format、runtime tap 和 CLI
  写 MCAP 路径；demo smoke 增加安装包后的 Operation 与 record 用户路径验证。

### 修复

- 修复 Rust codegen 在 Operation server 同时拥有普通 task 时未通过组件 mutex 调用普通 task 的问题，避免 generated shell 编译失败。

### 变更

- 明确 `v0.6.0` 的交付边界是 Operation + record-only 录制系统 + 时间事件模型基础。Operation 底层编译期 lower 成 Service + Channel，用户只看 Operation，调试时才展开底层拓扑。录制使用 MCAP 容器和 FlowRT record envelope，本版本只做 `flowrt record`，不做 `flowrt replay` 或 simulated clock 驱动执行。

## v0.5.0 - 2026-06-08

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
- 文档补充 supervisor readiness 条件启动、错峰启动、env 注入、CPU affinity/priority 资源提示、远程参数控制面和调度健康指标的用法说明和 RSDL 示例。
- README 示例矩阵补充 `workspace_demo` 和 `imu_demo_iox2` 条目。
- FlowRT core skills 套组入库：`.agents/skills/` 当前落地 `frt-core-*` 和跨 core/app 编排通用的 `frt-subtask`，先服务 FlowRT 仓库维护者；首批包含 change intake、RSDL/IR、codegen、runtime parity 和 backend 五个 P0 skill。`frt-app-*` 暂作为 1.0.0 之后的保留命名空间，`write-frt-skill` 元技能负责约束命名、触发条件、硬门控、验证证据和 FlowRT 专有术语。
- CI 新增 `v0.5.0 Runtime Smoke` amd64/arm64 focused gate，独立覆盖 supervisor readiness/resource、远程参数控制面、status/hz 健康展示和 scheduler health 相关测试，并接入 package/release 依赖链。

### 修复

- 修复 `scripts/check-release-readiness.sh` 在目标版本段缺失时提前退出的问题；现在会汇总版本号、CHANGELOG、release notes 和 v0.5.0 CI gate 覆盖缺口，便于发布前一次性定位缺失项。

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
- C++ app target 改为仅在用户提供 C++ 源码或显式设置 `FLOWRT_USER_CPP_SOURCES` 时生成，避免没有用户实现时链接出不可用可执行文件。
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
