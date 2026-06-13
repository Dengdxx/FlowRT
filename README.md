# FlowRT

FlowRT 是一个数据流编译型机器人运行时。应用开发者用 `.rsdl` 声明系统结构：消息、组件端口、实例、任务、数据流连接、部署目标和通信 backend；FlowRT 把这些声明编译成 Contract IR，校验后生成 C++/Rust 的薄 runtime shell、消息类型、启动配置和构建文件。

FlowRT 的边界：

```text
RSDL controls system structure.
Runtime controls execution.
User code controls algorithms.
```

也就是：你写业务算法，FlowRT 管系统结构、构建、调度、通信、生命周期和运行态观测。

## 面向谁

本文主要面向 **基于 FlowRT 开发机器人应用的人**。你通常只需要关心：

- 在 `rsdl/` 中声明系统契约。
- 在 `src/` 中实现组件算法。
- 用 `flowrt deps` 补全底层依赖缓存，用 `flowrt build` 生成并构建应用。
- 用 `flowrt run` 或 `flowrt launch` 运行应用。
- 用 `flowrt status`、`flowrt echo`、`flowrt hz`、`flowrt params`、`flowrt op` 和
  `flowrt record` 观察运行状态。

FlowRT 仓库开发者的验证、发布和维护规则见 [开发维护](docs/development.md)。

## 安装

推荐使用 GitHub Release 中的 Debian 包：

```bash
version=v0.10.3  # 替换为要安装的 release tag
arch="$(dpkg --print-architecture)"  # amd64 或 arm64，以 release 页面实际资产为准
curl -LO "https://github.com/Dengdxx/FlowRT/releases/download/${version}/flowrt_${version#v}_${arch}.deb"
curl -LO "https://github.com/Dengdxx/FlowRT/releases/download/${version}/SHA256SUMS"
sha256sum --ignore-missing -c SHA256SUMS
sudo dpkg -i "flowrt_${version#v}_${arch}.deb"
flowrt --version
```

安装包提供：

- `/usr/bin/flowrt`
- Rust runtime crate
- C++ runtime header（包含 C ABI 基础头）
- CMake package
- 私有 Rust crate vendor
- `iceoryx2-cxx 0.9.1` C++ SDK
- `zenoh-c` / `zenoh-cpp 1.9.0` C++ SDK
- 基础文档和 changelog

除 `/usr/bin/flowrt` 入口外，版本锁定的 runtime、C++ backend SDK 和 Rust vendor 都安装在 `/opt/flowrt/<version>` 私有前缀下。`flowrt deps` 会把 FlowRT 底层 Rust 依赖预热到全局共享 cache；生成的 Rust/C++ 应用会优先使用同一安装包内的依赖。用户不需要手动安装 iox2 或 zenoh C++ SDK，也不需要在生成项目构建时联网拉取 backend 依赖。

安装后，应用项目不需要克隆 FlowRT 仓库；用户项目只保留自己的 RSDL、业务代码和可重建的 `flowrt/` 生成目录。

## 核心概念

| 概念 | 含义 |
| --- | --- |
| RSDL | FlowRT 的源语言，声明系统结构，不写业务算法。 |
| Contract IR | RSDL 归一化、校验后的语义合同，是 codegen 和 runtime 的共同输入。 |
| message type | 数据 schema，例如 IMU、控制指令、检测结果。 |
| component | 可复用组件类型，声明输入、输出、参数和语言绑定。 |
| instance | graph 中的组件实例，绑定 component、参数、target 和 process。 |
| task | instance 的执行单元，描述 trigger、输入、输出、周期和 deadline。 |
| channel route | 从一个输出端口到一个输入端口的 typed dataflow 边。 |
| service route | 从 service client 到 service server 的 typed request/response 边。 |
| operation route | 从 operation client 到 operation server 的 typed long-running command 边。 |
| profile | 一套构建/部署选择，例如默认 backend、channel policy。 |
| Island Mode | 用 typed boundary endpoint 补齐外部 IO 的可拆测试脚手架；生产 graph 默认仍是 strict。 |
| boundary endpoint | 绑定真实 component port 的外部输入或待对比输出，可用 `flowrt pub`、`echo`、`record` 做单功能单位 IO 测试。 |
| target | 部署目标能力，例如 runtime 语言和可用 backend。 |
| backend | FlowRT 管理的通信实现，例如 `inproc`、`iox2`、`zenoh`。 |
| bridge | FlowRT 管理的外部系统适配进程；用户组件仍只读写 FlowRT message。 |
| io_boundary component | 进程内自研 I/O 和副作用边界，例如串口、SHM、UDP 或推理 SDK 接入；FlowRT 管生命周期、资源状态和观测。 |
| external component | 由外部 package/executable 提供的 typed 组件，FlowRT 管生命周期、通信绑定和观测，不生成其内部算法代码。 |
| runtime shell | FlowRT 生成的胶水代码，负责调度、通信、生命周期和观测。 |

核心模型：

```text
component -> instance -> task -> channel route / service route / operation route
```

FlowRT 的核心对象是可编译、可校验、可重新生成的数据流系统契约。

## Island Mode

普通 `strict` graph 要求每个 active input 都有一条明确的 dataflow bind。开发单个功能
单位、或把旧系统逐步迁到 FlowRT 时，可以临时使用 Island Mode：profile 显式声明
`mode = "island"`，未接入的外部输入写成 typed `boundary.input`，需要对比的输出写成
typed `boundary.output`。

```toml
[profile.default]
mode = "island"
backend = "inproc"

[[boundary.input]]
name = "sample_in"
port = "processor.sample"
type = "Sample"

[[boundary.output]]
name = "result_out"
port = "processor.result"
type = "ProcessedSample"
```

运行时可以用 `flowrt pub sample_in --json ...` 注入 boundary input，也可以把外部
样本转换成 JSONL 后用 `flowrt pub sample_in --file samples.jsonl --freq <hz>` 按
wall-clock 节奏喂入；输出用 `flowrt echo result_out` 或
`flowrt record --channel result_out` 捕获。boundary endpoint 不是 backend，也不是
ROS2 topic；FlowRT 不做 ROS2 drop-in，也不直接读取 rosbag。完成测试后应删除
boundary endpoint，改回普通 `[[bind.dataflow]]`，并把 profile 切回 `strict`。

## 应用目录

推荐的应用目录：

```text
my_robot/
  rsdl/
    robot.rsdl
  src/
    cpp/
      components.cpp
    rust/
      mod.rs
  external/
    driver_package/
      flowrt-external.toml
      bin/
  flowrt/              # FlowRT 生成目录，可删除、可重建，不放业务代码
```

约定：

- `rsdl/` 放系统契约。
- `src/` 放用户业务算法。
- `flowrt/` 是 FlowRT 管理产物，不手写、不承载业务逻辑。
- `external/` 可放本项目随包携带的 external package；系统级 external package 也可安装到 `/opt/flowrt/external/<package>`。

`flowrt/` 删除后可以通过 `flowrt build` 重新生成。构建出的用户项目二进制位于当前项目自己的 `flowrt/build/bin/release/`；全局 cache 只用于复用底层依赖编译产物，不是应用部署事实源。

## 最小 RSDL

RSDL v0.1 使用 TOML 表面语法。下面是一个 C++ counter 示例：

```toml
[package]
name = "counter_demo"
version = "0.2.0"
rsdl_version = "0.1"

[type.Count]
value = "u32"

[component.counter_source]
language = "cpp"
output = ["count:Count"]

[component.counter_sink]
language = "cpp"
input = ["count:Count"]

[instance.counter_source]
component = "counter_source"
process = "control"
target = "linux"

[instance.counter_source.task]
trigger = "periodic"
period_ms = 10
output = ["count"]

[instance.counter_sink]
component = "counter_sink"
process = "control"
target = "linux"

[instance.counter_sink.task]
trigger = "on_message"
input = ["count"]

[[bind.dataflow]]
from = "counter_source.count"
to = "counter_sink.count"
channel = "latest"

[profile.default]
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
max_age_ms = 50

[target.linux]
platform = "linux-amd64"
runtime = ["cpp"]
backends = ["inproc"]
```

这份 RSDL 表达：

- `Count` 是消息类型。
- `counter_source` 和 `counter_sink` 是组件类型。
- 两个 `instance` 放在同一个 `control` process。
- `counter_source` 每 10ms 发布一次 `count`。
- `counter_sink` 在收到 `count` 时被调度。
- `bind.dataflow` 把 source 输出接到 sink 输入。
- `profile.default` 选择 `inproc` backend。

## 任务

`task` 是 instance 内的执行单元。单 task 可以继续使用 `[instance.<name>.task]`，FlowRT 会把它命名为 `main`。

一个 instance 也可以声明多个 task：

```toml
[[instance.sensor.task]]
name = "fast_loop"
trigger = "periodic"
period_ms = 5
output = ["imu"]

[[instance.sensor.task]]
name = "health_loop"
trigger = "periodic"
period_ms = 1000
output = ["health"]
```

多 task 共享同一个 component 实例和同一份 typed params。每个 task 只读取自己声明的 `input`，只发布自己声明的 `output`；生成 shell 会按 task 分别调度同一个用户组件接口。task name 必须在同一个 instance 内唯一，并使用 `snake_case`。

## 构建和运行

检查契约：

```bash
flowrt check rsdl/robot.rsdl
```

生成并构建应用：

```bash
flowrt deps rsdl/robot.rsdl
flowrt build --launcher rsdl/robot.rsdl
```

`flowrt deps` 负责补全并预热 FlowRT 底层依赖缓存。项目只使用单一 backend 时通常不需要显式指定；要一次性补全所有内置 backend，可以运行 `flowrt deps --backend all`。`flowrt build` 默认使用 release 构建，只编译用户项目和生成 shell，并把二进制写入 `flowrt/build/bin/release/`。

运行单个 process group：

```bash
flowrt run rsdl/robot.rsdl --process control
```

由 generated supervisor 启动全部 process group：

```bash
flowrt launch rsdl/robot.rsdl
```

`run` 和 `launch` 只读取已生成、已构建产物；修改 RSDL、profile、生成模板或用户代码后，需要重新执行 `flowrt build`。修改 backend 组合、FlowRT 版本或清理全局 cache 后，需要先重新执行 `flowrt deps`。

## External Package

external component 用于把独立 executable 纳入 FlowRT 的 typed graph。RSDL 中 component 只声明端口和 `language = "external"`，graph 级 `[[external_process]]` 指向 package 和 executable：

```toml
[component.sensor]
language = "external"
kind = "external"
output = ["value:u32"]

[instance.sensor]
component = "sensor"
process = "sensor_proc"
target = "edge"

[[external_process]]
process = "sensor_proc"
package = "fake_sensor_driver"
executable = "bin/driver"
args = ["--mode", "smoke"]
health = "process_started"
required_backends = ["zenoh"]
```

external package 根目录包含 `flowrt-external.toml`：

```toml
[package]
name = "fake_sensor_driver"
version = "0.1.0"
flowrt_version = "0.7"
license = "MIT"

[[executable]]
name = "driver"
path = "bin/driver"
platforms = ["linux-amd64", "linux-arm64"]
backends = ["zenoh"]
health = "process_started"
```

常用命令：

```bash
flowrt external check external/fake_sensor_driver
flowrt deps rsdl/robot.rsdl
flowrt build --launcher rsdl/robot.rsdl
flowrt launch --run-steps 2 rsdl/robot.rsdl
```

supervisor 按 `FLOWRT_EXTERNAL_PATH`、`/opt/flowrt/external/<package>`、项目 `external/<package>` 查找 external package。external 进程通过 `FLOWRT_PROCESS`、`FLOWRT_BACKEND`、`FLOWRT_EXTERNAL_PACKAGE`、`FLOWRT_EXTERNAL_PACKAGE_ROOT`、`FLOWRT_WORKSPACE_ROOT` 和 `FLOWRT_RUN_STEPS` 等环境变量获取上下文；FlowRT 不把生成 app 的内部 `--process` 参数强加给 external executable。

## Bundle / Deploy

`flowrt bundle` 把已构建项目整理为离线运行目录，不复制 FlowRT 源码仓库，也不把 Cargo target cache 当作部署事实源：

```bash
flowrt bundle rsdl/robot.rsdl --output dist/robot-bundle
```

bundle 内容包括：

- `bundle.toml`
- `bin/` 下的本项目二进制
- `flowrt/contract`、`flowrt/launch`、`flowrt/selfdesc` 和 `flowrt/build/build-info.json`
- `external/` 下随项目携带的 external package 副本

目标机器需要安装同版本 FlowRT deb。baseline deploy 使用 SSH/SCP 上传 bundle 并做远端 FlowRT 版本检查：

```bash
flowrt deploy dist/robot-bundle --host user@host --target edge --remote-dir /opt/my_robot
```

只查看计划可加 `--dry-run`。

## 用户组件

FlowRT 生成 component interface，用户只实现算法。

C++ 组件通过生成接口和 `flowrt_user::build_app()` 注入：

```cpp
namespace flowrt_user {

flowrt_app::App build_app() {
    flowrt_app::App app;
    app.set_counter_source(std::make_unique<CounterSource>());
    app.set_counter_sink(std::make_unique<CounterSink>());
    return app;
}

}  // namespace flowrt_user
```

Rust 组件通过生成 trait 和用户模块接入。

业务代码只依赖 FlowRT runtime API，不直接依赖 backend 的 publisher/subscriber API。通信、调度、生命周期和观测由生成的 runtime shell 负责。

## 跨语言 ABI 边界

FlowRT 以 C ABI 作为后续 C、Python 和更多语言接入的共同边界。当前安装包中的 C++
runtime header 已包含 `flowrt/abi.h`，Rust runtime 也提供对应的 `repr(C)` 镜像类型，
用于稳定 status、backend health、重连策略和 borrowed string/bytes view 等基础形状。

这只是 ABI 边界准备，不表示已经提供 Python binding 或 C runtime wrapper。用户组件
当前仍以 C++ 和 Rust 生成接口为主。

## 参数

RSDL 可以声明 component 参数和 instance 覆盖值。生成代码会把参数变成 typed params，用户组件在 tick 边界读取参数快照。

运行时可用 CLI 查看或提交参数：

```bash
flowrt params list
flowrt params get controller.kp
flowrt params set controller.kp 2.0
```

`startup` 参数只在启动时生效；支持热更新的参数会先进入 pending 状态，再由生成 shell 在安全边界提交给用户组件。

### 远程参数

跨机器参数控制通过 zenoh control-plane 实现。加上 `--remote` 即可发现远端 runtime 并操作参数：

```bash
flowrt params list --image path/to/selfdesc.json --remote
flowrt params set --image path/to/selfdesc.json controller.kp 2.5 --remote
```

CLI 按 `flowrt/params/{package}/{selfdesc_hash}/{pid}` 格式的 key expression 查询远端参数端点，筛选与 `--image` 自描述 hash 匹配的 runtime。多个匹配时用 `--runtime <key_expr>` 显式选择。`--socket` 和 `--remote` 互斥。

远程参数复用本机 Unix socket 路径相同的 schema 校验、structured error 和 pending/apply 语义。

## 消息

v0.1 的 native ABI 基线是 fixed-size plain data：

- integers
- floats
- bool
- fixed arrays
- nested structs

当前也支持无界 variable frame：

- `bytes`
- `string`
- `sequence<T>`

`inproc` 和 `zenoh` 可直接传递 canonical frame。`iox2` 只承载 fixed-size plain data；当 profile 默认选择 `iox2` 且某条 route 使用 variable frame 时，FlowRT 会把该 route 自动降级到支持变长消息的 backend（当前为 `zenoh`），fixed-size route 仍继续走 `iox2`。

图像、mask 和其他大 payload 不应作为普通 `bytes` channel 在本机高频路径上传输。
推荐用标准 FrameDescriptor：channel 只传固定 64 字节 descriptor，字段包含
`resource_id_hash`、`slot`、`generation`、`size_bytes`、时间戳、宽高、stride、
format/encoding id 和 flags；真实 payload 的 attach/acquire/release 由 `io_boundary`
或 external package 的 side-channel 管理。这样本机 route 可以继续发挥 `iox2` 固定
slot 的低开销优势，`flowrt echo` / `status` / `record` 也默认只观测 descriptor。

## Channel Policy

基础 channel policy：

```text
latest(depth = 1)
fifo(depth = N)
```

overflow policy：

```text
drop_oldest
drop_newest
error
block
```

stale data policy：

```text
max_age_ms = N
stale_policy = "warn" | "drop" | "hold_last" | "error"
```

overflow 表示队列满，stale 表示数据过期。两者是不同问题。

## Service

Service 表达 request/response 语义，和 channel（dataflow push）是不同模型。

**Service 与 channel 的区别：** channel 是 publish/subscribe，生产者写入后不等
消费者处理；Service 是 call/response，client 发起请求后阻塞或轮询等待 server 返回。
channel 适合高频数据流（IMU、odom），Service 适合低频请求（路径规划、参数查询）。

**Service 与参数热更新的区别：** 参数是 runtime control-plane 的配置值，通过
`flowrt params set` 提交、在 tick 边界生效；Service 是 graph 业务逻辑的一部分，
由 RSDL 声明、codegen 生成 typed API、用户组件实现 handler。

component 声明 service client/server 端口，graph 用 `[[bind.service]]` 绑定双方：

```toml
[component.client]
language = "rust"
service_client = ["plan:PlanRequest->PlanResponse"]

[component.server]
language = "rust"
service_server = ["plan:PlanRequest->PlanResponse"]

[[bind.service]]
client = "client.plan"
server = "server.plan"
backend = "inproc"
timeout_ms = 1000
queue_depth = 16
overflow = "busy"
```

codegen 为 client 生成 `ServiceClient_{instance}_{port}` typed handle，暴露同步
`call()` 和非阻塞 `start_call()`；为 server 生成 `on_{port}_request` handler
方法，用户实现具体的 request -> response 转换。Service 通过 hidden task 集成到
scheduler，request arrival 直接唤醒 server 处理。

**Service policy：** `backend`（native generated Service 当前支持 `inproc`；`zenoh`
运行时已实现，但 native Service / Operation codegen 尚未接线，因此会 fail-fast，
不生成 placeholder）、`timeout_ms`（默认 5000）、`queue_depth`（默认 32）、
`overflow`（`busy` 或 `error`，默认 `busy`）、`max_in_flight`（默认 64）。auto
backend resolver 默认同进程选择 `inproc`，跨进程或 external endpoint 选择 `zenoh`。

**错误语义：** `Timeout`（超时）、`Busy`（队列满）、`Unavailable`（server 未注册）、
`WouldDeadlock`（同 lane 阻塞调用）、`HandlerError`（用户业务错误）。

**Service 与 Operation 的边界：** Service 是同步 request/response；Operation 是
typed long-running command，底层编译期 lower 成 Service + Channel，用户只看
Operation，调试时才展开底层拓扑。

查看 service 拓扑和运行态健康：

```bash
flowrt list path/to/selfdesc.json
flowrt status
```

## Operation

Operation 表达长耗时命令，例如导航、建图任务或一次可取消的规划执行。它和 Service
的区别是：Service 返回一个同步 response；Operation 有 goal、feedback、result、
状态机、取消入口和运行态可观测 handle。

component 声明 operation client/server 端口，graph 用 `[[bind.operation]]` 绑定双方：

```toml
[component.controller.operation_client.plan]
goal = "PlanGoal"
feedback = "PlanFeedback"
result = "PlanResult"

[component.navigator.operation_server.plan]
goal = "PlanGoal"
feedback = "PlanFeedback"
result = "PlanResult"

[[bind.operation]]
client = "controller.plan"
server = "navigator.plan"
backend = "inproc"
timeout_ms = 5000
queue_depth = 4
max_in_flight = 1
```

codegen 为 client 生成 `OperationClient_{instance}_{port}` typed handle，为 server
生成 `on_{port}_operation(goal, cancel, progress)` handler。`inproc` Operation 会被编译期
lower 成内部 start/cancel/status service 与 feedback/result endpoint；`flowrt list`
和 `flowrt op list` 默认展示 Operation 主语义，需要调试时再查看 lowering refs。

当前 generated Operation runtime 支持单个运行中的 invocation：`concurrency =
"reject"`、`preempt = "reject"`、`max_in_flight = 1`。第二个 start 会返回 `Busy`，
直到当前 invocation 进入终态。`queue`、`cancel_running` 和多 in-flight 策略已保留在
IR 长期模型中，但在 runtime 完整实现前由 validator 拒绝。

查看 Operation 拓扑和运行态状态：

```bash
flowrt build --launcher examples/operation_demo/rsdl/robot.rsdl
flowrt op list --image examples/operation_demo/flowrt/selfdesc/selfdesc.json
flowrt status
```

## Backend

v0.1 支持：

| Backend | 用途 |
| --- | --- |
| `inproc` | 单进程、本机 demo、测试、低依赖路径。 |
| `iox2` | 同机多进程、固定大小 slot、高性能 IPC。 |
| `zenoh` | 跨进程/跨主机 copy transport，适合变长序列化。 |

profile 提供默认 backend，单条 route 可显式覆盖或使用 `auto`：

```toml
[profile.default]
backend = "zenoh"

[[bind.dataflow]]
from = "source.packet"
to = "sink.packet"
channel = "latest"
backend = "auto"

[target.linux]
runtime = ["rust", "cpp"]
backends = ["zenoh"]
```

省略 `backend` 等价于 `auto`。`auto` 默认跟随 profile backend；如果 profile backend 是 `iox2` 且该 route 使用 variable frame，FlowRT 会自动选择 `zenoh`。`backend` 绑定在单条 `[[bind.dataflow]]` route 上；同一条 route 只能声明一次，跨 RSDL import 后仍归一化为同一个 Contract IR，不通过重复声明做隐式合并。message type 只描述数据 schema，不直接暴露 backend API；实际 transport 由 FlowRT 根据 RSDL 契约、profile 和 route 生成。

## ROS2 Bridge

FlowRT 与 ROS2 的桥接唯一走 `zenoh`。FlowRT 不生成 DDS fallback，也不把 ROS2 publisher/subscriber API 暴露给用户组件；bridge 是 FlowRT 管理的 C++ adapter process。ROS2 侧必须使用 `rmw_zenoh_cpp`，adapter 启动时会设置并校验 `RMW_IMPLEMENTATION=rmw_zenoh_cpp`。普通 FlowRT `zenoh` backend 仍使用 FlowRT 包内锁定的私有 zenoh SDK；ROS2 bridge adapter 进程使用 ROS2 安装中的 `zenoh_cpp_vendor`，以匹配 `rmw_zenoh_cpp` 的同进程 ABI。

当前桥接切片支持 FlowRT 输出端口到 ROS2 topic：

```toml
[type.TextFrame]
data = "string"

[component.source]
language = "rust"
output = ["text:TextFrame"]

[[bridge.ros2]]
flowrt = "source.text"
ros2_topic = "/flowrt/text"
ros2_type = "std_msgs/msg/String"
direction = "flowrt_to_ros2"
field = "data"

[profile.default]
backend = "zenoh"

[target.linux]
runtime = ["rust"]
backends = ["zenoh"]
```

约束：

- `direction` 当前只支持 `flowrt_to_ros2`。
- `ros2_type` 当前只支持 `std_msgs/msg/String`。
- `field` 必须指向 FlowRT message 中的 `string` 字段，省略时默认为 `data`。
- bridge route 固定使用 `zenoh`；source target 必须声明支持 `zenoh`。
- 构建 bridge 需要 ROS2 Jazzy 或之后版本的 C++ 开发包；运行 bridge 需要安装 `rmw_zenoh_cpp`。当前 CI 会在 Jazzy 和 Lyrical 上强制运行 bridge smoke。构建前应 source 对应 ROS2 环境，生成 CMake 会把 `AMENT_PREFIX_PATH` 映射进 `CMAKE_PREFIX_PATH`，以便 plain CMake 找到 `rclcpp`、`std_msgs` 和 `rmw_zenoh_cpp`。
- 如果 `ros2 topic echo` 看不到刚启动的 topic，先执行 `ros2 daemon stop` 后重试，避免 ROS2 daemon 的旧缓存干扰。

## 运行态观测

生成应用会启动 introspection socket。部署后，即使没有 RSDL 源文件，也可以用 CLI 自查。

查看静态拓扑：

```bash
flowrt list path/to/generated/app
flowrt nodes path/to/generated/app
```

查看运行状态：

```bash
flowrt status
```

查看发布频率：

```bash
flowrt hz channel_name --window-ms 1000
```

查看 channel 最新值：

```bash
flowrt echo channel_name
flowrt echo channel_name --follow
```

`flowrt echo` 的数据面 probe 按需启用。没有 observer 时，发布路径不会编码 payload、不会拷贝 payload、不会写 socket。
标准 FrameDescriptor 会按结构化字段展示，例如：

```text
descriptor=frame frame_descriptor={resource_id_hash=... slot=... generation=... size_bytes=... width=... height=... stride_bytes=... format_id=... encoding_id=... flags=...}
```

录制 FlowRT 事件到 MCAP：

```bash
flowrt record --output run.mcap --duration 10s --all
flowrt record --output op.mcap --operation controller.plan --force
```

`flowrt record` 通过 live runtime socket 按需启用 recorder tap。没有录制者时，发布热路径
不会持续复制 payload；命令结束时会输出 event、drop 和写入字节统计。
标准 FrameDescriptor 默认按 descriptor-only 记录，摘要中会出现
`descriptor_payload=descriptor_only`；真实图像 payload 录制需要后续显式建模，不能由
channel sample 隐式复制。

### 调度健康

FlowRT 支持 component/task 级 `concurrency = "exclusive" | "parallel"`。默认
`exclusive` 保护同一 instance 用户对象串行访问；显式 `parallel` 时，不同 lane 的 ready
task 可以跨 worker 并发执行。同一 lane 仍是串行队列，lane 不是线程。

generated shell 使用 two-phase output commit：worker 执行用户 task，scheduler 只在
`Ok` 后按 deterministic ready order 提交 output。`Retry`、`Error`、panic 或 C++ exception
不会发布本次 output。`iox2` route 的 transport commit 留在 scheduler/local owner 线程，
用户 task 本身仍可并发运行。

`flowrt status` 会展示 task 级和 lane 级调度健康指标：

```text
task_health=fast_loop lane=sensor_lane deadline_missed=0 stale_input=2 backpressure=0 overflow=0 fairness_violations=0 runs=1000 successes=998 consecutive_failures=0 last_run_ms=1717800000000 last_success_ms=1717800000000 socket=...
lane_health=sensor_lane queue_depth=0 dispatched_count=1000 fairness_violations=0 socket=...
```

task 健康字段包括：`deadline_missed`（截止时间超限次数）、`stale_input`（输入过期次数）、`backpressure`（背压事件次数）、`overflow`（溢出事件次数）、`fairness_violations`（lane 饥饿公平性违规次数）、`runs`/`successes`/`consecutive_failures`（运行计数）和 `last_run_ms`/`last_success_ms`（最近运行时间戳）。

lane 健康字段包括：`queue_depth`（当前排队任务数）、`dispatched_count`（累计调度次数）和 `fairness_violations`（lane 间饥饿违规次数）。

这些指标来自 runtime 内置的调度健康策略：deadline miss 会阻止 late output 发布、stale input 检测记录到健康计数器、backpressure 和 overflow 事件进入 health counters、lane 饥饿公平性检测通过阈值触发违规记录。Rust 和 C++ 生成 shell 行为一致。

## Supervisor

`flowrt launch` 会运行 generated supervisor。Supervisor 会：

- 读取 `flowrt/launch/launch.json`。
- 按 process 依赖顺序启动 Rust 或 C++ generated app。
- 启动自己的 introspection socket。
- 轮询子进程 PID socket。
- 把子进程 `starting`、`running`、`stale`、`restarting`、`exited`、`failed` 展示给 `flowrt status`。
- 对异常退出的子进程按内置 `on-failure` policy 最多重启 3 次。

正常退出不会重启。

### Readiness 条件启动

Supervisor 支持三种 readiness gate，决定子进程何时被视为"就绪"：

| Gate | 含义 |
| --- | --- |
| `process_started` | 进程已 spawn 即视为就绪（默认）。 |
| `runtime_ready` | 等待 introspection socket 握手，确认 runtime 已启动。 |
| `service_ready` | 在 `runtime_ready` 基础上，额外检查所有 service endpoint 就绪。 |

`startup_delay_ms` 可以在进程之间加入错峰延迟，避免多个进程同时竞争资源。

```toml
[[process]]
name = "sensors"
readiness = "runtime_ready"
startup_delay_ms = 200

[[process]]
name = "control"
depends_on = ["sensors"]
readiness = "service_ready"
```

### 进程资源提示

RSDL `[[process]]` 支持 Linux 进程级资源提示：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `cpu_affinity` | `[u32]` | 绑定到指定 CPU 核心列表。 |
| `nice` | `i32` | 进程 nice 值（-20..=19）。 |
| `rt_policy` | string | 可选 RT 调度策略：`fifo` 或 `round_robin`。 |
| `rt_priority` | `u32` | RT 优先级（1..=99），需配合 `rt_policy`。 |
| `env` | table | 注入子进程的环境变量键值对。 |

```toml
[[process]]
name = "control"
depends_on = ["sensors"]
cpu_affinity = [0, 1]
nice = -10
rt_policy = "fifo"
rt_priority = 50
env = { FLOWRT_LOG_LEVEL = "info", MY_ROBOT_MODE = "production" }
```

`flowrt status` 会展示每个进程的 `desired` 和 `applied` 资源状态；权限不足时会结构化报错而非静默忽略。

## 示例

| 示例 | Runtime | Backend | 推荐命令 | 用途 |
| --- | --- | --- | --- | --- |
| `examples/import_demo` | Rust | `inproc` | `flowrt build --launcher examples/import_demo/rsdl/robot.rsdl` | RSDL imports、Rust codegen、inproc run 和 launch。 |
| `examples/workspace_demo` | Rust | `inproc` | `flowrt build --launcher examples/workspace_demo/rsdl/robot.rsdl` | workspace / module / composition、跨模块引用。 |
| `examples/cpp_counter_demo` | C++ | `inproc` | `flowrt build --launcher examples/cpp_counter_demo/rsdl/robot.rsdl` | C++ only CMake app 路径。 |
| `examples/imu_demo` | Rust + C++ | `inproc` build smoke | `flowrt build examples/imu_demo/rsdl/robot.rsdl` | mixed contract 的接口和生成物边界。 |
| `examples/imu_demo_iox2` | Rust + C++ | `iox2` | `flowrt check examples/imu_demo_iox2/rsdl/robot.rsdl` | mixed iox2 分进程、Rust/C++ 参数接口。 |
| `examples/profile_switch_demo` | Rust | `inproc` / `iox2` | `flowrt build --profile iox2 examples/profile_switch_demo/rsdl/robot.rsdl` | profile 驱动 backend 切换。 |
| `examples/mixed_iox2_demo` | Rust + C++ | `iox2` | `flowrt check examples/mixed_iox2_demo/rsdl/robot.rsdl` | Rust source 与 C++ sink 的 iox2 分进程 contract。 |
| `examples/mixed_zenoh_demo` | Rust + C++ | `zenoh` | `flowrt build --launcher examples/mixed_zenoh_demo/rsdl/robot.rsdl` | 无界 variable frame、zenoh 跨主机 transport 和 mixed launch。 |
| `examples/ros2_bridge_demo` | Rust + ROS2 adapter | `zenoh` | `flowrt build --launcher examples/ros2_bridge_demo/rsdl/robot.rsdl` | FlowRT string 输出经 zenoh bridge 发布到 ROS2 topic。 |
| `examples/island_demo` | Rust | `inproc` | `flowrt build --launcher examples/island_demo/rsdl/robot.rsdl` | Island Mode 下通过 boundary input/output 做单功能单位 IO 测试。 |
| `examples/variable_frame_island_demo` | Rust | `inproc` | `scripts/test-v091-variable-frame-island-demo.sh` | 通过 JSONL 注入 `sequence<f32>` canonical frame 并观察 fixed summary。 |
| `examples/service_demo` | Rust | `inproc` | `flowrt build examples/service_demo/service_demo.rsdl` | Service request/response、typed API、inproc call、service policy 和健康观测。 |
| `examples/operation_demo` | Rust | `inproc` | `flowrt build --launcher examples/operation_demo/rsdl/robot.rsdl` | Operation client/server typed API、自描述和 `flowrt op list`。 |
| `examples/frame_descriptor_demo` | Rust | `iox2` | `flowrt build --launcher examples/frame_descriptor_demo/rsdl/robot.rsdl` | I/O boundary 只发布固定 FrameDescriptor，真实 payload 由 side-channel 管理。 |

完整说明见 [示例矩阵](docs/examples.md)。

## 文档

- [文档索引](docs/README.md)
- [快速开始](docs/getting-started.md)
- [CLI 参考](docs/cli.md)
- [示例矩阵](docs/examples.md)
- [开发维护](docs/development.md)

## 许可证

FlowRT 使用 [MIT License](LICENSE)。
