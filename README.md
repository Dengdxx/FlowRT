# FlowRT

FlowRT 是一个数据流编译型机器人运行时。用户用 `.rsdl` 声明消息、组件端口、任务、数据流连接、部署目标和通信 backend；FlowRT 把这些声明归一化为 Contract IR，校验后生成 C++/Rust 的薄 runtime shell、消息类型、启动配置和构建文件。

FlowRT 的边界是：

```text
RSDL controls system structure.
Runtime controls execution.
User code controls algorithms.
```

它不是 ROS2 的复刻，也不是运行时动态发现 node/topic 的框架。FlowRT 的核心对象是可编译、可校验、可重新生成的数据流系统契约。

## 当前能力

仓库目前处于 MVP 工具链和 runtime shell 打通阶段，已经具备：

- RSDL v0.1：合法 TOML 子集，支持单文件 contract 和 `[package.imports]` 模块化片段导入。
- Contract IR：源文件归一化、stable ID、canonical JSON、profile 投影和单 graph 校验。
- Validator：命名、message ABI、port/type、task、bind、acyclic graph、target/backend 和 mixed runtime readiness 校验。
- Codegen：生成 Rust/C++ 消息、组件接口、runtime shell、launch manifest、Cargo/CMake 构建文件和 ABI conformance 测试。
- Runtime：Rust/C++ inproc 基础 runtime，latest/FIFO channel、overflow policy、stale data policy 和生命周期清理语义。
- iox2 路径：Rust typed pub/sub runtime 支持；C++ iox2 binding 在启用 `FLOWRT_HAS_ICEORYX2_CXX` 并链接 `iceoryx2-cxx 0.9.1` 时使用真实 transport。
- CLI：`check`、`prepare`、`build`、`run`、`launch`、`inspect`，支持 `--profile <name>` 和 `--process <name>`。
- Mixed contract：允许语言分离 process group 在 `iox2` backend 下运行；拒绝同一 process 内 C++/Rust 混合和 mixed `inproc` 跨进程组合。

尚未完成安装包、发布流程、Rust 用户组件免 Cargo 分发、多 graph / 多 task 完整语义，以及更多 backend。

## 快速体验

从源码安装本机 `flowrt` 命令：

```bash
cargo install --path crates/flowrt-cli --locked
flowrt --version
```

检查模块化 RSDL 示例：

```bash
flowrt check examples/import_demo/rsdl/robot.rsdl
```

运行 Rust-only inproc 示例：

```bash
flowrt run examples/import_demo/rsdl/robot.rsdl --process main
flowrt launch examples/import_demo/rsdl/robot.rsdl
```

构建并运行 C++ only inproc 示例：

```bash
flowrt build examples/cpp_counter_demo/rsdl/robot.rsdl
flowrt run examples/cpp_counter_demo/rsdl/robot.rsdl --process control
```

生成的 FlowRT 管理产物会写入示例项目下的 `flowrt/` 目录，例如：

```text
examples/import_demo/flowrt/
  contract/contract.ir.json
  build/
  launch/launch.json
  src/
```

`flowrt/` 是可删除、可重建的管理目录，不放用户算法代码。

更多步骤见 [快速开始](docs/getting-started.md)。

## RSDL 一眼看懂

`.rsdl` 使用 TOML 表面语法。下面是一个 C++ only 计数器 contract 的核心结构：

```toml
[package]
name = "cpp_counter_demo"
version = "0.1.0"
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

`component` 描述可复用组件类型，`instance` 把组件放进 graph，`instance.<name>.task` 描述执行单元，`bind.dataflow` 描述 typed channel。Contract IR v0.1 当前每个 instance 最多一个 task；`task.input` 声明的每个 active input 必须有且只有一条 incoming bind。

## 文档

- [docs/README.md](docs/README.md)：入库配套文档索引和文档边界。
- [docs/getting-started.md](docs/getting-started.md)：从安装到跑通示例的最短路径。
- [docs/cli.md](docs/cli.md)：`flowrt` 命令、选项和生成物说明。
- [docs/examples.md](docs/examples.md)：示例目录、backend、runtime 和运行要求矩阵。
- [docs/development.md](docs/development.md)：仓库开发、验证、文档维护和提交规则。

架构计划和规格草案保留在本地 `docs/` 下但不入库；面向用户和维护者的配套文档应随代码入库。

## 用户代码边界

FlowRT 生成代码只做 glue：

- 消息定义。
- 组件接口。
- runtime shell。
- backend 绑定。
- 启动配置。
- 构建文件。

用户算法代码放在示例或项目自己的 `src/` 目录中。重新运行 `flowrt prepare` / `flowrt build` 可以覆盖 `flowrt/` 管理产物，但不应覆盖用户代码。

C++ 用户组件通过生成接口和 `flowrt_user::build_app()` 注入。Rust 用户组件通过生成 trait 和用户模块接入。`kind = "native"` 和 `kind = "adapter"` 当前都通过生成接口接入；`kind = "external"` 是后续外部进程接入模型的保留语义，v0.1 validator 会明确拒绝。业务代码只依赖 FlowRT runtime API，不直接使用 iox2 publisher/subscriber API。

## 仓库结构

```text
crates/
  flowrt-cli/          # flowrt 命令入口
  flowrt-rsdl/         # RSDL parser 和源 AST
  flowrt-ir/           # Contract IR 模型、归一化和 profile 投影
  flowrt-validate/     # Contract IR validation passes
  flowrt-codegen/      # Rust/C++ runtime shell、接口、构建和 launch manifest 生成
  flowrt-conformance/  # message ABI conformance 期望生成
runtime/
  rust/                # Rust runtime core 和可选 iox2 backend
  cpp/                 # C++20 runtime core 和可选 iox2 测试
examples/              # RSDL 和用户组件示例
docs/                  # 入库配套文档；本地设计/规格草案按 .gitignore 排除
```

## 开发验证

常用检查：

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

C++ runtime：

```bash
cmake -S runtime/cpp -B build/cpp
cmake --build build/cpp
ctest --test-dir build/cpp --output-on-failure
```

FlowRT demo smoke：

```bash
cargo run -p flowrt-cli -- build examples/cpp_counter_demo/rsdl/robot.rsdl
cargo run -p flowrt-cli -- run examples/cpp_counter_demo/rsdl/robot.rsdl --process control
cargo run -p flowrt-cli -- run examples/import_demo/rsdl/robot.rsdl --process main
cargo run -p flowrt-cli -- launch examples/import_demo/rsdl/robot.rsdl
cargo run -p flowrt-cli -- check examples/mixed_iox2_demo/rsdl/robot.rsdl
cargo run -p flowrt-cli -- check examples/imu_demo_iox2/rsdl/robot.rsdl
```

面向用户的文档和示例默认使用安装后的 `flowrt ...` 命令；`cargo run -p flowrt-cli -- ...` 只作为本仓库开发调试方式。
