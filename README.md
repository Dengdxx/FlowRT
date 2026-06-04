# FlowRT

FlowRT 是一个数据流编译型机器人运行时。

用户用 `.rsdl` 描述机器人系统结构：消息、组件端口、任务时序、数据流连接、部署目标和通信后端。工具链将 RSDL 编译为 Contract IR，完成校验后准备 FlowRT 管理的 runtime shell、消息类型、启动配置和构建文件。

核心原则：

```text
RSDL controls system structure.
Runtime controls execution.
User code controls algorithms.
```

设计和规格文档放在本地 `docs/` 目录维护，但当前不纳入 Git 版本库。
仓库入口文档只保留可公开同步的项目定位、命令和维护约定。

## 命令入口

FlowRT 的用户入口是安装后的独立命令 `flowrt`。`cargo run -p flowrt-cli -- ...`
只用于本仓库开发者调试 FlowRT 工具链，不是用户项目的启动方式。

安装和预编译分发流程仍在推进中；但用户主路径必须按已安装工具链来设计：
只写 C++ 业务逻辑的用户应能在没有 Rust/Cargo 的环境中使用 `flowrt` 处理 `.rsdl`、
准备 FlowRT 管理产物，并通过 CMake 构建 C++ runtime shell 和应用。

当前用户主路径：

```bash
flowrt check examples/imu_demo/rsdl/robot.rsdl
flowrt prepare examples/imu_demo/rsdl/robot.rsdl
flowrt build examples/imu_demo/rsdl/robot.rsdl
flowrt inspect examples/imu_demo/flowrt/contract/contract.ir.json
flowrt check examples/import_demo/rsdl/robot.rsdl
flowrt run examples/import_demo/rsdl/robot.rsdl --process main
flowrt launch examples/import_demo/rsdl/robot.rsdl
flowrt build examples/cpp_counter_demo/rsdl/robot.rsdl
flowrt run examples/cpp_counter_demo/rsdl/robot.rsdl --process control
flowrt check examples/mixed_iox2_demo/rsdl/robot.rsdl
flowrt check examples/imu_demo_iox2/rsdl/robot.rsdl
flowrt check examples/profile_switch_demo/rsdl/robot.rsdl
flowrt run --profile iox2 examples/profile_switch_demo/rsdl/robot.rsdl
```

`prepare` / `build` / `run` 会从 `.rsdl` 文件推导应用根目录，并将 FlowRT 管理产物写入该项目可见的 `flowrt/` 目录。
`prepare` / `build` / `run` / `launch` 还支持 `--profile <name>`，用于显式选择某个 profile 并据此投影生成产物；`examples/profile_switch_demo` 展示了同一份 RSDL 在 `inproc` 与 `iox2` 之间切换的路径。
当 contract 只含 C++ 组件时，`flowrt build` / `flowrt run` 使用 CMake 构建或运行 FlowRT 管理的 C++ shell、app 和 ABI test target；C++ only contract 不应触发 Cargo app 路径。
当 C++ only contract 选择 `iox2` backend 时，生成的 CMake 工程会显式依赖 `iceoryx2-cxx 0.9.1` 并启用 C++ iox2 transport；没有安装该依赖时应由 CMake 明确失败，而不是静默退回 inproc。
当 contract 含 Rust 组件时，当前实现仍使用 Cargo 构建 FlowRT 管理的 Rust 应用；Rust 用户组件的免 Cargo 分发属于后续安装/打包设计。
当 contract 同时含 C++ 和 Rust 组件时，当前实现支持语言分离 process group 在 `iox2` backend 下通过 `flowrt launch` 分别启动 Rust app 和 C++ app；`flowrt run --process <name>` 可运行其中一个单语言 process group。同一 process group 内混合 C++/Rust 以及 mixed `inproc` 仍会明确拒绝。
`examples/import_demo` 展示了 `[package.imports]` 如何把 `types/`、`components/`、`graphs/`、`profiles/` 和 `targets/` 下的模块化 `.rsdl` 文件合并到同一个 Contract IR。
`examples/imu_demo` 当前是 mixed contract 示例，用于验证 C++/Rust 接口、消息和构建产物生成；它不会伪装成单进程 mixed runtime 已可运行。
`examples/imu_demo_iox2` 是 `imu_demo` 的语言分离 iox2 变体，用于验证同一个主 demo 的 Rust source、C++ controller 和 Rust monitor 可以通过 `flowrt launch` 分进程运行；构建或启动该示例需要本机安装匹配的 `iceoryx2-cxx`，并通过 `CMAKE_PREFIX_PATH` 暴露给生成的 CMake 工程。
`examples/cpp_counter_demo` 展示了 C++ only contract：用户只在 `src/cpp/` 实现组件和 `flowrt_user::build_app()`，`flowrt build` / `flowrt run` 会通过 CMake 构建并运行 FlowRT 管理的 C++ inproc shell 和应用。
`examples/mixed_iox2_demo` 展示 Rust source 与 C++ sink 分进程运行的 iox2 mixed contract；构建或启动该示例需要本机安装匹配的 `iceoryx2-cxx`，并通过 `CMAKE_PREFIX_PATH` 暴露给生成的 CMake 工程。

仓库开发者验证 FlowRT 自身时可以使用：

```bash
cargo run -p flowrt-cli -- --version
cargo run -p flowrt-cli -- check examples/imu_demo/rsdl/robot.rsdl
cargo test --manifest-path examples/imu_demo/flowrt/build/Cargo.toml
cargo test -p flowrt --features iox2 -- --nocapture
```
