# CLI 参考

`flowrt` 是 FlowRT 面向用户的命令入口。仓库内可以用 `cargo run -p flowrt-cli -- ...` 调试，但对外文档和示例默认使用安装后的 `flowrt ...`。

## 命令概览

```bash
flowrt check <path/to/robot.rsdl>
flowrt prepare <path/to/robot.rsdl> [--out-dir flowrt] [--profile <name>]
flowrt build <path/to/robot.rsdl> [--out-dir flowrt] [--profile <name>] [--launcher]
flowrt run <path/to/robot.rsdl> [--out-dir flowrt] [--profile <name>] [--process <name>] [--run-ticks <N>]
flowrt launch <path/to/robot.rsdl> [--out-dir flowrt] [--profile <name>] [--run-ticks <N>]
flowrt inspect <path/to/flowrt/contract/contract.ir.json>
flowrt list <path/to/generated-app-or-selfdesc.json>
flowrt nodes <path/to/generated-app-or-selfdesc.json>
flowrt echo <path/to/generated-app-or-selfdesc.json> <channel> [--socket <path>] [--follow] [--interval-ms <ms>]
flowrt status
```

## `check`

```bash
flowrt check examples/import_demo/rsdl/robot.rsdl
```

`check` 解析 RSDL、展开 imports、生成内存中的 Contract IR 并运行 validator。它不会写入 `flowrt/` 目录，也不会构建应用。

Message ABI v0.1 仍以 fixed-size plain data 作为 native ABI 基线，但 RSDL type expression 现在也可以解析 `bytes<max=262144>`、`string<max=64>` 和 `sequence<u32,max=16>` 这类 bounded variable 字段。选中 backend 具备 `abi:variable_payload_frame` 时，`prepare` 和 `build` 生成的产物会输出 canonical frame codec；`iox2` 路径会额外生成固定容量 transport slot，在 typed IPC payload 中承载 canonical frame bytes。

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

`prepare` 和 `build` 会写入输出目录。CLI 会在输出目录中创建 `.flowrt.lock` 并在命令结束时释放；如果另一个写命令正在使用同一输出目录，当前命令会直接失败，避免并发写入损坏生成产物。`run` 和 `launch` 只读取已生成产物，不写输出目录，也不获取该锁。

## `build`

```bash
flowrt build examples/cpp_counter_demo/rsdl/robot.rsdl
```

`build` 先执行 `prepare`，再构建生成应用。

规则：

- Rust-only 或含 Rust component 的 contract 当前会触发 Cargo app 构建。
- C++ only contract 走 CMake app 路径，不依赖 Cargo app。
- `--launcher` 会额外构建 `flowrt launch` 需要的 generated supervisor；省略时只构建可由 `flowrt run` 直接执行的 app。
- 含 C++ component 时，生成的 CMake 工程会构建 managed shell、app target 和 ABI conformance test target。
- 选择 `iox2` 且 contract 含 C++ component 时，CMake 会先查找 `iceoryx2-cxx 0.9.1`；找不到时默认通过 `FetchContent` 拉取 `iceoryx2` v0.9.1，并调用 Cargo 构建其 Rust FFI 部分。
- 选择 `zenoh` 且 contract 含 C++ component 时，CMake 会要求 `zenohc 1.9.0` 与 `zenohcxx 1.9.0`；需要通过 `CMAKE_PREFIX_PATH` 指向对应安装前缀。

## `run`

```bash
flowrt run examples/import_demo/rsdl/robot.rsdl --process main
flowrt run examples/cpp_counter_demo/rsdl/robot.rsdl --process control
```

`run` 只读取 `flowrt/contract/contract.ir.json` 和已构建的 generated app，然后运行单个 process group。它不会执行 `prepare`、不会构建、不会写 `flowrt/` 目录。首次运行或修改 RSDL、profile、生成模板、用户代码后，应先执行匹配 profile 的 `flowrt build`。

`--process <name>` 运行一个 RSDL process group。process 名称来自 `instance.<name>.process`，未声明时默认属于 `main`；RSDL process label 必须使用 `snake_case`，并且不得使用大小写不敏感的保留 `flowrt` 前缀。

`--run-ticks <N>` 是 CLI 的显式运行上限，主要用于 smoke test 和调试观察。省略时，生成应用会持续运行，直到用户终止进程或 runtime shell 返回非 `Ok` 状态。该选项会被 CLI 转换为生成应用的内部 `--flowrt-run-ticks` 参数；核心 runtime scheduler 只服从调用方传入的 tick 数，不读取 CLI 环境变量。

如果传入 `--profile <name>`，`run` 只校验已生成产物是否使用同名 profile；不匹配时会要求重新执行 `flowrt build --profile <name>`。

mixed contract 规则：

- 同一 process group 内混合 C++/Rust 会被拒绝。
- mixed contract 使用 `inproc` 跨进程组合会被拒绝。
- language-separated mixed contract 在 `iox2` 或 `zenoh` backend 下可以使用 `flowrt run --process <name>` 运行某个单语言 process。
- 未指定 process 的 mixed iox2/zenoh contract 应使用 `flowrt launch` 启动全部 process group。

`inproc` backend 下，`run --process <name>` 只能运行没有跨 process dataflow 依赖的 process group；如果该 process 与其他 process 之间存在 bind，CLI 会拒绝单独运行它。此时应运行完整 inproc app，改用 `iox2`，或调整 RSDL process group。

## `launch`

```bash
flowrt launch examples/import_demo/rsdl/robot.rsdl
flowrt launch examples/cpp_counter_demo/rsdl/robot.rsdl
```

`launch` 只运行已构建的 generated supervisor。supervisor 读取 `flowrt/launch/launch.json`，遍历 graph 中的 process group，并按 `runtime_kind` 启动 Rust app executable 或 C++ app executable。首次 launch 或修改 RSDL、profile、生成模板、用户代码后，应先执行匹配 profile 的 `flowrt build --launcher`。

含 C++ component 的 contract 需要先通过 `flowrt build --launcher` 显式构建 CMake app 和 generated supervisor；C++ only contract 的 supervisor-only Rust crate 只负责编排 C++ app，不生成 Rust runtime shell 或 Rust app binary。

`inproc` 是单进程 backend。`launch` 如果发现 dataflow bind 跨越两个 RSDL process group，会拒绝该 contract；需要跨 process 通信时应选择 `iox2` 或 `zenoh` backend，或把相关 instance 放回同一 process group。

`--run-ticks <N>` 会传给 supervisor，再由 supervisor 转发给每个生成应用 process；省略时全部 process 按长期运行模式启动。

如果传入 `--profile <name>`，`launch` 只校验已生成产物是否使用同名 profile；不匹配时会要求重新执行 `flowrt build --launcher --profile <name>`。

launch manifest 的关键字段包括：

- process group 的 `runtimes` 和 `runtime_kind`
- graph instance 的 `runtime`
- task 的 `trigger`、`period_ms`、`deadline_ms`、`priority`、`inputs` 和 `outputs`
- graph `channels`
- iox2 channel 的 canonical service name
- zenoh channel 的 deterministic key expression

## `inspect`

```bash
flowrt inspect examples/import_demo/flowrt/contract/contract.ir.json
```

`inspect` 会先校验已落盘 Contract IR JSON，再显示摘要，用于确认 package、type、component、instance、task、bind、profile、target 和 deployment 是否符合预期。当前工具链不支持的 `ir_version`、`schema_version` 或 package `rsdl_version` 会被明确拒绝。

## `list` / `nodes`

```bash
flowrt list path/to/generated-app
flowrt nodes path/to/generated-app
```

`list` 和 `nodes` 读取生成应用二进制中的 `.flowrt.selfdesc` section，直接输出静态拓扑摘要或 instance 列表；也可以读取 `flowrt/selfdesc/selfdesc.json` 作为调试辅助。它们不需要 RSDL 源文件，适合部署后在目标机器上确认 package、graph、instance、task、channel 和 Message ABI layout 是否与预期一致。

当前这两个命令只读取编译期静态自描述；运行态 socket 由 `status` 和 `echo` 使用。

## `echo`

```bash
flowrt echo path/to/generated-app source.imu
flowrt echo path/to/generated-app source.imu_to_sink.imu --socket /tmp/flowrt.1000/12345.sock
flowrt echo path/to/generated-app source.imu --follow --interval-ms 100
```

`echo` 用静态 self-description 判断 channel 是否存在，并读取该 channel 的 message type 与 Message ABI layout；runtime socket 只返回 raw ABI bytes、`published_count` 和 `published_at_ms`。CLI 不从 runtime 协议读取业务字段 schema，也不会把 payload 反序列化成字段值。`<channel>` 可以写完整 channel 名 `<from>_to_<to>`，也可以写唯一的 source 或 target 端点名，例如 `source.imu`；端点名匹配多条 channel 时需要改用完整 channel 名。

省略 `--socket` 时，CLI 会扫描当前用户 runtime socket 目录，连接候选 socket 后读取 handshake，并选择 `self_description_hash` 与静态 self-description JSON hash 匹配的唯一进程。若没有匹配进程会报错；若多个进程匹配同一个 hash，也会报错并要求显式传入 `--socket <path>`，避免从错误进程读取 channel 快照。

输出是最小稳定摘要：

```text
channel=source.imu_to_sink.imu type=Imu abi_size=24 published_count=1 published_at_ms=42 payload_len=24 raw=...
```

如果 runtime 还没有该 channel 的 payload，例如当前进程尚未发布该 channel 的样本，输出会包含 `payload_len=0 no payload`。

默认情况下，`echo` 只读取一次 latest snapshot。传入 `--follow` 后，CLI 会按 `--interval-ms <ms>` 指定的间隔持续轮询同一 runtime socket；第一条 snapshot 一定输出，后续只在 `published_count`、`published_at_ms` 或 raw payload 变化时输出，避免没有新发布时重复刷屏。默认轮询间隔是 250 ms。

生成的 Rust/C++ runtime shell 会为当前 process 的 active channel 预注册 live 摘要，并在成功发布输出后记录 raw ABI payload 和 `published_at_ms`。因此 Rust 与 C++ 示例都可以通过 `flowrt status` 查看 live channel 摘要，并通过 `flowrt echo` / `flowrt echo --follow` 读取 live snapshot。

`zenoh` 示例在跨机时通常需要为两个进程分别注入连接信息。常用环境变量是：

- `FLOWRT_ZENOH_CONNECT`
- `FLOWRT_ZENOH_LISTEN`
- `FLOWRT_ZENOH_MODE`
- `FLOWRT_ZENOH_NO_MULTICAST`
- `FLOWRT_TICK_SLEEP_MS`

前四个用于给 runtime session 注入 zenoh 网络配置，`FLOWRT_TICK_SLEEP_MS` 用于把 demo 的同步 tick 间隔拉长到可观察窗口。tick 数上限由 `flowrt run --run-ticks <N>` 或 `flowrt launch --run-ticks <N>` 显式传入，不进入核心 runtime scheduler。

## `status`

```bash
flowrt status
```

`status` 扫描当前用户 runtime socket 目录中的 FlowRT 进程，并通过 handshake 验证 PID、package、process、runtime、静态自描述 hash 和 tick/channel 摘要。socket 路径只作为发现入口；CLI 不把文件名当作进程身份事实。

当前 Rust/C++ 生成应用都会启动 status socket，路径优先使用 `$XDG_RUNTIME_DIR/flowrt/<pid>.sock`，没有 `XDG_RUNTIME_DIR` 时使用 `/tmp/flowrt.<uid>/<pid>.sock` 风格的当前用户目录。生成 shell 会把 scheduler tick 计数、active channel 摘要、发布计数和 latest raw ABI payload 写入 live status/snapshot。

## `--profile`

```bash
flowrt build --profile iox2 examples/profile_switch_demo/rsdl/robot.rsdl
flowrt run --profile iox2 examples/profile_switch_demo/rsdl/robot.rsdl
```

`--profile <name>` 适用于 `prepare`、`build`、`run` 和 `launch`。`prepare` 和 `build` 会按显式或默认选定的 profile 投影 Contract IR，只保留对应 deployment 视图，再校验和生成产物。`run` 和 `launch` 不投影、不生成，只校验已生成产物的 profile 是否与显式参数匹配。

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
- 不应由多个 `flowrt prepare` / `build` 命令同时写入同一个输出目录；CLI 会通过 `.flowrt.lock` 做 fail-fast 保护。

用户代码应放在项目自己的 `src/` 目录。C++ 用户代码通过生成接口和 `flowrt_user::build_app()` 接入；Rust 用户代码通过生成 trait 和用户模块接入。
