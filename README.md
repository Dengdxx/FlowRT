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

当前 Rust 基建：

```bash
cargo run -p flowrt-cli -- check examples/imu_demo/rsdl/robot.rsdl
cargo run -p flowrt-cli -- prepare examples/imu_demo/rsdl/robot.rsdl
cargo run -p flowrt-cli -- build examples/imu_demo/rsdl/robot.rsdl
cargo run -p flowrt-cli -- run examples/imu_demo/rsdl/robot.rsdl
cargo run -p flowrt-cli -- run examples/imu_demo/rsdl/robot.rsdl --process main
cargo run -p flowrt-cli -- launch examples/imu_demo/rsdl/robot.rsdl
cargo test --manifest-path examples/imu_demo/flowrt/build/Cargo.toml
cargo test -p flowrt --features iox2 -- --nocapture
cargo run -p flowrt-cli -- inspect examples/imu_demo/flowrt/contract/contract.ir.json
```

`prepare` / `build` / `run` 会从 `.rsdl` 文件推导应用根目录，并将 FlowRT 管理产物写入该项目可见的 `flowrt/` 目录。
