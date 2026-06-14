# CONTEXT.md

本文档记录 FlowRT 仓库的当前上下文，供 coding agent 和维护者快速了解现状。它会
随版本演进更新；长期架构原则、语义约定、文档边界和提交纪律维护在 `AGENTS.md`。

## 当前版本背景

当前 workspace 版本为 `0.12.0`；`v0.12.0` 完成 Contract-driven App Authoring
发布收口。当前用户主路径
已经统一为 `flowrt.toml`、`rsdl/`、`app/` 和可重建的 `flowrt/` 生成目录：
`flowrt.toml` 记录 `[project].main = "rsdl/robot.rsdl"`，
`rsdl/` 放系统契约，`app/` 放用户算法，`flowrt/` 只放 FlowRT 管理产物。CLI 已新增
`flowrt init [path] --lang <rust|cpp|c>`，只生成现代项目入口和最小
`rsdl/robot.rsdl`；默认 Rust，可显式把初始 target runtime 设为 C++ 或 C，但不生成
默认用户实现文件、C++ `build_app()` 或 C callback table factory。`flowrt add
message/module/component` 省略 RSDL 时从 `flowrt.toml` 找主 RSDL，可追加根 message、
当前可解析的 `rsdl/modules/*.rsdl` workspace module 注册，以及 Rust/C++/C native
component、同名 instance 和最小 periodic task；写入前会重新解析、归一化并校验更新后
的 Contract IR，且不创建、追加或覆盖 `app/` 用户代码。`check`、`explain`、`prepare`、
`build`、`run`、`deps` 和 `doctor` 省略 RSDL 路径时会从当前目录向上发现最近的
`flowrt.toml`；
显式 RSDL 路径优先。`launch`、`bundle` 和 `deploy` 仍要求显式路径或 bundle 参数。
`flowrt explain [rsdl] [--format text|json]` 已开放，输出 component 用户实现路径、接口、
task、params、service 和 operation 摘要；C component 会展示 `app/c/<component>.c`
和 `flowrt_app/c_components.h` callback table 接入线索。手写或已有 RSDL 中
`language = "c"` 的 native component 已可由 codegen 生成 C++ adapter：C 用户文件放
`app/c/**` 并实现 generated header 声明的静态 callback table factory，generated C++
runtime shell 通过 C ABI callback table 转发生命周期和 periodic/on_message/startup/shutdown
task callback；callback table 必须设置 `FLOWRT_ABI_FEATURE_C_COMPONENT_CALLBACKS_V0`
且不得设置未知 feature bit。`examples/c_counter_demo` 覆盖 fixed-size `Count` message、
两个 C native component、CMake app build/run 和 supervisor launch；generated supervisor
已将 launch manifest 中的 `runtime_kind = "c"` 显式映射到 CMake app binary。CI 已切到
amd64/arm64 `v0.12.0 Authoring Smoke`：在临时项目中覆盖 `flowrt init` Rust/C++/C、
`flowrt add message/component`、`flowrt check`、`flowrt prepare` 和
`flowrt explain --format text/json`，确认 `init/add/prepare` 不写用户 `app/`，并在临时
`import_demo`、`cpp_counter_demo` 和 `c_counter_demo` 副本上按 runner 架构运行普通
build/run；package/release 依赖链会等待该 gate，`scripts/check-release-readiness.sh`
也会检查该 gate、`CHANGELOG.md` 版本段和 `CONTEXT.md` 当前版本状态。C v0 仍只支持 native component、
fixed-size plain data 输入/输出和 inproc demo，不支持 params、service、operation、
variable frame、`io_boundary`、`external`、`pkg_config`、动态加载、独立 C runtime 或
Python binding。`v0.11.1` 定位为 App SDK / C ABI v0 hardening：不扩展 C 功能，不引入
Python，只收紧 `flowrt init/add/explain` 诊断、C callback table 失败原因、用户接入边界
和发布就绪检查。RSDL / Contract IR / validator 现已承载
v0.10.0 并发
语义基础：component 可声明 `concurrency = "exclusive" | "parallel"`，task 可选声明
同名字段，未声明时默认继承 component 并解析为 `exclusive`；normalized
`ComponentIr` / `TaskIr` 会同时保留 resolved 值和用户显式来源。validator 当前采用
保守规则：task 声明 `parallel` 只有在所属 component 也声明 `parallel` 时才合法，task
不会隐式提升 component；`worker_threads = 1` 仍允许 parallel 声明，只是运行时行为
退化为串行。self-description / launch manifest 现已落盘 task `concurrency`、scheduler
task `lane` / `concurrency` 元数据；Rust/C++ generated runtime shell 已从串行
`scheduler.run_ready(...)` 主路径切到 `ReadyBatch` / `WorkerPool` admission。默认
`exclusive` component 继续保护同一用户对象串行访问；显式 `parallel` component 才生成
`Send + Sync` 用户 trait / interface，并按显式 lane 让不同 ready task 真正跨 worker
并行执行。Contract IR 现为 dataflow route 派生 `thread_affinity` metadata：`iox2`
为 `scheduler_local_commit`，`inproc` / `zenoh` 为 `send_safe`；validator 会重新推导并拒绝
手工篡改，launch manifest 与 self-description 会暴露该字段。Rust/C++ generated
runtime shell 已采用 two-phase output commit：worker 只执行用户 task 并收集 task-local
output，scheduler 线程在 `Status::Ok` 后按 ready batch canonical order 提交 backend
输出，`Retry`、`Error`、panic 和 exception 都丢弃本次 output。Rust iox2 endpoint 仍由
scheduler/local owner 持有，不做 unsafe `Send/Sync` 包装；含 iox2 route 的 task 仍可跨
worker 并发执行，只是 transport commit 留在 scheduler 线程。`flowrt list` 和 live
`flowrt status` 会展示 route `thread_affinity`，让用户能区分 task 并发执行与 backend
transport commit 线程亲和。`v0.10.1` 收紧了 CLI
工程化细节：
boundary input 注入优先使用 canonical frame layout，zenoh/boundary echo 优先按
canonical frame 解码而 inproc fixed channel 继续按 native Message ABI 解码；显式
`--socket` 指向 FlowRT 管理目录内 stale runtime socket 时，self-description 和静态
image hash 校验路径会删除已确认失效的 socket 并返回明确错误；native C++ build 现在会
应用 host toolchain profile 的编译/链接选项，native pkg-config 通过 `PKG_CONFIG_PATH`
扩展用户路径，不再用 `PKG_CONFIG_LIBDIR` 隔离系统默认搜索路径。`v0.9.0` 是 Island
Mode / Boundary Endpoint 版本，
用于支持单功能单位开发、ROS2 项目逐功能包迁移、边界输入输出和 `flowrt pub`；
`v0.9.1` 在此基础上补齐迁移验证常用工具：canonical frame JSON 注入、`pub --file
--freq`、`params set --file`、多 channel `echo` 和显式空消息。
已接入 RSDL 语法、Contract IR normalization 和 validator 拓扑规则：profile mode canonical 为
`strict` 或 `island`；graph 级 `BoundaryEndpointIr` 记录 stable id、name、direction、
真实 `instance.port` 引用和解析后的 `TypeExpr`，并按方向和名称稳定排序；strict
profile 拒绝 boundary endpoint，island profile 下 typed boundary input 可以满足 task
active input，但同一 input port 不允许同时由 dataflow bind 和 boundary input 满足。
self-description 和 launch manifest 已暴露 profile/graph mode 与 typed
`boundary_endpoints`；`flowrt list` 可以展示 `island_profiles`、`boundary_endpoints` 和
每个 boundary input/output 的绑定端点。Rust/C++ runtime 已提供显式
`BoundaryInput` / `BoundaryOutput` primitive：boundary input 使用 latest snapshot 注入、
revision、stale policy 和 scheduler waiter 唤醒；boundary output 使用 sink guard 自动
回收临时观测。Rust/C++ generated runtime shell 已接入 boundary primitive：island
boundary input 会参与 `on_message` revision/wake 和 task 输入读取，boundary output 会在
用户输出后发布到显式 sink，strict 生成物不携带 boundary 字段。CLI `flowrt pub` 已接入
fixed Message ABI、canonical frame JSON、JSONL/JSON array 文件流和 wall-clock `--freq`
注入，只允许写 boundary input；显式 `empty = true` 空消息使用零长度 wire payload，
可通过 `{}` 或 `null` 注入；`flowrt params set --file` 可用于迁移测试参数批量导入；
`flowrt echo` 支持单 channel 旧格式和多 channel `channel=<name>` 前缀输出；`flowrt status`
和 `flowrt record` 已能围绕 boundary output 做观测/录制；`flowrt bundle` / `flowrt deploy`
默认拒绝 island 脚手架产物，只有显式 `--allow-island` 才允许。ROS2/zenoh boundary
adapter 已进入窄切片：`[[bridge.ros2]].flowrt` 可以引用普通 `instance.port`，也可以
在 island profile 下引用 `boundary.input` / `boundary.output` 名称；Contract IR 会保留
可校验的 boundary endpoint 引用，generated shell 通过 zenoh-only bridge key 把 ROS2
输入注入 boundary input，并把 boundary output 发布给 ROS2 adapter。`examples/island_demo`
已提供无硬件依赖的最小闭环：`flowrt pub` 向 `sample_in` 注入 typed JSON，组件处理后
通过 `result_out` 供 `flowrt echo` / `flowrt record` 观察；`examples/variable_frame_island_demo`
展示 `string` / `sequence<f32>` boundary input、`pub --file --freq` JSONL 注入和 fixed
summary 输出。迁移旧系统或普通单功能单位开发时，外部 live topic、bag 片段或测试
fixture 应先在 FlowRT 外部转换成 RSDL 字段自然 JSONL 或 JSON array，再通过 island
boundary input 注入；FlowRT 不做 ROS2 drop-in，不在 core 中实现 ROS2 message 语义，
也不让用户代码直接读取 rosbag。后续 `flowrt replay` 或 bag 播放能力必须是
runtime/control-plane 注入：像真实传感器一样把样本灌入 FlowRT 输入，让用户组件只看到
普通 FlowRT input、`on_message` 和 latest view。replay/bag 注入只属于 island 语义；
strict 生产模式必须拒绝，避免真实上游和测试输入形成多来源数据竞争。为了避免临时测试
反复修改 `.rsdl`，后续应支持 CLI 触发的临时 island overlay：基础 RSDL 保持 strict，
CLI 生成一次性的 test-only island projection、manifest 和 self-description，显式声明
哪些端口成为 boundary input/output；该产物仍按 island 规则被 `bundle` / `deploy`
默认拒绝，除非显式允许。CI 已加入 amd64/arm64 的 `v0.9.0 Island Demo Smoke` 和
`v0.9.1 Island Migration Tooling Smoke`，smoke 会按 CI runner 架构只改写临时 demo
RSDL 的 target platform，避免 arm64 runner 误按示例默认 `linux-amd64` target 构建；
发布就绪脚本也会检查该 focused gate。最终集成 hardening 已收掉 generated Rust fixed
`WireCodec` 的 `cursor` unused warning：生成代码在 encode/decode 末尾断言 cursor
等于 `WIRE_SIZE`，避免 release smoke 输出噪声。

为后续 runtime shell 真并发 dispatch 做准备，Rust/C++ runtime executor 已补齐共同
primitive：`DeterministicExecutor` 保留原有串行 `run_ready`，同时新增稳定的
`ReadyBatch` / `run_ready_parallel` admission 形态，按 ready set 的确定性顺序为每个 lane
一次只取一个 task，并在 dispatch 前更新 `lane_last_dispatched_tick` 后从 ready set
移除。`WorkerPool` 两侧也已对齐 `close_admission`、`drain`、panic/exception 聚合为
`Status::Error`、空队列 shutdown 和多 worker active 计数释放语义；用户组件 API 仍不
直接暴露这些 executor primitive，后续 generated shell 只需复用该边界。
runtime 已补齐 output transaction primitive：Rust/C++ `Output<T>::write(...)`
用户 API 保持不变，generated shell 可以在 worker 回调内使用 task-local output，回调
返回后把输出转成带 task、port、payload、`published_at_ms` 和 `tick_time_ms` 的 commit
record，并通过 `ReadyBatch` collect 路径把结果按原 ready 顺序交还 scheduler 线程；
只有 `Status::Ok` 的结果保留可提交 output，`Retry`、`Error`、panic 和 exception
都会丢弃本次输出。
Rust/C++ generated runtime shell 已接入 two-phase output commit 主路径：
scheduler 仍负责 backend output commit，worker 只执行用户 task 并返回可提交 closure。
commit 顺序按 ready batch canonical task order，而不是 worker 完成顺序；`iox2` 路径不再
依赖整批 `run_local` 作为并发规避手段。

`flowrt cache status/clean` 已用于解释和安全清理 FlowRT deps cache、项目 build 目录、
incremental cache 和 stale 临时候选。清理命令必须按默认可清、条件可清、仅展示、
永不自动清区分，不得自动删除安装前缀、用户 SDK overlay、`.flowrt/toolchains.toml`、
最终二进制、live socket、MCAP 或日志。用户最终二进制通常不是磁盘大头；GB 级占用
主要来自 Cargo/FRT deps cache、中间产物、多 target、多 feature 和 vendor hash。

CLI 已新增正式 `flowrt explain [rsdl] [--format text|json]`：命令与
`check` 一样只解析、归一化和校验 RSDL，不写生成物；省略 RSDL 路径时复用
`flowrt.toml` 的 `project.main` 发现。`check` 保持简短校验和 generated handler 摘要，
`explain` 负责输出完整 component 实现说明，包括 package / graph / profile mode、
component language/kind、建议用户文件路径、task trigger/readiness/lane/concurrency、
`on_tick` / `on_params_update` 签名、params、输入输出以及 service / operation handle。
JSON 输出由结构体序列化生成，供 agent 和工具稳定消费；`language = "c"` component
会展示 `app/c/<component>.c` 用户文件建议路径和 `flowrt_app/c_components.h` callback
table 接入线索。

当前仍只承诺 `linux-amd64 -> linux-arm64` 交叉编译，不承诺
`linux-arm64 -> linux-amd64`。amd64 deb 继续内嵌完整 `linux-arm64` target SDK，包含
FlowRT C++ runtime、`iceoryx2-cxx`、`zenoh-c`、`zenoh-cpp`、CMake package 和
pkg-config 事实源。RSDL target 描述目标语义，toolchain profile 描述本机如何编译；
交叉编译器、sysroot、CMake toolchain、pkg-config 路径、SDK overlay、C++ compile/link
args 和 runtime dependency policy 都属于 toolchain/profile 配置，不写入 RSDL 或
Contract IR。

CI/release 侧使用按架构隔离的 Rust/Cargo cache 和外部 `FLOWRT_CACHE_DIR` 降低重复
构建成本。`v0.8.6 Cross UX SDK Smoke` 固定在 amd64 host 上安装 package job 产出的
amd64 deb，准备公开 arm64 SDK overlay，运行 `toolchain init/show`、带 RSDL 的
`doctor`、`deps/build --target linux-arm64`，并检查 AArch64 ELF。deb 成品、release
notes 和 artifact manifest 不缓存，仍每次从源码重建。

`v0.8.1` 是 `v0.8.0` 之后的大 payload descriptor
小升级，聚焦标准 64 字节 FrameDescriptor、I/O boundary descriptor port 绑定、
descriptor-only 观测/录制、`frame_descriptor_demo` 示例、microbench 和 v0.8.1
focused release gate。

`v0.8.4` 是在 `v0.8.3` 交叉编译基础上的板级私有依赖工程化小升级，聚焦 component
build 的 pkg-config 依赖声明、toolchain profile 的 C++ compile/link 选项，以及
`cpp_link_libraries` 对板级私有 SDK 裸库或私有 `.so` 的注入。

`v0.8.5` 是 `v0.8.4` 之后的公开交叉 SDK 示例与验证线：仓库提供
`examples/cross_sdk_deps` 作为显式 prepare 项目，用 CMake 拉取并交叉编译公开真实依赖
`libjpeg-turbo` 和 `Arm KleidiAI` 到 demo-local arm64 SDK overlay；FlowRT 构建阶段只
消费 `component.build.pkg_config` 和 toolchain profile 中的 overlay，不隐式联网。对应
示例 `libjpeg_cross_demo` 覆盖平台无关 C/C++ 库，`kleidiai_cross_demo` 覆盖 Arm 专用
公开 SDK，CI 使用安装后的 amd64 deb 执行 `flowrt doctor/deps/build --target
linux-arm64` 并检查 AArch64 ELF。

`v0.8.0` 已发布，是真实机器人应用接入边界版本，聚焦 I/O boundary component、
variable frame 工程化、FrameDescriptor / side-channel descriptor、ROS2 zenoh 共存
桥接扩展、多目标部署闭环和 v0.8.0 focused release gate。

`v0.7.1` 已发布，是 `v0.7.0` 之后的 hardening 版本，聚焦现有能力的生产边界修复：
deploy/bundle 参数边界、supervisor 子进程生命周期与关闭路径、runtime introspection
控制面、Service / Operation / backend 错误分类、Contract IR 派生元数据防篡改、
C++/Rust runtime parity、安装包离线依赖标记和 `--run-steps` / `record` / `hz` 等
调试主路径。

`v0.7.0` 已发布，核心主题是 external process / driver package typed 接入边界、
ARM64/跨机器部署 baseline 和离线 bundle 交付闭环。FlowRT 主项目不做硬件 backend；
Linux 和外部 driver package 管硬件，FlowRT 管结构、执行、通信、观测、external
process 生命周期和 typed 接入边界。

`v0.6.1` 已发布，是构建/打包可靠性小升级：引入 `flowrt deps` 预热共享底层依赖
cache，`flowrt build` 默认 release 并只构建用户项目，用户二进制统一落在
`flowrt/build/bin/release/`。

`v0.5.0` 已发布，核心主题是 launch-grade
supervisor、参数控制面、高频调度硬化和 FlowRT core skills 套组：补齐 readiness
gate、错峰启动、env 注入、CPU affinity / priority 资源提示、远程参数控制面、
task/lane 调度健康观测和 v0.5.0 focused release gate。

`v0.4.0` 已发布，核心主题是 Service runtime：生成 Rust/C++ service client/server
用户 API，支持 `inproc` 与 `zenoh` service transport，补齐 request/response frame、
错误语义、self-description、`flowrt list` / `flowrt status` 观测和示例文档，并完成
amd64 + arm64 release 安装包闭环。

`v0.3.0` 已发布，核心主题是 Scheduler v2：task-centric scheduler plan、
`startup` / `shutdown` task、`on_message` readiness、serial lane、worker thread
配置、channel revision/cache 和同一 scheduler step 内的 drain loop 级联唤醒。

`v0.3.1` 聚焦复杂系统结构能力：

- workspace / module / composition 机制。
- module name resolver 和跨模块引用。
- supervisor 编排增强，包括 process 依赖顺序、故障传播和可配置 restart policy。
- Service 请求/响应语义切片。
- C/Python ABI 边界准备：先稳定 C ABI 基础类型，不实现 Python binding。

`v0.3.2` 定义为 hardening / architecture repair 版本：不新增用户语义，不推进
v0.4 Service runtime，只修复现有能力缺陷。修复范围：

- codegen 深化。
- 打包 hermetic。
- arm64 deb 支持。
- self-description / introspection schema 共享。
- generated startup 去 panic。
- supervisor engine 下沉 runtime。
- parser / normalizer seam 拆分。
- C++ backend capability 硬化。
- CMake repo fallback 收口。
- CI 主路径迁到 `--run-steps`。

录制回放系统暂不实施，但实现上述能力时应预留 runtime、self-description 或 CLI
边界，避免后续引入时破坏已发布契约。

后续版本路线是当前长期演进主线，不是一次性重写目标。每个版本只推动一个主轴，
但新语义必须按长期边界设计，避免为短期演示留下兼容负担。

| 版本 | 主线 |
| --- | --- |
| `v0.4.0` | Service runtime。 |
| `v0.5.0` | launch-grade supervisor、参数控制面、高频调度硬化和 FlowRT core skills 套组。 |
| `v0.6.0` | Operation + record-only 录制系统 + 时间事件模型基础。 |
| `v0.6.1` | `flowrt deps`、共享依赖 cache、默认 release 构建和 deb smoke 修复。 |
| `v0.7.0` | external process / driver package 接入边界、ARM64/跨机器部署闭环。 |
| `v0.7.1` | v0.7.0 现有能力 hardening 和发布前缺陷修复。 |
| `v0.8.0` | 真实机器人应用接入边界、variable frame 工程化、多目标部署和发布硬化。 |
| `v0.8.1` | 标准 FrameDescriptor、descriptor-only 大 payload 观测/录制和安装后 smoke。 |
| `v0.8.2` | `linux-amd64` host 到 `linux-arm64` target 的交叉编译支持基础。 |
| `v0.8.3` | 完整 `linux-amd64 -> linux-arm64` target SDK、SDK overlay、doctor 预检和真实交叉 smoke。 |
| `v0.8.4` | 板级私有依赖工程化：component build pkg-config、toolchain C++ 选项和私有 SDK 链接配置。 |
| `v0.8.5` | 公开真实交叉 SDK 示例、demo-local overlay prepare 和安装后 cross SDK smoke。 |
| `v0.8.6` | 交叉编译 UX hardening：toolchain init/show、Contract-aware doctor、build diagnostics、cache 治理和 Cross UX SDK smoke。 |
| `v0.9.0` | Island Mode / Boundary Endpoint：支持单功能单位开发、ROS2 项目逐功能包迁移、边界输入输出和 `flowrt pub`。 |
| `v0.9.1` | Island 迁移验证工具补强：canonical frame JSON 注入、批量参数、文件流 pub、多 channel echo 和显式空消息。 |
| `v0.9.2` | Island offline validation：replay/bag 播放、临时 island overlay、fixture 工作流和 strict/island 诊断收口。 |
| `v0.10.2` | 真并发收口：backend thread-affinity、two-phase output commit、scheduler-local transport commit 和并发验收 gate。 |
| `v0.10.3` | 标准 app/ 用户代码布局：废弃旧 `src/` 用户路径，用户实现统一进入 `app/`。 |
| `v0.11.0` | FlowRT App SDK 化与 C ABI v0：项目脚手架、用户 API 可发现性、C component 最小可运行路径。 |
| `v0.11.1` | App SDK / C ABI v0 hardening：诊断、失败路径、用户骨架和 release readiness 收口。 |
| `v0.12.0` | Contract-driven App Authoring：RSDL 先行，`prepare` / `explain` 产出真实用户实现接口。 |
| `v0.13.0` | Robot Runtime Completion：补齐 package/test/clock/抽象资源契约/trace/deployment/control authority 等核心设施。 |
| `v0.14.0` | Python 与更多语言入口：建立在稳定 C ABI 与 App API manifest 之上。 |
| `v1.0.0` | ABI/schema 稳定、兼容策略、故障注入和性能矩阵。 |

路线边界：

- `v0.4.0` 先把 Service 做成稳定的 request/response runtime 语义。
- `v0.5.0` 优先补齐复杂应用复刻所需的系统编排基础：条件启动、错峰启动、环境变量、
  restart policy、CPU affinity、profile、参数发现/远程控制面，以及高频多 lane
  调度的 deadline、stale、backpressure 和 fairness 语义。同时引入 FlowRT core
  skills 套组，先沉淀开发 FlowRT 本身的 agent 工作流。
- `v0.6.0` 在 Service 之上引入 Operation，并落地只录不放的 record 系统。
  Operation、录制事件和未来 replay / simulated clock / deterministic debug 必须共享
  同一时间与事件模型；本版本只稳定事件和 timestamp 字段，不让模拟时钟驱动 scheduler。
- `v0.7.0` 定义 external process / driver package 的 typed 接入边界，并补齐 ARM64、
  跨机器部署和离线交付闭环。到 `v0.7.0`，FlowRT 应具备复刻一套复杂车载机器人
  应用的系统能力，但不把硬件 backend 做进 FlowRT 主项目。
- `v0.8.0` 把多目标部署、交叉编译、多架构安装包和发布硬化推进到真实机器人应用
  接入边界：I/O boundary component、external package 多平台交付、variable frame
  工程化、FrameDescriptor + side-channel lease、ROS2 共存桥接和运行态诊断深化应
  形成一条可迁移、可部署、可观测的主路径。FlowRT 仍不做硬件 backend，也不做
  ROS2 drop-in 兼容层。
- `v0.8.1` 收紧大 payload 的可工程化路径：本机高频图像/大帧只在 channel 上传递
  标准 fixed descriptor，payload 生命周期仍归 I/O boundary 或 external package 的
  side-channel 负责；echo、status、record 和 smoke 都围绕 descriptor-only 路径验证。
- `v0.9.0` 先解决单功能单位开发和 ROS2 项目迁移不顺手的问题：引入显式
  Island Mode 和 Boundary Endpoint。常规 strict 模式下，缺 active input bind 仍是
  error，不允许 warning 后生成不完整生产代码；island mode 必须在 RSDL/IR/profile
  中显式标记，并把外部输入表达为 typed boundary input，把需要对比的输出表达为
  typed boundary output、record sink 或 ROS2 adapter sink。island 生成物必须在
  self-description、manifest 和 status 中标注，`bundle` / `deploy` 默认拒绝 island
  产物，除非用户显式允许。`flowrt pub` 作为 island 开发和迁移测试工具，只向
  boundary input 注入按 self-description / Message ABI 编码的数据，不默认向任意生产
  channel 写入。迁移完成后应拆掉 boundary endpoint，把 profile 切回 strict，使功能
  单位回到普通 FlowRT graph。
- `v0.9.1` 把 island 从“可隔离运行”推进到“可做行为验证”：外部样本、bag 片段或
  旧系统录制数据先转换成 FlowRT RSDL 字段自然 JSONL / JSON array，再由 `flowrt pub`
  作为 boundary input 注入；参数快照通过 `params set --file` 恢复；输出用多 channel
  `echo`、`record` 或 test sink 对比。FlowRT core 不引入 ROS2 message 类型，用户仍以
  自己的 RSDL message 表达业务数据。
- `v0.9.2` 应补齐 replay/offline validation 与 island 的有机整体：FlowRT 只支持
  “播放数据，像真实传感器一样灌给运行中代码”的模式，不支持用户算法直接读取 bag
  或绕过 FlowRT graph。bag/replay 注入只允许 island 语义；strict 生产 contract
  必须拒绝，避免多来源数据竞争。为了让生产 contract 可临时测试，CLI 应提供
  temporary island overlay：不修改源 `.rsdl`，而是在 build/run/replay 阶段生成
  test-only island projection，并在 self-description、manifest、status 和 artifact
  metadata 中明确标记。初始实现应要求用户显式声明 boundary mapping；自动把所有缺失
  input 变成 boundary 只能作为后续便利功能，且必须打印完整映射并保持 test-only 标记。
- `v0.10.3` 把用户代码目录模型一次性切到长期形态：`app/` 是唯一用户业务代码根，
  `app/rust/mod.rs` 是 Rust 用户入口，`app/cpp/**` 和 `app/c/**` 是 C/C++ 用户实现与
  同目录头文件位置；`flowrt/` 仍只放可删除生成物。
- `v0.11.0` 把 FlowRT 从“可生成可运行”推进到“可作为 app SDK 顺手使用”：新增
  `flowrt.toml` 项目入口、`flowrt init`、`flowrt add`、`flowrt explain` 和 C ABI v0
  最小 demo。`v0.12.0` 已把 app 作者路径纠正为 RSDL 先行：`init` 只创建入口和最小
  RSDL，`add` 只作为 RSDL 编辑助手，`prepare` / `explain` 产出 App API manifest、
  实现清单和参考 stubs；用户再把需要保留的实现放进 `app/`。含 `flowrt.toml` 的项目
  中，`add`、`check`、`explain`、`prepare`、`build`、`run`、`deps` 和 `doctor`
  省略路径时会从当前目录向上发现 manifest；显式 RSDL 路径优先；RSDL 和 Contract IR
  仍是语义事实源。当前 C v0 用户入口通过 `language = "c"` component、App API 参考
  stubs 和 `flowrt_app/c_components.h` callback table 接入。
- `v0.11.0` 同时落地 C ABI v0 基础：C 侧事实源继续放
  `runtime/cpp/include/flowrt/abi.h`，只暴露 POD、固定宽度整数、borrowed
  string/bytes/frame view、状态码、错误码和 callback table；不暴露 C++/Rust 对象、
  backend SDK handle、动态插件 ABI 或所有权语义。C component 先编进 app binary，
  形成最小可运行 demo；Python binding 不进入本版本，后续只能建立在该 C ABI 边界上。
  `language = "c"` 已进入 RSDL/Contract IR/validator/codegen/CLI 用户入口；validator
  会拒绝 C v0 暂不支持的 params、service、operation、variable frame、`io_boundary`、
  `external`、`pkg_config`、动态加载、独立 C runtime 和 Python binding，codegen 只在
  C v0 native / fixed-size message 范围内生成 callback table adapter。generated
  supervisor 已支持 `runtime_kind = "c"` 并启动 CMake app binary。
- `v0.11.1` 不扩展 C ABI v0 的功能面，只做硬化：callback table 校验必须报告明确
  失败原因；`flowrt add` 写入前要尽量完成冲突和 Contract IR 校验；用户骨架要把
  callback table size/version/feature bit 和 borrowed view 所有权边界写清；release
  readiness 必须检查 `CONTEXT.md` 当前版本状态，避免发布后状态文档滞后。
- `v0.12.0` 纠正 FlowRT app 作者主路径：`flowrt init` 只创建项目入口和最小
  RSDL，不再把默认 component 的用户实现写进 `app/`；`flowrt add` 只作为 RSDL
  编辑助手，不再生成或修改用户 `app/` 代码；`prepare` / codegen 从 validated
  Contract IR 生成 App API manifest、实现清单和 `flowrt/app/stubs/` 参考模板，但只能写
  `flowrt/` 可重建生成物，不直接创建、追加或覆盖用户 `app/`。
- `v0.13.0` 的资源模型必须保持抽象。FlowRT core 只建模 component 需要的
  capability / resource contract、访问模式、必需/可选、readiness gate、health、
  故障传播和观测字段；target / deployment / external package 声明哪些 provider
  满足这些抽象需求。FlowRT 不在核心语义里建模串口、TCP、UDP、USB、V4L2、
  RKNN、CUDA、设备路径或端口号等具体硬件和协议细节；这些映射属于 external
  package、driver package、target profile 或部署配置。validator 只校验抽象需求能否
  被目标提供者满足，supervisor 只按抽象 contract 做启动门控、health 汇报和失败传播。
- `v0.14.0` 才进入 Python 与更多语言入口：Python binding / generator 必须建立在
  v0.13.0 收口后的 C ABI、App API manifest 和 FlowRT 语义边界上，不能直接暴露
  iox2、zenoh、C++ runtime 对象或 backend SDK 句柄。
- `v1.0.0` 才进入正式稳定线：ABI/schema 冻结、兼容策略、故障注入、性能矩阵和
  长期 release policy。0.x 版本继续承载功能突破和 SDK 体验完善。

`v0.4.0` 的 Service runtime 目标是：生成 Rust/C++ service client/server 用户 API；
Service transport 支持 `inproc` 与 `zenoh`；`iox2` 暂不作为 Service transport，
继续专注 fixed-size shared-memory dataflow；request arrival 直接驱动 server，不靠
tick polling；补齐 request id、correlation、timeout、server unavailable 和
structured error 语义；self-description、`flowrt list` 和 `flowrt status` 展示
service endpoints 与 health。ROS2 Service bridge 先固定语义和 manifest 边界；
external process adapter 先预留 process/service 接入边界；`flowrt pub` 与
record/replay 只预留接口。

Service 与参数热更新有相似的控制面形状，但职责不同：参数热更新是 runtime
control-plane service-like RPC，服务于运行中配置管理；Service 是 graph 业务语义，
服务于用户组件之间的 typed request/response。两者可以复用 schema、validation、
structured error、pending/apply 和 self-description 经验，但不能混成同一个概念。

`v0.5.0` 聚焦三条 runtime 主线：launch-grade supervisor、参数控制面和高频调度
硬化；同时新增 FlowRT core skills 套组，服务 AI 辅助开发和编码。

supervisor 主线：让 generated supervisor 足以替代常见 launch 脚本的核心职责。已拍板
支持 `process_started`、`runtime_ready`、`service_ready` 三级 readiness gate、错峰
启动、env 注入、restart/failure policy、显式 CPU affinity 绑核，以及 CPU priority
采用 `nice` + 可选 Linux RT policy / priority。

参数控制面主线：参数系统从单应用热更新扩展到可发现、可校验、可远程操作的 runtime
control-plane。参数远程控制必须支持跨机器，推荐走 zenoh control-plane。

高频调度硬化主线：deadline、stale、backpressure 和 fairness 要进入 runtime 行为和
status 观测，补齐多 lane 调度的语义硬化。

已知后续项：当前 task health 的 backpressure/overflow 语义主要覆盖 inproc FIFO
写入路径；`iox2` / `zenoh` route 仍主要按 backend publish error 处理。后续深化
backend health / reconnect 时，应把 route-level backend backpressure、publish
error 和恢复状态统一纳入 health/self-description/status，而不是继续只暴露粗粒度
backend error。

FlowRT core skills 套组：入库事实源为 `.agents/skills/`，当前只落地 `frt-core-*`。
`frt-core-*` 面向 FlowRT 仓库维护者，覆盖 RSDL/IR、codegen、runtime、backend、
CLI、CI 和 release。`frt-app-*` 是 1.0.0 之后的保留命名空间；在 schema、runtime、
安装包和兼容策略稳定前，不把 app 开发流程固化成 skill。该套组只追求 FlowRT 范围
内的通用性和抽象性，不写成任意项目都适用的泛用技能。

非目标：Web/HTTP/WebSocket/UI、硬件 backend、新语言 binding、新 backend、ROS2
bridge 扩展。`v0.6.0` 仍不做 `flowrt replay`、simulated clock 驱动 scheduler 或
deterministic replay report。Web、HTTP、WebSocket 和 UI 不进入 FlowRT core，只能
作为应用层或外部工具消费 FlowRT 暴露的 typed introspection / params / record API。

`v0.6.0` 不复制 ROS2 Action。FlowRT 需要的是一等 Operation 语义：typed
long-running command、generated state machine、explicit policy、observable handle，
底层编译期 lower 成 Service + Channel。用户只看 Operation，调试时才展开底层拓扑。
Operation policy 必须显式声明 concurrency、preempt、cancel、timeout 和
result retention；用户不得手写 start/cancel/result/progress 四套底层协议。当前
generated Operation runtime 先支持单 in-flight reject 子集：`concurrency =
"reject"`、`preempt = "reject"`、`max_in_flight = 1`；`queue`、`cancel_running`
和多 in-flight 仍保留为长期 IR 语义，在 runtime 完整实现前由 validator 拒绝。

Operation 解决的不是“长时间 service call”，而是机器人系统里常见的可取消、
可抢占、可观测、可恢复的长任务。生成器负责把 Operation lowered 成 request、
progress、feedback、cancel、result 和状态观测通道；用户只实现业务 handler 和策略
钩子，不手写底层协议。这样保留 Action 的实用能力，同时避免让用户维护分散的
start/cancel/result/progress glue。

Operation 观测路径沿用 FlowRT 自描述和本机 introspection socket：self-description
记录 operation client/server 端口、goal/feedback/result 类型、policy、backend 和
内部 lowering refs；runtime status 记录 ready/running/queued、当前 operation id、
成功/失败/取消/超时/抢占计数和最近状态转换时间；`flowrt op list/status/cancel`
提供本机观测和 cooperative cancel 控制面。`flowrt op start`、跨机 Operation 控制面
和 replay 驱动执行不属于 `v0.6.0` 范围。

`v0.6.0` 的录制系统只做 record，不做 replay。录制使用 MCAP 作为容器，FlowRT 自有
record envelope 作为 schema，覆盖 channel sample、parameter control-plane event、
service event、operation event、scheduler/time metadata 和 runtime/process metadata。
默认未录制时，runtime 热路径不得持续复制 payload 或 per-sample 分配；开启录制后，
dropped event 必须计数并进入 introspection/status。record event 必须携带
self-description hash、entity id、monotonic timestamp 和 wall-clock timestamp，为
未来 replay / simulated clock / deterministic debug 留稳定输入，但本版本不实现
`flowrt replay`。

`v0.6.0` 已完成 record-only 录制主路径：Rust/C++ runtime 都提供
`IntrospectionStatus.recorder`、recorder start/stop/drain socket 控制面、有界事件
队列和 active filters；生成的 Rust/C++ runtime shell 在 channel publish 路径同时
接入 echo probe 与 recorder tap；C++ recorder drain JSON 与 Rust `RecordEnvelope`
保持同一 schema。`flowrt record` 可以自动发现唯一 live runtime，也可用 `--socket`
显式选择进程，并把 channel / Operation / control-plane / scheduler / runtime 事件写入
MCAP 文件。

FlowRT 主项目不做硬件 backend。Linux 和外部 driver package 管硬件；FlowRT 管结构、
执行、通信、观测、external process 生命周期和 typed 接入边界。

`v0.8.0` 的目标是把 `v0.7.x` 的 external process / bundle / deploy baseline 推进为
真实机器人应用可用的接入边界版本。它不是继续堆 runtime demo，也不是把 FlowRT 做成
ROS2 兼容层；它要解决 fixed ABI 控制岛之外的真实阻塞点：

- **I/O boundary component 正式化**：在 `native component` 与 `external process
  component` 之间补齐进程内 I/O boundary component 语义，用于承载自研串口、SHM、
  UDP、Linux 采样、推理 SDK 等副作用边界。RSDL/IR/manifest 必须能表达其生命周期、
  side effect、resource requirement、health、readiness、restart、graceful shutdown
  和 profile/env 注入；串口 byte frame、网络 wire 和设备私有协议不得暴露成 FlowRT
  dataflow message。
- **Variable frame 工程化**：把 `sequence<fixed struct>`、`string`、`bytes` 和嵌套
  variable frame 做成 Rust/C++ codegen、message ABI conformance、backend resolver、
  self-description、`echo`/`record`/`status` 观测工具的可靠主路径。`iox2` 继续只承载
  fixed-size plain data；涉及无界变长数据的 route 自动选择支持 variable frame 的
  backend，不为 `iox2` 重新引入临时 envelope。
- **FrameDescriptor + side-channel lease**：图像、mask 和其他大 payload 不作为普通
  channel payload 承载。FlowRT channel 传递 descriptor、resource id、slot、generation、
  size、format 和 metadata；attach / acquire / release / lease keepalive 归 I/O boundary
  或 external package 管理。录制系统默认记录 descriptor 和事件，是否记录 payload 必须
  显式建模。
- **ROS2 coexistence bridge 扩展**：迁移期桥接是显式 adapter/profile，不污染 RSDL 核心
  语义。唯一桥梁仍固定为 `zenoh`；不增加 DDS fallback。优先补齐 `FlowRT -> ROS2` 与
  `ROS2 -> FlowRT` 的 typed subset，包括固定 header 映射、常见 pose/scan/image
  descriptor 和必要自定义消息；FlowRT 内部 ABI 不机械复制 ROS2 wire。
- **诊断和调试深化**：`flowrt status` / `echo` / `hz` / `record` 必须更直接回答真实
  调试问题：哪个 input 缺失、哪个 latest stale、哪个 route drop/overflow、backend
  为什么被选中、哪个 boundary 不健康、哪个 resource acquire 失败、哪个 process 正在
  restart 或等待 readiness。
- **多目标部署闭环**：继续推进原 `v0.8.0` 的部署主线。bundle manifest 应成为部署事实
  源，支持 Linux `amd64` / `arm64` 优先的 target platform、external package 多平台
  选择、cross build orchestration、远端 FlowRT 版本校验和 release 安装包 smoke。

当前 `v0.8.0` 已落地边界：

- RSDL / Contract IR 支持 `kind = "io_boundary"` component，并记录 resource
  requirement、side effect、readiness、health 和 shutdown policy；launch manifest 与
  self-description 输出对应摘要。Rust/C++ runtime 已提供 `BoundaryContext` 和
  introspection `io_boundaries` 状态；generated shell 会为 I/O boundary 生命周期钩子传入
  boundary context，并在 `component_started` readiness 下自动标记 ready。
- target platform 输入统一归一化为 `linux-amd64` / `linux-arm64`；`linux-x86_64` 和
  `linux-aarch64` 只作为旧输入别名接受，落盘 IR、自描述和 bundle manifest 均输出
  canonical 字符串。
- `flowrt bundle` 输出 schema v2，保留旧 deploy 字段，并新增 artifact 列表记录
  target、platform、相对路径和 sha256；external package executable 在 bundle 阶段按
  target platform 校验支持矩阵。
- `flowrt deploy` 对 schema v2 bundle 以 artifact 列表为部署事实源，按请求 target
  选择产物，并校验 platform、相对路径、文件存在性和 sha256；schema v1 bundle 继续按
  顶层 target 字段兼容。
- variable frame 的 Rust/C++ generated message API 覆盖 `bytes`、`string`、
  `sequence<primitive>` 和 `sequence<fixed struct>`；runtime frame codec 的 canonical
  tail order 已在 Rust/C++ smoke 中覆盖。`iox2` 仍只承载 fixed-size plain data，变长
  route 自动选择支持 variable frame 的 backend。
- CI 增加 `v0.8.0 Integration Smoke` amd64/arm64 focused gate，并在安装后 demo smoke
  中运行 `scripts/test-v080-installed-smoke.sh`，覆盖 variable frame、I/O boundary
  status、FrameDescriptor self-description、bundle schema v2 和 deploy dry-run。

当前 `v0.8.1` 已落地边界：

- 标准 FrameDescriptor message 是固定 64 字节 plain data，字段为
  `resource_id_hash`、`slot`、`generation`、`size_bytes`、`timestamp_unix_ns`、
  `width`、`height`、`stride_bytes`、`format_id`、`encoding_id` 和 `flags`。
- `io_boundary` resource descriptor 通过 `port` 绑定输出端口；validator 会拒绝缺失
  output port、非标准字段、变长字段或不满足 fixed-size plain data 的 descriptor。
- `iox2` 可以承载标准 FrameDescriptor route，因为 channel 内只传固定 descriptor；
  图像、mask 等真实 payload 不走普通 FlowRT message，也不通过 `bytes` 绕进 iox2。
- Rust/C++ runtime 和 codegen 都提供 `FrameDescriptorFields` helper；生成 message 可
  从 helper 构造，并可还原 fields 供用户逻辑或 boundary event 使用。
- `flowrt echo` 会结构化展示标准 descriptor 字段；`flowrt status` 会展示 resource
  descriptor schema；`flowrt record` 默认记录 descriptor/event，摘要中标注
  `descriptor_payload=descriptor_only`。
- 新增 `examples/frame_descriptor_demo` 和 `scripts/bench-frame-descriptor.sh`；
  CI 增加 `v0.8.1 FrameDescriptor Smoke` amd64/arm64 focused gate，安装后 demo smoke
  增加 `scripts/test-v081-installed-smoke.sh`。

当前 `v0.8.4` 已落地边界：

- `flowrt deps` 和 `flowrt build` 支持 `--target linux-amd64|linux-arm64`。显式
  `--target` 优先于 Contract IR target platform；仍无 platform 时保持 native 构建。
- CLI 内部有 toolchain profile 配置层，维护 target platform 到 Rust target triple、
  Debian multiarch、默认 C/C++ compiler、sysroot、CMake toolchain file、pkg-config
  路径、SDK overlay 和 runtime dependency policy 的映射；profile 配置不写入 RSDL
  或 Contract IR。
- component build 可以声明可移植的 `pkg_config` 依赖名；codegen 会据此生成
  CMake 的 `find_package(PkgConfig)` / `pkg_check_modules(...)` / `PkgConfig::...`
  链接路径，但不会把板端路径写进 RSDL。
- toolchain profile 还支持 `cpp_compile_args`、`cpp_link_args` 和
  `cpp_link_libraries`，用于把板端私有 SDK 需要的编译选项、链接选项、私有库路径
  和 FlowRT 运行时链接约束放在配置层，不污染组件语义。
- Rust/Cargo build 和 deps prewarm 会使用对应 Rust target triple，并把 cache key、
  ready marker、Cargo target dir 和本地二进制输出按 target 隔离。
- C++/CMake cross build 会优先使用 `/opt/flowrt/<version>/targets/<platform>` 的
  target SDK root、CMake wrapper、pkg-config、include 和 lib 事实源；SDK 缺失或
  `complete = false` 时 fail-fast。
- amd64 deb 内嵌 `linux-amd64` 和 `linux-arm64` 两个 complete target SDK；arm64 deb
  当前只保证 `linux-arm64` complete，不承诺反向 `linux-arm64 -> linux-amd64`。
- 新增 `flowrt doctor --target <platform>`，用于预检 Rust target、C/C++ 交叉编译器、
  target SDK、sysroot、CMake toolchain、pkg-config 路径和 SDK overlay。
- `flowrt bundle` 优先读取 `build-info.json` 中的 artifact closure；带 platform 的
  项目二进制复制到 bundle 的 `bin/<platform>/`，`deploy` 会校验 target、platform、
  路径层级和 sha256。
- CI 增加 `v0.8.3 Cross Toolchain Smoke` 和 `v0.8.3 Installed amd64 to arm64 Smoke`；
  package 阶段运行 target SDK layout smoke，安装版 cross smoke 运行
  `scripts/test-v083-installed-smoke.sh`，并用 ELF header 验证 C++ demo 输出为 AArch64。

`v0.7.0` 已落地 external package 主路径：

- RSDL 支持 `language = "external"` / `kind = "external"` 的 component 和 graph 级
  `[[external_process]]`。
- Contract IR 记录 external process 的 package、executable、args、working directory、
  health 和 required backend。
- external route auto resolver 默认选择 `zenoh`；显式 `inproc` 被拒绝，`iox2` 在
  package capability 与 fixed-size 条件未完整建模前默认拒绝。
- `flowrt external check/list` 校验和列出 `flowrt-external.toml` package manifest。
- launch manifest 和 self-description 暴露 external process/package 摘要。
- generated supervisor 能按 `FLOWRT_EXTERNAL_PATH`、`/opt/flowrt/external/<package>`、
  项目 `external/<package>` 查找 package，启动 executable，注入 `FLOWRT_*` 上下文，
  并纳入 restart/readiness/status 机制。
- `flowrt bundle` 输出离线 bundle 目录；`flowrt deploy` 提供 SSH/SCP baseline 和
  `--dry-run`。
- `examples/external_driver_demo` 覆盖 external package、supervisor、bundle 和 deploy
  dry-run。

## 当前仓库状态

仓库已经形成 FlowRT 工具链、Rust/C++ runtime shell、跨进程 backend、ROS2 zenoh
bridge、运行态观测、参数系统和安装包闭环。

主要目录：

```text
Cargo.toml
Cargo.lock
README.md
CHANGELOG.md
CONTEXT.md
AGENTS.md
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
docs/
scripts/
```

当前主要示例覆盖：

- `import_demo`：模块化 RSDL import。
- `workspace_demo`：workspace / module / composition、跨模块引用和同名 module symbol 生成命名。
- `imu_demo`：Rust 主 demo。
- `imu_demo_iox2`：Rust source、C++ controller、Rust monitor 通过 iox2 分进程运行。
- `profile_switch_demo`：同一 RSDL 在 inproc 与 iox2 profile 间切换。
- `cpp_counter_demo`：C++ only 用户逻辑。
- `c_counter_demo`：C callback v0、fixed-size `Count` message、CMake app run 和
  supervisor launch。
- `mixed_iox2_demo`：Rust source 与 C++ sink 通过 iox2 分进程连接。
- `mixed_zenoh_demo`：跨主机 copy backend、variable frame 和 mixed launch。
- `ros2_bridge_demo`：Rust source 到 ROS2 `/flowrt/text` 的 zenoh-only bridge。
- `external_driver_demo`：无硬件依赖的 external package、supervisor 和 bundle/deploy
  baseline。

仓库内可以用 `cargo run -p flowrt-cli -- ...` 调试 CLI。README、示例、对外文档和
最终回复默认使用安装后的 `flowrt ...` 命令。

## 已实现能力

工具链已经支持：

- RSDL 解析、import 展开、Contract IR 归一化和 canonical JSON。
- workspace / module / composition 装载、module name resolver 和跨模块
  `module::Name` 引用；module 内短名优先解析本 module，root/composition 层短名
  必须全局唯一。
- Contract IR validator 对名称、ID、版本、canonical ordering、deployment、
  capability、参数、target 和 derived metadata 的防篡改校验。
- Rust/C++ message ABI conformance 测试生成，覆盖 size、alignment、field offset、
  byte-level roundtrip、default initialization 和 IR-derived expected byte fixtures。
- fixed-size plain data 与 variable frame 主线。`bytes`、`string` 和 `sequence<T>`
  使用 canonical frame；`iox2` 只承载 fixed-size plain data，变长 route 自动选择
  支持变长消息的 backend。
- Rust/C++ generated runtime shell 的生命周期、task 调度、latest/FIFO channel、
  bind-level stale freshness、deadline 检查和参数 pending apply。
- Service 请求/响应语义：RSDL component 可声明 `service_client` / `service_server`，
  graph 可用 `[[bind.service]]` 绑定 client/server；Contract IR、validator、
  generated typed API、inproc runtime、self-description、`flowrt list` 和
  `flowrt status` 已覆盖 service 主路径，native generated `zenoh` service 仍 fail-fast。
- C/Python ABI 边界准备：`runtime/cpp/include/flowrt/abi.h` 定义 C ABI 版本、
  status/backend/health 整数编码、borrowed string/bytes view、reconnect policy、
  backend health snapshot，以及 C component context、fixed input view、output slot 和
  callback table；Rust runtime 提供对应 `repr(C)` 镜像类型和转换函数。当前已提供 C
  component v0 adapter、`app/c` 用户接入路径和最小 demo，但不提供完整 C runtime wrapper、
  动态加载或 Python binding。
- C++ only 和 C callback v0 contract 的 CMake app 路径，支持普通 `flowrt build` /
  `flowrt run`，以及 `flowrt build --launcher` 后由 `flowrt launch` 通过 generated
  supervisor 启动。
- language-separated mixed contract over `iox2` 或 `zenoh`，并拒绝同一 process group
  内混合 C++/Rust 以及 mixed `inproc` process boundary。
- Rust/C++ `iox2` typed pub/sub endpoint、QoS 映射、同名 `FlowRTIox2Header` user
  header、canonical service name 和 endpoint 自动恢复。
- Rust/C++ `zenoh` endpoint、deterministic `key_expr`、copy transport、自动恢复和
  本机 launch 自动 mesh。
- zenoh-only ROS2 bridge：生成 source process bridge tap 和 FlowRT 管理的 C++ ROS2
  adapter process，ROS2 侧强制 `rmw_zenoh_cpp`。
- runtime introspection socket、自描述、status、channel snapshot、echo observer、
  `hz` 采样、参数控制面和按需 recorder tap。
- generated supervisor 对 Rust、C++、C callback v0 和 ROS2 bridge process 的分流启动、health
  汇总、PID socket 轮询、tick stale/exit/restart 状态展示、process 依赖顺序、
  可配置 restart policy 和失败传播 baseline。

## CLI 状态

当前已实现的用户入口：

```bash
flowrt check [path/to/robot.rsdl]
flowrt explain [path/to/robot.rsdl] [--format text|json]
flowrt prepare [path/to/robot.rsdl]
flowrt deps [path/to/robot.rsdl]
flowrt deps --backend all
flowrt deps [path/to/robot.rsdl] --target linux-arm64
flowrt doctor --target linux-arm64
flowrt build [path/to/robot.rsdl]
flowrt build [path/to/robot.rsdl] --target linux-arm64
flowrt init [path] --lang rust|cpp|c
flowrt add message <Name> field:type ...
flowrt add module <Name>
flowrt add component <Name> --lang rust|cpp|c [--input name:Type] [--output name:Type]
flowrt run [path/to/robot.rsdl]
flowrt run [path/to/robot.rsdl] --process main
flowrt run [path/to/robot.rsdl] --run-steps 5 --process main
flowrt launch path/to/robot.rsdl
flowrt list path/to/generated-app
flowrt nodes path/to/generated-app
flowrt status
flowrt hz [channel] [--socket path/to/runtime.sock] [--window-ms 1000]
flowrt echo <channel> [--socket path/to/runtime.sock] [--image path/to/generated-app-or-selfdesc.json] [--follow]
flowrt record --output run.mcap [--socket path/to/runtime.sock] [--duration 10s] [--channel name] [--operation name] [--all] [--force]
flowrt params list --image path/to/generated-app-or-selfdesc.json
flowrt params get <instance.param> --image path/to/generated-app-or-selfdesc.json
flowrt params set <instance.param> <json-value> --image path/to/generated-app-or-selfdesc.json
flowrt inspect flowrt/contract/contract.ir.json
```

含 `flowrt.toml` 的项目中，`check`、`explain`、`prepare`、`build`、`run`、`deps` 和 `doctor`
可以省略 RSDL 路径。CLI 会使用最近父目录中的 `[project].main`；显式 RSDL 路径优先。
`deps` 和 `doctor` 找不到 manifest 时仍保留无契约预热或基础环境预检模式。

`prepare` / `build` / `run` / `launch` 支持 `--profile <name>`，用于显式选择 profile
并按该 profile 生成或校验产物。省略参数时会先投影到 `default` profile 或首个
profile。RSDL 未声明任何 profile 时，normalization 会插入隐式 `default` profile，
backend 为 `inproc`。

命令职责边界：

- `deps` 只写全局 FlowRT cache，不生成用户项目产物。cache root 默认
  `~/.cache/flowrt`，可用 `FLOWRT_CACHE_DIR` 覆盖。
- `deps` / `build` 的 `--target <platform>` 当前支持 `linux-amd64` 和 `linux-arm64`，
  Rust/Cargo 路径会使用 toolchain profile 中的 Rust target triple；显式 target
  优先，省略时从 Contract IR target platform 推导，仍无 platform 时保持 native。
- `doctor --target <platform>` 只做环境预检，不生成用户产物；缺失 Rust target、
  交叉编译器、完整 target SDK 或显式 SDK overlay 时返回非零状态。
- `prepare` 和 `build` 会写 `flowrt/` 输出目录，必须持有 OS advisory lock。
- `.flowrt.lock` 文件可残留，PID 只用于诊断，真实占用状态由锁判断。
- `check`、`inspect`、`run`、`launch`、`list`、`nodes`、`status`、`hz`、`echo`、
  `record` 和 `params` 不写生成物，不应获取生成物锁。
- `run` / `launch` 省略 `--run-steps` / `--run-ticks` 时长期运行，直到生成应用返回
  `Error` 或收到 SIGINT/SIGTERM。
- `--run-steps` 是推荐外部名称，`--run-ticks` 是兼容别名；`launch` 下 supervisor
  也会根据 live tick 快照终止达到上限后仍在运行的子进程。
- `flowrt status --live-only` 只输出成功返回 live status 的 runtime，默认 `status`
  仍保留 stale socket 诊断行。
- `flowrt run --process <name>` 运行生成应用中的单个 RSDL process group；mixed
  contract 必须选择一个单语言 process group。
- `launch` 运行 FlowRT 管理的 Rust supervisor。C++ only 和 C callback v0 contract 会生成
  supervisor-only Rust crate，launch 先构建 CMake app 再运行 supervisor；C callback v0
  process 的 `runtime_kind = "c"` 由 supervisor 显式映射到 CMake app binary。
- `inproc` 是单进程 backend；launch 和单独 process run 必须拒绝 inproc dataflow
  跨 RSDL process group。
- `build` 默认 release。Rust app、generated supervisor、C++ app 和 ROS2 bridge
  adapter 在 native 或无 cross target triple 时继续复制到兼容路径
  `flowrt/build/bin/<mode>/`；实际 cross target 构建复制到
  `flowrt/build/bin/<platform>/<mode>/`，避免 host/target 同名二进制互相覆盖。
  `flowrt/build/build-info.json` 记录 build mode、target name、platform、
  target identity、Rust target triple、host triple、deps target 目录和 executable
  相对路径。缺少匹配 deps ready marker 时，`build` 会 fail-fast，提示先运行
  `flowrt deps`。
- `bundle` 会优先使用 `build-info.json` 的 artifact closure，并把带 platform 的本项目
  二进制复制到 `bin/<platform>/<filename>`；manifest schema v2 的 artifact path、
  platform 和 sha256 与实际 bundle 文件保持一致。`deploy` 继续只以 artifact 列表为
  事实源，校验请求 target、platform 别名、路径安全、文件存在性和 sha256，发现目标
  产物缺失、platform 不匹配或 hash 不匹配时提示重新执行对应 platform 的
  `flowrt build --target <platform> --launcher`。
- Rust app、generated supervisor 和 deps prewarm 使用同一个 Rust target triple。
  target triple 会进入 cache key 和 ready marker；Cargo cross target 输出位于
  `CARGO_TARGET_DIR/<triple>/<profile>/`，CLI 会按该路径定位二进制。

## Runtime 和观测状态

`flowrt/launch/launch.json` 当前包含 process group 的 `runtimes`、`runtime_kind`、
`depends_on`、`restart` 和 `failure`，graph instance 的 `runtime`，graph 的
`channels`、`services`，以及 channel 的 backend 元数据。
iox2 channel 暴露 canonical service name，zenoh channel 和 ROS2 bridge 暴露
deterministic `key_expr`。

runtime introspection socket 使用 `$XDG_RUNTIME_DIR/flowrt/<pid>.sock` 或
`/tmp/flowrt.<uid>/<pid>.sock`。socket 路径只用于发现，真实身份来自 handshake。
启动 status server 时不能覆盖仍可连接的 live socket；SIGKILL 后残留且不可连接的
socket 文件可以回收。status server 对活跃连接线程设置上限，超过上限时返回结构化
error。Rust `IntrospectionState` 会在 mutex poison 后恢复访问。

Runtime 已提供 C ABI 基础边界和 Rust/C++ health/reconnect 抽象。C ABI 当前覆盖
`Status`、backend kind、backend health state、borrowed string/bytes view、
`ReconnectPolicy`、`BackendHealthSnapshot`、C component context、fixed input view、
output slot 和 callback table 的稳定 POD 形状；Rust/C++ runtime 内部仍使用各自语言的
高层类型，并通过转换函数或 C header 对齐。该 callback table 只表达边界；当前 C v0
已开放 adapter、`app/c` 用户接入路径和最小 demo，但不表示完整 C runtime、动态加载或 Python
binding 已开放。`iox2` 和 `zenoh`
endpoint 已接入自动恢复：本地 transport 资源丢失或操作失败会重建本地
publisher/subscriber/session；codec/schema 错误不得触发重连。

## ROS2 Bridge 状态

FlowRT 与 ROS2 bridge 的唯一通信桥梁固定为 `zenoh`。RSDL 使用 `[[bridge.ros2]]`
声明 bridge，当前支持：

- `direction = "flowrt_to_ros2"` 和 `direction = "ros2_to_flowrt"`。
- `ros2_type = "std_msgs/msg/String"` 与 `geometry_msgs/msg/Pose` 的 typed subset。
- `field` 指向 FlowRT message 中的 `string` 字段；Pose 映射要求 FlowRT message 结构与
  ROS2 Pose 的 position/orientation 字段匹配。
- `flowrt = "instance.port"` 继续表示普通 FlowRT 端口；island profile 下也可以写
  `flowrt = "boundary_endpoint_name"`，把 ROS2 topic 显式接到 typed boundary input/output。

normalization 生成的 bridge backend 必须是 `zenoh`；validator 必须拒绝 source target
不支持 `zenoh` 的 contract，不得添加 DDS fallback。codegen 会在 source process 中
生成 zenoh bridge tap，并额外生成 FlowRT 管理的 C++ ROS2 adapter process
`ros2_bridge`。boundary-bound bridge 不把 ROS2 输入伪装成普通 dataflow bind；generated
shell 会先把样本注入对应 `BoundaryInput`，再由正常 island boundary 路径驱动 task。
launch manifest 中该 process 的 `runtime_kind` 为 `ros2_bridge`。

generated supervisor 必须为 ROS2 bridge process 选择 CMake 产出的 adapter executable，
启动时设置 `RMW_IMPLEMENTATION=rmw_zenoh_cpp`；adapter 自身也必须校验 ROS2 侧实际
使用 `rmw_zenoh_cpp`。

## 打包与分发状态

FlowRT 作为标准 Linux 应用分发。当前单个 deb 包同时安装：

- `flowrt` CLI。
- Rust runtime crate。
- C++ runtime header 和 CMake package。
- Rust crate vendor。
- 私有 backend SDK 依赖。
- 基础文档。

包内文件安装到 `/opt/flowrt/<version>` 私有前缀，并通过 `/usr/bin/flowrt` 暴露入口。
`flowrt deps` 会使用包内 vendor 预热全局共享 cache；生成 Rust app 会复用该 cache；
生成 CMake 会使用 FlowRT 私有前缀解析 C++ runtime 和 backend SDK。生成 CMake 不应
通过 `FetchContent` 联网拉取 backend SDK；缺失依赖应要求安装 FlowRT 包或显式设置
`FLOWRT_CPP_RUNTIME_DIR` / `CMAKE_PREFIX_PATH`。
`flowrt bundle` 复制项目二进制后对 ELF 可执行文件 best-effort 执行
`strip --strip-unneeded`；非 ELF 文件跳过，strip 失败只进入命令摘要 warning，不修改
用户工作区原始产物。

## CI 和 Release 状态

当前 `.github/workflows/ci.yml` 将 Linux 验证拆为分层 job，覆盖生成物保护、Rust
格式化/测试/clippy、C++ runtime、v0.5.0 / v0.6.0 / v0.7.0 / v0.8.0 / v0.8.1
focused smoke、v0.8.3 交叉编译 focused smoke、v0.9.x island focused smoke、
v0.10.2 concurrency focused smoke、打包、C++ zenoh runtime、demo smoke、ROS2 bridge
smoke 和 release。Linux job 默认
运行在官方 ROS2 Jazzy base 容器上；ROS2 bridge smoke 覆盖 Jazzy 和 Lyrical 两个
发行版，并安装对应的 `rmw_zenoh_cpp`。

CI 的架构相关 job 使用 `amd64` / `arm64` 双矩阵：Rust fmt/test/clippy、C++ runtime、
v0.5.0 / v0.6.0 / v0.7.0 / v0.8.0 / v0.8.1 / v0.9.x / v0.10.2 focused smoke、Debian
package、C++ zenoh runtime、demo smoke、ROS2 Jazzy bridge 和 ROS2 Lyrical bridge 都在
对应架构 runner 上执行。
`v0.8.3 Cross Toolchain Smoke` 固定在 amd64 host 上准备
`aarch64-unknown-linux-gnu` Rust target、`aarch64-linux-gnu` C/C++ 交叉编译器和
`pkg-config`，并运行 `flowrt-cli` 的 toolchain、build model、command 和 CMake target
SDK focused tests。`v0.8.3 Installed amd64 to arm64 Smoke` 下载 package job 产出的
amd64 deb，安装后运行 `flowrt doctor --target linux-arm64` 和真实
`flowrt build --target linux-arm64` C++ demo，并用 ELF header 验证输出为 AArch64。
package job 分别上传 `flowrt-linux-amd64-deb` 和 `flowrt-linux-arm64-deb` artifact。
demo smoke 先安装同架构 deb，再用安装后的 `flowrt deps` 预热依赖，然后用
`flowrt ...` 跑示例。推送 `v*` tag 且全部 gate 成功后，release job 会下载两种架构
deb artifact，从 `CHANGELOG.md` 对应版本段抽取 release notes，并创建 GitHub Release
上传 `flowrt_*_amd64.deb`、`flowrt_*_arm64.deb` 与统一 `SHA256SUMS`。tag 版本必须
匹配根 `Cargo.toml` 的 workspace version。

`v0.5.0 Runtime Smoke` focused gate 使用 `-j1` 聚焦 supervisor readiness/resource、
远程参数控制面、status/hz 健康展示、scheduler health 和 runtime introspection 相关
测试；`v0.6.0 Runtime Smoke` focused gate 聚焦 Operation RSDL/IR/validator/codegen/
runtime/CLI/status、record format、runtime recorder tap 和 CLI MCAP 写入路径，使
这些新增能力的 CI 失败原因比全量 Rust test 更可定位。`v0.7.0 External/Deploy Smoke`
聚焦 external process、bundle 和 deploy；`v0.8.0 Integration Smoke` 聚焦 I/O
boundary、variable frame、FrameDescriptor、ROS2 typed bridge、diagnostics、bundle
和 deploy；`v0.8.1 FrameDescriptor Smoke` 聚焦标准 descriptor demo、CLI 结构化
echo/status、descriptor-only record 和 microbench；`v0.8.3 Cross Toolchain Smoke`
聚焦安装版交叉编译 toolchain profile、Rust target、C/C++ 交叉编译器和 CMake target
SDK 语义；`v0.8.3 Installed amd64 to arm64 Smoke` 覆盖安装版真实交叉构建。
`v0.10.2 Concurrency Hardening Smoke` 覆盖 codegen 并发 focused tests、Rust iox2
generated shell、backend route、Rust/C++ runtime executor，以及临时 generated Rust/C++
shell 构建，确保 two-phase commit 和 scheduler-local transport commit 进入 release
gate。发布前应运行
`scripts/check-release-readiness.sh <version>`；脚本会汇总版本来源、CHANGELOG 段、
release notes 抽取和 v0.5.0 / v0.6.0 / v0.7.0 / v0.8.0 / v0.8.1 / v0.8.3 / v0.8.6 /
v0.9.x / v0.10.2 / v0.12.0 focused gate 覆盖状态。
v0.12.0 当前覆盖通过既有 Rust/CLI 测试、App API 产物测试和 authoring smoke 收口：
`init`、`add`、`check`、`prepare`、`explain`、`flowrt.toml` 发现、显式 RSDL 优先级、
C ABI layout、C codegen adapter、C reference stub、C v0 fail-fast，以及
`import_demo`、`cpp_counter_demo` 和 `c_counter_demo` 的普通 build/run 路径都有验证入口。
当前发布由推送对应 `vX.Y.Z` tag 触发 GitHub Release。

workflow 对 Rust 构建产物、FlowRT deps cache 和必要的公开 SDK overlay 做分层缓存；
release artifact、release notes 和 deb 成品仍每次从源码重建。多架构 CI 的首要目标是保证发布包能在 amd64 与 arm64 原生
runner 上构建、安装和通过同等 smoke。

## 文档维护提示

`CONTEXT.md` 记录当前事实，不替代 `AGENTS.md` 的长期规范，也不替代
`CHANGELOG.md` 的发布记录。发生以下变化时应同步更新本文档：

- 版本背景或近期路线变化。
- CLI 命令、选项或职责边界变化。
- runtime、supervisor、backend、ROS2 bridge 或观测能力变化。
- 打包、安装路径、依赖解析或 release 流程变化。
- 示例矩阵或 CI smoke 覆盖变化。

阶段计划和设计草案仍然不入库；不要为了更新本文档而新增设计/计划文档。
