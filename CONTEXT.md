# CONTEXT.md

本文档记录 FlowRT 仓库的当前上下文，供 coding agent 和维护者快速了解现状。它会
随版本演进更新；长期架构原则、语义约定、文档边界和提交纪律维护在 `AGENTS.md`。

## 当前版本背景

当前 workspace 版本为 `0.4.0`。`v0.4.0` 已发布，核心主题是 Service runtime：
生成 Rust/C++ service client/server 用户 API，支持 `inproc` 与 `zenoh` service
transport，补齐 request/response frame、错误语义、self-description、`flowrt list` /
`flowrt status` 观测和示例文档，并完成 amd64 + arm64 release 安装包闭环。

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
| `v0.6.0` | Operation + record/replay + simulated clock + deterministic debug。 |
| `v0.7.0` | external process / driver package 接入边界、ARM64/跨机器部署闭环。 |
| `v0.8.0` | 多目标部署、交叉编译、多架构安装包和发布硬化。 |
| `v0.9.0` | C/Python API、生态互操作扩展。 |
| `v1.0.0` | ABI/schema 稳定、兼容策略、故障注入和性能矩阵。 |

路线边界：

- `v0.4.0` 先把 Service 做成稳定的 request/response runtime 语义。
- `v0.5.0` 优先补齐复杂应用复刻所需的系统编排基础：条件启动、错峰启动、环境变量、
  restart policy、CPU affinity、profile、参数发现/远程控制面，以及高频多 lane
  调度的 deadline、stale、backpressure 和 fairness 语义。同时引入 FlowRT core
  skills 套组，先沉淀开发 FlowRT 本身的 agent 工作流。
- `v0.6.0` 在 Service 之上引入 Operation，并让运行时具备可复现调试能力。
  Operation、record/replay、模拟时钟和确定性调试必须共享同一时间与事件模型。
- `v0.7.0` 定义 external process / driver package 的 typed 接入边界，并补齐 ARM64、
  跨机器部署和离线交付闭环。到 `v0.7.0`，FlowRT 应具备复刻一套复杂车载机器人
  应用的系统能力，但不把硬件 backend 做进 FlowRT 主项目。
- `v0.8.0` 深化多目标部署、交叉编译、多架构安装包和发布硬化，使已生成应用可脱离
  源码仓库交付。
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

FlowRT core skills 套组：入库事实源为 `.agents/skills/`，当前只落地 `frt-core-*`。
`frt-core-*` 面向 FlowRT 仓库维护者，覆盖 RSDL/IR、codegen、runtime、backend、
CLI、CI 和 release。`frt-app-*` 是 1.0.0 之后的保留命名空间；在 schema、runtime、
安装包和兼容策略稳定前，不把 app 开发流程固化成 skill。该套组只追求 FlowRT 范围
内的通用性和抽象性，不写成任意项目都适用的泛用技能。

非目标：Operation、record/replay、Web/HTTP/WebSocket/UI、硬件 backend、新语言
binding、新 backend、ROS2 bridge 扩展。Web、HTTP、WebSocket 和 UI 不进入 FlowRT
core，只能作为应用层或外部工具消费 FlowRT 暴露的 typed introspection / params /
record API。

`v0.6.0` 不复制 ROS2 Action。FlowRT 需要的是一等 Operation 语义：typed
long-running command、generated state machine、explicit policy、observable handle，
底层编译期 lower 成 Service + Channel。用户只看 Operation，调试时才展开底层拓扑。
Operation policy 必须显式声明 concurrency、preempt、cancel、timeout 和
result retention；用户不得手写 start/cancel/result/progress 四套底层协议。

Operation 解决的不是“长时间 service call”，而是机器人系统里常见的可取消、
可抢占、可观测、可恢复的长任务。生成器负责把 Operation lowered 成 request、
progress、feedback、cancel、result 和状态观测通道；用户只实现业务 handler 和策略
钩子，不手写底层协议。这样保留 Action 的实用能力，同时避免让用户维护分散的
start/cancel/result/progress glue。

FlowRT 主项目不做硬件 backend。Linux 和外部 driver package 管硬件；FlowRT 管结构、
执行、通信、观测、external process 生命周期和 typed 接入边界。

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
  `hz` 采样和参数控制面。
- generated supervisor 对 Rust、C++ 和 ROS2 bridge process 的分流启动、health
  汇总、PID socket 轮询、tick stale/exit/restart 状态展示、process 依赖顺序、
  可配置 restart policy 和失败传播 baseline。

## CLI 状态

当前已实现的用户入口：

```bash
flowrt check path/to/robot.rsdl
flowrt prepare path/to/robot.rsdl
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
flowrt params list|get|set path/to/generated-app-or-selfdesc.json ...
flowrt inspect flowrt/contract/contract.ir.json
```

`prepare` / `build` / `run` / `launch` 支持 `--profile <name>`，用于显式选择 profile
并按该 profile 生成或校验产物。省略参数时会先投影到 `default` profile 或首个
profile。RSDL 未声明任何 profile 时，normalization 会插入隐式 `default` profile，
backend 为 `inproc`。

命令职责边界：

- `prepare` 和 `build` 会写 `flowrt/` 输出目录，必须持有 OS advisory lock。
- `.flowrt.lock` 文件可残留，PID 只用于诊断，真实占用状态由锁判断。
- `check`、`inspect`、`run`、`launch`、`list`、`nodes`、`status`、`hz`、`echo`
  和 `params` 不写生成物，不应获取生成物锁。
- `run` / `launch` 省略 `--run-steps` / `--run-ticks` 时长期运行，直到生成应用返回
  `Error` 或收到 SIGINT/SIGTERM。
- `--run-steps` 是推荐外部名称，`--run-ticks` 是兼容别名。
- `flowrt run --process <name>` 运行生成应用中的单个 RSDL process group；mixed
  contract 必须选择一个单语言 process group。
- `launch` 运行 FlowRT 管理的 Rust supervisor。C++ only contract 会生成
  supervisor-only Rust crate，launch 先构建 CMake app 再运行 supervisor。
- `inproc` 是单进程 backend；launch 和单独 process run 必须拒绝 inproc dataflow
  跨 RSDL process group。

## Runtime 和观测状态

`flowrt/launch/launch.json` 当前包含 process group 的 `runtimes`、`runtime_kind`、
`depends_on`、`restart` 和 `failure`，graph instance 的 `runtime`，graph 的
`channels`、`services`，以及 channel 的 backend 元数据。
iox2 channel 暴露 canonical service name，zenoh channel 和 ROS2 bridge 暴露
deterministic `key_expr`。

runtime introspection socket 使用 `$XDG_RUNTIME_DIR/flowrt/<pid>.sock` 或
`/tmp/flowrt.<uid>/<pid>.sock`。socket 路径只用于发现，真实身份来自 handshake。
启动 status server 时不能覆盖仍可连接的 live socket；SIGKILL 后残留且不可连接的
socket 文件可以回收。Rust `IntrospectionState` 会在 mutex poison 后恢复访问。

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
生成 Rust app 会优先使用包内 vendor；生成 CMake 会使用 FlowRT 私有前缀解析 C++
runtime 和 backend SDK。生成 CMake 不应通过 `FetchContent` 联网拉取 backend SDK；
缺失依赖应要求安装 FlowRT 包或显式设置 `FLOWRT_CPP_RUNTIME_DIR` / `CMAKE_PREFIX_PATH`。

## CI 和 Release 状态

当前 `.github/workflows/ci.yml` 将 Linux 验证拆为分层 job，覆盖生成物保护、Rust
格式化/测试/clippy、C++ runtime、打包、C++ zenoh runtime、demo smoke、ROS2 bridge
smoke 和 release。Linux job 默认运行在官方 ROS2 Jazzy base 容器上；ROS2 bridge
smoke 覆盖 Jazzy 和 Lyrical 两个发行版，并安装对应的 `rmw_zenoh_cpp`。

CI 的架构相关 job 使用 `amd64` / `arm64` 双矩阵：Rust fmt/test/clippy、C++ runtime、
Debian package、C++ zenoh runtime、demo smoke、ROS2 Jazzy bridge 和 ROS2 Lyrical
bridge 都在对应架构 runner 上执行。package job 分别上传
`flowrt-linux-amd64-deb` 和 `flowrt-linux-arm64-deb` artifact。demo smoke 先安装同
架构 deb，再用安装后的 `flowrt ...` 跑示例。推送 `v*` tag 且全部 gate 成功后，
release job 会下载两种架构 deb artifact，从 `CHANGELOG.md` 对应版本段抽取 release
notes，并创建 GitHub Release 上传 `flowrt_*_amd64.deb`、`flowrt_*_arm64.deb` 与统一
`SHA256SUMS`。tag 版本必须匹配根 `Cargo.toml` 的 workspace version。

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
