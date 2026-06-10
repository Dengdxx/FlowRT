# CONTEXT.md

本文档记录 FlowRT 仓库的当前上下文，供 coding agent 和维护者快速了解现状。它会
随版本演进更新；长期架构原则、语义约定、文档边界和提交纪律维护在 `AGENTS.md`。

## 当前版本背景

当前 workspace 版本为 `0.7.1`。`v0.7.1` 是 `v0.7.0` 之后的 hardening 版本，
聚焦现有能力的生产边界修复：deploy/bundle 参数边界、supervisor 子进程生命周期与
关闭路径、runtime introspection 控制面、Service / Operation / backend 错误分类、
Contract IR 派生元数据防篡改、C++/Rust runtime parity、安装包离线依赖标记和
`--run-steps` / `record` / `hz` 等调试主路径。

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
| `v0.9.0` | C/Python API、生态互操作扩展。 |
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
- `v0.9.0` 扩展 C/Python API 和可选生态互操作，但仍以 FlowRT 自身语义为中心。
- `v1.0.0` 冻结 ABI/schema 基线，并补齐兼容策略、故障注入和性能矩阵。

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
generated Operation runtime 先支持单 in-flight reject 子集：`concurrency =
"reject"`、`preempt = "reject"`、`max_in_flight = 1`；`queue`、`cancel_running`
和多 in-flight 仍保留为长期 IR 语义，在 runtime 完整实现前由 validator 拒绝。

Operation 解决的不是“长时间 service call”，而是机器人系统里常见的可取消、
可抢占、可观测、可恢复的长任务。生成器负责把 Operation lowered 成 request、
progress、feedback、cancel、result 和状态观测通道；用户只实现业务 handler 和策略
钩子，不手写底层协议。这样保留 Action 的实用能力，同时避免让用户维护分散的
start/cancel/result/progress glue。

Operation 观测路径沿用 FlowRT 自描述和本机 introspection socket：self-description
记录 operation client/server 端口、goal/feedback/result 类型、policy、backend 和
内部 lowering refs；runtime status 记录 ready/running/queued、当前 operation id、
成功/失败/取消/超时/抢占计数和最近状态转换时间；`flowrt op list/status/cancel`
提供本机观测和 cooperative cancel 控制面。`flowrt op start`、跨机 Operation 控制面
和 replay 驱动执行不属于 `v0.6.0` 范围。

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

当前 `v0.8.0` 已落地的静态边界：

- RSDL / Contract IR 支持 `kind = "io_boundary"` component，并记录 resource
  requirement、side effect、readiness、health 和 shutdown policy；launch manifest 与
  self-description 输出对应摘要，runtime/codegen 用户 API 仍是后续主路径。
- target platform 输入统一归一化为 `linux-amd64` / `linux-arm64`；`linux-x86_64` 和
  `linux-aarch64` 只作为旧输入别名接受，落盘 IR、自描述和 bundle manifest 均输出
  canonical 字符串。
- `flowrt bundle` 输出 schema v2，保留旧 deploy 字段，并新增 artifact 列表记录
  target、platform、相对路径和 sha256；external package executable 在 bundle 阶段按
  target platform 校验支持矩阵。

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
- `mixed_iox2_demo`：Rust source 与 C++ sink 通过 iox2 分进程连接。
- `mixed_zenoh_demo`：跨主机 copy backend、variable frame 和 mixed launch。
- `ros2_bridge_demo`：Rust source 到 ROS2 `/flowrt/text` 的 zenoh-only bridge。
- `external_driver_demo`：无硬件依赖的 external package、supervisor 和 bundle/deploy
  baseline。

仓库内可以用 `cargo run -p flowrt-cli -- ...` 调试 CLI，但面向用户的文档、示例和
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
- Service 请求/响应语义切片：RSDL component 可声明 `service_client` /
  `service_server`，graph 可用 `[[bind.service]]` 绑定 client/server；Contract IR、
  validator 和 launch manifest 已保留 service 拓扑，但 runtime RPC 调用 API 仍是后续
  切片。
- C/Python ABI 边界准备：`runtime/cpp/include/flowrt/abi.h` 定义 C ABI 版本、
  status/backend/health 整数编码、borrowed string/bytes view、reconnect policy 和
  backend health snapshot；Rust runtime 提供对应 `repr(C)` 镜像类型和转换函数。当前
  只是稳定跨语言边界，不提供 C runtime wrapper 或 Python binding。
- C++ only contract 的 CMake app 路径，支持 `flowrt build` / `flowrt run` / `flowrt launch`。
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
- generated supervisor 对 Rust、C++ 和 ROS2 bridge process 的分流启动、health
  汇总、PID socket 轮询、tick stale/exit/restart 状态展示、process 依赖顺序、
  可配置 restart policy 和失败传播 baseline。

## CLI 状态

当前已实现的用户入口：

```bash
flowrt check path/to/robot.rsdl
flowrt prepare path/to/robot.rsdl
flowrt deps [path/to/robot.rsdl]
flowrt deps --backend all
flowrt build path/to/robot.rsdl
flowrt run path/to/robot.rsdl
flowrt run path/to/robot.rsdl --process main
flowrt run path/to/robot.rsdl --run-steps 5 --process main
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

`prepare` / `build` / `run` / `launch` 支持 `--profile <name>`，用于显式选择 profile
并按该 profile 生成或校验产物。省略参数时会先投影到 `default` profile 或首个
profile。RSDL 未声明任何 profile 时，normalization 会插入隐式 `default` profile，
backend 为 `inproc`。

命令职责边界：

- `deps` 只写全局 FlowRT cache，不生成用户项目产物。cache root 默认
  `~/.cache/flowrt`，可用 `FLOWRT_CACHE_DIR` 覆盖。
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
- `launch` 运行 FlowRT 管理的 Rust supervisor。C++ only contract 会生成
  supervisor-only Rust crate，launch 先构建 CMake app 再运行 supervisor。
- `inproc` 是单进程 backend；launch 和单独 process run 必须拒绝 inproc dataflow
  跨 RSDL process group。
- `build` 默认 release。Rust app、generated supervisor、C++ app 和 ROS2 bridge
  adapter 都复制到 `flowrt/build/bin/<mode>/`；`flowrt/build/build-info.json` 记录
  build mode、deps target 目录和 executable 相对路径。缺少匹配 deps ready marker 时，
  `build` 会 fail-fast，提示先运行 `flowrt deps`。

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
`ReconnectPolicy` 和 `BackendHealthSnapshot` 的稳定 POD 形状；Rust/C++ runtime 内部
仍使用各自语言的高层类型，并通过转换函数或 C header 对齐。`iox2` 和 `zenoh`
endpoint 已接入自动恢复：本地 transport 资源丢失或操作失败会重建本地
publisher/subscriber/session；codec/schema 错误不得触发重连。

## ROS2 Bridge 状态

FlowRT 与 ROS2 bridge 的唯一通信桥梁固定为 `zenoh`。RSDL 使用 `[[bridge.ros2]]`
声明 bridge，当前只支持：

- `direction = "flowrt_to_ros2"`。
- `ros2_type = "std_msgs/msg/String"`。
- `field` 指向 FlowRT message 中的 `string` 字段。

normalization 生成的 bridge backend 必须是 `zenoh`；validator 必须拒绝 source target
不支持 `zenoh` 的 contract，不得添加 DDS fallback。codegen 会在 source process 中
生成 zenoh bridge tap，并额外生成 FlowRT 管理的 C++ ROS2 adapter process
`ros2_bridge`。launch manifest 中该 process 的 `runtime_kind` 为 `ros2_bridge`。

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
格式化/测试/clippy、C++ runtime、v0.5.0 / v0.6.0 runtime focused smoke、打包、
C++ zenoh runtime、demo smoke、ROS2 bridge smoke 和 release。Linux job 默认运行在
官方 ROS2 Jazzy base 容器上；ROS2 bridge smoke 覆盖 Jazzy 和 Lyrical 两个发行版，
并安装对应的 `rmw_zenoh_cpp`。

CI 的架构相关 job 使用 `amd64` / `arm64` 双矩阵：Rust fmt/test/clippy、C++ runtime、
v0.5.0 / v0.6.0 runtime focused smoke、Debian package、C++ zenoh runtime、demo
smoke、ROS2 Jazzy bridge 和 ROS2 Lyrical bridge 都在对应架构 runner 上执行。package
job 分别上传 `flowrt-linux-amd64-deb` 和 `flowrt-linux-arm64-deb` artifact。demo
smoke 先安装同架构 deb，再用安装后的 `flowrt deps` 预热依赖，然后用 `flowrt ...`
跑示例。推送 `v*` tag 且全部 gate 成功后，release job 会下载两种架构 deb artifact，从 `CHANGELOG.md` 对应版本段
抽取 release notes，并创建 GitHub Release 上传 `flowrt_*_amd64.deb`、
`flowrt_*_arm64.deb` 与统一 `SHA256SUMS`。tag 版本必须匹配根 `Cargo.toml` 的
workspace version。

`v0.5.0 Runtime Smoke` focused gate 使用 `-j1` 聚焦 supervisor readiness/resource、
远程参数控制面、status/hz 健康展示、scheduler health 和 runtime introspection 相关
测试；`v0.6.0 Runtime Smoke` focused gate 聚焦 Operation RSDL/IR/validator/codegen/
runtime/CLI/status、record format、runtime recorder tap 和 CLI MCAP 写入路径，使
这些新增能力的 CI 失败原因比全量 Rust test 更可定位。发布前应运行
`scripts/check-release-readiness.sh <version>`；脚本会汇总版本来源、CHANGELOG 段、
release notes 抽取和 v0.5.0 / v0.6.0 focused gate 覆盖状态。`v0.6.1` 发布由推送
`v0.6.1` tag 触发 GitHub Release。

workflow 暂不做 cache。多架构 CI 的首要目标是保证发布包能在 amd64 与 arm64 原生
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
