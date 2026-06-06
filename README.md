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
- 用 `flowrt build` 生成并构建应用。
- 用 `flowrt run` 或 `flowrt launch` 运行应用。
- 用 `flowrt status`、`flowrt echo`、`flowrt hz` 和 `flowrt params` 观察运行状态。

FlowRT 仓库开发者的验证、发布和维护规则见 [开发维护](docs/development.md)。

## 安装

推荐使用 GitHub Release 中的 Debian 包：

```bash
curl -LO https://github.com/Dengdxx/FlowRT/releases/download/v0.2.0/flowrt_0.2.0_amd64.deb
curl -LO https://github.com/Dengdxx/FlowRT/releases/download/v0.2.0/SHA256SUMS
sha256sum -c SHA256SUMS
sudo dpkg -i flowrt_0.2.0_amd64.deb
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

除 `/usr/bin/flowrt` 入口外，版本锁定的 runtime、C++ backend SDK 和 Rust vendor 都安装在 `/opt/flowrt/<version>` 私有前缀下。生成的 Rust/C++ 应用会优先使用同一安装包内的依赖；用户不需要手动安装 iox2 或 zenoh C++ SDK，也不需要在生成项目构建时联网拉取 backend 依赖。

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
| profile | 一套构建/部署选择，例如默认 backend、channel policy。 |
| target | 部署目标能力，例如 runtime 语言和可用 backend。 |
| backend | FlowRT 管理的通信实现，例如 `inproc`、`iox2`、`zenoh`。 |
| bridge | FlowRT 管理的外部系统适配进程；用户组件仍只读写 FlowRT message。 |
| runtime shell | FlowRT 生成的胶水代码，负责调度、通信、生命周期和观测。 |

核心模型：

```text
component -> instance -> task -> channel route / service route
```

FlowRT 的核心对象是可编译、可校验、可重新生成的数据流系统契约。

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
  flowrt/              # FlowRT 生成目录，可删除、可重建，不放业务代码
```

约定：

- `rsdl/` 放系统契约。
- `src/` 放用户业务算法。
- `flowrt/` 是 FlowRT 管理产物，不手写、不承载业务逻辑。

`flowrt/` 删除后可以通过 `flowrt build` 重新生成。

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
platform = "linux-x86_64"
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
flowrt build --launcher rsdl/robot.rsdl
```

运行单个 process group：

```bash
flowrt run rsdl/robot.rsdl --process control
```

由 generated supervisor 启动全部 process group：

```bash
flowrt launch rsdl/robot.rsdl
```

`run` 和 `launch` 只读取已生成、已构建产物；修改 RSDL、profile、生成模板或用户代码后，需要重新执行 `flowrt build`。

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

Service 表达 request/response 拓扑。component 可以声明 service client/server 端口，
graph 用 `[[bind.service]]` 绑定双方：

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
```

当前 Service 已进入 RSDL、Contract IR、validator 和 launch manifest；runtime RPC 用户
API 还未生成，因此它目前用于稳定系统结构和后续 ABI 边界。

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

## Supervisor

`flowrt launch` 会运行 generated supervisor。Supervisor 会：

- 读取 `flowrt/launch/launch.json`。
- 按 process group 启动 Rust 或 C++ generated app。
- 启动自己的 introspection socket。
- 轮询子进程 PID socket。
- 把子进程 `starting`、`running`、`stale`、`restarting`、`exited`、`failed` 展示给 `flowrt status`。
- 对异常退出的子进程按内置 `on-failure` policy 最多重启 3 次。

正常退出不会重启。

## 示例

| 示例 | Runtime | Backend | 推荐命令 | 用途 |
| --- | --- | --- | --- | --- |
| `examples/import_demo` | Rust | `inproc` | `flowrt build --launcher examples/import_demo/rsdl/robot.rsdl` | RSDL imports、Rust codegen、inproc run 和 launch。 |
| `examples/cpp_counter_demo` | C++ | `inproc` | `flowrt build --launcher examples/cpp_counter_demo/rsdl/robot.rsdl` | C++ only CMake app 路径。 |
| `examples/imu_demo` | Rust + C++ | `inproc` build smoke | `flowrt build examples/imu_demo/rsdl/robot.rsdl` | mixed contract 的接口和生成物边界。 |
| `examples/profile_switch_demo` | Rust | `inproc` / `iox2` | `flowrt build --profile iox2 examples/profile_switch_demo/rsdl/robot.rsdl` | profile 驱动 backend 切换。 |
| `examples/mixed_iox2_demo` | Rust + C++ | `iox2` | `flowrt check examples/mixed_iox2_demo/rsdl/robot.rsdl` | Rust source 与 C++ sink 的 iox2 分进程 contract。 |
| `examples/mixed_zenoh_demo` | Rust + C++ | `zenoh` | `flowrt build --launcher examples/mixed_zenoh_demo/rsdl/robot.rsdl` | 无界 variable frame、zenoh 跨主机 transport 和 mixed launch。 |
| `examples/ros2_bridge_demo` | Rust + ROS2 adapter | `zenoh` | `flowrt build --launcher examples/ros2_bridge_demo/rsdl/robot.rsdl` | FlowRT string 输出经 zenoh bridge 发布到 ROS2 topic。 |

完整说明见 [示例矩阵](docs/examples.md)。

## 文档

- [文档索引](docs/README.md)
- [快速开始](docs/getting-started.md)
- [CLI 参考](docs/cli.md)
- [示例矩阵](docs/examples.md)
- [开发维护](docs/development.md)

## 许可证

FlowRT 使用 [MIT License](LICENSE)。
