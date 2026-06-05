# AGENTS.md

本文档为参与 FlowRT 仓库工作的 coding agent 提供项目专用约束。

## 项目概览

FlowRT 是一个数据流编译型机器人运行时。用户用 `.rsdl` 声明消息、组件端口、任务周期、数据流连接、部署目标和通信后端；工具链将 RSDL 编译为 Contract IR，完成校验后生成 C++/Rust 薄 runtime shell、消息类型、启动配置和构建文件。

FlowRT 的核心原则是：

```text
RSDL controls system structure.
Runtime controls execution.
User code controls algorithms.
```

FlowRT 的核心对象是可编译、可校验、可生成的数据流系统契约，而不是运行时动态拼装的通信对象集合。

当前阶段仍需要持续维护架构计划、规格草案和入库配套文档。架构计划和规格草案放在本地 `docs/` 目录但不纳入 Git；面向用户或维护者的配套文档应放在 `docs/` 下并随代码入库。

## 当前仓库状态

仓库目前处于 v0.1 alpha 阶段，已经形成工具链、Rust/C++ runtime shell、跨进程 backend 和安装包闭环。主要目录包括：

```text
Cargo.toml
Cargo.lock
README.md
CHANGELOG.md
crates/
  flowrt-cli/
  flowrt-rsdl/
  flowrt-ir/
  flowrt-validate/
  flowrt-codegen/
  flowrt-conformance/
runtime/
  cpp/
  rust/
examples/
  import_demo/rsdl/robot.rsdl
  import_demo/rsdl/graphs/default.rsdl
  imu_demo/rsdl/robot.rsdl
  imu_demo/src/rust/mod.rs
  imu_demo_iox2/rsdl/robot.rsdl
  imu_demo_iox2/src/rust/mod.rs
  imu_demo_iox2/src/cpp/components.cpp
  profile_switch_demo/rsdl/robot.rsdl
  profile_switch_demo/src/rust/mod.rs
  cpp_counter_demo/rsdl/robot.rsdl
  cpp_counter_demo/src/cpp/components.cpp
  mixed_iox2_demo/rsdl/robot.rsdl
  mixed_iox2_demo/src/rust/mod.rs
  mixed_iox2_demo/src/cpp/components.cpp
  variable_iox2_demo/rsdl/robot.rsdl
  variable_iox2_demo/src/rust/mod.rs
  variable_iox2_demo/src/cpp/components.cpp
  mixed_zenoh_demo/rsdl/robot.rsdl
  mixed_zenoh_demo/src/rust/mod.rs
  mixed_zenoh_demo/src/cpp/components.cpp
docs/
scripts/
AGENTS.md
```

当前已实现 Rust CLI 的 `check`、`prepare`、`build`、`run`、`launch`、`inspect`、`list`、`nodes`、`status`、`hz`、`echo` 和 `params` 基础闭环，安装后的 binary 名称为 `flowrt`。FlowRT 作为标准 Linux 应用分发：单个 deb 包同时安装 CLI、Rust runtime crate、C++ runtime header、CMake package 和基础文档。仓库内可以用 `cargo run -p flowrt-cli -- ...` 调试 CLI，但面向用户的文档、示例和最终回复应默认使用安装后的 `flowrt ...` 命令。
`prepare` / `build` / `run` / `launch` 还支持 `--profile <name>`，用于显式选择 profile 并按该 profile 生成产物；省略参数时会先投影到 `default` profile 或首个 profile，再校验和生成。profile 投影必须重算来自 profile default 的 bind-level `overflow`、`stale_policy` 和 `max_age_ms`，但不能覆盖 bind 上显式声明的 policy。RSDL 未声明任何 profile 时，normalization 会插入隐式 `default` profile，backend 为 `inproc`，使后续 deployment 校验和 codegen 共享同一默认语义。

当前已接入 Contract IR 驱动的 Rust/C++ message ABI conformance 测试生成。生成的 Rust/C++ ABI 测试使用同一份 IR-derived expected byte fixtures，覆盖 size、alignment、field offset、byte-level roundtrip 和跨语言 field value equivalence。C++ only contract 已能生成 inproc runtime shell，支持 `App` 注入接口、生命周期调度、latest/FIFO channel 转发、Contract IR 驱动的 process group 分发、bind-level stale freshness 策略和 `flowrt_user::build_app()` 用户工厂入口；`flowrt build` / `flowrt run` 对 C++ only contract 走 CMake app 路径，`examples/cpp_counter_demo` 用于验证只写 C++ 用户逻辑的构建和运行路径。

C++ runtime 已用 `std::chrono::milliseconds` 建模 `StaleConfig` 时间窗口，并提供 `LatestChannel<T>::publish_at` / `view_at`。Rust runtime 已有 feature-gated `iceoryx2` 0.9.x typed pub/sub endpoint、初始 `Iox2ChannelConfig` QoS 映射，以及 `StaleConfig`/`StalePolicy` freshness 语义；Rust codegen 可在 profile 选择 `iox2` 时生成 `Iox2PubSub<T>` channel shell，并传入 bind-level latest/FIFO depth、overflow policy 和 stale intent。
Rust/C++ generated runtime shell 会把 task-level `deadline_ms` 转成 `timing:deadline_aware` required capability，并在用户 `on_tick` 返回 `Ok` 后、发布输出前检查 elapsed time；超出 deadline 时返回 `Error`。

profile 选择 `iox2` 时，Rust message codegen 必须生成 `#[type_name("TypeName")]`，C++ message codegen 必须生成同名 `IOX2_TYPE_NAME`；iox2 runtime timestamp 使用两端同名的 `FlowRTIox2Header` user header，payload 必须保持业务消息 `T`，不要重新引入 `Iox2Stamped<T>` 这类 payload envelope。C++ iox2 shell 会使用 `flowrt::iox2::Iox2PubSub<T>`、canonical service name、bind-level channel/stale config、`receive_latest_at` 和 `publish_at`；runtime 中的 C++ `Iox2PubSub<T>` 在定义 `FLOWRT_HAS_ICEORYX2_CXX` 并链接 `iceoryx2-cxx` 时会绑定真实 publisher/subscriber，默认未启用宏时仍安全返回 `ChannelError::Transport`。

Mixed contract 必须保持语言边界诚实：Rust codegen 不得为 C++ component 伪造 Rust trait，C++ codegen 不得为 Rust component 伪造 C++ interface。语言分离 process group 在 `iox2` 或 `zenoh` backend 下可以通过 `flowrt launch` 由 supervisor 分别启动 Rust app 和 C++ app；`flowrt run --process <name>` 可以运行其中一个单语言 process group。仍然必须拒绝同一 RSDL process group 内混合 C++/Rust，以及 selected backend 为 `inproc` 的跨语言 process boundary。`examples/imu_demo_iox2` 用于验证主 demo 的 Rust source、C++ controller 和 Rust monitor 通过 iox2 分进程运行，`examples/mixed_iox2_demo` 用于验证 Rust source 与 C++ sink 通过 iox2 分进程连接，`examples/mixed_zenoh_demo` 用于验证跨主机 copy backend、bounded variable frame 和 mixed launch 路径。

`flowrt/launch/launch.json` 的 process group 必须包含 `runtimes` 和 `runtime_kind`，graph instance 必须包含 `runtime`，graph 必须包含 `channels`，每条 channel 在 `iox2` backend 下必须暴露 canonical service name；生成的 Rust supervisor 会遍历 manifest 中的全部 graph，并读取 `runtime_kind`，为 Rust process 选择 Rust app executable、为 C++ process 选择 C++ app executable，同时继续拒绝 mixed process group。generated supervisor 还会启动 `runtime=supervisor` 的 introspection socket，轮询子进程 PID socket，把子进程 heartbeat、tick stale、exit 和 restart 状态交给 `flowrt status` 展示；当前内置 `on-failure` policy 对异常退出最多重启 3 次，正常退出不重启。默认构建仍走轻量 inproc 路径。不要提前引入大型依赖、复杂目录或半成品 runtime 代码。

当前已存在 `.github/workflows/ci.yml` CI：Linux 上分 job 运行 `guard-generated`、`rust-fmt`、`rust-test`、`rust-clippy`、`cpp-runtime`、`package`、`cpp-zenoh-runtime`、`demo-smoke` 和 `release`。CI 会上传 `flowrt-linux-deb` artifact；deb 包把 CLI、Rust runtime crate、C++ runtime package、Rust crate vendor、`iceoryx2-cxx 0.9.1`、`zenoh-c 1.9.0` 和 `zenoh-cpp 1.9.0` 安装到 `/opt/flowrt/<version>` 私有前缀，并通过 `/usr/bin/flowrt` 暴露入口。demo smoke 会先安装 deb，再用安装后的 `flowrt ...` 跑示例；C++ only demo 执行 build/run/launch，mixed `imu_demo` 只执行 build，Rust-only `import_demo` 执行 run/launch，`mixed_iox2_demo` 与 `imu_demo_iox2` 执行 check，`profile_switch_demo` 执行 profile 切换 smoke，`variable_iox2_demo` 执行 C++ iox2 mixed launch 和 bounded variable frame marker 验证，`mixed_zenoh_demo` 执行 zenoh build/launch smoke。`cpp-zenoh-runtime` 也使用 deb 包内私有 zenoh SDK，不再单独源码构建 zenoh 依赖。推送 `v*` tag 且全部 gate 成功后，release job 会下载 deb artifact、从 `CHANGELOG.md` 对应版本段抽取 release notes，并创建 GitHub Release 上传 `.deb` 与 `SHA256SUMS`；tag 版本必须匹配 `Cargo.toml`。该 workflow 暂不做 cache 或多平台矩阵；生成 CMake 不得通过 `FetchContent` 联网拉取 backend SDK，缺失依赖应要求安装 FlowRT 包或显式设置 `FLOWRT_CPP_RUNTIME_DIR` / `CMAKE_PREFIX_PATH`。

## 设计原则

1. **契约优先**：RSDL 是源语言，Contract IR 是 normalized 后的语义合同。工具链、validator、codegen 和 runtime 都应面向 Contract IR，而不是直接依赖 RSDL 文本结构。
2. **FlowRT 管理产物可丢弃**：`flowrt/` 下的 FlowRT 管理产物必须可删除、可重建，不得承载用户业务逻辑。
3. **用户代码隔离**：用户算法代码通过接口、trait、factory 或 adapter 接入，不写进生成文件。
4. **Backend 隔离**：业务层只依赖 FlowRT runtime API，不直接依赖 iox2。iox2 是初代 backend，不是系统语义本身。
5. **C++/Rust 语义一致**：C++ runtime 和 Rust runtime 可以独立实现，但必须共享 RSDL 语义、Contract IR、消息 ABI 和 conformance tests。
6. **低学习成本**：普通用户只应关心组件输入、输出和算法实现。通信、调度、生命周期和部署由系统生成或管理。
7. **MVP 是长期模型的子集**：可以先做单 graph、单 task 和简单触发模式，但 IR、validator、codegen 与本地设计文档要预留模块化 package、backend capability、bounded variable frame 和跨语言一致性。

## 非目标

- 不把通信库 API 暴露为 FlowRT 用户语义；用户算法代码不直接使用 iox2 或 zenoh publisher/subscriber API。
- 不支持无界动态 string/vector、map 或递归消息结构；当前只支持带 `max` 上界的 bounded variable frame。
- 不在未定语义前堆砌 runtime 代码。

## 目录规划

长期建议目录如下，实际创建应随实现阶段逐步推进：

```text
crates/
  flowrt-cli/          # CLI：check / prepare / build / run / launch / inspect / list / status / hz / echo / params
  flowrt-rsdl/         # RSDL parser 和 AST
  flowrt-ir/           # Contract IR 模型和 canonical JSON
  flowrt-validate/     # 校验 passes
  flowrt-codegen/      # C++/Rust 代码生成器
runtime/
  cpp/                 # C++ runtime core 和 backends
  rust/                # Rust runtime core 和 backends
examples/
  imu_demo/            # imu_sim -> estimator -> controller -> monitor
  imu_demo_iox2/       # imu_demo 的语言分离 iox2 运行变体
  mixed_zenoh_demo/    # mixed Rust/C++ zenoh backend 运行变体
```

不要一次性创建空目录树。只有当对应内容或测试存在时再落目录。

## RSDL 约定

- RSDL 源文件使用 `.rsdl` 后缀。
- RSDL v0.1 语法是 TOML 子集，文件扩展名仍为 `.rsdl`。
- 不要写“像 TOML 但不是 TOML”的含糊语法；v0.1 `.rsdl` 文件应能被标准 TOML parser 解析。
- TOML 只是初代语法载体，不是 Contract IR 或长期语义模型。
- MVP 可以支持单文件 RSDL。
- 长期语义模型必须区分 `types`、`components`、`graphs`、`profiles`、`targets` 和 `package`。
- RSDL 不应出现 backend 的底层 API 名称，例如 iox2 publisher/subscriber。应描述 channel、policy、target 和 capability requirement。
- MVP 单文件示例建议使用 `[type.*]`、`[component.*]`、`[instance.*]`、`[instance.*.task]`、`[[bind.dataflow]]`、`[profile.*]`、`[target.*]` 这些表。
- RSDL parser 必须拒绝未知顶层 section 和固定 schema 表中的未知字段，例如 `[components.*]`、`instance.foo.proces` 这类拼写错误不能被静默忽略；`type.<Name>` 的 message fields 与 `params` 表是开放键空间。
- `instance.<name>.task` 是 v0.1 推荐写法。
- `instance.<name>.params` 覆盖 component 默认参数，并在 normalization 阶段合并；override 不能引入 component 未声明参数，类型必须与默认值兼容，空默认数组不能被非空数组覆盖。
- `instance.<name>.process` 是 launch grouping label，必须使用 `snake_case`，且不得使用大小写不敏感的保留 `flowrt` 前缀；未声明时默认为 `main`。
- `bind.dataflow` 表示图连线，归一化后展开成具体 channel edges。
- Contract IR v0.1 必须恰好包含一个 graph，且至少包含一个 profile；顶层 `graphs` 保留为数组是为了后续扩展，validator 和 codegen 当前必须拒绝 0 graph 和多 graph contract，空 profiles 也必须被拒绝。
- v0.1 dataflow graph 必须无环；instance 间闭环和 self-loop 都必须由 validator 拒绝。反馈环、delay、初始值或时间同步窗口必须在后续 RSDL/IR 中显式建模后才能支持。

## Contract IR 约定

Contract IR 的真实模型应是 Rust 中的强类型 `struct/enum`。落盘格式使用 canonical JSON：

```text
flowrt/contract/contract.ir.json
```

IR 生成前必须归一化：

- 展开 imports。
- 解析 names。
- 分配 stable IDs。
- 填充 defaults。
- 规范化 ordering；不具备顺序语义的实体数组、参数数组、deployment 数组、`package.imports` kind/pattern、`bind.dataflow`、`target.runtime` 和 `target.backends` 必须稳定排序，不能让声明顺序改变 canonical Contract IR。`bind.dataflow` 按端点排序；deployment 按 graph/profile/target 排序；排序不得吞掉重复项，重复声明仍由 parser 或 validator 拒绝。validator 必须拒绝落盘 IR 中非 canonical 的这些集合顺序、重复的 `package.imports` kind/pattern，以及未知 import kind。
- 附加 backend capabilities。

Contract IR validator 必须独立拒绝同一作用域内的重复实体名称，包括顶层 type、component、profile、target、graph，以及 graph 内 instance；不能只依赖 RSDL parser 的 map 结构保证唯一性。`EntityId` 也必须在同一 Contract IR 内保持全局唯一，且所有 `EntityRef` 的 `id` 和 `name` 必须同时指向同一个实体，不能让手工改写或旧工具产物造成引用歧义。

IR 文档必须内建版本字段，例如：

```text
ir_version
schema_version
source_hash
package_id
```

validator 必须拒绝当前工具链不支持的 `ir_version`、`schema_version` 和 package `rsdl_version`，避免旧版或未来版本的契约被现有 codegen/runtime 静默误读。
`source_hash` 必须保持 64 位小写 hex digest；`EntityId` 必须保持当前生成器使用的 `<kind>_<16hex>` canonical 形状。

不要把 RSDL 原文直接转换成一份浅层 JSON 就称作 IR。

## 执行模型约定

长期模型中，`component`、`instance` 和 `task` 要分开：

- `component` 是可复用组件类型，描述端口、参数、生命周期和语言绑定。
- `instance` 是 graph 中的组件实例，绑定 component、参数、端口名、部署目标和执行任务。
- `task` 是 instance 的执行单元，描述触发方式、周期、deadline、输入读取策略、输出写入策略和部署目标。

- 最小生命周期接口保留 `on_init`、`on_start`、`on_stop`、`on_shutdown`。生成的 Rust/C++ runtime shell 只对成功进入对应阶段的组件执行逆序清理：成功 start 的组件执行 `on_stop`，成功 init 的组件执行 `on_shutdown`；scheduler 或前序 hook 失败后仍必须继续清理。原始非 `Ok` 状态优先，原始状态为 `Ok` 时任一清理 hook 失败统一返回 `Error`。

初代可以简化为 `instance ~= task`，但 IR 中必须保留 task 概念。Contract IR v0.1 当前每个 instance 最多只能有一个 task；validator 必须拒绝同一 instance 的多 task，避免 codegen 静默只消费第一条 task。

优先支持：

```text
periodic
on_message
startup
shutdown
```

多输入默认语义为 `latest snapshot`。codegen 必须在用户接口中表达该语义，例如 C++ 使用 `Latest<T>` view，Rust 使用 `Latest<'_, T>` view，并暴露 present/stale 信息。task 的 `input` / `output` 是端口集合语义，同一列表不得重复端口；task 声明的每个输入端口必须有且只有一条 incoming dataflow bind；缺失或多重绑定都必须由 validator 拒绝，不能让 codegen 隐式传空视图或 panic。v0.1 同步 tick shell 中，`periodic` task 每 tick 调用且必须声明大于 0 的 `period_ms`；`on_message` task 只有声明输入中至少一个 `Latest::present()` / `Latest<T>::present()` 为真时才调用；`startup` task 在组件成功 start 后、scheduler 前调用一次；`shutdown` task 在 scheduler 正常返回、显式 `--run-ticks` 到达或 SIGINT/SIGTERM 触发 graceful shutdown 后、组件 stop 前调用一次。非 `periodic` task 不得声明 `period_ms`，避免无效周期字段被 runtime shell 忽略。后续可扩展显式 `all_ready`、`any_ready`、时间同步窗口和 stale-data policy。

v0.1 生成 shell 使用同步拓扑 tick，因此 codegen 可以假设已经通过 validator 的 graph 是 acyclic。不要在 codegen 中把环路隐式解释成反馈、延迟或跨 tick 状态。

## 通道语义约定

RSDL 描述 typed data channel。

基础 channel policy：

```text
latest(depth = 1)
fifo(depth = N)
```

`latest` 只能保留当前 snapshot；`depth` 只能省略或为 `1`。如果用户需要 backlog，必须显式使用 `fifo(depth = N)`。validator 必须拒绝 `latest` 上的 `depth > 1`，避免 codegen 静默忽略不支持的策略。

overflow policy 必须显式：

```text
drop_oldest
drop_newest
error
block
```

实时路径默认避免无界阻塞。优先使用 `drop_oldest` 或 `error`。

stale data policy 也必须建模：

```text
max_age_ms = N
stale_policy = "warn" | "drop" | "hold_last" | "error"
```

overflow 表示队列满，stale 表示数据过期；两者不能混为一个策略。

## 消息 ABI 约定

FlowRT Message ABI v0.1 的 native ABI 基线支持 fixed-size plain data：

- integers
- floats
- bool
- fixed arrays with `N > 0`
- nested structs

同时已支持带最大长度上界的 bounded variable frame：

- `bytes<max=N>`
- `string<max=N>`
- `sequence<T,max=N>`

bounded variable frame 使用固定 header + 尾部变长区的 canonical frame codec；`inproc` 和 `zenoh` 传递 canonical frame，`iox2` 通过 codegen 生成的固定容量 transport slot 承载 frame bytes。

暂不支持：

- unbounded dynamic strings
- unbounded dynamic vectors
- maps
- recursive structures
- language-specific ownership types
- empty message structs

C++/Rust 生成类型必须通过 conformance tests 验证：

- size
- alignment
- field offset
- byte-level roundtrip
- default initialization
- Contract IR-derived expected byte fixtures，用于证明 Rust/C++ sample field value 的跨语言等价性

## Backend 约定

初代 backend 规划：

```text
inproc  # tests, CI, single-process demos
iox2    # local multi-process high-performance dataflow
zenoh   # multi-host copy transport dataflow
```

长期 backend 方向：

```text
serial
CAN
MCU static backend
```

backend capability 应被显式建模。validator 必须拒绝未知 backend 名称，以及 selected backend 无法满足的 contract。未知 backend 不得因为没有被当前 profile 选中而在 `target.<name>.backends` 中静默保留。
`ChannelEdgeIr.policy_source`、`ChannelEdgeIr.capability_requirements`、`TargetIr.capabilities`、`DeploymentIr.required_capabilities` 和 `DeploymentIr.satisfied` 都是由 channel policy、backend catalog、graph 和 profile/target 组合派生或记录的字段；validator 必须重新推导并拒绝不一致值，能力列表顺序也必须与推导结果一致，不能信任落盘 IR 中可被手工改写的派生元数据。`ComponentIr.params` 和 `InstanceIr.params` 也必须保持参数名唯一、实例参数集合与 component 默认参数集合一致，且实例覆盖值必须与 component 默认值类型兼容。`TargetIr.runtime` 和 `TargetIr.backends` 也必须保持去重。声明 target 时，每个 graph/profile/target 组合必须恰好有一条 deployment，缺失或重复行都必须被拒绝。

## Codegen 边界

生成代码只能做 glue：

- 消息定义。
- 组件接口。
- runtime shell 入口。
- backend 绑定。
- 启动配置。
- 构建文件。

Codegen public 入口必须重新运行 Contract IR validator；即使调用方绕过 CLI，也不得让未验证或手工改坏的 IR 进入生成阶段后触发 panic 或写出半成品。

用户代码放在 `src/` 或示例项目自己的用户代码目录。生成器不得覆盖用户代码。

组件接入类型必须显式区分：

```text
  native component            用户直接实现 FlowRT 生成接口
  adapter component           用户把已有 C++/Rust 代码包装成 FlowRT 接口
  external process component  FlowRT 启动或连接已有外部进程
```

这用于长期接入已有控制器、Python 脚本、串口程序和其他 legacy code。v0.1 generated runtime shell 当前只支持 `native` 和 `adapter` 通过生成接口接入；`external` 是保留语义，必须由 validator 明确拒绝，直到外部进程生命周期、端口绑定和错误传播语义落地。

C++ 推荐形态：

```cpp
class ControllerInterface {
public:
  virtual ~ControllerInterface() = default;
  virtual flowrt::Status on_odom(
      const Odom& odom,
      flowrt::Output<MotorCmd>& cmd) = 0;
};
```

Rust 推荐形态：

```rust
pub trait Controller {
    fn on_odom(
        &mut self,
        odom: &Odom,
        cmd: &mut Output<MotorCmd>,
    ) -> flowrt::Status;
}
```

## 技术选型

- 基础选型：C++20 + Rust 2024 Edition。
- 选择较新的语言版本不是摆设；实现 runtime API 和工具链时应优先使用能提升语义清晰度和安全边界的现代特性，例如 C++20 的 `std::chrono`、`std::span`、`std::optional`、强枚举和 concept-ready 接口边界，以及 Rust 2024 Edition 下清晰的 `enum` / `struct` / trait / ownership 表达。
- 工具链和开发者工具：全部使用 Rust。
- FlowRT 是安装式工具链；用户入口是预编译或本机安装后的 `flowrt` 命令，不是 `cargo run -p flowrt-cli -- ...`。
- FlowRT 的标准 Linux 分发当前采用单个 deb 包，安装 CLI、Rust runtime crate、C++ runtime header、CMake package 和基础文档；用户项目不应要求克隆 FlowRT 仓库。
- C++ 只承载 runtime core、backend bindings、FlowRT 管理的 runtime shells 和用户侧组件 API。
- C++ build：优先使用 CMake。
- Rust build：使用 Cargo workspace。
- 生成的混合工程由 `flowrt` CLI 统一调度。
- C++ only contract 的 `flowrt build` / `flowrt run` 不得依赖 Cargo app 路径，必须走 CMake 路径，以支持只写 C++ 业务逻辑且未安装 Rust/Cargo 的用户。
- 含 Rust 用户组件的 contract 当前仍会触发 Cargo 构建；后续若要做到 Rust 用户组件免 Cargo 分发，需要单独设计安装包、预编译 runtime 和组件 ABI/插件边界。

如需引入 parser、serialization、CLI、template 或测试库，优先选择成熟、维护良好、依赖面可控的库，并在相关文档中记录原因。

## CLI 状态

当前已实现：

```bash
flowrt check path/to/robot.rsdl
flowrt prepare path/to/robot.rsdl
flowrt build path/to/robot.rsdl
flowrt run path/to/robot.rsdl
flowrt run path/to/robot.rsdl --process main
flowrt run path/to/robot.rsdl --run-ticks 5 --process main
flowrt launch path/to/robot.rsdl
flowrt list path/to/generated-app
flowrt nodes path/to/generated-app
flowrt status
flowrt hz [channel] [--socket path/to/runtime.sock] [--window-ms 1000]
flowrt echo <channel> [--socket path/to/runtime.sock] [--image path/to/generated-app-or-selfdesc.json] [--follow]
flowrt params list|get|set path/to/generated-app-or-selfdesc.json ...
flowrt inspect flowrt/contract/contract.ir.json
```

`--process` 运行生成应用中的单个 RSDL process group；mixed contract 使用 `flowrt run --process <name>` 时必须选择一个单语言 process group。
`run` / `launch` 当前支持 Rust only、C++ only，以及 language-separated mixed contract over `iox2` 或 `zenoh`。同一 process group 内混合 C++/Rust 或 mixed `inproc` 必须明确拒绝。
`prepare` 和 `build` 会写 `flowrt/` 输出目录，必须在命令级持有 OS advisory lock；`.flowrt.lock` 文件可残留，PID 只用于诊断，真实占用状态必须由锁判断。`check`、`inspect`、`run`、`launch`、`list`、`nodes`、`status`、`echo` 和 `params` 不写生成物，不应获取该锁。
`run` / `launch` 省略 `--run-ticks` 时长期运行，直到生成应用返回非 `Ok` 或收到 SIGINT/SIGTERM；生成 shell 必须通过 runtime `ShutdownToken` 触发 graceful shutdown，继续执行 `shutdown` task、`on_stop` 和 `on_shutdown`。
`launch` 运行 FlowRT 管理的 Rust supervisor；supervisor 读取 `flowrt/launch/launch.json`，遍历全部 graph，并按 process group 启动生成应用。C++ only contract 会生成 supervisor-only Rust crate，`launch` 先构建 CMake app 再运行 supervisor，不生成 Rust runtime shell 或 Rust app binary。`inproc` 是单进程 backend，`launch` 必须拒绝 inproc dataflow 跨 RSDL process group；`run --process <name>` 也必须拒绝单独运行带跨 process dataflow 的 inproc process group。跨 process dataflow 必须选择 `iox2` / `zenoh`、运行完整 inproc app，或把相关 instance 放回同一 process group。launch manifest 的 process group 必须暴露 `runtimes` 和 `runtime_kind`，task metadata 必须暴露 `inputs`、`outputs` 和 `priority` scheduler hint，便于后续 supervisor/scheduler 消费；graph instance 必须暴露 `runtime`，graph 必须暴露 `channels`。supervisor health 必须通过 introspection 暴露 `starting` / `running` / `stale` / `restarting` / `exited` / `failed`、PID、restart count、tick 计数、最后一次可见时间和退出码；当前内置 `on-failure` policy 对异常退出最多重启 3 次，正常退出不重启。
runtime introspection socket 使用 `$XDG_RUNTIME_DIR/flowrt/<pid>.sock` 或 `/tmp/flowrt.<uid>/<pid>.sock`，socket 路径只用于发现；真实身份必须来自 handshake。启动 status server 时不能覆盖仍可连接的 live socket，SIGKILL 后残留且不可连接的 socket 文件可以回收。Rust `IntrospectionState` 必须在 mutex poison 后恢复访问，不得因单个连接或线程异常导致全局 panic。
当前 iox2/zenoh endpoint 已有对 peer endpoint 重建后的继续收发回归测试；Runtime 已提供 C ABI 友好形状的 `BackendHealthState`、`BackendHealthSnapshot`、`ReconnectPolicy` 和 `BackendHealthTracker`。`iox2` 和 `zenoh` endpoint 已接入自动恢复：本地 transport 资源丢失或操作失败会重建本地 publisher/subscriber/session，codec/schema 错误不得触发重连。后续 C、Python 或更多语言 runtime 应复用这套稳定状态形状，不要在 shell 中临时吞掉错误。

`cargo run -p flowrt-cli -- ...` 只允许作为仓库开发者调试 FlowRT CLI 的内部命令，不得写成最终用户主路径。

## 文档规范

任何影响语义、接口、目录结构、命令或生成物边界的变更，都必须同步更新配套文档或在最终回复说明不更新原因。README、CHANGELOG 和面向用户/维护者的配套文档需要入库；只有本地架构计划和规格设计文档不入库。

优先维护这些文档：

- `README.md`：项目入口和简短定位。
- `CHANGELOG.md`：阶段性变更记录。
- `docs/README.md`：入库配套文档索引和文档边界。
- `docs/getting-started.md`：面向用户的快速开始。
- `docs/cli.md`：CLI 命令、选项和生成物边界。
- `docs/examples.md`：示例矩阵和运行要求。
- `docs/development.md`：开发验证、文档维护和提交规则。
- 入门、操作指南、维护说明等后续配套文档：随代码入库。
- `docs/architecture-plan.md`：总架构计划，本地维护但不入库。
- `docs/rsdl-v0.1.md`：RSDL 语法和示例，本地维护但不入库。
- `docs/contract-ir-v0.1.md`：IR schema 与 normalization 规则，本地维护但不入库。
- `docs/message-abi-v0.1.md`：跨语言消息 ABI，本地维护但不入库。
- `docs/backend-contract.md`：backend capability 与行为契约，本地维护但不入库。

文档要写可执行决策，不写空泛愿景。架构取舍必须说明原因。
实现代码时要顺手更新配套文档和 `CHANGELOG.md`。如果某次改动确实不影响文档或 changelog，最终回复需要说明原因。
所有项目文档和 `CHANGELOG.md` 必须用中文维护，包括 `README.md`、`docs/*.md`、`AGENTS.md` 中的项目说明和变更记录。代码标识符、命令、配置键、协议名和必要的专有术语可以保留英文。代码注释仍按对应语言代码风格要求维护。
`docs/` 下的设计和规格文档不纳入 Git；提交时不得把 `.gitignore` 明确排除的本地设计/规格文件加入索引。普通配套文档不受该限制，并应随相关变更正常入库。

### CHANGELOG 维护规范

`CHANGELOG.md` 是 GitHub Release notes 的事实源，不能随意改格式。推送 `v*` tag 时，CI 会运行：

```bash
scripts/extract-release-notes.sh "$GITHUB_REF_NAME" CHANGELOG.md
```

必须遵守：

- 每个正式发布版本必须有独立二级标题，格式固定为 `## vX.Y.Z - YYYY-MM-DD`，例如 `## v0.1.0 - 2026-06-06`。
- tag 名必须是 `vX.Y.Z`，且版本号必须与根 `Cargo.toml` 中的 workspace version 一致。
- 发布前必须把待发布条目从 `## 未发布` 移入对应版本段；CI 找不到 tag 对应版本段或版本段为空时会失败。
- 版本段内部优先使用三级标题 `### 新增`、`### 修复`、`### 变更`、`### 测试` 组织内容；条目必须是面向用户或维护者的具体变化，不写空泛总结。
- 不要把 release notes 写在 `Latest`、`Release`、日期-only heading、四级标题或自由文本段落下；CI 只按二级版本标题抽取。
- `## 未发布` 只用于尚未 tag 的后续变化，且应位于最新正式版本段上方；正式发版前必须重新检查抽取结果。

## 代码风格

根目录的 `.clang-format` 和 `rustfmt.toml` 是格式化的权威来源。若规则与口头约定冲突，以配置文件为准。

### 通用

- 以自动格式化工具为准，不手工对齐、补空格或维护视觉列宽。
- 源文件默认保持 ASCII；只有在已有语义明确、确实必要时才引入非 ASCII 内容。
- 不要用格式去掩盖设计问题。阅读困难时优先重构命名、函数边界和数据结构。

### Rust

- `cargo fmt` 是权威格式化来源。
- `cargo clippy --all-targets --all-features -- -D warnings` 是主要静态检查。
- 默认遵循 rustfmt 风格，不手动排列 import、字段或链式调用。
- 公共 API 和 crate 级能力边界用中文 `///` 文档注释说明契约、输入、输出、错误、所有权和副作用。
- 代码内注释只用于解释不明显的意图、约束、unsafe 边界、后端差异和关键算法选择。
- 注释正文使用中文；代码标识符、类型名、trait 名、协议名和必要专有术语可以保留英文。

### C++

- `clang-format` 是权威格式化来源。
- 使用 4 空格缩进、`BreakBeforeBraces: Attach`、约 100 列目标宽度、禁止 tabs。
- C++ 头文件分组约定为：`flowrt/...` 运行时头文件 -> `flowrt_app/...` FlowRT 管理的应用头文件 -> 其他 quoted 头文件 -> `<...>` 外部/系统头文件。
- 公共 runtime API 用高质量中文 Doxygen 风格注释说明契约、参数、返回值、错误语义、生命周期和所有权。
- 注释只解释意图、约束和边界条件，不重复代码表面动作。
- 注释正文使用中文；代码标识符、类型名、协议名和必要专有术语可以保留英文。

### Markdown / Docs

- 标题短，层级清楚。
- 示例尽量可解析、可运行、可验证。
- 术语保持统一：RSDL、Contract IR、runtime shell、backend、message ABI、component、instance、task、channel。
- 文档和 changelog 正文使用中文；只有代码标识符、命令、配置键、协议名、专有名词和必要引用保留英文。

### FlowRT 管理代码

- FlowRT 管理的应用产物只保留必要的机器标记和来源说明。
- 不在 FlowRT 管理的应用产物里维护手写解释性注释。
- 可读性通过命名、结构和分层保证，不通过大量注释补救。

## 验证要求

每次修改后运行与改动匹配的最小验证。

当前文档阶段：

```bash
git status --short
```

如果后续引入 Rust workspace：

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

如果后续引入 C++ runtime：

```bash
cmake -S runtime/cpp -B build/cpp
cmake --build build/cpp
ctest --test-dir build/cpp --output-on-failure
```

如果无法运行验证，最终回复必须明确说明原因。

## Git 提交规范

采用 Conventional Commits，提交标题使用中文：

```text
<type>(<scope>): <中文标题>
```

常用类型：

```text
feat      新功能
fix       修复
docs      文档
refactor  重构
test      测试
chore     工具、配置、依赖
style     格式调整
perf      性能优化
```

示例：

```text
docs(architecture): 增加 FlowRT 架构计划
feat(ir): 添加 Contract IR 初始模型
test(abi): 补充 C++ 与 Rust 消息布局测试
```

不要提交旧项目残留说明，除非是在比较背景文档中明确引用。

需要经常、阶段性地原子提交：

- 每个可验证实现切片完成后，先运行匹配验证，再提交。
- 提交应只包含同一主题的代码、测试、文档和 changelog 更新。
- 不把未验证的半成品、无关生成产物或大型构建输出混进提交。
- 不为了提交而提交；只有形成阶段性、可验证成果后才提交。
- 设计文档不入库；提交前确认 `.gitignore` 明确排除的本地设计/规格文件未进入索引，普通配套文档应随相关变更入库。
- 如果仓库缺少基线提交，应先建立一次明确的 baseline commit；之后保持增量原子提交。

## Agent 工作方式

- 开始编码前先阅读 `README.md`。如果任务涉及架构或规格，再阅读本地 `docs/architecture-plan.md` 等设计文档，但不要把被 `.gitignore` 排除的本地设计/规格文件提交入库。
- 涉及 OpenAI、第三方库、iox2 当前 API 或编译器版本的问题时，使用官方文档或当前仓库实际配置确认，不凭记忆假设。
- 优先小步提交可验证产物，不创建大量空架子。
- 不覆盖用户代码，不删除用户未要求删除的文件。
- 如果发现旧项目残留内容，先判断是否仍有价值；明显无关的残留应替换为 FlowRT 语境。
- 如果用户要求“先计划、不动手”，只更新计划或回答问题，不实现代码。
- 如果用户要求实现，默认完成实现、验证和简洁汇报。

## 重要提醒

FlowRT 的长期竞争力来自清晰的系统契约，而不是把通信库包一层。任何实现都应反复检查是否仍然符合：

```text
RSDL controls system structure.
Runtime controls execution.
User code controls algorithms.
```
