# 快速开始

本文从单包 Debian 包安装 `flowrt`，并跑通 Contract-driven App Authoring 主路径、
Rust-only、C++ only 和 C callback v0 示例。

## 前置条件

- Rust toolchain，支持当前 workspace 使用的 Rust 2024 Edition。
- C++20 编译器、CMake 和 CTest，用于构建 C++ runtime 与 C++ 示例。
- ROS2 bridge 示例需要 ROS2 Jazzy 或之后版本的 C++ 开发环境；运行 bridge 时还需要 `rmw_zenoh_cpp`。CI 当前强制验证 Jazzy 和 Lyrical。

## 安装 FlowRT

在仓库根目录执行：

```bash
scripts/package-deb.sh --output-dir dist
sudo dpkg -i dist/flowrt_*_*.deb
flowrt --version
```

面向用户的入口是系统安装后的 `flowrt ...`。单包 `flowrt` 会同时安装 CLI、Rust runtime crate、C++ runtime header、CMake package、私有 Rust crate vendor、`iceoryx2-cxx 0.9.1`、`zenoh-c 1.9.0` 和 `zenoh-cpp 1.9.0`。这些版本锁定依赖位于 `/opt/flowrt/<version>` 私有前缀，用户项目不需要克隆 FlowRT 仓库，也不需要手动安装 iox2 或 zenoh C++ SDK。Rust 用户组件当前仍通过 Cargo 构建生成 app，因此目标机仍需要 Rust toolchain；C++ 用户组件仍需要 C++20 编译器、CMake 和 CTest。仓库开发者可以用 `cargo run -p flowrt-cli -- ...` 调试 CLI，但文档、示例和对外说明应默认使用系统 PATH 中的 `flowrt ...`。

安装包还会在 `/opt/flowrt/<version>/targets/<platform>` 下提供 target SDK 布局基础。
amd64 安装包会同时提供 `linux-amd64` 本机 SDK 和 `linux-arm64` 交叉 target SDK；
manifest 中 `complete = true` 表示该目录可作为对应架构的 C++ SDK 查找事实源。arm64
安装包当前只承诺本机 `linux-arm64` SDK 完整，不承诺反向 `linux-arm64 -> linux-amd64`
交叉编译。

首次构建前先补全底层依赖缓存：

```bash
flowrt deps --backend all
```

日常项目内也可以按 RSDL 精确预热：

```bash
cd examples/import_demo
flowrt deps
```

`flowrt deps` 只编译 FlowRT 底层依赖；`flowrt build` 只编译用户项目和生成 shell。默认构建模式是 release，用户二进制位于项目自己的 `flowrt/build/bin/release/`。

创建新项目时可以直接使用 `flowrt init`：

```bash
flowrt init my_robot
cd my_robot
flowrt check
```

`flowrt init` 只创建项目入口 `flowrt.toml` 和最小 `rsdl/robot.rsdl`。它不生成默认
component，不写 `app/`，也不替用户创建业务实现。`--lang` 只设置初始 target runtime；
C++ 和 C 项目使用：

```bash
flowrt init my_cpp_robot --lang cpp
flowrt init my_c_robot --lang c
```

声明真实 C component 后，`flowrt prepare` / `flowrt explain` 会展示
`app/c/<component>.c` 和 generated `flowrt_app/c_components.h` callback table 接入线索。
当前 C component 是 fixed-size message 的最小切片，不是完整 C runtime；params、
service、operation、variable frame、`io_boundary` 和 `external` 仍由 validator 拒绝。

继续声明 message 和 component。可以手写 RSDL，也可以用 `flowrt add ...` 做小步编辑：

```bash
flowrt add message Sample value:u32
flowrt add component Source --lang rust --output sample:Sample
flowrt add component CSource --lang c --output sample:Sample
flowrt check
flowrt prepare
flowrt explain
```

`add` 默认使用当前项目 `flowrt.toml` 指向的主 RSDL。`add message` 会追加
`[type.Sample]`；`add component` 会追加同名 component、instance、最小 periodic task，
但不会创建、合并或覆盖 `app/` 用户代码。带 `--input name:Type` 的 component 会先声明
input port，但初始 task 不自动消费 input；需要上游数据时，应补上 `[[bind.dataflow]]`
或 island boundary 后再把 input 加入 task，避免 `flowrt check` 因缺失 incoming bind
失败。`flowrt add component --lang c` 也只编辑 RSDL。
C 用户文件由用户自行放入 `app/c/`。

`flowrt prepare` 会在 `flowrt/` 下生成可重建的 App API 参考产物：

```text
flowrt/app/app_api.json
flowrt/app/implementation.md
flowrt/app/stubs/
```

`flowrt explain` 复用同一 App API 模型，把用户需要实现的 component、handler、params、
service、operation 和 C callback table 线索输出到终端或 JSON。用户参考
`flowrt/app/stubs/` 后，把需要保留的实现手写或复制到项目 `app/`；`prepare` 不直接写
用户 `app/`。

FlowRT app 项目根可以放置 `flowrt.toml` 作为入口 manifest：

```toml
[project]
main = "rsdl/robot.rsdl"
```

在含 `flowrt.toml` 的项目根或子目录中，`flowrt check`、`flowrt explain`、
`flowrt deps`、`flowrt prepare`、`flowrt build`、`flowrt run` 和 `flowrt doctor` 可以
省略 RSDL 路径；CLI 会向上发现最近的 manifest 并使用 `project.main`。显式传入 RSDL
路径时仍以显式路径为准。`flowrt.toml` 只记录项目入口，不替代 RSDL 或 Contract IR
的语义事实源。

交叉编译时，用 target platform 选择 toolchain profile：

```bash
flowrt toolchain init --target linux-arm64
flowrt toolchain show --target linux-arm64
flowrt doctor examples/external_driver_demo/rsdl/robot.rsdl --target linux-arm64
flowrt deps examples/external_driver_demo/rsdl/robot.rsdl --target linux-arm64
flowrt build --launcher examples/external_driver_demo/rsdl/robot.rsdl --target linux-arm64
```

`toolchain init` 会在当前 workspace 下生成 `.flowrt/toolchains.toml`，用户不需要手写完整
配置。如果需要接入板级私有 SDK，可以用 `--sdk-overlay <path>` 指定路径：

```bash
flowrt toolchain init --target linux-arm64 --sdk-overlay /opt/vendor/rknn
```

生成后可以用 `flowrt toolchain show --target linux-arm64` 确认合并后的 profile 是否符合预期；
推荐在进入 `deps/build` 前运行带 RSDL 的 `flowrt doctor <rsdl> --target <platform>`，
让 CLI 按 Contract IR 检查 C++ component 声明的 `pkg_config` 依赖是否能被 selected target
看到。

`--target` 当前支持 `linux-amd64` 和 `linux-arm64`。显式参数优先于 RSDL/Contract IR target
platform；省略时如果选定 Contract IR target 已声明 platform，CLI 会自动使用该 platform，
否则保持 native 构建。Rust/Cargo 路径会传递对应 `--target <rust-target-triple>`，缺少
Rust target 时需要先执行 `rustup target add <triple>` 或配置本机 Rust toolchain。
含 C++/CMake 产物时，CLI 会使用对应 target SDK、toolchain file 或 C/C++ compiler
配置。amd64 安装包内嵌完整 `linux-arm64` FlowRT target SDK；板级私有 SDK 通过
toolchain profile 的 `sdk_overlays`、`cmake_prefix_paths`、`pkg_config_libdirs` 或
`sysroot` 接入，不写进 RSDL。FlowRT 不自动下载系统交叉编译器或板级 SDK。

## 检查 RSDL

先检查模块化 RSDL 示例：

```bash
cd examples/import_demo
flowrt check
```

预期输出类似：

```text
OK package=import_demo types=2 components=2 instances=2 tasks=2 binds=1
generated user API summary:
graph default
  component source language=rust kind=native
    user handlers:
      fn on_tick(&mut self, imu: &mut flowrt::Output<Imu>) -> flowrt::Status
```

`check` 会解析 RSDL、展开 `[package.imports]`、归一化 Contract IR，并运行 validator。
它不会生成或构建应用产物。摘要中的 handler 签名来自同一套 codegen 规则；如果组件声明
了参数，`on_tick` 会显示额外的 typed params 参数，用户不需要等编译失败后再去翻生成头文件。

## 生成产物

```bash
cd examples/import_demo
flowrt prepare
```

生成物写入示例项目下的 `flowrt/`：

```text
examples/import_demo/flowrt/
  app/
    app_api.json
    implementation.md
    stubs/
  contract/contract.ir.json
  build/
  launch/launch.json
  rust/
  cpp/
```

`flowrt/app/app_api.json` 是 App API manifest，`flowrt/app/implementation.md` 是用户实现
清单，`flowrt/app/stubs/` 是参考模板。它们和其他 `flowrt/` 内容一样由 FlowRT 管理，
可以删除后重新生成。用户算法代码应放在项目自己的 `app/` 目录，不放进生成目录；
`prepare` 不直接修改用户 `app/`。

查看已落盘的 Contract IR 摘要：

```bash
flowrt inspect examples/import_demo/flowrt/contract/contract.ir.json
```

## 运行 Rust-only 示例

```bash
cd examples/import_demo
flowrt deps
flowrt build --launcher
flowrt run --process main
```

也可以通过生成的 supervisor 启动全部 process group：

```bash
flowrt launch rsdl/robot.rsdl
```

当前 `import_demo` 是 Rust-only inproc 示例，适合验证 RSDL import、Contract IR、Rust codegen 和 launch manifest 的基础闭环。

## 运行 Island Mode 单功能单位示例

`examples/island_demo` 只包含一个 Rust component，profile 显式声明 `mode = "island"`。
示例用 `boundary.input sample_in` 代替尚未接入的上游，用 `boundary.output result_out`
暴露可对比输出，因此不需要先搭完整 graph 也能测试组件 IO。

```bash
cd examples/island_demo
flowrt deps --backend inproc
flowrt build --launcher
flowrt run --process main
```

另开一个终端注入输入并观察输出：

```bash
flowrt pub sample_in \
  --json '{"seq": 7, "value": 21}' \
  --image flowrt/selfdesc/selfdesc.json \
  --published-at-ms 1000
flowrt echo result_out --image flowrt/selfdesc/selfdesc.json
```

`echo` 会按 self-description 把 canonical payload 格式化成字段，例如 `seq=7` 和
`doubled=42`。fixed ABI、canonical frame 和显式空消息都走同一条 boundary 观测路径。
如果要留下对比证据，可以把输出录成 MCAP：

```bash
flowrt record --output island.mcap --duration 500ms --channel result_out
```

Island Mode 是可拆卸脚手架。组件行为稳定后，删除 `boundary.input` / `boundary.output`，
补上普通 `[[bind.dataflow]]`，并把 profile 切回默认 `strict`，同一份用户算法代码不需要改。

如果输入含 `string`、`bytes` 或 `sequence<T>`，可以参考
`examples/variable_frame_island_demo`。它用 `flowrt pub --file --freq` 从 JSONL 注入
`sequence<f32>`，再用 `flowrt echo` 观察 fixed summary：

```bash
scripts/test-v091-variable-frame-island-demo.sh
```

把旧系统逐功能单位迁到 FlowRT 时，推荐把 live topic、bag 片段或测试 fixture 先在
FlowRT 外部转换成 RSDL 字段自然 JSONL，再按同一条路径验证：

```bash
flowrt params set --image flowrt/selfdesc/selfdesc.json --file params.json
flowrt pub scan_in --file samples.jsonl --freq 100 --image flowrt/selfdesc/selfdesc.json
flowrt echo summary_out --image flowrt/selfdesc/selfdesc.json
flowrt record --output island.mcap --duration 2s --channel summary_out
```

这条路径同样适用于普通开发中“只先写一个功能单位”的 IO 测试。FlowRT 仍不会在
`strict` 模式下 warning 后生成残缺拓扑；残缺输入输出必须显式建模为 island
boundary，验证完成后再拆掉。

如果源 RSDL 想保持 `strict`，可以用 CLI 触发临时 island projection，而不是手改
`.rsdl`：

```bash
flowrt build robot.rsdl \
  --temporary-island \
  --boundary-input scan_in=planner.scan \
  --boundary-output summary_out=planner.summary
flowrt replay --file fixture.jsonl --image flowrt/selfdesc/selfdesc.json --as-fast-as-possible
flowrt echo summary_out --image flowrt/selfdesc/selfdesc.json
```

临时 projection 只影响本次生成物，`selfdesc.json` 和 `launch.json` 会标记
`temporary_island=true` 与 `test_only=true`。它仍属于 island 测试脚手架，默认不能
`bundle` 或 `deploy` 为生产产物；验证完成后，应移除命令行 overlay，或把真实拓扑补成
普通 dataflow bind。

## 运行 C++ only 示例

```bash
cd examples/cpp_counter_demo
flowrt deps
flowrt build --launcher
flowrt run --process control
flowrt launch rsdl/robot.rsdl
```

C++ only contract 的普通 `build` / `run` 走 CMake app 路径，不依赖 Cargo app。需要 `launch` 时，先用 `build --launcher` 显式构建 generated supervisor，再由 `launch` 执行已有 supervisor。用户 C++ 组件通过生成接口和 `flowrt_user::build_app()` 注入。

## 运行 C callback v0 示例

`examples/c_counter_demo` 是 C component v0 的最小闭环。它只使用 fixed-size `Count`
message、两个 C native component 和 `inproc` channel；用户代码位于
`examples/c_counter_demo/app/c/`，实现 generated `flowrt_app/c_components.h` 声明的
callback table factory。

普通 app 运行先构建 CMake app，再运行已构建产物：

```bash
flowrt build examples/c_counter_demo/rsdl/robot.rsdl
flowrt run examples/c_counter_demo/rsdl/robot.rsdl --run-steps 3
```

supervisor 路径需要先构建 launcher，再 launch：

```bash
flowrt build --launcher examples/c_counter_demo/rsdl/robot.rsdl
flowrt launch examples/c_counter_demo/rsdl/robot.rsdl --run-steps 3
```

C v0 通过 C ABI callback table 静态编进 generated C++ runtime shell，不是完整 C
runtime；params、service、operation、variable frame、`io_boundary`、`external`、
动态加载和 Python binding 均不在当前支持范围。

## 运行 external package 示例

external component 由外部 package/executable 提供，FlowRT 负责声明、校验、启动和观测。
`examples/external_driver_demo` 不依赖真实硬件，只验证 external package 主路径：

```bash
flowrt external check examples/external_driver_demo/external/fake_sensor_driver
flowrt deps examples/external_driver_demo/rsdl/robot.rsdl
flowrt build --launcher examples/external_driver_demo/rsdl/robot.rsdl
flowrt launch --run-steps 2 examples/external_driver_demo/rsdl/robot.rsdl
```

打成离线 bundle：

```bash
flowrt bundle examples/external_driver_demo/rsdl/robot.rsdl --output dist/external-driver-demo
flowrt deploy dist/external-driver-demo --host user@host --target edge --remote-dir /opt/external-driver-demo --dry-run
```

实际部署时，目标机器需要安装同版本 FlowRT deb；`deploy` baseline 通过 SSH/SCP 上传 bundle，不负责远端安装系统包。

## 切换 profile

```bash
flowrt check examples/profile_switch_demo/rsdl/robot.rsdl
flowrt deps examples/profile_switch_demo/rsdl/robot.rsdl --profile iox2
flowrt build --profile iox2 examples/profile_switch_demo/rsdl/robot.rsdl
flowrt run --profile iox2 examples/profile_switch_demo/rsdl/robot.rsdl
```

`build --profile <name>` 会先投影 Contract IR，只保留选定 profile 的 deployment 视图，并让未显式写在 `bind.dataflow` 上的 channel policy 使用该 profile 的默认值，再校验和生成对应产物。`run --profile <name>` 只校验已生成产物的 profile 是否匹配，不会临时重生成。选择 `iox2` 或 `zenoh` profile 时，Rust 生成物会启用 runtime crate 的对应 feature；含 C++ backend 组件时，生成 CMake 会优先使用 FlowRT 安装包内 `/opt/flowrt/<version>` 的私有 SDK，缺失时才要求显式设置 `FLOWRT_CPP_RUNTIME_DIR` 或 `CMAKE_PREFIX_PATH`。C++ 项目可以用 `flowrt build --target linux-arm64 ...` 选择交叉编译 toolchain profile；此时 CMake 会要求 `/opt/flowrt/<version>/targets/linux-arm64` 或显式配置位置下存在 `complete = true` 的 target SDK，并把其 CMake/pkg-config 路径作为目标架构事实源。

## Supervisor readiness 和资源提示

多进程应用可以用 `[[process]]` 声明进程依赖、readiness gate 和资源提示：

```toml
[[process]]
name = "sensors"
readiness = "runtime_ready"
startup_delay_ms = 200

[[process]]
name = "control"
depends_on = ["sensors"]
readiness = "service_ready"
cpu_affinity = [0, 1]
nice = -10
env = { MY_ROBOT_MODE = "production" }
```

`readiness` 控制 supervisor 何时认为子进程就绪：

- `process_started`：进程 spawn 即就绪（默认）。
- `runtime_ready`：等待 introspection socket 握手。
- `service_ready`：额外检查所有 service endpoint 就绪。

`startup_delay_ms` 在进程之间加入错峰延迟。`cpu_affinity` 绑定 CPU 核心，`nice` 设置进程优先级，`env` 注入环境变量。

构建后用 `flowrt launch` 启动，`flowrt status` 会展示每个进程的 readiness 等待状态和资源应用情况：

```bash
flowrt build --launcher examples/mixed_zenoh_demo/rsdl/robot.rsdl
flowrt launch examples/mixed_zenoh_demo/rsdl/robot.rsdl
```

另开终端查看状态：

```bash
flowrt status
```

supervisor_process 行会包含 `readiness_wait=runtime_ready`（正在等待 readiness）和 `resource_placement={...}`（desired/applied 资源状态）。

## 查看运行态参数

含参数的应用运行时会启动 introspection socket。可以在另一个终端用静态 self-description 匹配 live process，并查看或提交参数 pending 更新：

```bash
flowrt build --launcher examples/imu_demo_iox2/rsdl/robot.rsdl
FLOWRT_TICK_SLEEP_MS=20 flowrt launch --run-steps 500 examples/imu_demo_iox2/rsdl/robot.rsdl
```

另开一个终端查询或提交参数：

```bash
flowrt params list --image examples/imu_demo_iox2/flowrt/selfdesc/selfdesc.json
flowrt params get --image examples/imu_demo_iox2/flowrt/selfdesc/selfdesc.json estimator.gravity
flowrt params set --image examples/imu_demo_iox2/flowrt/selfdesc/selfdesc.json estimator.gravity 9.7
```

跨机远程参数控制需要 zenoh 网络连通。加上 `--remote` 即可通过 zenoh control-plane 发现远端 runtime：

```bash
flowrt params list --image examples/imu_demo_iox2/flowrt/selfdesc/selfdesc.json --remote
flowrt params set --image examples/imu_demo_iox2/flowrt/selfdesc/selfdesc.json estimator.gravity 9.7 --remote
```

如果同一 zenoh 网络中有多个匹配的 runtime，CLI 会提示候选 `key expression`，再用 `--runtime <key_expr>` 显式选择目标。`--socket` 只用于本机 Unix socket 路径，不能和 `--remote` 同用。

`params set` 的值必须是合法 JSON。`on_tick` 参数会在下一个 tick 边界通过用户组件的 `on_params_update` 钩子提交；`startup` 参数运行时不可修改。

## 查看调度健康

多 task app 可以用 `concurrency = "parallel"` 和显式 `lane` 表达可并发执行的任务。
lane 是串行调度队列，不是线程；不同 lane 的 ready task 才可能被 worker 并发执行。
输出提交采用 two-phase 模型：task 返回 `Ok` 后由 scheduler 提交，非 `Ok` 本次输出丢弃。

`flowrt status` 在展示进程状态的同时，还会输出 task 级和 lane 级调度健康指标：

```bash
flowrt status
```

task 健康行示例：

```text
task_health=fast_loop lane=sensor_lane deadline_missed=0 stale_input=2 backpressure=0 overflow=0 fairness_violations=0 runs=1000 successes=998 consecutive_failures=0 last_run_ms=... last_success_ms=... socket=...
```

lane 健康行示例：

```text
lane_health=sensor_lane queue_depth=0 dispatched_count=1000 fairness_violations=0 socket=...
```

关键字段含义：

- `deadline_missed`：task 执行超过 `deadline_ms` 的次数。超限时 runtime 会阻止 late output 发布。
- `stale_input`：输入数据超过 `max_age_ms` 的次数。
- `backpressure`：下游队列满导致的背压事件次数。
- `overflow`：channel 溢出事件次数。
- `fairness_violations`：lane 饥饿公平性违规次数。
- `queue_depth`：lane 当前排队任务数。
- `dispatched_count`：lane 累计调度次数。

这些指标由 runtime 内置的调度健康策略自动采集，Rust 和 C++ 生成 shell 行为一致。所有健康字段使用 `serde(default)` 保证前向兼容，旧版 JSON 不含健康字段时解析为零值。

## 录制运行态事件

应用运行时可以用 `flowrt record` 把 FlowRT 事件写入 MCAP 文件。该命令只需要 live runtime socket，不需要 RSDL 源文件：

```bash
flowrt record --output run.mcap --duration 5s --all
```

如果同一台机器有多个 FlowRT runtime，先用 `flowrt status` 查看 socket，再显式选择：

```bash
flowrt status
flowrt record --output imu.mcap --channel source.imu_to_sink.imu --socket /run/user/1000/flowrt/12345.sock
```

录制期间 runtime 按需开启数据面 tap；没有执行 `flowrt record` 时，发布热路径不会持续复制 payload。命令结束时会输出 `event_count`、`dropped_count` 和 `bytes_written`，用于判断本次录制是否发生丢弃。

## Service request/response 示例

Service 是 request/response 语义，和 channel（dataflow push）不同。client 发起请求后
等待 server 返回结果，适合路径规划、参数查询等低频请求场景。

构建并运行 service_demo：

```bash
flowrt build examples/service_demo/service_demo.rsdl
flowrt run examples/service_demo/service_demo.rsdl --process main --run-steps 50
```

该示例中 `planner` 组件每 100ms 调用一次 `plan_service` 的 plan service，server
根据请求的 goal 值返回接受或拒绝。运行后可以看到 `result` 输出端口的值随 service
响应变化。

查看 service 拓扑和运行态健康：

```bash
flowrt list examples/service_demo/flowrt/selfdesc/selfdesc.json
flowrt status
```

`flowrt list` 展示 service endpoint 的 client/server 绑定和 request/response 类型；
`flowrt status` 展示每个 service 的 ready、in_flight、queued、timeout、busy 等运行态指标。

Service 与 channel 的区别：channel 是 publish/subscribe，生产者写入后不等消费者处理；
Service 是 call/response，client 阻塞或轮询等待 server 返回。Service 与参数热更新的区别：
参数是 runtime control-plane 的配置值，Service 是 graph 业务逻辑的一部分。

## Operation long-running command 示例

Operation 用于表达带 goal、feedback、result、状态和取消入口的长耗时命令。构建并运行
最小示例：

```bash
flowrt check examples/operation_demo/rsdl/robot.rsdl
flowrt build --launcher examples/operation_demo/rsdl/robot.rsdl
flowrt run --run-steps 5 examples/operation_demo/rsdl/robot.rsdl --process main
```

查看 Operation 拓扑：

```bash
flowrt op list --image examples/operation_demo/flowrt/selfdesc/selfdesc.json
```

输出会包含 `operation=controller.plan`、client/server 端口、goal/feedback/result 类型、
backend 和 policy 摘要。Operation 不是 Service 别名；内部 start/cancel/status service
只属于生成物 lowering 和调试视图。

## ROS2 bridge 示例

FlowRT 与 ROS2 的 bridge 固定走 `zenoh`，ROS2 侧必须使用 `rmw_zenoh_cpp`，不会回退到 DDS。ROS2 bridge adapter 进程使用 ROS2 安装中的 `zenoh_cpp_vendor`，以匹配 `rmw_zenoh_cpp` 的同进程 ABI。构建前 source ROS2 环境即可；生成 CMake 会把 `AMENT_PREFIX_PATH` 映射到 `CMAKE_PREFIX_PATH`。当前示例把 FlowRT `TextFrame.data` 发布到 ROS2 `/flowrt/text`：

```bash
source /opt/ros/jazzy/setup.bash
flowrt build --launcher examples/ros2_bridge_demo/rsdl/robot.rsdl
flowrt launch --run-steps 200 examples/ros2_bridge_demo/rsdl/robot.rsdl
```

另开 ROS2 环境终端观察：

```bash
source /opt/ros/jazzy/setup.bash
export RMW_IMPLEMENTATION=rmw_zenoh_cpp
ros2 topic echo /flowrt/text --once
```

如果 `ros2 topic echo` 没看到刚启动的 topic，先执行 `ros2 daemon stop` 后重试。

## 出错时先看什么

- `flowrt check` 失败：优先修正 RSDL 命名、类型、端口、task、bind、target/backend 声明。
- `flowrt build` 失败：检查用户组件实现是否匹配生成接口，以及 Rust/C++ toolchain、CMake、FlowRT 安装前缀是否存在。
- `flowrt run --process <name>` 失败：先确认已经执行过匹配 profile 的 `flowrt build`；再确认 process 名称来自 RSDL `instance.<name>.process`；mixed contract 必须选择单语言 process，或使用 `flowrt launch`；`inproc` backend 下不能单独运行带跨 process dataflow 的 process group。
- `flowrt launch` 失败：先确认已经执行过匹配 profile 的 `flowrt build --launcher`；再检查 `flowrt/launch/launch.json` 是否生成；确认 mixed process group 没有把 C++ 和 Rust component 放在同一 process 内；如果 backend 是 `inproc`，还要确认 dataflow bind 没有跨 RSDL process group。
