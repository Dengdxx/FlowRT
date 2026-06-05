# FlowRT

FlowRT 是一个数据流编译型机器人运行时。用户用 `.rsdl` 声明消息、组件端口、任务、数据流连接、部署目标和通信 backend；工具链把这些声明归一化为 Contract IR，校验后生成 C++/Rust 的薄 runtime shell、消息类型、启动配置和构建文件。

FlowRT 的边界很明确：

```text
RSDL controls system structure.
Runtime controls execution.
User code controls algorithms.
```

这意味着 FlowRT 关注的是“系统结构能否被编译、校验和重新生成”，而不是把系统事实散落到运行时动态对象里。用户算法代码仍由用户自己写；FlowRT 负责把 RSDL 契约变成可构建、可运行、可验证的应用骨架。

## 当前状态

FlowRT 还处在 MVP 工具链和 runtime shell 打通阶段，适合做本机示例、Contract IR 语义验证、生成代码边界验证和 C++/Rust runtime 语义收敛。它还不是发布稳定的生产运行时。

当前已经可用的主路径：

- `flowrt check`：解析 RSDL、展开 imports、归一化 Contract IR 并运行 validator。
- `flowrt prepare`：生成 `flowrt/` 管理产物。
- `flowrt build`：生成并构建应用；需要 `launch` 时用 `--launcher` 显式构建 generated supervisor。
- `flowrt run`：读取已构建应用并运行单个 process group，不重新生成或构建。
- `flowrt launch`：读取已构建 supervisor 并启动全部 process group，不重新生成或构建。
- `flowrt inspect`：查看已落盘 Contract IR 摘要。
- `flowrt list` / `flowrt nodes`：从生成应用二进制或 `selfdesc.json` 读取静态自描述拓扑。
- `flowrt status`：扫描当前用户 runtime socket 并输出 live process handshake、scheduler tick 与 channel 摘要。
- `flowrt hz`：通过 live status 控制面按采样窗口统计 channel 发布频率，不启用 echo 数据面 probe。
- `flowrt echo`：从 live runtime 自动读取 self-description，按消息 layout 格式化单个 channel 的 latest 快照；也可用 `--image` 指定离线 self-description。
- `flowrt params`：结合静态自描述和 live runtime socket，列出、读取或提交 runtime 参数 pending 更新。

已覆盖的核心能力包括 RSDL v0.1 TOML 子集、模块化 imports、canonical Contract IR、profile 投影、acyclic dataflow 校验、Rust/C++ 消息与组件接口生成、ABI conformance 测试生成、Rust/C++ inproc runtime、latest/FIFO channel、overflow policy、stale data policy、task lifecycle、task deadline 检查，以及 language-separated mixed contract over `iox2` 的生成和启动边界。

当前 Message ABI v0.1 仍以 fixed-size plain data 作为 native ABI 基线；同时，FlowRT 已经把 `bytes<max=N>`、`string<max=N>` 和 `sequence<T,max=N>` 作为 bounded variable frame 进入 codegen/runtime，生成固定 header + 尾部变长区的 canonical frame codec。`inproc` 和 `zenoh` 直接传递 canonical frame；`iox2` 会为每个变长消息生成固定容量 transport slot，在 typed IPC payload 中承载 canonical frame bytes，用户组件接口仍只看到结构化消息。

当前仍未完成安装包和发布流程、多 graph / 多 task 完整语义、Artifact ABI、外部进程组件语义，以及 Rust 用户组件免 Cargo 分发。`zenoh` 已作为跨主机 copy backend 进入 capability catalog、Rust/C++ runtime backend、真实 transport endpoint 和 mixed demo 路径；生成物会输出 deterministic channel key expression，并对 bounded variable frame 生成 canonical codec。

Rust/C++ 生成的 runtime shell 会启动与 Rust wire JSON 兼容的 introspection socket，控制面常驻暴露 status、self-description、参数和 PID 命名 socket 路径；channel 数据面 probe 只在 `flowrt echo` 建立观察连接期间启用。无观察者时发布热路径只做 channel-local 原子检查，不编码、不拷贝、不写 socket。
组件参数现在有显式 schema 和更新策略：`startup` 参数只在启动时生效，`on_tick` 参数可以通过 `flowrt params set` 写入 pending 值，并由生成 shell 在 tick 边界调用用户组件的 `on_params_update` 后提交。
长期运行的生成应用会在收到 SIGINT/SIGTERM 时触发 runtime shutdown token，退出同步 tick loop 后继续执行 `shutdown` task、`on_stop` 和 `on_shutdown`。runtime introspection socket 会拒绝覆盖仍可连接的 live socket，并自动回收 SIGKILL 后残留且不可连接的 socket 文件。

## 快速体验

前置条件：

- Rust toolchain，支持 workspace 使用的 Rust 2024 Edition。
- C++20 编译器、CMake 和 CTest，用于 C++ runtime 与 C++ 示例。
- 可选：`iceoryx2-cxx 0.9.1`、基于 `zenoh-c` backend 的 `zenohcxx 1.9.0`。含 C++ `iox2` 组件的构建会先查找本机安装；`zenoh` 组件要求本机提供 `zenohcxx::zenohc` 目标，找不到时 configure 直接失败。

构建并安装单包 Debian 包：

```bash
scripts/package-deb.sh --output-dir dist
sudo dpkg -i dist/flowrt_*_*.deb
flowrt --version
```

该单包会安装 CLI、Rust runtime crate、C++ runtime header 和 CMake package。安装后用户项目不需要克隆 FlowRT 仓库；只需要自己的 `rsdl/`、`src/` 和可删除重建的 `flowrt/` 生成目录。

检查模块化 RSDL 示例：

```bash
flowrt check examples/import_demo/rsdl/robot.rsdl
```

运行 Rust-only inproc 示例：

```bash
flowrt build --launcher examples/import_demo/rsdl/robot.rsdl
flowrt run examples/import_demo/rsdl/robot.rsdl --process main
flowrt launch examples/import_demo/rsdl/robot.rsdl
```

构建并运行 C++ only inproc 示例：

```bash
flowrt build --launcher examples/cpp_counter_demo/rsdl/robot.rsdl
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

运行 `flowrt prepare` 或 `flowrt build` 后，FlowRT 会在项目下生成 `flowrt/` 管理目录：

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
| `examples/import_demo` | Rust | `inproc` | `flowrt build --launcher examples/import_demo/rsdl/robot.rsdl` | 验证 RSDL imports、Rust codegen、inproc run 和 launch |
| `examples/cpp_counter_demo` | C++ | `inproc` | `flowrt build --launcher examples/cpp_counter_demo/rsdl/robot.rsdl` | 验证 C++ only CMake app 路径 |
| `examples/imu_demo` | Rust + C++ | `inproc` build smoke | `flowrt build examples/imu_demo/rsdl/robot.rsdl` | 验证 mixed contract 的接口和生成物边界 |
| `examples/profile_switch_demo` | Rust | `inproc` / `iox2` | `flowrt build --profile iox2 examples/profile_switch_demo/rsdl/robot.rsdl` | 验证 profile 驱动 backend 切换 |
| `examples/mixed_iox2_demo` | Rust + C++ | `iox2` | `flowrt check examples/mixed_iox2_demo/rsdl/robot.rsdl` | 验证 Rust source 与 C++ sink 的 iox2 分进程 contract |
| `examples/imu_demo_iox2` | Rust + C++ | `iox2` | `flowrt check examples/imu_demo_iox2/rsdl/robot.rsdl` | 验证主 demo 的 language-separated iox2 变体 |
| `examples/variable_iox2_demo` | Rust + C++ | `iox2` | `flowrt build --launcher examples/variable_iox2_demo/rsdl/robot.rsdl` | 验证 bounded variable frame 经 iox2 fixed slot 跨语言传递 |
| `examples/mixed_zenoh_demo` | Rust + C++ | `zenoh` | `flowrt build --launcher examples/mixed_zenoh_demo/rsdl/robot.rsdl` | 验证 bounded variable frame、zenoh 跨主机 transport 和 mixed launch 路径 |

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
flowrt build --launcher examples/cpp_counter_demo/rsdl/robot.rsdl
flowrt run --run-ticks 5 examples/cpp_counter_demo/rsdl/robot.rsdl --process control
flowrt launch --run-ticks 5 examples/cpp_counter_demo/rsdl/robot.rsdl
flowrt build examples/imu_demo/rsdl/robot.rsdl
flowrt build --launcher examples/import_demo/rsdl/robot.rsdl
flowrt run --run-ticks 5 examples/import_demo/rsdl/robot.rsdl --process main
flowrt launch --run-ticks 5 examples/import_demo/rsdl/robot.rsdl
flowrt check examples/mixed_iox2_demo/rsdl/robot.rsdl
flowrt check examples/imu_demo_iox2/rsdl/robot.rsdl
flowrt check examples/profile_switch_demo/rsdl/robot.rsdl
flowrt build --profile iox2 examples/profile_switch_demo/rsdl/robot.rsdl
flowrt run --run-ticks 5 --profile iox2 examples/profile_switch_demo/rsdl/robot.rsdl
flowrt build --launcher examples/variable_iox2_demo/rsdl/robot.rsdl
rm -f /tmp/flowrt-variable-iox2-saw-packet
FLOWRT_TICK_SLEEP_MS=5 FLOWRT_VARIABLE_IOX2_SAW_PACKET_PATH=/tmp/flowrt-variable-iox2-saw-packet \
  flowrt launch --run-ticks 200 examples/variable_iox2_demo/rsdl/robot.rsdl
test -s /tmp/flowrt-variable-iox2-saw-packet
flowrt build --launcher examples/mixed_zenoh_demo/rsdl/robot.rsdl
FLOWRT_TICK_SLEEP_MS=5 flowrt launch --run-ticks 200 examples/mixed_zenoh_demo/rsdl/robot.rsdl
```

面向用户的文档和示例默认使用系统安装后的 `flowrt ...` 命令；`cargo run -p flowrt-cli -- ...` 只作为本仓库开发调试方式。
