# FlowRT

FlowRT 是一个数据流编译型机器人运行时。用户用 `.rsdl` 声明消息、组件端口、任务、数据流连接、部署目标和通信 backend；工具链把这些声明归一化为 Contract IR，校验后生成 C++/Rust 的薄 runtime shell、消息类型、启动配置和构建文件。

FlowRT 的边界很明确：

```text
RSDL controls system structure.
Runtime controls execution.
User code controls algorithms.
```

这意味着 FlowRT 关注的是“系统结构能否被编译、校验和重新生成”，不是运行时动态发现 node/topic 的框架，也不是 ROS2 的复刻。用户算法代码仍由用户自己写；FlowRT 负责把 RSDL 契约变成可构建、可运行、可验证的应用骨架。

## 当前状态

FlowRT 还处在 MVP 工具链和 runtime shell 打通阶段，适合做本机示例、Contract IR 语义验证、生成代码边界验证和 C++/Rust runtime 语义收敛。它还不是发布稳定的生产运行时。

当前已经可用的主路径：

- `flowrt check`：解析 RSDL、展开 imports、归一化 Contract IR 并运行 validator。
- `flowrt prepare`：生成 `flowrt/` 管理产物。
- `flowrt build`：构建生成应用；C++ only contract 走 CMake app 路径。
- `flowrt run`：运行单个 process group。
- `flowrt launch`：通过生成 supervisor 启动全部 process group。
- `flowrt inspect`：查看已落盘 Contract IR 摘要。
- `flowrt list` / `flowrt nodes`：从生成应用二进制或 `selfdesc.json` 读取静态自描述拓扑。
- `flowrt status`：扫描当前用户 runtime socket 并输出 live process handshake、scheduler tick 与 channel 摘要。
- `flowrt echo`：结合静态自描述和 live runtime socket，读取单个 channel 的 latest raw ABI bytes 快照。

已覆盖的核心能力包括 RSDL v0.1 TOML 子集、模块化 imports、canonical Contract IR、profile 投影、acyclic dataflow 校验、Rust/C++ 消息与组件接口生成、ABI conformance 测试生成、Rust/C++ inproc runtime、latest/FIFO channel、overflow policy、stale data policy、task lifecycle、task deadline 检查，以及 language-separated mixed contract over `iox2` 的生成和启动边界。

当前 Message ABI v0.1 仍只支持 fixed-size plain data。RSDL type expression 已能结构化表示未来 `bytes<max=N>`、`string<max=N>` 和 `sequence<T,max=N>`，但 validator 会明确拒绝这些字段并提示需要未来 Variable Frame ABI；codegen、runtime 和 conformance 不会假装支持 variable payload。

当前仍未完成安装包和发布流程、多 graph / 多 task 完整语义、可运行的跨机器通信、variable payload runtime、Artifact ABI、外部进程组件语义，以及 Rust 用户组件免 Cargo 分发。`zenoh` 已作为跨主机 copy backend 进入 capability catalog 和 Rust/C++ runtime backend 骨架，生成物会输出 deterministic channel key expression；Rust/C++ message codegen 已开始生成无 padding 的 canonical wire codec，但真实 transport endpoint 和跨机示例还在后续实现中。

Rust/C++ 生成的 runtime shell 会启动与 Rust wire JSON 兼容的 introspection socket，暴露 status、channel snapshot、结构化错误响应和 PID 命名 socket 路径，并在成功发布 channel 后写入 latest raw ABI snapshot。

## 快速体验

前置条件：

- Rust toolchain，支持 workspace 使用的 Rust 2024 Edition。
- C++20 编译器、CMake 和 CTest，用于 C++ runtime 与 C++ 示例。
- 可选：`iceoryx2-cxx 0.9.1`，仅在构建或运行含 C++ `iox2` 组件的示例时需要。

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
flowrt launch examples/cpp_counter_demo/rsdl/robot.rsdl
```

更多步骤见 [快速开始](docs/getting-started.md)。

## 用户写什么

RSDL v0.1 使用 TOML 表面语法。一个最小 contract 会描述 package、message type、component、instance、task、dataflow bind、profile 和 target：

```toml
[package]
name = "counter_demo"
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

`component` 是可复用组件类型，`instance` 是 graph 中的实例，`instance.<name>.task` 是执行单元，`bind.dataflow` 是 typed channel。Contract IR v0.1 当前要求恰好一个 graph，且 dataflow graph 必须无环。

## FlowRT 生成什么

运行 `flowrt prepare` / `flowrt build` / `flowrt run` 后，FlowRT 会在项目下生成 `flowrt/` 管理目录：

```text
flowrt/
  contract/contract.ir.json
  launch/launch.json
  build/
  src/
```

这个目录可以删除、可以重新生成，不应放用户算法代码。用户代码应放在项目自己的 `src/` 目录中：

- C++ 用户组件通过生成接口和 `flowrt_user::build_app()` 注入。
- Rust 用户组件通过生成 trait 和用户模块接入。
- 业务代码只依赖 FlowRT runtime API，不直接依赖 iox2 publisher/subscriber API。

生成代码只做 glue：消息定义、组件接口、runtime shell、backend 绑定、启动配置和构建文件。

## 示例

| 示例 | Runtime | Backend | 推荐命令 | 用途 |
| --- | --- | --- | --- | --- |
| `examples/import_demo` | Rust | `inproc` | `flowrt run examples/import_demo/rsdl/robot.rsdl --process main` | 验证 RSDL imports、Rust codegen、inproc run 和 launch |
| `examples/cpp_counter_demo` | C++ | `inproc` | `flowrt run examples/cpp_counter_demo/rsdl/robot.rsdl --process control` | 验证 C++ only CMake app 路径 |
| `examples/imu_demo` | Rust + C++ | `inproc` build smoke | `flowrt build examples/imu_demo/rsdl/robot.rsdl` | 验证 mixed contract 的接口和生成物边界 |
| `examples/profile_switch_demo` | Rust | `inproc` / `iox2` | `flowrt run --profile iox2 examples/profile_switch_demo/rsdl/robot.rsdl` | 验证 profile 驱动 backend 切换 |
| `examples/mixed_iox2_demo` | Rust + C++ | `iox2` | `flowrt check examples/mixed_iox2_demo/rsdl/robot.rsdl` | 验证 Rust source 与 C++ sink 的 iox2 分进程 contract |
| `examples/imu_demo_iox2` | Rust + C++ | `iox2` | `flowrt check examples/imu_demo_iox2/rsdl/robot.rsdl` | 验证主 demo 的 language-separated iox2 变体 |

完整示例说明见 [示例矩阵](docs/examples.md)。

## 文档边界

配套文档需要随代码入库；只有本地架构计划和语义规格草案不入库。

已入库的配套文档：

- [docs/README.md](docs/README.md)：文档索引和入库边界。
- [docs/getting-started.md](docs/getting-started.md)：从安装到跑通示例的最短路径。
- [docs/cli.md](docs/cli.md)：`flowrt` 命令、参数和生成物说明。
- [docs/examples.md](docs/examples.md)：示例目录、backend、runtime 和运行要求矩阵。
- [docs/development.md](docs/development.md)：开发验证、文档维护和提交规则。

默认不入库的本地设计和规格草案：

- `docs/architecture-plan.md`
- `docs/rsdl-v0.1.md`
- `docs/contract-ir-v0.1.md`
- `docs/message-abi-v0.1.md`
- `docs/backend-contract.md`
- `docs/project-layout.md`

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
  cpp/                 # C++20 runtime core 和可选 iox2 binding
examples/              # RSDL 和用户组件示例
docs/                  # 入库配套文档；本地设计/规格草案按 .gitignore 排除
```

## 开发验证

Rust workspace：

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
