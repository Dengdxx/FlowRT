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
flowrt run examples/imu_demo/rsdl/robot.rsdl
flowrt run examples/imu_demo/rsdl/robot.rsdl --process main
flowrt launch examples/imu_demo/rsdl/robot.rsdl
flowrt inspect examples/imu_demo/flowrt/contract/contract.ir.json
flowrt check examples/import_demo/rsdl/robot.rsdl
flowrt build examples/cpp_counter_demo/rsdl/robot.rsdl
flowrt run examples/cpp_counter_demo/rsdl/robot.rsdl --process control
```

`prepare` / `build` / `run` 会从 `.rsdl` 文件推导应用根目录，并将 FlowRT 管理产物写入该项目可见的 `flowrt/` 目录。
当 contract 只含 C++ 组件时，`flowrt build` / `flowrt run` 使用 CMake 构建或运行 FlowRT 管理的 C++ shell、app 和 ABI test target；C++ only contract 不应触发 Cargo app 路径。
当 contract 含 Rust 组件时，当前实现仍使用 Cargo 构建 FlowRT 管理的 Rust 应用；Rust 用户组件的免 Cargo 分发属于后续安装/打包设计。
`examples/import_demo` 展示了 `[package.imports]` 如何把 `types/`、`components/`、`profiles/` 和 `targets/` 下的模块化 `.rsdl` 文件合并到同一个 Contract IR。
`examples/cpp_counter_demo` 展示了 C++ only contract：用户只在 `src/cpp/` 实现组件和 `flowrt_user::build_app()`，`flowrt build` / `flowrt run` 会通过 CMake 构建并运行 FlowRT 管理的 C++ inproc shell 和应用。

仓库开发者验证 FlowRT 自身时可以使用：

```bash
cargo run -p flowrt-cli -- --version
cargo run -p flowrt-cli -- check examples/imu_demo/rsdl/robot.rsdl
cargo test --manifest-path examples/imu_demo/flowrt/build/Cargo.toml
cargo test -p flowrt --features iox2 -- --nocapture
```
