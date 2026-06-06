# 示例矩阵

本仓库的 `examples/` 目录用于验证 RSDL、Contract IR、validator、codegen、runtime 和 CLI 的端到端切片。每个示例都尽量覆盖一个明确边界，不把所有能力塞进同一个 demo。

## 示例列表

| 示例 | Runtime | Backend | 推荐命令 | 用途 |
| --- | --- | --- | --- | --- |
| `examples/import_demo` | Rust | `inproc` | `flowrt build --launcher examples/import_demo/rsdl/robot.rsdl` | 验证 `[package.imports]`、Rust codegen、inproc run 和 launch manifest |
| `examples/cpp_counter_demo` | C++ | `inproc` | `flowrt build --launcher examples/cpp_counter_demo/rsdl/robot.rsdl` | 验证 C++ only CMake app 路径、用户工厂、C++ runtime shell 和 supervisor 启动 |
| `examples/imu_demo` | Rust + C++ | `inproc` 声明用于 build smoke | `flowrt build examples/imu_demo/rsdl/robot.rsdl` | 验证 mixed contract 的接口、消息、参数 schema 和生成物边界；不伪装为 mixed inproc 可运行 |
| `examples/profile_switch_demo` | Rust | `inproc` / `iox2` | `flowrt build --profile iox2 examples/profile_switch_demo/rsdl/robot.rsdl` | 验证同一份 RSDL 通过 profile 切换 backend |
| `examples/mixed_iox2_demo` | Rust + C++ | `iox2` | `flowrt check examples/mixed_iox2_demo/rsdl/robot.rsdl` | 验证 Rust source 与 C++ sink 通过 iox2 分进程连接的 contract |
| `examples/imu_demo_iox2` | Rust + C++ | `iox2` | `flowrt check examples/imu_demo_iox2/rsdl/robot.rsdl` | 验证主 demo 的语言分离 iox2 运行变体，并覆盖 Rust/C++ 用户组件参数接口 |
| `examples/mixed_zenoh_demo` | Rust + C++ | `zenoh` | `flowrt build --launcher examples/mixed_zenoh_demo/rsdl/robot.rsdl` | 验证无界 variable frame、zenoh 跨主机 transport 和 mixed launch 路径 |
| `examples/ros2_bridge_demo` | Rust + ROS2 adapter | `zenoh` | `flowrt build --launcher examples/ros2_bridge_demo/rsdl/robot.rsdl` | 验证 FlowRT 输出经 zenoh-only ROS2 bridge 发布到 ROS2 topic |

## `import_demo`

入口文件：

```text
examples/import_demo/rsdl/robot.rsdl
```

该文件只声明 package 和 imports：

```toml
[package.imports]
types = ["types/*.rsdl"]
components = ["components/*.rsdl"]
graphs = ["graphs/*.rsdl"]
profiles = ["profiles/*.rsdl"]
targets = ["targets/*.rsdl"]
```

它用于证明 v0.1 可以把 `types`、`components`、`graphs`、`profiles` 和 `targets` 拆成多个 RSDL 片段，同时仍归一化到同一份 Contract IR。

常用命令：

```bash
flowrt check examples/import_demo/rsdl/robot.rsdl
flowrt build --launcher examples/import_demo/rsdl/robot.rsdl
flowrt run examples/import_demo/rsdl/robot.rsdl --process main
flowrt launch examples/import_demo/rsdl/robot.rsdl
```

## `cpp_counter_demo`

入口文件：

```text
examples/cpp_counter_demo/rsdl/robot.rsdl
```

该示例是 C++ only inproc contract：

```text
counter_source.count -> counter_sink.count
```

它验证：

- C++ message codegen。
- C++ interface codegen。
- C++ inproc runtime shell。
- `flowrt_user::build_app()` 用户工厂入口。
- C++ only 普通 `flowrt build` / `flowrt run` 走 CMake app 路径。
- C++ only `flowrt build --launcher` 会显式构建 generated supervisor；`flowrt launch` 只执行已有 supervisor。

常用命令：

```bash
flowrt build --launcher examples/cpp_counter_demo/rsdl/robot.rsdl
flowrt run examples/cpp_counter_demo/rsdl/robot.rsdl --process control
flowrt launch examples/cpp_counter_demo/rsdl/robot.rsdl
```

## `imu_demo`

入口文件：

```text
examples/imu_demo/rsdl/robot.rsdl
```

该示例表达主线数据流：

```text
imu_sim -> estimator -> controller -> monitor
```

它用于验证 mixed contract 的生成能力，包括 C++/Rust message、接口和构建产物。当前规则要求 mixed contract 保持语言边界诚实：Rust codegen 不为 C++ component 伪造 Rust trait，C++ codegen 不为 Rust component 伪造 C++ interface。

该示例还声明了运行态参数：

- `estimator.gravity`
- `controller.kp`
- `controller.kd`

这些参数使用显式 schema，`update = "on_tick"`。生成的 Rust/C++ 用户接口会接收 typed params，生成 shell 会通过 runtime socket 暴露参数状态。

基础 smoke：

```bash
flowrt build examples/imu_demo/rsdl/robot.rsdl
```

需要实际观察参数热更新控制面时，使用语言分离的 `examples/imu_demo_iox2`：

```bash
flowrt build --launcher examples/imu_demo_iox2/rsdl/robot.rsdl
FLOWRT_TICK_SLEEP_MS=20 flowrt launch --run-ticks 500 examples/imu_demo_iox2/rsdl/robot.rsdl
```

另开一个终端查看 live 参数：

```bash
flowrt params list examples/imu_demo_iox2/flowrt/selfdesc/selfdesc.json
```

## `profile_switch_demo`

入口文件：

```text
examples/profile_switch_demo/rsdl/robot.rsdl
```

该示例用于验证 `--profile`：

```bash
flowrt check examples/profile_switch_demo/rsdl/robot.rsdl
flowrt build --profile iox2 examples/profile_switch_demo/rsdl/robot.rsdl
flowrt run --profile iox2 examples/profile_switch_demo/rsdl/robot.rsdl
```

`build --profile <name>` 会先投影 Contract IR，再做 validation 和 codegen。`run --profile <name>` 只校验已生成产物的 profile 是否匹配，不会临时重生成。默认 profile 仍是 `default` 或首个 profile。未在 `bind.dataflow` 上显式声明的 channel policy 会随选中 profile 的默认值一起投影；显式 bind policy 不会被 profile 覆盖。

## iox2 mixed 示例

入口文件：

```text
examples/mixed_iox2_demo/rsdl/robot.rsdl
examples/imu_demo_iox2/rsdl/robot.rsdl
```

这些示例验证 language-separated mixed contract over `iox2`：

- process group 必须按语言拆分。
- selected backend 必须是 `iox2`。
- launch manifest 中的 channel 必须暴露 canonical service name。
- Rust 和 C++ shell 消费同一份 Contract IR-derived transport 契约。
- `iox2` 只承载 fixed-size plain data；如果 route 使用 variable frame，Contract IR 会把该 route 自动选择到支持变长消息的 backend（当前为 `zenoh`），不生成变长 over iox2 的兼容承载层。

`mixed_iox2_demo` 和 `imu_demo_iox2` 的基础 smoke 仍以 `check` 为主：

```bash
flowrt check examples/mixed_iox2_demo/rsdl/robot.rsdl
flowrt check examples/imu_demo_iox2/rsdl/robot.rsdl
```

含 C++ iox2 组件的生成 CMake 会查找 `iceoryx2-cxx 0.9.1`。通过 Debian 包安装 FlowRT 时，该 SDK 已在 `/opt/flowrt/<version>` 私有前缀内，`flowrt build` 会自动传入对应路径；直接调试生成 CMake 时，可以显式设置 `FLOWRT_CPP_RUNTIME_DIR` 或 `CMAKE_PREFIX_PATH`。

## zenoh mixed 示例

入口文件：

```text
examples/mixed_zenoh_demo/rsdl/robot.rsdl
```

该示例验证 language-separated mixed contract over `zenoh`，同时覆盖无界 variable frame：

- `bytes`
- `string`
- `sequence<T>`

推荐命令：

```bash
flowrt build --launcher examples/mixed_zenoh_demo/rsdl/robot.rsdl
FLOWRT_TICK_SLEEP_MS=5 flowrt launch --run-ticks 200 examples/mixed_zenoh_demo/rsdl/robot.rsdl
```

含 C++ zenoh 组件的生成 CMake 会查找 `zenohcxx 1.9.0` 的 `zenohcxx::zenohc` 目标，并链接该目标。通过 Debian 包安装 FlowRT 时，`zenoh-c` / `zenoh-cpp` 已在 `/opt/flowrt/<version>` 私有前缀内，`flowrt build` 会自动传入对应路径；直接调试生成 CMake 时，可以显式设置 `FLOWRT_CPP_RUNTIME_DIR` 或 `CMAKE_PREFIX_PATH`。

本机 `flowrt launch` 在没有显式 `FLOWRT_ZENOH_MODE` / `FLOWRT_ZENOH_LISTEN` / `FLOWRT_ZENOH_CONNECT` 时，会为同一个 supervisor 启动的 zenoh process 自动分配本地 TCP mesh。跨机器运行时，需要让两个进程分别拿到对应的 zenoh session 配置，例如通过 `FLOWRT_ZENOH_CONNECT` 和 `FLOWRT_ZENOH_LISTEN` 注入端点；如果要在本机观察足够多的样本，`FLOWRT_TICK_SLEEP_MS` 可以把同步 tick 拉长。

## ROS2 bridge 示例

入口文件：

```text
examples/ros2_bridge_demo/rsdl/robot.rsdl
```

该示例声明一个 Rust source：

```text
source.text -> /flowrt/text
```

RSDL 中通过 `[[bridge.ros2]]` 声明外部 bridge：

```toml
[[bridge.ros2]]
flowrt = "source.text"
ros2_topic = "/flowrt/text"
ros2_type = "std_msgs/msg/String"
direction = "flowrt_to_ros2"
field = "data"
```

FlowRT 与 ROS2 的唯一桥梁固定为 `zenoh`。FlowRT source 会把 bridge tap 发布到 deterministic zenoh key，生成的 C++ ROS2 adapter 订阅该 key 并发布 `std_msgs/msg/String`。ROS2 侧必须使用 `rmw_zenoh_cpp`；adapter 启动时会设置并校验 `RMW_IMPLEMENTATION=rmw_zenoh_cpp`，不会回退到 DDS。普通 FlowRT `zenoh` backend 使用 FlowRT 包内私有 zenoh SDK；ROS2 bridge adapter 进程使用 ROS2 安装中的 `zenoh_cpp_vendor`，以匹配 `rmw_zenoh_cpp` 的同进程 ABI。构建前 source ROS2 环境即可；生成 CMake 会把 `AMENT_PREFIX_PATH` 映射到 `CMAKE_PREFIX_PATH`，让 plain CMake 找到 ROS2 C++ packages。

构建和运行：

```bash
source /opt/ros/jazzy/setup.bash
flowrt build --launcher examples/ros2_bridge_demo/rsdl/robot.rsdl
flowrt launch --run-ticks 200 examples/ros2_bridge_demo/rsdl/robot.rsdl
```

另开 ROS2 环境终端观察：

```bash
source /opt/ros/jazzy/setup.bash
export RMW_IMPLEMENTATION=rmw_zenoh_cpp
ros2 topic echo /flowrt/text --once
```

如果 `ros2 topic echo` 没看到刚启动的 topic，先执行 `ros2 daemon stop` 后重试。

当前限制：

- 只支持 `direction = "flowrt_to_ros2"`。
- 只支持 `ros2_type = "std_msgs/msg/String"`。
- `field` 必须是 FlowRT message 的 `string` 字段。
- `target.<name>.backends` 必须包含 `zenoh`。
- 构建需要 ROS2 C++ 开发包；运行需要安装 `rmw_zenoh_cpp`。

## 添加新示例

新增示例时应明确它验证的边界：

- RSDL 语法或 import 行为。
- validator 规则。
- Rust/C++ codegen 边界。
- runtime channel 或 lifecycle 行为。
- backend capability 或 launch 行为。

不要新增只展示目录结构、但没有可验证命令的空示例。示例如果引入新语义、命令或生成物边界，应同步更新 README、本文档和 `CHANGELOG.md`。
