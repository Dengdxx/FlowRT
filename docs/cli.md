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
flowrt echo <channel> [--socket <path>] [--image <path/to/generated-app-or-selfdesc.json>] [--follow] [--interval-ms <ms>]
flowrt echo <path/to/generated-app-or-selfdesc.json> <channel> [--socket <path>] [--follow] [--interval-ms <ms>]
flowrt params list <path/to/generated-app-or-selfdesc.json> [--socket <path>]
flowrt params get <path/to/generated-app-or-selfdesc.json> <instance.param> [--socket <path>]
flowrt params set <path/to/generated-app-or-selfdesc.json> <instance.param> <json-value> [--socket <path>]
flowrt status
flowrt hz [channel] [--socket <path>] [--window-ms <ms>]
```

## `check`

```bash
flowrt check examples/import_demo/rsdl/robot.rsdl
```

`check` 解析 RSDL、展开 imports、生成内存中的 Contract IR 并运行 validator。它不会写入 `flowrt/` 目录，也不会构建应用。

Message ABI v0.1 仍以 fixed-size plain data 作为 native ABI 基线，但 RSDL type expression 现在也可以解析 `bytes`、`string` 和 `sequence<u32>` 这类无界 variable 字段。选中 backend 具备 `abi:variable_payload_frame` 时，`prepare` 和 `build` 生成的产物会输出 canonical frame codec。`iox2` 只承载 fixed-size plain data；当 profile 默认 backend 为 `iox2` 且某条 route 使用 variable frame 时，该 route 会自动选择支持变长消息的 backend（当前为 `zenoh`），其他 fixed-size route 仍继续走 `iox2`。

`u128` 和 `i128` 属于 fixed-size primitive，但它们需要额外的 `abi:int128` capability。当前 `inproc`、`iox2` 和 `zenoh` backend 不提供该能力，因此把这些类型用于 channel route 的 contract 会在 route backend capability 校验阶段被判定为不满足。

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

`prepare` 和 `build` 会写入输出目录。CLI 会在输出目录中创建 `.flowrt.lock` 并持有 OS advisory lock；如果另一个写命令正在使用同一输出目录，当前命令会直接失败，避免并发写入损坏生成产物。锁文件可以在进程崩溃后残留，后续命令会重新打开该文件并用 OS 锁判断是否仍被占用；文件中的 PID 只作为诊断信息。`run` 和 `launch` 只读取已生成产物，不写输出目录，也不获取该锁。

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
- 选择 `iox2` 且 contract 含 C++ component 时，CMake 会查找 `iceoryx2-cxx 0.9.1` 的 `iceoryx2-cxx::static-lib-cxx` 目标。
- 选择 `zenoh` 且 contract 含 C++ component 时，CMake 会查找 `zenohcxx 1.9.0` 的 `zenohcxx::zenohc` 目标。
- 声明 `[[bridge.ros2]]` 时，`build` 会额外构建 FlowRT 管理的 C++ ROS2 adapter target；即使没有 C++ 用户 component，也会运行生成 CMake。

Debian 包会把 FlowRT 锁定版本的 Rust crate vendor、`iceoryx2-cxx`、`zenoh-c` 和 `zenoh-cpp` 安装到 `/opt/flowrt/<version>`。安装后的 `flowrt build` 会把该私有前缀传给生成 CMake，并为生成 Rust app 写入离线 Cargo config；生成项目构建不需要联网拉取 backend 依赖。源码树内直接调试生成 CMake 时，可以用 `FLOWRT_CPP_RUNTIME_DIR` 或 `CMAKE_PREFIX_PATH` 指向同一私有前缀。

## `run`

```bash
flowrt run examples/import_demo/rsdl/robot.rsdl --process main
flowrt run examples/cpp_counter_demo/rsdl/robot.rsdl --process control
```

`run` 只读取 `flowrt/contract/contract.ir.json` 和已构建的 generated app，然后运行单个 process group。它不会执行 `prepare`、不会构建、不会写 `flowrt/` 目录。首次运行或修改 RSDL、profile、生成模板、用户代码后，应先执行匹配 profile 的 `flowrt build`。

`--process <name>` 运行一个 RSDL process group。process 名称来自 `instance.<name>.process`，未声明时默认属于 `main`；RSDL process label 必须使用 `snake_case`，并且不得使用大小写不敏感的保留 `flowrt` 前缀。

`--run-ticks <N>` 是 CLI 的显式运行上限，主要用于 smoke test 和调试观察。省略时，生成应用会持续运行，直到收到 SIGINT/SIGTERM 或 runtime shell 返回非 `Ok` 状态。SIGINT/SIGTERM 会触发 runtime shutdown token，生成应用退出 tick loop 后继续执行 `shutdown` task、`on_stop` 和 `on_shutdown`。该选项会被 CLI 转换为生成应用的内部 `--flowrt-run-ticks` 参数；核心 runtime scheduler 只服从调用方传入的 tick 数，不读取 CLI 环境变量。

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

`--run-ticks <N>` 会传给 supervisor，再由 supervisor 转发给每个生成应用 process；省略时全部 process 按长期运行模式启动，并通过生成应用自己的 shutdown token 响应 SIGINT/SIGTERM。

如果传入 `--profile <name>`，`launch` 只校验已生成产物是否使用同名 profile；不匹配时会要求重新执行 `flowrt build --launcher --profile <name>`。

launch manifest 的关键字段包括：

- process group 的 `runtimes` 和 `runtime_kind`
- graph instance 的 `runtime`
- task 的 `name`、`trigger`、`period_ms`、`deadline_ms`、`priority`、`inputs` 和 `outputs`
- graph `channels`
- graph `ros2_bridges`
- iox2 channel 的 canonical service name
- zenoh channel 的 deterministic key expression

## ROS2 Bridge

RSDL 可以用 `[[bridge.ros2]]` 声明 FlowRT 到 ROS2 的外部 bridge：

```toml
[[bridge.ros2]]
flowrt = "source.text"
ros2_topic = "/flowrt/text"
ros2_type = "std_msgs/msg/String"
direction = "flowrt_to_ros2"
field = "data"
```

FlowRT 与 ROS2 的唯一桥梁固定为 `zenoh`。生成物包含：

- source process 中的 zenoh bridge tap。
- C++ ROS2 adapter `*_ros2_bridge`。
- launch manifest 中的 `runtime_kind = "ros2_bridge"` process。

约束：

- `direction` 当前只支持 `flowrt_to_ros2`。
- `ros2_type` 当前只支持 `std_msgs/msg/String`。
- `field` 必须是 source message 的 `string` 字段，省略时默认为 `data`。
- bridge backend 固定为 `zenoh`，source instance 的 target 必须在 `backends` 中声明 `zenoh`。
- ROS2 侧必须使用 `rmw_zenoh_cpp`；generated adapter 会设置并校验 `RMW_IMPLEMENTATION=rmw_zenoh_cpp`，不会回退到 DDS。
- 普通 FlowRT `zenoh` backend 使用 FlowRT 包内私有 zenoh SDK；ROS2 bridge adapter 进程使用 ROS2 安装中的 `zenoh_cpp_vendor`，以匹配 `rmw_zenoh_cpp` 的同进程 ABI。

构建和运行示例：

```bash
source /opt/ros/jazzy/setup.bash
flowrt build --launcher examples/ros2_bridge_demo/rsdl/robot.rsdl
flowrt launch --run-ticks 200 examples/ros2_bridge_demo/rsdl/robot.rsdl
```

观察 ROS2 topic 时如果遇到 daemon 旧缓存，先执行 `ros2 daemon stop` 后重试。

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

当前这两个命令只读取编译期静态自描述；运行态 socket 由 `status`、`echo` 和 `params` 使用。

## `echo`

```bash
flowrt echo source.imu
flowrt echo source.imu_to_sink.imu --socket /run/user/1000/flowrt/12345.sock
flowrt echo source.imu --image flowrt/selfdesc/selfdesc.json --follow --interval-ms 100
flowrt echo path/to/generated-app source.imu
```

`echo` 默认从 live runtime socket 请求 self-description，再按消息 layout 格式化 payload。也可以用 `--image <path>` 或兼容旧式 `flowrt echo <image> <channel>` 显式指定生成应用二进制或 `selfdesc.json`。`<channel>` 可以写完整 channel 名 `<from>_to_<to>`，也可以写唯一的 source 或 target 端点名，例如 `source.imu`；端点名匹配多条 channel 时需要改用完整 channel 名。

省略 `--socket` 时，CLI 会扫描当前用户 runtime socket 目录。未指定 `--image` 时，需要恰好一个 live FlowRT 进程暴露 self-description；指定 `--image` 时，会选择 `self_description_hash` 与静态 self-description JSON hash 匹配的唯一进程。若多个进程匹配，需要显式传入 `--socket <path>`，避免从错误进程读取 channel。

输出是最小稳定摘要：

```text
channel=source.imu_to_sink.imu type=Imu abi_size=24 published_count=1 published_at_ms=42 payload_len=24 fields={timestamp=1,ax=0.1,ay=0.0,az=9.81} raw=...
```

fixed-size Message ABI 会按 self-description 中的 field offset 和类型格式化整数、浮点、布尔和固定数组。variable frame 会按固定 header + tail layout 格式化 `bytes`、`string` 和 `sequence<T>`；runtime socket 仍只暴露 raw/canonical bytes，字段 schema 来自 self-description。

如果 runtime 还没有该 channel 的 payload，例如当前进程尚未发布该 channel 的样本，输出会包含 `payload_len=0 no payload`。

默认情况下，`echo` 只读取一次 latest snapshot。传入 `--follow` 后，CLI 会按 `--interval-ms <ms>` 指定的间隔持续轮询同一 runtime socket；第一条 snapshot 一定输出，后续只在 `published_count`、`published_at_ms` 或 raw payload 变化时输出，避免没有新发布时重复刷屏。默认轮询间隔是 250 ms。

生成的 Rust/C++ runtime shell 会为当前 process 的 active channel 预注册 live 摘要。控制面常驻，数据面 probe 按需启用：`flowrt echo` 打开 `observe_channel` 连接后，发布路径才会 best-effort 记录 latest payload；连接断开后自动关闭。无观察者时发布热路径只做 channel-local 原子检查，不做 payload 拷贝、frame 编码或 socket 写入。

## `params`

```bash
flowrt params list path/to/generated-app
flowrt params get path/to/generated-app controller.kp
flowrt params set path/to/generated-app controller.kp 2.5
flowrt params set path/to/generated-app controller.mode '"safe"'
```

`params` 操作运行态参数控制面。静态 self-description 用于确认参数属于该应用，并通过 `self_description_hash` 选择匹配的 live runtime socket；实际值来自 runtime socket。省略 `--socket` 时使用与 `echo` 相同的自动发现规则，多个同 hash 进程同时存在时需要显式传入 `--socket <path>`。

参数不是 dataflow channel。RSDL/Contract IR 声明参数 schema，生成 shell 持有 typed params 快照，并在 scheduler tick 边界把 `on_tick` 参数的 pending 值应用到用户组件。用户组件可以实现默认提供的 `on_params_update(old, new, context)` 钩子；该钩子返回 `Ok` 后，新参数才会提交并反映到后续 `on_tick`。

`set` 的值必须是合法 JSON：数字写 `2.5`，布尔写 `true`，字符串需要带 JSON 引号，例如 shell 中常写成 `'"safe"'`。`startup` 参数运行时不可修改；`on_tick` 参数可以提交 pending 值，由生成 shell 在下一个 tick 边界应用。输出格式是行式摘要：

```text
controller.kp type=f32 update=on_tick current=1.0 pending=2.5 min=0.0 max=5.0 choices=[]
```

`zenoh` 示例在跨机时通常需要为两个进程分别注入连接信息。常用环境变量是：

- `FLOWRT_ZENOH_CONNECT`
- `FLOWRT_ZENOH_LISTEN`
- `FLOWRT_ZENOH_MODE`
- `FLOWRT_ZENOH_NO_MULTICAST`
- `FLOWRT_TICK_SLEEP_MS`

前四个用于给 runtime session 注入 zenoh 网络配置。`flowrt launch` 在这些变量都未显式设置时，会为同一个 supervisor 本机启动的 zenoh process 自动分配 `127.0.0.1` TCP mesh；只要设置了任一 `FLOWRT_ZENOH_MODE` / `FLOWRT_ZENOH_LISTEN` / `FLOWRT_ZENOH_CONNECT`，就视为用户接管 session 配置。`FLOWRT_TICK_SLEEP_MS` 用于把 demo 的同步 tick 间隔拉长到可观察窗口。tick 数上限由 `flowrt run --run-ticks <N>` 或 `flowrt launch --run-ticks <N>` 显式传入，不进入核心 runtime scheduler。

## `status`

```bash
flowrt status
```

`status` 扫描当前用户 runtime socket 目录中的 FlowRT 进程，并通过 handshake 验证 PID、package、process、runtime、静态自描述 hash 和 tick/channel 摘要。socket 路径只作为发现入口；CLI 不把文件名当作进程身份事实。

当前 Rust/C++ 生成应用都会启动 status socket，路径优先使用 `$XDG_RUNTIME_DIR/flowrt/<pid>.sock`，没有 `XDG_RUNTIME_DIR` 时使用 `/tmp/flowrt.<uid>/<pid>.sock` 风格的当前用户目录。生成 shell 会把 scheduler tick 计数、active channel 摘要、发布计数、active echo observer 数量和 probe drop 计数写入 live status；payload 只在 echo 数据面 probe 启用期间 best-effort 记录。

generated supervisor 会启动自己的 status socket，`runtime=supervisor`，`process=flowrt_supervisor`。它会按子进程 PID socket 轮询 live status，并额外输出 `supervisor_process=<name>` 行，字段包括 `state`、`pid`、`restarts`、`ticks`、`last_seen_ms`、`tick_stale` 和 `exit_code`。`state` 当前取值为 `starting`、`running`、`stale`、`restarting`、`exited` 或 `failed`。内置 restart policy 是 `on-failure`：子进程异常退出时最多重启 3 次，退避 100ms 起步、上限 1000ms；正常退出不重启。

runtime 启动 status socket 时会先探测同路径 socket 是否仍可连接：仍可连接时拒绝覆盖，避免同机多个进程互相抢占；不可连接时按 stale socket 回收，处理 SIGKILL 后遗留的 socket 文件。

## `hz`

```bash
flowrt hz
flowrt hz source.imu_to_sink.imu
flowrt hz source.imu_to_sink.imu --socket /run/user/1000/flowrt/12345.sock --window-ms 500
```

`hz` 通过 live status 控制面读取 channel `published_count`，等待一个采样窗口后再次读取，并用计数差除以实际 elapsed time 得到发布频率。它不打开 `observe_channel`，不读取 payload，不启用 echo 数据面 probe，因此不会让发布热路径做 payload 拷贝或 frame 编码。

省略 channel 时输出所有 live channel；传入 channel 时只输出完全匹配的 canonical channel 名。省略 `--socket` 时扫描当前用户 runtime socket 目录；多个进程同时存在时会分别输出并带上 socket 路径。`--window-ms` 默认 1000，必须大于 0。

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

profile 投影还会重算来自 profile default 的 bind-level policy：未在 `bind.dataflow` 上显式声明的 `overflow`、`stale_policy` 和 `max_age_ms` 会采用选中 profile 的默认值；bind 上显式声明的 policy 保持不变。未显式声明 `backend` 或声明 `backend = "auto"` 的 bind 会跟随选中 profile backend；如果该 route 使用 variable frame 且 profile backend 为 `iox2`，会自动选择 `zenoh`。`backend` 是单条 route 的属性；同一 `from`/`to` route 只能声明一次，跨 import 的 RSDL 片段会先合并成一个 Contract IR，再由 validator 拒绝多重 incoming bind 或冲突连线。投影后的 `contract.ir.json` 会同时刷新 route 和 deployment 的 capability 元数据。

## RSDL task 写法

单 task 可以继续使用 `[instance.<name>.task]`，归一化后的 task name 为 `main`。一个 instance 需要多个执行单元时，使用数组表 `[[instance.<name>.task]]`，并为每个 task 声明唯一的 `name`：

```toml
[[instance.worker.task]]
name = "fast_loop"
trigger = "periodic"
period_ms = 5
output = ["fast"]

[[instance.worker.task]]
name = "slow_loop"
trigger = "periodic"
period_ms = 100
output = ["slow"]
```

多 task 共享同一个 component 实例。生成 shell 会按 task 的 `input` / `output` 子集分别调度同一个用户组件接口，并在 self-description 和 launch manifest 中保留 task name。

## 生成物边界

`flowrt/` 下的内容由 FlowRT 管理：

- 可以删除。
- 可以重新生成。
- 不应放用户算法代码。
- 不应手写维护生成 runtime shell。
- 不应由多个 `flowrt prepare` / `build` 命令同时写入同一个输出目录；CLI 会通过 `.flowrt.lock` 做 fail-fast 保护。

用户代码应放在项目自己的 `src/` 目录。C++ 用户代码通过生成接口和 `flowrt_user::build_app()` 接入；Rust 用户代码通过生成 trait 和用户模块接入。
