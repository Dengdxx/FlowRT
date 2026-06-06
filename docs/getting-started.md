# 快速开始

本文从单包 Debian 包安装 `flowrt`，并跑通当前仓库中最小的 Rust-only 和 C++ only 示例。

## 前置条件

- Rust toolchain，支持当前 workspace 使用的 Rust 2024 Edition。
- C++20 编译器、CMake 和 CTest，用于构建 C++ runtime 与 C++ 示例。
- ROS2 bridge 示例需要 ROS2 Jazzy 或之后版本的 C++ 开发环境；运行 bridge 时还需要 `rmw_zenoh_cpp`。CI 当前强制验证 Jazzy 和 Lyrical。

## 安装 FlowRT

在仓库根目录执行：

```bash
scripts/package-deb.sh --output-dir dist
sudo dpkg -i dist/flowrt_*_*.deb
flowrt --version
```

面向用户的入口是系统安装后的 `flowrt ...`。单包 `flowrt` 会同时安装 CLI、Rust runtime crate、C++ runtime header、CMake package、私有 Rust crate vendor、`iceoryx2-cxx 0.9.1`、`zenoh-c 1.9.0` 和 `zenoh-cpp 1.9.0`。这些版本锁定依赖位于 `/opt/flowrt/<version>` 私有前缀，用户项目不需要克隆 FlowRT 仓库，也不需要手动安装 iox2 或 zenoh C++ SDK。Rust 用户组件当前仍通过 Cargo 构建生成 app，因此目标机仍需要 Rust toolchain；C++ 用户组件仍需要 C++20 编译器、CMake 和 CTest。仓库开发者可以用 `cargo run -p flowrt-cli -- ...` 调试 CLI，但文档、示例和对外说明应默认使用系统 PATH 中的 `flowrt ...`。

## 检查 RSDL

先检查模块化 RSDL 示例：

```bash
flowrt check examples/import_demo/rsdl/robot.rsdl
```

预期输出类似：

```text
OK package=import_demo types=2 components=2 instances=2 tasks=2 binds=1
```

`check` 会解析 RSDL、展开 `[package.imports]`、归一化 Contract IR，并运行 validator。它不会生成或构建应用产物。

## 生成产物

```bash
flowrt prepare examples/import_demo/rsdl/robot.rsdl
```

生成物写入示例项目下的 `flowrt/`：

```text
examples/import_demo/flowrt/
  contract/contract.ir.json
  build/
  launch/launch.json
  src/
```

`flowrt/` 是 FlowRT 管理目录，可以删除后重新生成。用户算法代码应放在项目自己的 `src/` 目录，不放进生成目录。

查看已落盘的 Contract IR 摘要：

```bash
flowrt inspect examples/import_demo/flowrt/contract/contract.ir.json
```

## 运行 Rust-only 示例

```bash
flowrt build --launcher examples/import_demo/rsdl/robot.rsdl
flowrt run examples/import_demo/rsdl/robot.rsdl --process main
```

也可以通过生成的 supervisor 启动全部 process group：

```bash
flowrt launch examples/import_demo/rsdl/robot.rsdl
```

当前 `import_demo` 是 Rust-only inproc 示例，适合验证 RSDL import、Contract IR、Rust codegen 和 launch manifest 的基础闭环。

## 运行 C++ only 示例

```bash
flowrt build --launcher examples/cpp_counter_demo/rsdl/robot.rsdl
flowrt run examples/cpp_counter_demo/rsdl/robot.rsdl --process control
flowrt launch examples/cpp_counter_demo/rsdl/robot.rsdl
```

C++ only contract 的普通 `build` / `run` 走 CMake app 路径，不依赖 Cargo app。需要 `launch` 时，先用 `build --launcher` 显式构建 generated supervisor，再由 `launch` 执行已有 supervisor。用户 C++ 组件通过生成接口和 `flowrt_user::build_app()` 注入。

## 切换 profile

```bash
flowrt check examples/profile_switch_demo/rsdl/robot.rsdl
flowrt build --profile iox2 examples/profile_switch_demo/rsdl/robot.rsdl
flowrt run --profile iox2 examples/profile_switch_demo/rsdl/robot.rsdl
```

`build --profile <name>` 会先投影 Contract IR，只保留选定 profile 的 deployment 视图，并让未显式写在 `bind.dataflow` 上的 channel policy 使用该 profile 的默认值，再校验和生成对应产物。`run --profile <name>` 只校验已生成产物的 profile 是否匹配，不会临时重生成。选择 `iox2` 或 `zenoh` profile 时，Rust 生成物会启用 runtime crate 的对应 feature；含 C++ backend 组件时，生成 CMake 会优先使用 FlowRT 安装包内 `/opt/flowrt/<version>` 的私有 SDK，缺失时才要求显式设置 `FLOWRT_CPP_RUNTIME_DIR` 或 `CMAKE_PREFIX_PATH`。

## 查看运行态参数

含参数的应用运行时会启动 introspection socket。可以在另一个终端用静态 self-description 匹配 live process，并查看或提交参数 pending 更新：

```bash
flowrt build --launcher examples/imu_demo_iox2/rsdl/robot.rsdl
FLOWRT_TICK_SLEEP_MS=20 flowrt launch --run-steps 500 examples/imu_demo_iox2/rsdl/robot.rsdl
```

另开一个终端查询或提交参数：

```bash
flowrt params list examples/imu_demo_iox2/flowrt/selfdesc/selfdesc.json
flowrt params get examples/imu_demo_iox2/flowrt/selfdesc/selfdesc.json estimator.gravity
flowrt params set examples/imu_demo_iox2/flowrt/selfdesc/selfdesc.json estimator.gravity 9.7
```

`params set` 的值必须是合法 JSON。`on_tick` 参数会在下一个 tick 边界通过用户组件的 `on_params_update` 钩子提交；`startup` 参数运行时不可修改。

## ROS2 bridge 示例

FlowRT 与 ROS2 的 bridge 固定走 `zenoh`，ROS2 侧必须使用 `rmw_zenoh_cpp`，不会回退到 DDS。ROS2 bridge adapter 进程使用 ROS2 安装中的 `zenoh_cpp_vendor`，以匹配 `rmw_zenoh_cpp` 的同进程 ABI。构建前 source ROS2 环境即可；生成 CMake 会把 `AMENT_PREFIX_PATH` 映射到 `CMAKE_PREFIX_PATH`。当前示例把 FlowRT `TextFrame.data` 发布到 ROS2 `/flowrt/text`：

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

## 出错时先看什么

- `flowrt check` 失败：优先修正 RSDL 命名、类型、端口、task、bind、target/backend 声明。
- `flowrt build` 失败：检查用户组件实现是否匹配生成接口，以及 Rust/C++ toolchain、CMake、FlowRT 安装前缀是否存在。
- `flowrt run --process <name>` 失败：先确认已经执行过匹配 profile 的 `flowrt build`；再确认 process 名称来自 RSDL `instance.<name>.process`；mixed contract 必须选择单语言 process，或使用 `flowrt launch`；`inproc` backend 下不能单独运行带跨 process dataflow 的 process group。
- `flowrt launch` 失败：先确认已经执行过匹配 profile 的 `flowrt build --launcher`；再检查 `flowrt/launch/launch.json` 是否生成；确认 mixed process group 没有把 C++ 和 Rust component 放在同一 process 内；如果 backend 是 `inproc`，还要确认 dataflow bind 没有跨 RSDL process group。
