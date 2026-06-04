# CLI 参考

`flowrt` 是 FlowRT 面向用户的命令入口。仓库内可以用 `cargo run -p flowrt-cli -- ...` 调试，但对外文档和示例默认使用安装后的 `flowrt ...`。

## 命令概览

```bash
flowrt check <path/to/robot.rsdl>
flowrt prepare <path/to/robot.rsdl> [--out-dir flowrt] [--profile <name>]
flowrt build <path/to/robot.rsdl> [--out-dir flowrt] [--profile <name>]
flowrt run <path/to/robot.rsdl> [--out-dir flowrt] [--profile <name>] [--process <name>]
flowrt launch <path/to/robot.rsdl> [--out-dir flowrt] [--profile <name>]
flowrt inspect <path/to/flowrt/contract/contract.ir.json>
```

## `check`

```bash
flowrt check examples/import_demo/rsdl/robot.rsdl
```

`check` 解析 RSDL、展开 imports、生成内存中的 Contract IR 并运行 validator。它不会写入 `flowrt/` 目录，也不会构建应用。

Message ABI v0.1 只接受 fixed-size plain data。RSDL type expression 可以解析未来有界变长语法，例如 `bytes<max=262144>`、`string<max=64>` 和 `sequence<u32,max=16>`，但 `check` 会拒绝包含这些字段的 v0.1 contract，并提示需要未来 Variable Frame ABI；当前 `prepare`、`build`、`run`、`launch` 和 conformance 生成都不会继续处理这些字段。

`u128` 和 `i128` 属于 fixed-size primitive，但它们需要额外的 `abi:int128` capability。当前 `inproc` 和 `iox2` backend 不提供该能力，因此使用这些类型的 contract 会在 deployment capability 校验阶段被判定为不满足。

## `prepare`

```bash
flowrt prepare examples/import_demo/rsdl/robot.rsdl
```

`prepare` 会生成 FlowRT 管理产物，包括：

- `flowrt/contract/contract.ir.json`
- `flowrt/launch/launch.json`
- `flowrt/src/` 下的生成 runtime shell、接口和消息代码
- `flowrt/build/` 下的生成构建元数据

默认输出目录是 RSDL 所在项目根目录下的 `flowrt/`。可以用 `--out-dir <dir>` 改写。

`prepare`、`build`、`run` 和 `launch` 会写入输出目录。CLI 会在输出目录中创建 `.flowrt.lock` 并在命令结束时释放；如果另一个 `flowrt` 命令正在使用同一输出目录，当前命令会直接失败，避免并发写入损坏生成产物。

## `build`

```bash
flowrt build examples/cpp_counter_demo/rsdl/robot.rsdl
```

`build` 先执行 `prepare`，再构建生成应用。

规则：

- Rust-only 或含 Rust component 的 contract 当前会触发 Cargo app 构建。
- C++ only contract 走 CMake app 路径，不依赖 Cargo app。
- 含 C++ component 时，生成的 CMake 工程会构建 managed shell、app target 和 ABI conformance test target。
- 选择 `iox2` 且 contract 含 C++ component 时，CMake 会要求 `iceoryx2-cxx 0.9.1`。

## `run`

```bash
flowrt run examples/import_demo/rsdl/robot.rsdl --process main
flowrt run examples/cpp_counter_demo/rsdl/robot.rsdl --process control
```

`run` 先执行 `prepare` 和必要的构建，再运行生成应用。

`--process <name>` 运行一个 RSDL process group。process 名称来自 `instance.<name>.process`，未声明时默认属于 `main`；RSDL process label 必须使用 `snake_case`，并且不得使用大小写不敏感的保留 `flowrt` 前缀。

mixed contract 规则：

- 同一 process group 内混合 C++/Rust 会被拒绝。
- mixed contract 使用 `inproc` 跨进程组合会被拒绝。
- language-separated mixed contract 在 `iox2` backend 下可以使用 `flowrt run --process <name>` 运行某个单语言 process。
- 未指定 process 的 mixed iox2 contract 应使用 `flowrt launch` 启动全部 process group。

`inproc` backend 下，`run --process <name>` 只能运行没有跨 process dataflow 依赖的 process group；如果该 process 与其他 process 之间存在 bind，CLI 会拒绝单独运行它。此时应运行完整 inproc app，改用 `iox2`，或调整 RSDL process group。

## `launch`

```bash
flowrt launch examples/import_demo/rsdl/robot.rsdl
flowrt launch examples/cpp_counter_demo/rsdl/robot.rsdl
```

`launch` 会准备、构建运行所需的 generated app，并运行生成的 process supervisor。supervisor 读取 `flowrt/launch/launch.json`，遍历 graph 中的 process group，并按 `runtime_kind` 启动 Rust app executable 或 C++ app executable。

含 C++ component 的 contract 会先构建生成的 CMake app；C++ only contract 只生成 supervisor 所需的最小 Rust crate，不生成 Rust runtime shell 或 Rust app binary。

`inproc` 是单进程 backend。`launch` 如果发现 dataflow bind 跨越两个 RSDL process group，会拒绝该 contract；需要跨 process 通信时应选择 `iox2` backend，或把相关 instance 放回同一 process group。

launch manifest 的关键字段包括：

- process group 的 `runtimes` 和 `runtime_kind`
- graph instance 的 `runtime`
- task 的 `trigger`、`period_ms`、`deadline_ms`、`priority`、`inputs` 和 `outputs`
- graph `channels`
- iox2 channel 的 canonical service name

## `inspect`

```bash
flowrt inspect examples/import_demo/flowrt/contract/contract.ir.json
```

`inspect` 会先校验已落盘 Contract IR JSON，再显示摘要，用于确认 package、type、component、instance、task、bind、profile、target 和 deployment 是否符合预期。当前工具链不支持的 `ir_version`、`schema_version` 或 package `rsdl_version` 会被明确拒绝。

## `--profile`

```bash
flowrt run --profile iox2 examples/profile_switch_demo/rsdl/robot.rsdl
```

`--profile <name>` 适用于 `prepare`、`build`、`run` 和 `launch`。CLI 会先按显式或默认选定的 profile 投影 Contract IR，只保留对应 deployment 视图，再校验和生成产物。省略 `--profile` 不会把全部 profile 一起写入生成产物。

默认 profile 选择规则是：

- 优先使用名为 `default` 的 profile。
- 没有 `default` 时使用首个 profile。
- RSDL 未声明任何 profile 时，归一化阶段会插入隐式 `default` profile，backend 为 `inproc`。

profile 投影还会重算来自 profile default 的 bind-level policy：未在 `bind.dataflow` 上显式声明的 `overflow`、`stale_policy` 和 `max_age_ms` 会采用选中 profile 的默认值；bind 上显式声明的 policy 保持不变。投影后的 `contract.ir.json` 会同时刷新 channel 和 deployment 的 capability 元数据。

## 生成物边界

`flowrt/` 下的内容由 FlowRT 管理：

- 可以删除。
- 可以重新生成。
- 不应放用户算法代码。
- 不应手写维护生成 runtime shell。
- 不应由多个 `flowrt prepare` / `build` / `run` / `launch` 命令同时写入同一个输出目录；CLI 会通过 `.flowrt.lock` 做 fail-fast 保护。

用户代码应放在项目自己的 `src/` 目录。C++ 用户代码通过生成接口和 `flowrt_user::build_app()` 接入；Rust 用户代码通过生成 trait 和用户模块接入。
