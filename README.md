# FlowRT

FlowRT 是一个数据流编译型机器人运行时。用户用 `.rsdl` 声明系统结构，FlowRT 将它归一化为 Contract IR，校验后生成 C++/Rust 的薄 runtime shell、消息类型、启动配置和构建文件。

FlowRT 的边界很明确：

```text
RSDL controls system structure.
Runtime controls execution.
User code controls algorithms.
```

它不是 ROS2 的复刻，也不是运行时动态发现 node/topic 的框架。FlowRT 的核心对象是可编译、可校验、可重新生成的数据流系统契约。

## 当前状态

仓库目前处于 MVP 工具链和 runtime shell 打通阶段，已经覆盖这些路径：

- RSDL v0.1：合法 TOML 子集，支持单文件和 `[package.imports]` 模块化片段导入。
- Contract IR：归一化、stable ID、canonical JSON、profile 投影和 v0.1 单 graph 校验。
- Validator：命名、消息 ABI、port/type、task、bind、acyclic graph、target/backend 和 mixed runtime readiness 校验。
- Codegen：生成 Rust/C++ 消息、组件接口、runtime shell、launch manifest、Cargo/CMake 构建文件和 ABI conformance 测试。
- Runtime：Rust/C++ inproc 基础 runtime，latest/FIFO channel、overflow policy、stale data policy 和生命周期清理语义。
- iox2：Rust typed pub/sub runtime 支持；C++ iox2 binding 在启用 `FLOWRT_HAS_ICEORYX2_CXX` 并链接 `iceoryx2-cxx 0.9.1` 时使用真实 transport。
- CLI：`check`、`prepare`、`build`、`run`、`launch`、`inspect`，以及 `--profile <name>` 和 `--process <name>`。
- Mixed contract：允许语言分离 process group 在 `iox2` backend 下运行；拒绝同一 process 内 C++/Rust 混合，以及 mixed `inproc` 跨进程组合。

仍在推进的内容包括安装包、发布流程、Rust 用户组件免 Cargo 分发、更多 backend，以及更完整的多 graph / 多 task 语义。

## 安装

当前推荐从源码安装本机 `flowrt` 命令：

```bash
cargo install --path crates/flowrt-cli --locked
flowrt --version
```

仓库开发时也可以直接运行：

```bash
cargo run -p flowrt-cli -- --version
```

面向用户的命令和文档应使用安装后的 `flowrt ...`。`cargo run -p flowrt-cli -- ...` 只作为本仓库开发调试方式。

## 快速开始

检查一个 RSDL contract：

```bash
flowrt check examples/import_demo/rsdl/robot.rsdl
```

生成 FlowRT 管理产物：

```bash
flowrt prepare examples/import_demo/rsdl/robot.rsdl
```

生成产物会写入示例项目下的 `flowrt/` 目录，例如：

```text
examples/import_demo/flowrt/
  contract/contract.ir.json
  build/
  launch/launch.json
  src/
```

`flowrt/` 是可丢弃、可重建的管理目录，不应放用户业务逻辑。

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

切换 profile：

```bash
flowrt check examples/profile_switch_demo/rsdl/robot.rsdl
flowrt run --profile iox2 examples/profile_switch_demo/rsdl/robot.rsdl
```

查看已生成的 Contract IR：

```bash
flowrt inspect examples/import_demo/flowrt/contract/contract.ir.json
```

## RSDL 最小示例

`.rsdl` 文件使用 TOML 表面语法。下面是一个 C++ only 计数器 contract：

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

`component` 描述可复用组件类型，`instance` 把组件放进 graph，`instance.<name>.task` 描述执行单元，`bind.dataflow` 描述 typed channel。`task.input` 中声明的每个 active input 必须有且只有一条 incoming bind。

## 示例目录

- `examples/import_demo`：模块化 RSDL package，通过 `[package.imports]` 导入 `types/`、`components/`、`graphs/`、`profiles/` 和 `targets/`。
- `examples/cpp_counter_demo`：C++ only inproc contract，验证 CMake app 路径和 `flowrt_user::build_app()` 用户工厂入口。
- `examples/imu_demo`：主 mixed contract 示例，用于验证 C++/Rust 接口、消息和构建产物生成；不伪装为 mixed inproc 可运行。
- `examples/profile_switch_demo`：同一份 RSDL 通过 `--profile` 在 `inproc` 与 `iox2` backend 之间切换。
- `examples/mixed_iox2_demo`：Rust source 与 C++ sink 通过 iox2 分进程连接的 mixed contract。
- `examples/imu_demo_iox2`：主 demo 的 iox2 分进程变体。

`mixed_iox2_demo` 和 `imu_demo_iox2` 的构建/启动需要本机安装匹配的 `iceoryx2-cxx 0.9.1`，并通过 `CMAKE_PREFIX_PATH` 暴露给生成的 CMake 工程。基础 CI 只对它们执行 `check`。

## 用户代码边界

FlowRT 生成代码只做 glue：

- 消息定义。
- 组件接口。
- runtime shell。
- backend 绑定。
- 启动配置。
- 构建文件。

用户算法代码放在示例或项目自己的 `src/` 目录中。重新运行 `flowrt prepare` / `flowrt build` 可以覆盖 `flowrt/` 管理产物，但不应覆盖用户代码。

C++ 用户组件通过生成接口和 `flowrt_user::build_app()` 注入。Rust 用户组件通过生成 trait 和用户模块接入。业务代码只依赖 FlowRT runtime API，不直接使用 iox2 publisher/subscriber API。

## CLI

```bash
flowrt check <path/to/robot.rsdl>
flowrt prepare <path/to/robot.rsdl> [--out-dir flowrt] [--profile <name>]
flowrt build <path/to/robot.rsdl> [--out-dir flowrt] [--profile <name>]
flowrt run <path/to/robot.rsdl> [--out-dir flowrt] [--profile <name>] [--process <name>]
flowrt launch <path/to/robot.rsdl> [--out-dir flowrt] [--profile <name>]
flowrt inspect <path/to/flowrt/contract/contract.ir.json>
```

规则：

- `prepare` / `build` / `run` / `launch` 会从 `.rsdl` 路径推导应用根目录，并默认写入该项目下的 `flowrt/`。
- `--profile <name>` 会先投影 Contract IR，只保留选定 profile 的 deployment 视图，再校验和生成。
- `--process <name>` 运行一个 RSDL process group。mixed iox2 contract 必须指定单语言 process，或者使用 `flowrt launch` 启动全部 process group。
- C++ only contract 的 `build` / `run` 走 CMake app 路径，不依赖 Cargo app。
- 含 Rust 组件的 contract 当前仍会触发 Cargo 构建生成应用。

## 仓库结构

```text
crates/
  flowrt-cli/          # flowrt 命令入口
  flowrt-rsdl/         # RSDL parser 和源 AST
  flowrt-ir/           # Contract IR 模型、归一化和 profile 投影
  flowrt-validate/     # Contract IR validation passes
  flowrt-codegen/      # Rust/C++ runtime shell、接口、构建和 launch manifest 生成
  flowrt-conformance/  # 消息 ABI conformance 期望生成
runtime/
  rust/                # Rust runtime core 和可选 iox2 backend
  cpp/                 # C++20 runtime core 和可选 iox2 测试
examples/              # RSDL 和用户组件示例
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

## 文档维护

- `README.md`、`CHANGELOG.md` 和面向用户/维护者的配套文档应随代码入库。
- 架构计划和规格草案在本地 `docs/` 下维护，默认不入库，例如 `docs/architecture-plan.md`、`docs/rsdl-v0.1.md`、`docs/contract-ir-v0.1.md`、`docs/message-abi-v0.1.md` 和 `docs/backend-contract.md`。
- 文档正文使用中文；代码标识符、命令、配置键、协议名和必要专有名词可以保留英文。
