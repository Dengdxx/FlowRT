# 快速开始

本文从源码安装 `flowrt`，并跑通当前仓库中最小的 Rust-only 和 C++ only 示例。

## 前置条件

- Rust toolchain，支持当前 workspace 使用的 Rust 2024 Edition。
- C++20 编译器、CMake 和 CTest，用于构建 C++ runtime 与 C++ 示例。
- 可选：`iceoryx2-cxx 0.9.1`、基于 `zenoh-c` backend 的 `zenohcxx 1.9.0`。C++ iox2 / zenoh 示例会先查找本机安装；zenoh 找不到本机 `zenohcxx::zenohc` 目标时，CMake 会直接失败，需要先安装 `zenoh-c` / `zenoh-cpp` 1.9.0。

## 安装 CLI

在仓库根目录执行：

```bash
cargo install --path crates/flowrt-cli --locked
flowrt --version
```

面向用户的入口是安装后的 `flowrt ...`。仓库开发者可以用 `cargo run -p flowrt-cli -- ...` 调试 CLI，但文档、示例和对外说明应默认使用 `flowrt ...`。

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

`build --profile <name>` 会先投影 Contract IR，只保留选定 profile 的 deployment 视图，并让未显式写在 `bind.dataflow` 上的 channel policy 使用该 profile 的默认值，再校验和生成对应产物。`run --profile <name>` 只校验已生成产物的 profile 是否匹配，不会临时重生成。选择 `iox2` 或 `zenoh` profile 时，Rust 生成物会启用 runtime crate 的对应 feature；含 C++ `iox2` 组件时，生成 CMake 会先使用 `CMAKE_PREFIX_PATH` 中的本机依赖，找不到再自动拉取并构建；含 C++ `zenoh` 组件时，必须预先安装并暴露 `zenohcxx::zenohc`。

## 查看运行态参数

含参数的应用运行时会启动 introspection socket。可以在另一个终端用静态 self-description 匹配 live process，并查看或提交参数 pending 更新：

```bash
flowrt params list examples/imu_demo/flowrt/selfdesc/selfdesc.json
flowrt params get examples/imu_demo/flowrt/selfdesc/selfdesc.json estimator.gravity
flowrt params set examples/imu_demo/flowrt/selfdesc/selfdesc.json estimator.gravity 9.7
```

`params set` 的值必须是合法 JSON。`on_tick` 参数会在下一个 tick 边界通过用户组件的 `on_params_update` 钩子提交；`startup` 参数运行时不可修改。

## 出错时先看什么

- `flowrt check` 失败：优先修正 RSDL 命名、类型、端口、task、bind、target/backend 声明。
- `flowrt build` 失败：检查用户组件实现是否匹配生成接口，以及 C++ toolchain / CMake / 可选 iox2 依赖是否存在。
- `flowrt run --process <name>` 失败：先确认已经执行过匹配 profile 的 `flowrt build`；再确认 process 名称来自 RSDL `instance.<name>.process`；mixed contract 必须选择单语言 process，或使用 `flowrt launch`；`inproc` backend 下不能单独运行带跨 process dataflow 的 process group。
- `flowrt launch` 失败：先确认已经执行过匹配 profile 的 `flowrt build --launcher`；再检查 `flowrt/launch/launch.json` 是否生成；确认 mixed process group 没有把 C++ 和 Rust component 放在同一 process 内；如果 backend 是 `inproc`，还要确认 dataflow bind 没有跨 RSDL process group。
