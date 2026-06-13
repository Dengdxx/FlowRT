# 示例矩阵

本仓库的 `examples/` 目录用于验证 RSDL、Contract IR、validator、codegen、runtime 和 CLI 的端到端切片。每个示例都尽量覆盖一个明确边界，不把所有能力塞进同一个 demo。

首次运行涉及 `flowrt build` 的示例前，先执行 `flowrt deps --backend all`，或对单个示例执行 `flowrt deps <示例 rsdl>`。`flowrt deps` 只预热 FlowRT 底层依赖缓存；示例二进制由 `flowrt build` 写入各自项目的 `flowrt/build/bin/release/`。

## 示例列表

| 示例 | Runtime | Backend | 推荐命令 | 用途 |
| --- | --- | --- | --- | --- |
| `examples/import_demo` | Rust | `inproc` | `flowrt build --launcher examples/import_demo/rsdl/robot.rsdl` | 验证 `[package.imports]`、Rust codegen、inproc run 和 launch manifest |
| `examples/workspace_demo` | Rust | `inproc` | `flowrt build --launcher examples/workspace_demo/rsdl/robot.rsdl` | 验证 workspace / module / composition、跨模块引用和同名 module symbol 的生成命名 |
| `examples/cpp_counter_demo` | C++ | `inproc` | `flowrt build --launcher examples/cpp_counter_demo/rsdl/robot.rsdl` | 验证 C++ only CMake app 路径、用户工厂、C++ runtime shell 和 supervisor 启动 |
| `examples/imu_demo` | Rust + C++ | `inproc` 声明用于 build smoke | `flowrt build examples/imu_demo/rsdl/robot.rsdl` | 验证 mixed contract 的接口、消息、参数 schema 和生成物边界；不伪装为 mixed inproc 可运行 |
| `examples/profile_switch_demo` | Rust | `inproc` / `iox2` | `flowrt build --profile iox2 examples/profile_switch_demo/rsdl/robot.rsdl` | 验证同一份 RSDL 通过 profile 切换 backend |
| `examples/mixed_iox2_demo` | Rust + C++ | `iox2` | `flowrt check examples/mixed_iox2_demo/rsdl/robot.rsdl` | 验证 Rust source 与 C++ sink 通过 iox2 分进程连接的 contract |
| `examples/imu_demo_iox2` | Rust + C++ | `iox2` | `flowrt check examples/imu_demo_iox2/rsdl/robot.rsdl` | 验证主 demo 的语言分离 iox2 运行变体，并覆盖 Rust/C++ 用户组件参数接口 |
| `examples/mixed_zenoh_demo` | Rust + C++ | `zenoh` | `flowrt build --launcher examples/mixed_zenoh_demo/rsdl/robot.rsdl` | 验证无界 variable frame、zenoh 跨主机 transport 和 mixed launch 路径 |
| `examples/ros2_bridge_demo` | Rust + ROS2 adapter | `zenoh` | `flowrt build --launcher examples/ros2_bridge_demo/rsdl/robot.rsdl` | 验证 FlowRT 输出经 zenoh-only ROS2 bridge 发布到 ROS2 topic |
| `examples/island_demo` | Rust | `inproc` | `flowrt build --launcher examples/island_demo/rsdl/robot.rsdl` | 验证 Island Mode 下 boundary input/output 的单功能单位 IO 测试闭环 |
| `examples/variable_frame_island_demo` | Rust | `inproc` | `scripts/test-v091-variable-frame-island-demo.sh` | 验证 `sequence<f32>` canonical frame boundary input、`flowrt pub --file --freq` 和 echo 输出摘要 |
| `examples/service_demo` | Rust | `inproc` | `flowrt build examples/service_demo/service_demo.rsdl` | 验证 service client/server typed API、inproc request/response、service policy 和 `flowrt status` 健康观测 |
| `examples/operation_demo` | Rust | `inproc` | `flowrt build --launcher examples/operation_demo/rsdl/robot.rsdl` | 验证 Operation client/server typed API、自描述、inproc lowering 和 `flowrt op list` |
| `examples/external_driver_demo` | External executable | `zenoh` | `flowrt build --launcher examples/external_driver_demo/rsdl/robot.rsdl` | 验证 external package manifest、supervisor 启动、环境变量契约和 bundle/deploy baseline |
| `examples/frame_descriptor_demo` | Rust | `iox2` | `flowrt build --launcher examples/frame_descriptor_demo/rsdl/robot.rsdl` | 验证 I/O boundary 标准 FrameDescriptor、iox2 fixed descriptor route、echo/status/record descriptor-only 观测 |
| `examples/libjpeg_cross_demo` | C++ | `inproc` | `scripts/test-v086-cross-sdk-demos.sh` | 验证公开可移植 C/C++ 库通过 pkg-config overlay 接入 amd64 到 arm64 交叉构建 |
| `examples/kleidiai_cross_demo` | C++ | `inproc` | `scripts/test-v086-cross-sdk-demos.sh` | 验证 Arm 专用公开 SDK 通过 pkg-config overlay 接入 FlowRT C++ component 并在 arm64 运行 |

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

## `workspace_demo`

入口文件：

```text
examples/workspace_demo/rsdl/robot.rsdl
```

该示例使用 workspace root 装载两个 module 和一个 composition：

```toml
[workspace]
modules = ["modules/*.rsdl"]
compositions = ["composition/default.rsdl"]
```

module 只声明自己的 `type` 和 `component`，composition 统一声明 `instance`、`task`、
`bind`、`profile` 和 `target`。跨模块引用使用 `module::Name`：

```toml
[component.processor]
language = "cpp"
input = ["sample:perception::Sample"]
output = ["command:Sample"]
```

该示例刻意让 `perception` 和 `control` 两个 module 都声明 `Sample` 和 `processor`
短名，用于验证：

- module 内部短类型名优先解析为本 module 符号。
- root/composition 层短名存在歧义时必须显式写 `module::Name`。
- 生成的 Rust/C++ 用户接口使用稳定 generated symbol，例如 `PerceptionProcessor`
  和 `ControlProcessorInterface`，避免同名 module symbol 互相撞。

常用命令：

```bash
flowrt check examples/workspace_demo/rsdl/robot.rsdl
flowrt build --launcher examples/workspace_demo/rsdl/robot.rsdl
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
FLOWRT_TICK_SLEEP_MS=20 flowrt launch --run-steps 500 examples/imu_demo_iox2/rsdl/robot.rsdl
```

另开一个终端查看 live 参数：

```bash
flowrt params list --image examples/imu_demo_iox2/flowrt/selfdesc/selfdesc.json
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

## `external_driver_demo`

入口文件：

```text
examples/external_driver_demo/rsdl/robot.rsdl
```

该示例声明一个 `language = "external"` 的 `sensor` component，并用
`[[external_process]]` 绑定到 `examples/external_driver_demo/external/fake_sensor_driver`：

```toml
[[external_process]]
process = "sensor_proc"
package = "fake_sensor_driver"
executable = "bin/driver"
args = ["--mode", "smoke"]
health = "process_started"
required_backends = ["zenoh"]
```

external package 自描述文件：

```text
examples/external_driver_demo/external/fake_sensor_driver/flowrt-external.toml
```

常用命令：

```bash
flowrt external check examples/external_driver_demo/external/fake_sensor_driver
flowrt deps examples/external_driver_demo/rsdl/robot.rsdl
flowrt build --launcher examples/external_driver_demo/rsdl/robot.rsdl
flowrt launch --run-steps 2 examples/external_driver_demo/rsdl/robot.rsdl
flowrt bundle examples/external_driver_demo/rsdl/robot.rsdl --output dist/external-driver-demo
flowrt deploy dist/external-driver-demo --host user@host --target edge --remote-dir /opt/external-driver-demo --dry-run
```

该示例不访问真实硬件。`bin/driver` 只校验 supervisor 注入的 `FLOWRT_*` 环境变量和
manifest args，用于证明 external process 可以纳入 FlowRT 的 Contract IR、launch
manifest、self-description、supervisor 和离线 bundle 主路径。

## 公开交叉 SDK 示例

入口文件：

```text
examples/libjpeg_cross_demo/rsdl/robot.rsdl
examples/kleidiai_cross_demo/rsdl/robot.rsdl
```

配套依赖 prepare 项目：

```text
examples/cross_sdk_deps/CMakeLists.txt
```

这组示例验证 `v0.8.4` 引入的 component build `pkg_config` 与 toolchain SDK overlay 能否
覆盖真实交叉编译场景。`cross_sdk_deps` 用 CMake 显式拉取并交叉编译公开依赖，不把第三方
源代码或二进制提交进仓库；`flowrt build` 阶段只消费已经准备好的
`lib/aarch64-linux-gnu/pkgconfig/*.pc`、头文件和静态库。

推荐 smoke：

```bash
scripts/test-v086-cross-sdk-demos.sh
```

手动拆开执行时，先准备公开 arm64 SDK overlay：

```bash
sdk_root="$PWD/.flowrt/public-arm64-sdk"
cmake -S examples/cross_sdk_deps -B build/cross_sdk_deps -G Ninja \
  -DFLOWRT_CROSS_SDK_PREFIX="$sdk_root" \
  -DFLOWRT_CROSS_BUILD_JOBS=1
cmake --build build/cross_sdk_deps --target flowrt_public_arm64_sdk -j1
```

然后在每个示例工作区用 CLI 生成最小 toolchain profile。用户不需要手写完整
`.flowrt/toolchains.toml`；`sdk_overlays` 会自动派生常见 CMake prefix 和
pkg-config 搜索路径：

```bash
flowrt toolchain init --target linux-arm64 --sdk-overlay "$sdk_root" --force
flowrt toolchain show --target linux-arm64
```

最后进入对应示例目录执行带 RSDL 的 `doctor`、依赖预热和构建：

```bash
cd examples/libjpeg_cross_demo
flowrt toolchain init --target linux-arm64 --sdk-overlay "$sdk_root" --force
flowrt toolchain show --target linux-arm64
flowrt doctor rsdl/robot.rsdl --target linux-arm64
flowrt deps rsdl/robot.rsdl --target linux-arm64 --backend inproc
flowrt build --target linux-arm64 --launcher rsdl/robot.rsdl

cd ../kleidiai_cross_demo
flowrt toolchain init --target linux-arm64 --sdk-overlay "$sdk_root" --force
flowrt toolchain show --target linux-arm64
flowrt doctor rsdl/robot.rsdl --target linux-arm64
flowrt deps rsdl/robot.rsdl --target linux-arm64 --backend inproc
flowrt build --target linux-arm64 --launcher rsdl/robot.rsdl
```

`libjpeg_cross_demo` 覆盖平台无关公开 C/C++ 库；`kleidiai_cross_demo` 覆盖 Arm 专用公开
SDK 和 NEON kernel。它们都不代表 FlowRT 内置硬件 backend，只验证用户项目如何通过
toolchain overlay 接入外部 SDK。推荐先运行带 RSDL 的 `flowrt doctor`，确认
`component.build.pkg_config` 模块已经在 selected target 的 overlay / pkg-config 路径中
可见，再继续 `deps` 和 `build`。

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
FLOWRT_TICK_SLEEP_MS=5 flowrt launch --run-steps 200 examples/mixed_zenoh_demo/rsdl/robot.rsdl
```

含 C++ zenoh 组件的生成 CMake 会查找 `zenohcxx 1.9.0` 的 `zenohcxx::zenohc` 目标，并链接该目标。通过 Debian 包安装 FlowRT 时，`zenoh-c` / `zenoh-cpp` 已在 `/opt/flowrt/<version>` 私有前缀内，`flowrt build` 会自动传入对应路径；直接调试生成 CMake 时，可以显式设置 `FLOWRT_CPP_RUNTIME_DIR` 或 `CMAKE_PREFIX_PATH`。

本机 `flowrt launch` 在没有显式 `FLOWRT_ZENOH_MODE` / `FLOWRT_ZENOH_LISTEN` / `FLOWRT_ZENOH_CONNECT` 时，会为同一个 supervisor 启动的 zenoh process 自动分配本地 TCP mesh。跨机器运行时，需要让两个进程分别拿到对应的 zenoh session 配置，例如通过 `FLOWRT_ZENOH_CONNECT` 和 `FLOWRT_ZENOH_LISTEN` 注入端点；如果要在本机观察足够多的样本，`FLOWRT_TICK_SLEEP_MS` 可以把同步 tick 拉长。

## FrameDescriptor 示例

入口文件：

```text
examples/frame_descriptor_demo/rsdl/robot.rsdl
```

该示例验证本机大 payload 的推荐表达：FlowRT channel 只传固定 64 字节
`FrameHandle` descriptor，真实图像 payload 由 `camera` I/O boundary 的 side-channel
资源管理，不作为普通 `bytes` message 进入 dataflow。

RSDL 中的 descriptor 绑定到输出端口：

```toml
[component.camera.resource.frames.descriptor]
kind = "frame"
port = "frame"
format = "rgb8"
encoding = "row_major"
metadata = { width = "640", height = "480", stride_bytes = "1920" }
record_payload = false
```

推荐命令：

```bash
flowrt check examples/frame_descriptor_demo/rsdl/robot.rsdl
flowrt build --launcher examples/frame_descriptor_demo/rsdl/robot.rsdl
FLOWRT_TICK_SLEEP_MS=10 flowrt run examples/frame_descriptor_demo/rsdl/robot.rsdl --process main
```

另开终端观察：

```bash
flowrt status --live-only
flowrt echo camera.frame --image examples/frame_descriptor_demo/flowrt/selfdesc/selfdesc.json
```

`flowrt echo` 会把 payload 识别为标准 FrameDescriptor 并按字段展示；
`flowrt record` 默认记录 descriptor/event，摘要中输出
`descriptor_payload=descriptor_only`。真实 payload 录制需要显式建模，不由该示例隐式
复制图像数据。

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

FlowRT 与 ROS2 的唯一桥梁固定为 `zenoh`。FlowRT source 会把 bridge tap 发布到
deterministic zenoh key，生成的 C++ ROS2 adapter 订阅该 key 并发布 ROS2 topic；
`ros2_to_flowrt` 方向则由 adapter 订阅 ROS2 topic 后发布到同一类 bridge key。ROS2
侧必须使用 `rmw_zenoh_cpp`；adapter 启动时会设置并校验
`RMW_IMPLEMENTATION=rmw_zenoh_cpp`，不会回退到 DDS。普通 FlowRT `zenoh` backend
使用 FlowRT 包内私有 zenoh SDK；ROS2 bridge adapter 进程使用 ROS2 安装中的
`zenoh_cpp_vendor`，以匹配 `rmw_zenoh_cpp` 的同进程 ABI。构建前 source ROS2 环境即可；
生成 CMake 会把 `AMENT_PREFIX_PATH` 映射到 `CMAKE_PREFIX_PATH`，让 plain CMake 找到
ROS2 C++ packages。CI 当前在 Jazzy 和 Lyrical 上强制验证该示例。

Island profile 下，`[[bridge.ros2]].flowrt` 也可以引用 `boundary.input` 或
`boundary.output` 名称，而不是普通 `instance.port`：

```toml
[profile.default]
backend = "zenoh"
mode = "island"

[[boundary.input]]
name = "request_in"
port = "echo.request"
type = "TextFrame"

[[bridge.ros2]]
flowrt = "request_in"
ros2_topic = "/ros2/request"
ros2_type = "std_msgs/msg/String"
direction = "ros2_to_flowrt"
field = "data"
```

这条路径用于 ROS2 共存和逐功能单位迁移测试，不是 ROS2 drop-in。generated shell 会把
ROS2 样本先注入 `BoundaryInput`，task 仍从 island boundary 读取；拆掉 bridge 或拆掉
boundary 后，功能单位可以回到普通 strict graph。

构建和运行：

```bash
source /opt/ros/jazzy/setup.bash
flowrt build --launcher examples/ros2_bridge_demo/rsdl/robot.rsdl
flowrt launch --run-steps 200 examples/ros2_bridge_demo/rsdl/robot.rsdl
```

另开 ROS2 环境终端观察：

```bash
source /opt/ros/jazzy/setup.bash
export RMW_IMPLEMENTATION=rmw_zenoh_cpp
ros2 topic echo /flowrt/text --once
```

如果 `ros2 topic echo` 没看到刚启动的 topic，先执行 `ros2 daemon stop` 后重试。

当前限制：

- 支持 `flowrt_to_ros2` / `ros2_to_flowrt` 的窄 typed subset。
- 支持 `std_msgs/msg/String` 和 `geometry_msgs/msg/Pose`；暂不覆盖完整 ROS2 消息生态。
- `std_msgs/msg/String` 的 `field` 必须是 FlowRT message 的 `string` 字段。
- `target.<name>.backends` 必须包含 `zenoh`。
- 构建需要 ROS2 C++ 开发包；运行需要安装 `rmw_zenoh_cpp`。

## `island_demo`

入口文件：

```text
examples/island_demo/rsdl/robot.rsdl
examples/island_demo/app/rust/mod.rs
```

该示例只有一个 Rust `processor` 组件：

```text
boundary input sample_in -> processor.sample -> processor.result -> boundary output result_out
```

`sample_in` 和 `result_out` 都绑定真实 component port，因此用户算法代码仍是普通
FlowRT component trait 实现。Island Mode 只负责在完整拓扑缺席时补齐外部 IO：

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

构建并运行：

```bash
flowrt deps examples/island_demo/rsdl/robot.rsdl --backend inproc
flowrt build --launcher examples/island_demo/rsdl/robot.rsdl
flowrt run examples/island_demo/rsdl/robot.rsdl --process main
```

另开终端注入输入并读取输出：

```bash
flowrt pub sample_in \
  --json '{"seq": 7, "value": 21}' \
  --image examples/island_demo/flowrt/selfdesc/selfdesc.json \
  --published-at-ms 1000
flowrt echo result_out --image examples/island_demo/flowrt/selfdesc/selfdesc.json
```

输出 payload 会按 Message ABI 格式化，能看到 `seq=7`、`doubled=42`。如果需要保存
对比证据，可以录制 boundary output：

```bash
flowrt record --output island-demo.mcap --duration 500ms --channel result_out
```

`flowrt pub` 只允许写 boundary input；尝试写普通 channel、strict graph 或 boundary
output 都会报错。完成单功能单位测试后，删除 boundary endpoint，补上普通
`[[bind.dataflow]]`，再把 profile 切回 `strict`。

## `variable_frame_island_demo`

入口文件：

```text
examples/variable_frame_island_demo/rsdl/robot.rsdl
examples/variable_frame_island_demo/app/rust/mod.rs
examples/variable_frame_island_demo/samples/scan.jsonl
```

该示例验证迁移测试里常见的变长输入：`ScanFrame` 包含 `string` 和
`sequence<f32>`，island boundary input 接收 JSONL 后由组件计算固定大小摘要：

```toml
[type.ScanFrame]
seq = "u32"
label = "string"
ranges = "sequence<f32>"
```

运行 smoke：

```bash
scripts/test-v091-variable-frame-island-demo.sh
```

手动运行时，先启动 runtime：

```bash
flowrt deps examples/variable_frame_island_demo/rsdl/robot.rsdl --backend inproc
flowrt build --launcher examples/variable_frame_island_demo/rsdl/robot.rsdl
flowrt run examples/variable_frame_island_demo/rsdl/robot.rsdl --process main
```

另开终端按 wall-clock 节奏注入 JSONL，并观察摘要：

```bash
flowrt pub scan_in \
  --file examples/variable_frame_island_demo/samples/scan.jsonl \
  --freq 200 \
  --image examples/variable_frame_island_demo/flowrt/selfdesc/selfdesc.json
flowrt echo summary_out --image examples/variable_frame_island_demo/flowrt/selfdesc/selfdesc.json
```

`summary_out` 是 fixed ABI 摘要，便于稳定断言 `seq`、`count` 和 `mean_milli`。该示例
只展示可拆卸 island 脚手架，不要求 ROS2、硬件或外部私有库。

如果直接观察包含长 `sequence<T>` 的 boundary output 或 channel，`flowrt echo` 默认会
把超过 16 个元素的 sequence 压缩成 `sequence_summary(...)`。需要完整数组时传入
`--raw`，该选项只影响 CLI 展示，不改变 runtime payload 或 record 文件。

迁移验证时可以把旧系统的 live topic、bag 片段或测试 fixture 先转换成同形状 JSONL，
再复用该示例路径：

```bash
flowrt params set --file params.json --image examples/variable_frame_island_demo/flowrt/selfdesc/selfdesc.json
flowrt pub scan_in \
  --file samples.jsonl \
  --freq 100 \
  --image examples/variable_frame_island_demo/flowrt/selfdesc/selfdesc.json
flowrt replay \
  --file fixture.jsonl \
  --image examples/variable_frame_island_demo/flowrt/selfdesc/selfdesc.json \
  --as-fast-as-possible
flowrt echo summary_out --image examples/variable_frame_island_demo/flowrt/selfdesc/selfdesc.json
flowrt record --output scan-compare.mcap --duration 2s --channel summary_out
```

单个 boundary input 可继续用 `pub --file`；需要一条 fixture 驱动多个 boundary input 时，
使用 `flowrt replay`。FlowRT 不在该路径中原生读取 rosbag，也不提供 ROS2 drop-in
兼容层；边界输入输出是可拆的行为测试脚手架。验证完成后应删除 boundary endpoint，改为普通
`[[bind.dataflow]]` 并切回 `strict`。

## `service_demo`

入口文件：

```text
examples/service_demo/service_demo.rsdl
examples/service_demo/app/rust/mod.rs
```

该示例验证 Service request/response 运行时的完整闭环：RSDL 声明、codegen 生成、
用户组件实现、inproc service call 和 `flowrt status` 健康观测。

RSDL 声明 service 类型、组件端口、bind policy、profile 和 target：

```toml
[type.PlanRequest]
goal = "u32"

[type.PlanResponse]
accepted = "bool"

[component.plan_service]
language = "rust"
service_server = ["plan:PlanRequest->PlanResponse"]

[component.planner]
language = "rust"
service_client = ["plan:PlanRequest->PlanResponse"]
output = ["result:i32"]

[[bind.service]]
client = "plan_client.plan"
server = "plan_svc.plan"
backend = "inproc"
timeout_ms = 1000
queue_depth = 16
overflow = "busy"
```

用户实现 service server handler 和 service client 调用方：

```rust
// service server：实现 on_plan_request handler
impl PlanService for PlanServiceImpl {
    fn on_plan_request(&mut self, request: &PlanRequest) -> flowrt::ServiceResult<PlanResponse> {
        let accepted = request.goal % 2 == 0;
        flowrt::ServiceResult::ok(PlanResponse { accepted })
    }
}

// service client：在 on_tick 中通过 typed handle 发起非阻塞调用
impl Planner for PlannerImpl {
    fn on_tick(
        &mut self,
        plan: &ServiceClient_planner_plan,
        result: &mut flowrt::Output<i32>,
    ) -> flowrt::Status {
        let handle = plan.start_call(PlanRequest { goal: 1 }, std::time::Duration::from_millis(500));
        // 后续 tick 中通过 handle.poll() / handle.complete() 取得响应。
        let _ = (handle, result);
        flowrt::Status::ok()
    }
}
```

它验证：

- RSDL `[[bind.service]]` 声明、service policy 字段（`backend`、`timeout_ms`、`queue_depth`、`overflow`）。
- Contract IR `ServiceEdgeIr` 归一化和 auto backend resolver。
- Rust codegen：`ServiceClient_{instance}_{port}` typed handle（`call()` / `start_call()`），示例在 scheduler 回调内使用非阻塞 `start_call()`。
- Rust codegen：component trait 中 `on_{port}_request` handler 方法。
- Rust codegen：hidden service task 注册和 scheduler wake glue。
- `InprocServiceConfig` 生成：`queue_depth`、`max_in_flight`、`overflow`。
- `flowrt list` 展示 service endpoint 拓扑。
- `flowrt status` 展示 service 运行态健康（ready、in_flight、queued、timeout、busy）。

构建和运行：

```bash
flowrt build examples/service_demo/service_demo.rsdl
flowrt run examples/service_demo/service_demo.rsdl --process main --run-steps 50
```

查看 service 拓扑和健康：

```bash
flowrt list examples/service_demo/flowrt/selfdesc/selfdesc.json
flowrt status
```

## `operation_demo`

入口文件：

```text
examples/operation_demo/rsdl/robot.rsdl
examples/operation_demo/app/rust/mod.rs
```

该示例验证 Operation 的用户主语义：RSDL 声明 typed operation client/server，
`[[bind.operation]]` 绑定双方，codegen 生成 typed client handle 和 server handler，
self-description 保留 Operation endpoint 与 lowering refs，CLI 用 `flowrt op list`
展示 Operation 主视图。

RSDL 声明 goal、feedback 和 result 类型：

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
concurrency = "reject"
preempt = "reject"
feedback = "latest"
result_retention_ms = 60000
```

当前 generated Operation runtime 只支持单 in-flight reject 子集：`concurrency =
"reject"`、`preempt = "reject"`、`max_in_flight = 1`。多 invocation queue 和
cancel-running preempt 策略属于长期 IR 语义，在 runtime 完整实现前由 validator
拒绝。

用户代码实现 server handler：

```rust
impl Navigator for NavigatorImpl {
    fn on_plan_operation(
        &mut self,
        goal: &PlanGoal,
        cancel: flowrt::OperationCancelToken,
        progress: &mut flowrt::OperationProgressPublisher<PlanFeedback>,
    ) -> flowrt::OperationHandlerResult<PlanResult> {
        if cancel.is_canceled() {
            return flowrt::OperationHandlerResult::canceled();
        }
        progress.publish(PlanFeedback { progress: 1.0 });
        flowrt::OperationHandlerResult::succeeded(PlanResult {
            accepted: goal.target > 0,
        })
    }
}
```

构建、运行和查看 Operation 拓扑：

```bash
flowrt check examples/operation_demo/rsdl/robot.rsdl
flowrt build --launcher examples/operation_demo/rsdl/robot.rsdl
flowrt run --run-steps 5 examples/operation_demo/rsdl/robot.rsdl --process main
flowrt op list --image examples/operation_demo/flowrt/selfdesc/selfdesc.json
```

当前 inproc Operation 会在生成物内部 lower 成 start/cancel/status service 与
feedback/result endpoint；用户文档和 CLI 的主视图仍是 Operation。native generated
`zenoh` Operation 尚未接线到真实 transport，codegen 会 fail-fast，不生成 placeholder。

## record smoke

`flowrt record` 录制 live runtime 事件到 MCAP 文件，不需要 RSDL 源文件或生成应用
二进制。最小 smoke 可以用 C++ counter demo 启动一个运行中进程，然后按 socket
录制：

```bash
export XDG_RUNTIME_DIR="$(mktemp -d)"
flowrt build --launcher examples/cpp_counter_demo/rsdl/robot.rsdl
FLOWRT_TICK_SLEEP_MS=10 flowrt run examples/cpp_counter_demo/rsdl/robot.rsdl --process control &
pid=$!
flowrt status
socket="$(
  flowrt status | awk '/package=cpp_counter_demo/ {
    for (i = 1; i <= NF; i++) {
      if ($i ~ /^socket=/) {
        sub(/^socket=/, "", $i)
        print $i
        exit
      }
    }
  }'
)"
flowrt record --output counter.mcap --duration 250ms --all --socket "$socket"
kill "$pid"
wait "$pid" 2>/dev/null || true
```

命令结束后会输出 `event_count`、`dropped_count` 和 `bytes_written`。没有执行
`flowrt record` 时，runtime recorder tap 处于关闭状态，不持续复制 channel payload。

## Supervisor 和调度健康

v0.5.0 新增的 supervisor 特性（readiness 条件启动、错峰启动、env 注入、CPU
affinity/priority）适用于任何多进程示例。以 `mixed_zenoh_demo` 为例，可以在
RSDL 中添加 `[[process]]` 声明：

```toml
[[process]]
name = "rust_source"
readiness = "runtime_ready"
startup_delay_ms = 100

[[process]]
name = "cpp_sink"
depends_on = ["rust_source"]
readiness = "service_ready"
cpu_affinity = [0, 1]
nice = -10
env = { FLOWRT_LOG_LEVEL = "info" }
```

构建后用 `flowrt launch` 启动，`flowrt status` 会展示每个进程的 readiness 等待
状态和资源应用情况。

调度健康指标（deadline miss、stale input、backpressure、overflow、fairness
violations、queue depth、dispatched count）在所有示例中自动采集，通过
`flowrt status` 即可查看，无需额外配置。带 `deadline_ms` 的 task（如
`imu_demo_iox2` 的 controller task）会触发 deadline miss 检测。

远程参数控制面适用于任何含参数的示例（如 `imu_demo_iox2`），加上 `--remote`
即可通过 zenoh 跨机器操作参数。

## 添加新示例

新增示例时应明确它验证的边界：

- RSDL 语法或 import 行为。
- validator 规则。
- Rust/C++ codegen 边界。
- runtime channel 或 lifecycle 行为。
- backend capability 或 launch 行为。

不要新增只展示目录结构、但没有可验证命令的空示例。示例如果引入新语义、命令或生成物边界，应同步更新 README、本文档和 `CHANGELOG.md`。
