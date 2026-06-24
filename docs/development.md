# 开发维护

本文记录 FlowRT 仓库开发时的常用验证、文档维护和提交规则。

## 常用验证

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

Codegen evidence matrix 与 C++ static quality：

```bash
scripts/check-evidence-matrix.sh
scripts/check-codegen-compile-coverage.sh
scripts/test-codegen-compile.sh
scripts/test-cpp-static-quality.sh
scripts/test-v0260-transport-compile-evidence-smoke.sh
scripts/test-v0271-debt-closure-smoke.sh
scripts/test-v0280-module-app-layout-smoke.sh
scripts/test-v0290-example-module-layout-smoke.sh
```

codegen golden snapshot 只锁定生成文本漂移，不能证明 generated shell 可被 Rust/C++ 编译器接受。
`scripts/evidence-matrix.toml` 是 generated/runtime/CLI 证据覆盖事实源；新增或删除
generated runtime shell golden case 时，必须同步矩阵条目。`scripts/check-evidence-matrix.sh`
会确认所有已生成 Rust/C++ runtime shell 的 golden case 都声明 `golden + syntax_compile`
证据，并检查 dataflow、feedback、Service、Operation、self-description 和 variable frame 等关键
surface 至少有 Rust/C++ 证据。`scripts/check-codegen-compile-coverage.sh` 仅作为兼容入口委托
该矩阵检查。

`scripts/test-codegen-compile.sh` 从同一 evidence matrix 读取 `syntax_compile` case，复用
golden corpus，对 inproc、iox2、zenoh dataflow、Service、Operation 和 bounded variable frame
generated shell 做语法或 crate 真编译；默认临时工作目录位于
`target/flowrt-codegen-compile-tmp`，避免大型 Rust compile case 占满 `/tmp` tmpfs。它仍不同于
真实 SDK demo build/run：compile net 证明生成物可编译，`zenoh_service_demo`、
`iox2_service_demo` 等安装后或 SDK smoke 才证明依赖解析、链接和运行路径可用。
Rust generated case 会先用默认 5 次外层重试的 `cargo fetch` 解析临时 workspace 依赖，再用
`cargo check --locked --offline` 执行编译证明；这样 crates.io 瞬断只影响 fetch 阶段，不会和
生成物真实编译错误混在一起。
`transport compile evidence matrix` 是 v0.26.0 对这套 generated compile net 的发布锚点，
用于确保 transport 相关 golden 覆盖从文本快照升级为可编译证据。

`scripts/test-cpp-static-quality.sh` 是长期 C++ 静态质量门禁，分三类 profile 执行：
`runtime` profile 读取 `runtime/cpp` 的 CMake `compile_commands.json`，`generated` profile
读取 evidence matrix 中 `cpp_static_quality = true` 的代表性 generated shell case，`ABI/POD`
profile 聚焦 `runtime/cpp/include/flowrt/abi.h` 和 runtime smoke。generated code 优先改
emitter 产出干净代码，不通过新增 blanket suppression 过门禁；ABI/POD 例外必须保留在局部
profile 和 layout/static smoke 证据内。

operation control-plane completion smoke：

```bash
scripts/test-v0270-operation-control-plane-smoke.sh
```

该 smoke 聚焦 `v0.27.0` 的发布缺口：本机/远程 Operation CLI、bounded variable frame
payload 编解码、fault matrix 多 boundary input replay source、self-description canonical
message identity 和 generated Operation 关键接线。

debt closure smoke：

```bash
scripts/test-v0271-debt-closure-smoke.sh
```

该 smoke 聚焦 `v0.27.1` 的长期 debt 收束：evidence matrix、generated compile net、C++
static quality、feedback typed literal、route typed transport error、Operation observation
record/replay verification 和 C ABI readonly string params 必须同时具备定向回归或门禁证据。

module app layout smoke：

```bash
scripts/test-v0280-module-app-layout-smoke.sh
```

该 smoke 聚焦 `v0.28.0` 的 module-aware 用户侧目录：App API manifest、
`implementation.md` 和 reference stubs 使用 module-aware 路径，其中 C++ module source
位于 `app/<module>/cpp/src/<component>.cpp`，reference stub 位于
`flowrt/app/stubs/<module>/cpp/src/<component>.cpp`；`prepare` 不创建或覆盖用户 `app/`，
generated CMake 自动发现 `app/<module>/cpp/src/**` 和 `app/<module>/c/**`，并加入
`app/<module>/cpp/inc` include 路径。

入库示例 module layout smoke：

```bash
scripts/test-v0290-example-module-layout-smoke.sh
```

该 smoke 聚焦 `v0.29.0` 的入库示例迁移：`examples/workspace_demo` 采用 module-local
用户目录，Rust 实现位于 `app/perception/rust/processor.rs`，C++ 实现位于
`app/control/cpp/src/processor.cpp`，C++ headers 位于 `app/control/cpp/inc/`；示例 contract
使用 Rust/C++ process 分离和 `iox2` backend。smoke 会复制示例到临时目录，验证
App API / reference stub / generated CMake 路径，并执行 `flowrt deps` 与
`flowrt build --launcher`。

VSCode / clangd：

```bash
cmake -S runtime/cpp -B build/cpp
cargo run -p flowrt-cli -- prepare examples/cpp_counter_demo/rsdl/robot.rsdl
cargo run -p flowrt-cli -- prepare examples/workspace_demo/rsdl/robot.rsdl
cargo run -p flowrt-cli -- prepare examples/imu_demo_iox2/rsdl/robot.rsdl
cargo run -p flowrt-cli -- prepare examples/mixed_iox2_demo/rsdl/robot.rsdl
cargo run -p flowrt-cli -- prepare examples/mixed_zenoh_demo/rsdl/robot.rsdl
cargo run -p flowrt-cli -- prepare examples/ros2_bridge_demo/rsdl/robot.rsdl
```

仓库根目录的 `.clangd` 会让 `runtime/cpp/**` 使用 `build/cpp/compile_commands.json`，并让
`examples/*/app/cpp/**` 和 `examples/*/app/*/cpp/**` 读取本示例自己的
`flowrt/cpp/include` 生成头；module-local C++ 文件还会加入相邻 `cpp/inc`。`flowrt/` 和
`examples/*/flowrt/` 仍是可删除、可重建的生成物，不入库；如果清理过这些目录，需要先重新执行
对应示例的 `prepare` 或 `build`，再重启 clangd。

FlowRT demo smoke：

```bash
scripts/package-deb.sh --output-dir dist
sudo dpkg -i dist/flowrt_*_*.deb
flowrt --version

flowrt build --launcher examples/cpp_counter_demo/rsdl/robot.rsdl
flowrt run --run-steps 5 examples/cpp_counter_demo/rsdl/robot.rsdl --process control
flowrt launch --run-steps 5 examples/cpp_counter_demo/rsdl/robot.rsdl
flowrt build examples/c_counter_demo/rsdl/robot.rsdl
flowrt run --run-steps 3 examples/c_counter_demo/rsdl/robot.rsdl
flowrt build --launcher examples/c_counter_demo/rsdl/robot.rsdl
flowrt launch --run-steps 3 examples/c_counter_demo/rsdl/robot.rsdl
flowrt build examples/imu_demo/rsdl/robot.rsdl
flowrt build --launcher examples/import_demo/rsdl/robot.rsdl
flowrt run --run-steps 5 examples/import_demo/rsdl/robot.rsdl --process main
flowrt launch --run-steps 5 examples/import_demo/rsdl/robot.rsdl
flowrt check examples/mixed_iox2_demo/rsdl/robot.rsdl
flowrt check examples/imu_demo_iox2/rsdl/robot.rsdl
flowrt check examples/profile_switch_demo/rsdl/robot.rsdl
flowrt build --profile iox2 examples/profile_switch_demo/rsdl/robot.rsdl
flowrt run --run-steps 5 --profile iox2 examples/profile_switch_demo/rsdl/robot.rsdl
flowrt build --launcher examples/mixed_zenoh_demo/rsdl/robot.rsdl
FLOWRT_TICK_SLEEP_MS=5 flowrt launch --run-steps 200 examples/mixed_zenoh_demo/rsdl/robot.rsdl
flowrt build --launcher examples/operation_demo/rsdl/robot.rsdl
flowrt run --run-steps 5 examples/operation_demo/rsdl/robot.rsdl --process main
flowrt op list --image examples/operation_demo/flowrt/selfdesc/selfdesc.json
scripts/test-v060-installed-smoke.sh
```

ROS2 bridge 本地 smoke 需要 ROS2 Jazzy 或之后版本的 C++ 开发环境，以及运行时 `rmw_zenoh_cpp`。该 bridge 只允许 zenoh 路径，不接受 DDS fallback。普通 FlowRT `zenoh` backend 使用 FlowRT 包内私有 zenoh SDK；ROS2 bridge adapter 进程使用 ROS2 安装中的 `zenoh_cpp_vendor`，以匹配 `rmw_zenoh_cpp` 的同进程 ABI。执行前 source ROS2 环境，生成 CMake 会把 `AMENT_PREFIX_PATH` 映射到 `CMAKE_PREFIX_PATH`：

```bash
source /opt/ros/jazzy/setup.bash
flowrt build --launcher examples/ros2_bridge_demo/rsdl/robot.rsdl
export RMW_IMPLEMENTATION=rmw_zenoh_cpp
flowrt launch --run-steps 200 examples/ros2_bridge_demo/rsdl/robot.rsdl
```

Debian 包和安装后用户项目 smoke：

```bash
scripts/test-package-deb.sh
scripts/test-deb-installed-user-project.sh
sudo dpkg -i dist/flowrt_*_*.deb
scripts/test-ros2-bridge-installed.sh --distro jazzy
```

`scripts/test-deb-installed-user-project.sh` 会从 deb 中解包 `flowrt`，在临时用户项目目录里先运行 `flowrt deps --backend all --build-mode release`，再构建示例，验证生成项目不依赖 FlowRT 源码树，并确认用户二进制位于 `flowrt/build/bin/release/`。ROS2 bridge 安装后 smoke 由 `scripts/test-ros2-bridge-installed.sh` 单独负责；它要求本机已经安装对应 ROS2 发行版、`rmw_zenoh_cpp`，并且 PATH 中的 `flowrt` 来自 FlowRT 安装包或解包后的安装前缀。该脚本会先预热 zenoh 依赖，再构建并运行 `ros2_bridge_demo`，确认 generated adapter 链接 ROS2 自带 `zenoh_cpp_vendor` 并能被 `ros2 topic echo /flowrt/text --once` 观察到。CI 会在官方 `ros:jazzy-ros-base-noble` 基础容器中运行常规 Linux job，并额外并行运行 `ros2-jazzy-bridge` 与 `ros2-lyrical-bridge` 两个强制 bridge smoke。

`scripts/test-v060-installed-smoke.sh` 复用系统安装后的 `flowrt`，在临时用户项目中预热 deps、构建
`operation_demo` 并运行 `flowrt op list`，再启动 `cpp_counter_demo` runtime 并用
`flowrt record` 写出 MCAP 文件。它用于验证 v0.6.0 的 Operation 自描述和 record-only
用户路径已经进入安装包 smoke。

Mixed contract 的跨语言 Message ABI roundtrip 可用生成工程直接验证。先构建含 C++ 和 Rust 消息生成物的示例，CMake 会在构建 `message_abi` target 后写出 C++ sample bytes fixture；再运行生成 Rust crate 的 `message_abi` 测试读取并重建这些 fixture：

```bash
flowrt build examples/imu_demo/rsdl/robot.rsdl
cargo test --manifest-path examples/imu_demo/flowrt/build/Cargo.toml --test message_abi
```

含 variable frame message 的 mixed contract 会额外生成 `message_frame` conformance 测试。
C++ 测试编码 canonical frame 并写出 `flowrt/build/abi-fixtures/cpp/*.frame`，Rust 测试读取
该 fixture 并比对同一组固定 header + tail bytes，同时覆盖空 string/bytes/sequence、
多元素 sequence、`sequence<fixed struct>`、UTF-8 string 和 malformed decode：

```bash
flowrt build path/to/native-mixed-variable-frame/rsdl/robot.rsdl
cargo test --manifest-path path/to/native-mixed-variable-frame/flowrt/build/Cargo.toml --test message_frame
```

本地规格与生成物入库防回归：

```bash
tracked=$(
  git ls-files -- \
    docs/architecture-plan.md \
    docs/backend-contract.md \
    docs/contract-ir-v0.1.md \
    docs/message-abi-v0.1.md \
    docs/rsdl-v0.1.md \
    'flowrt/**' \
    'examples/*/flowrt/**'
)
if [ -n "$tracked" ]; then
  printf '%s\n' "以下本地规格或 FlowRT 生成物已被 git tracked，必须移出索引："
  printf '%s\n' "$tracked"
  exit 1
fi
printf '%s\n' "未发现被 tracked 的本地规格或 FlowRT 生成物。"
```

仓库开发可以使用 `cargo run -p flowrt-cli -- ...`，但完整 smoke、README、对外文档、示例和最终说明应使用系统安装后的 `flowrt ...`。

仓库内 `flowrt build` 默认不会回退到源码树 `runtime/cpp`；如果安装包未配置且未设置 `FLOWRT_CPP_RUNTIME_DIR`，CLI 和生成 CMake 都会报错。在 FlowRT 仓库内开发时，设置 `FLOWRT_ALLOW_REPO_RUNTIME_FALLBACK=1` 环境变量或 `-DFLOWRT_ALLOW_REPO_RUNTIME_FALLBACK=ON` CMake 变量即可启用源码树回退，不影响正式用户路径。

`scripts/test-v0120-authoring-smoke.sh` 是 Contract-driven App Authoring 的 focused
gate：覆盖 Rust、C++ 和 C 的 `init`、`add`、`check`、`prepare`、`explain`，确认
`init/add/prepare` 不写用户 `app/`，并跑通 Rust/C++/C 示例的普通 `build/run`。
CI 的 `v0.12.0 Authoring Smoke` 会在 amd64 和 arm64 runner 上执行该脚本，package
和 release job 必须依赖该 gate。

`scripts/test-v0130-runtime-completion-smoke.sh` 是 Robot Runtime Completion 的 focused
gate：覆盖 replay、temporary island overlay、抽象 resource contract、external /
boundary health、variable frame、params runtime apply、Operation lifecycle、结构化
diagnostics、record/status、bundle/deploy/doctor/cross 和 C ABI。CI 的
`v0.13.0 Robot Runtime Completion Smoke` 会在 amd64 和 arm64 runner 上执行该脚本，
package 和 release job 必须依赖该 gate；低资源本地机器可用
`FLOWRT_V0130_SMOKE_DRY_RUN=1` 只验证脚本入口。

## 代码与生成物边界

- `flowrt/` 和 `examples/*/flowrt/` 是生成物目录，不入库。
- 生成目录可以删除并重新生成。
- `flowrt/app/app_api.json`、`flowrt/app/implementation.md` 和 `flowrt/app/stubs/` 是
  `prepare` 生成的 App API 事实源、实现清单和参考模板，同样不入库。root component 的
  参考 stub 使用 `flowrt/app/stubs/<lang>/`；module component 使用
  `flowrt/app/stubs/<module>/<lang>/<component>.*`，其中 C++ 使用
  `flowrt/app/stubs/<module>/cpp/src/<component>.cpp`。
- 用户算法代码应放在示例或项目自己的 `app/` 目录，不写进生成文件。
  root component 继续使用 `app/rust/mod.rs`、`app/cpp/**` 和 `app/c/**`；module component
  建议使用 `app/<module>/rust/<component>.rs`、`app/<module>/cpp/src/<component>.cpp` 或
  `app/<module>/c/<component>.c`；C++ headers 放在 `app/<module>/cpp/inc/`。Rust 生成入口
  仍是 `app/rust/mod.rs`，C/C++ module sources 由 generated CMake 自动发现。
- `flowrt init` 只创建项目入口和最小 RSDL；`flowrt add` 只编辑 RSDL，不创建、追加或
  覆盖用户 `app/`。用户参考 `flowrt/app/stubs/` 后自行手写或复制实现。
- FlowRT 管理代码只做 glue：消息、接口、runtime shell、backend 绑定、启动配置和构建文件。
- Codegen 入口必须只消费通过 validator 的 Contract IR；crate public API 也要重新校验传入 IR，避免调用方绕过 CLI 后生成半成品或触发 panic。
- 静态自描述产物必须来自已验证、已投影的 Contract IR。`flowrt/selfdesc/selfdesc.json` 只作为可读 sidecar 和测试辅助；部署后的事实源是生成应用二进制中的 `.flowrt.selfdesc` section。自描述 JSON 要包含静态拓扑、process、channel、profile/target/deployment 和 Message ABI layout，供后续 CLI 在没有 RSDL 源文件时自查。`flowrt-selfdesc` crate 承载 CLI、codegen 和 runtime 共用的 schema 类型、加载/校验和 ABI 格式化，避免在多处复制结构体导致 drift。
- runtime introspection socket 路径只用于发现候选进程，真实身份必须来自 handshake。默认路径优先使用 `$XDG_RUNTIME_DIR/flowrt/<pid>.sock`，fallback 要按当前用户隔离；CLI status 连接后再验证 PID、process、runtime 和 self-description hash。runtime 启动 status socket 时必须先探测同路径 socket 是否仍可连接：live socket 不得覆盖，不可连接的 stale socket 可以回收。Rust runtime 的 live state 是 `IntrospectionState`，status 响应从该 state 取 tick/channel 摘要；`IntrospectionState` 必须在 mutex poison 后恢复访问，避免单个异常连接或线程把全局 live state 带崩。
- generated supervisor 要作为独立控制面进程暴露 `runtime=supervisor` 的 introspection socket，并把 `flowrt/launch/launch.json` 中的子进程作为健康观测对象。supervisor health 要采集 heartbeat、tick stale、exit、restart count 和当前状态，并在 `flowrt status` 展示。当前内置 `on-failure` restart policy 只处理异常退出，最多重启 3 次，退避 100ms 起步、上限 1000ms；正常退出不重启。后续如果把 policy 暴露到 RSDL/IR，必须先明确生命周期、退出码和依赖进程传播语义。
- runtime introspection 控制面常驻，数据面按需启用。生成的 Rust/C++ runtime shell 要为当前 process 的 active channel 预注册 canonical channel 名、message type 和 probe 容量，并注册编译期 self-description JSON；`flowrt echo` 打开 `observe_channel` 连接后，发布路径才允许在成功发布输出后 best-effort 记录 latest payload。无观察者时，热路径最多做 channel-local 原子检查，不做 payload 拷贝、variable frame 编码或 socket 写入；观察连接断开后 probe 必须自动回收。`status` 可以报告 active observer 和 probe drop 计数，`channel_snapshot` 返回 raw/canonical bytes、发布计数和发布时间；CLI 展示必须结合 self-description 的 Message ABI layout，不要在 runtime 层重复定义业务 payload schema。
- Rust runtime tracing exporter 必须保持 additive：默认关闭，关闭时只做 cheap boolean check，不派生 span；启用后从 `IntrospectionStatus` 快照派生 FlowRT span，不驱动 scheduler 或 backend。sink 失败只能返回 tracing diagnostic，不能改写 live status、调度状态或原始 introspection 快照。`observability.trace` resource satisfied 时，launch manifest 和 self-description 的 graph section 才输出 `tracing` 配置；未声明或未满足时不得生成该 section。
- C++ runtime introspection API 要保持与 Rust JSON-line wire 格式兼容：`status`、`self_description`、`channel_snapshot`、`observe_channel` 和结构化 error 的字段语义必须一致。generated Rust/C++ shell 都应启动 PID 命名 socket、注册当前 process active channel，并使用同一套按需 probe 规则。
- C++ only 和 C callback v0 contract 的普通 `flowrt build` / `flowrt run` 走 CMake app
  路径，不依赖 Cargo app；C component 由 generated C++ runtime shell 通过 callback table
  adapter 调用。
- C++ only 和 C callback v0 contract 的 `flowrt build --launcher` 会生成并构建
  supervisor-only Rust crate；该 crate 只负责编排 CMake app，不生成 Rust runtime shell
  或 Rust app binary。
- `[[bridge.ros2]]` 会生成 FlowRT 管理的 C++ ROS2 adapter process；CLI `build` 必须构建 CMake bridge target，即使 contract 没有 C++ 用户 component。FlowRT 与 ROS2 的唯一 bridge backend 是 `zenoh`，ROS2 侧必须使用 `rmw_zenoh_cpp`，不得添加 DDS fallback。
- `flowrt run` 和 `flowrt launch` 只读取已生成产物，不执行 prepare/build，不写 `flowrt/` 输出目录。
- 所有会写 `flowrt/` 输出目录的 CLI 命令都必须在命令级持有 OS advisory lock；`.flowrt.lock` 文件可以残留，PID 只作为诊断内容，真实占用状态必须由锁判断。`check`、`inspect`、`run`、`launch`、`list`、`nodes`、`status`、`echo` 和 `params` 不写生成物，不应获取该锁。
- 生成的 Rust/C++ runtime shell 必须把 SIGINT/SIGTERM 转成 runtime `ShutdownToken`，让长期运行的 scheduler loop 优雅退出，并继续执行 `shutdown` task、`on_stop` 和 `on_shutdown`。CLI 的 `--run-steps` 只是显式运行上限，`--run-ticks` 是兼容别名，二者都不是核心 runtime 行为来源。
- Scheduler v2 以 task 为调度单元：`periodic` 由 timer 唤醒，`on_message` 由输入 revision 或 FIFO backlog 唤醒。前序 task 在同一 step 发布数据后，依赖它的 `on_message` task 必须在同一 drain loop 中继续执行，不能退回“下一轮 polling 才看见”。阻塞等待前的 data-generation barrier 必须在 drain loop 的最后一轮 wake probe 前刷新，避免同一 step 内部发布的数据把 scheduler 自己再次无意义唤醒。transport backend 的 wake probe 只能刷新 endpoint cache，task 输入读取必须使用 cached latest view，避免探测本身消费掉用户回调需要的样本。iox2 typed pub/sub 不直接暴露 sample-arrival waitable，因此 FlowRT 使用同名 iox2 event service 做 sideband wake；zenoh 使用 subscriber callback 唤醒 scheduler。
- `v0.14.0` 的 scheduler 改造必须把 admission 和 task completion 解耦：scheduler 线程负责
  ready 判定、admission、backend commit、introspection 和 deterministic output commit；
  worker 只运行用户 task，并通过 completion queue 或等价通知交还结果。不得因为保持
  deterministic commit order 而同步等待长任务完成，也不得回退成临时 polling。Rust
  generated scheduler 进入 worker 前必须先在 scheduler 线程读取输入 owned snapshot，
  worker closure 只捕获用户组件 handle、参数/输入快照和上下文元数据，不捕获整个
  `App` 或 backend endpoint；iox2 endpoint 必须保持 scheduler-local。Rust/C++
  runtime、generated shell、status、record 和 `flowrt::Context` 必须使用同一套 runtime
  scheduling time 字段：`scheduled_time_ms`、`observed_time_ms`、`lateness_ms`、
  `missed_periods`、`overrun` 和相邻运行 `dt`。这些字段只表达 runtime 观察到的调度
  时序；`v0.14.0` 不承诺硬实时，不实现 sensor event-time、clock domain、PTP、NTP、
  exact sync、approx sync 或多传感器同步策略。
- Runtime 与 codegen 不能吞掉 bind-level channel 语义：`latest` 和 `fifo` 都要保留 `overflow`、`max_age_ms` 与 `stale_policy`，inproc shell 也应使用 timestamped read/write 路径传递 freshness。
- 跨 process group 的 bind 会在该 route 的 Contract IR capability 派生中要求 `topology:multi_process`；跨 target route 还会要求 `topology:multi_host`。validator、normalizer 和 CLI 必须共享同一套 route topology 判定，不要再各自手写 process-boundary 特判。
- Task-level execution intent 也必须映射到 runtime 行为：`deadline_ms` 要进入 required capabilities，并由生成 shell 在用户回调和输出发布边界执行检查。
- 单 instance 多 task 必须由 RSDL parser、Contract IR、validator、launch manifest、self-description 和 Rust/C++ runtime shell 同步支持。旧 `[instance.<name>.task]` 单 task 写法归一化为 `main`，新 `[[instance.<name>.task]]` 必须显式声明唯一 task name。生成 shell 复用同一个用户组件接口，每个 task 只读取/发布自己的端口子集，并为每个 task 建立独立局部 scope；参数 pending apply 仍按 instance 每 Scheduler v2 step 执行一次。`exclusive` task 共享 instance 串行 lane，显式 `parallel` task 可通过不同 lane 进入 worker 并发执行。输出采用 two-phase commit：worker 只运行用户回调并收集 task-local output，scheduler 在线程亲和 owner 上按 deterministic ready order 提交 backend output。
- Message ABI v0.1 的 native ABI 基线仍是 fixed-size plain data；`bytes`、`string` 和 `sequence<T>` 已作为无界 variable frame 落地，`bytes<max=N>`、`string<max=N>` 和 `sequence<T,max=N>` 作为 bounded variable frame 落地。backend 支持必须通过 `abi:variable_payload_frame`、`allocation:bounded` 与 `allocation:unbounded_dynamic` capability 明确声明；`iox2` 只承载 fixed-size plain data 和可推导上界的 bounded variable frame，不承载无界变长或指针所有权 payload。profile 默认 backend 为 `iox2` 且 route 使用无界 variable frame 时，normalizer 会把该 route 自动选择到支持无界变长消息的 backend（当前为 `zenoh`），fixed-size 和 bounded route 仍继续走 `iox2`。
- iox2/zenoh endpoint 需要保持 peer endpoint 重建后的继续收发回归测试。Runtime 提供 C ABI 基础边界和 Rust/C++ health/reconnect 抽象：C header 只包含固定宽度整数编码、borrowed string/bytes/frame view、显式 `has_*` optional 标志和 POD snapshot；Rust 侧必须用 `repr(C)` 镜像类型和转换函数对齐。C ABI `0.3` 在既有 params view/update result、operation status/progress/result summary view、diagnostic view 和 resource health snapshot 基础上新增 params snapshot v1 typed value view。C component callback ABI 当前为 `0.4`，callback table 必须设置 v0 callback 和 task timing 两个 feature bit；context 同时携带 `flowrt_c_param_snapshot_v0_t` JSON snapshot 和 `flowrt_c_param_snapshot_v1_t` typed readonly snapshot，callback 不拥有其中的 string/param view 内存。C component callback table 只表达已编入 app binary 的 C component 与 runtime shell 之间的边界，覆盖 context、fixed input、output slot、lifecycle callback 和 task callback；当前已开放 C codegen adapter、`app/c` 用户接入路径、App API reference stub、readonly params snapshot（含 string borrowed view）和最小 demo，但它不表示完整 C runtime、动态加载或 Python binding 已开放。修改 `runtime/cpp/include/flowrt/abi.h` 时必须同步 Rust ABI layout 测试和 C++ runtime smoke。`iox2` 和 `zenoh` endpoint 已接入自动恢复，本地 transport 资源丢失或操作失败会重建本地 publisher/subscriber/session；codec/schema 错误不触发重连。恢复逻辑必须留在 backend endpoint 层，不要在 generated shell 中临时吞掉错误。
- Mixed contract 的 Message ABI conformance 不能只依赖同一生成器内嵌的 expected bytes；C++ test 写出的 fixture 和 Rust test 读取后的 typed roundtrip 都应保持可运行。
- 扩展 backend capability 时，先在 `flowrt-ir` 的 typed capability catalog 中维护全局 canonical 顺序，再由 `backend_capabilities`、`channel_route_capabilities`、`channel_capabilities`、`trigger_capability` 或 message ABI 推导函数输出既有 `CapabilityAtom` 字符串。凡是 backend、target、deployment、route、channel 的 capability 组合，都要先去重再按该 catalog 顺序输出，不能依赖声明顺序或首次出现顺序；新增或重排 catalog 都会改变 canonical IR 顺序，因此必须同步补顺序独立测试。不要在 validator、normalizer 或 codegen 中散落新 capability 字符串。
- Rust/C++ runtime 的 backend capability 报告顺序也必须跟随同一个 catalog；runtime smoke test 应精确断言顺序，避免自描述、诊断和跨语言对比输出出现漂移。
- deployment satisfaction 和 route backend satisfaction 都只能通过 `flowrt-ir` 的集中 typed decision 推导；normalizer 和 validator 必须复用同一 decision 入口，不能各自复制 unknown backend、target 未声明支持、missing required capabilities 或 satisfied 的判断逻辑，也不能把 `TargetIr.capabilities`、`ChannelEdgeIr.capability_requirements` 或 `DeploymentIr.satisfied` 当作事实源。
- 抽象 resource contract 只表达 component 需要的 capability、访问方式、必需/可选、
  readiness、health 和失败传播；provider 只表达 target、process 或 external package
  作用域能够提供哪些抽象 capability。不要把串口、TCP、UDP、USB、V4L2、RKNN、
  CUDA、设备路径、端口号或板级 SDK 名称放进 RSDL/IR core resource schema。
  `resource_satisfactions` 是派生元数据，validator 必须重新推导并拒绝篡改。

## 文档维护

必须入库：

- `README.md`
- `CHANGELOG.md`
- `AGENTS.md`
- `docs/README.md`
- `docs/getting-started.md`
- `docs/project-layout.md`
- `docs/cli.md`
- `docs/examples.md`
- `docs/development.md`
- 后续新增的用户指南、维护指南、示例说明和 troubleshooting 文档

默认不入库：

- `docs/architecture-plan.md`
- `docs/rsdl-v0.1.md`
- `docs/contract-ir-v0.1.md`
- `docs/message-abi-v0.1.md`
- `docs/backend-contract.md`

判断规则：

- 已落地的用户流程、命令、示例、验证方法和维护规则属于配套文档，应入库。
- 未冻结的架构计划、语义规格草案和本地设计推演属于设计文档，不入库。
- 任何影响语义、接口、目录结构、命令或生成物边界的变更，都要同步更新配套文档或在最终说明中解释为什么不需要更新。

文档正文使用中文；代码标识符、命令、配置键、协议名和必要专有名词可以保留英文。

## 发布流程

FlowRT 的 release notes 来自 `CHANGELOG.md`。正式推送 `v*` tag 前，必须先把待发布
分支推到 `dev/vX.Y.Z`。发布分支 push 会触发 `release-candidate.yml`，运行完整发布
同款矩阵、构建 amd64/arm64 deb artifact，并在最后运行 `Release Evidence Gate`：该
gate 校验版本、release notes、deb 架构、deb 版本和 `SHA256SUMS`，然后上传
`flowrt-release-evidence` artifact。日常 `ci.yml` 只做快速 push/PR 验证，不产出
release evidence；仓库不再使用手工 `workflow_dispatch` 发布候选入口。

推送 `v*` tag 后，独立的 `release.yml` 会解析 tag 指向的 commit SHA，并只查询同一
commit SHA 上已经成功的 `release-candidate.yml` push run。
只有该 run 内的 `Release Evidence Gate` 成功，tag release 才会下载同一 run 的 deb 与
evidence artifact，复核 evidence 的 version/tag/sha、deb 元数据和 `SHA256SUMS`，然后创建
GitHub Release。tag workflow 不重跑完整矩阵，避免发布 tag 因重复构建再次消耗资源或引入
新的不确定性。

发布分支 release candidate run 会等待
`guard-generated`、amd64/arm64 Rust fmt/test/clippy、amd64/arm64 C++ runtime、
amd64/arm64 v0.5.0 runtime focused smoke、amd64/arm64 v0.6.0 runtime focused smoke、
amd64/arm64 v0.7.0 external/deploy focused smoke、amd64/arm64 v0.8.0 integration
focused smoke、amd64/arm64 v0.8.1 FrameDescriptor focused smoke、v0.8.3 交叉编译
focused smoke、amd64/arm64 v0.9.0 island focused smoke、amd64/arm64 v0.9.1 island
tooling focused smoke、amd64/arm64 v0.9.2 island offline validation focused smoke、
amd64/arm64 v0.10.2 concurrency focused smoke、amd64/arm64 v0.11.0 App SDK
focused smoke、amd64/arm64 v0.12.0 authoring focused smoke、amd64/arm64 v0.13.0
robot runtime completion focused smoke、amd64/arm64 v0.14.0 realtime scheduler
focused smoke、amd64/arm64 v0.14.1 architecture focused smoke、amd64/arm64
v0.15.0 architecture convergence focused smoke、v0.15.1 CI release evidence smoke、
v0.15.2 scheduler clock focused smoke、v0.23.0 Zenoh Service focused smoke、
v0.23.1 Route Health focused smoke、v0.23.2 C++ clang-tidy focused smoke、
v0.23.3 Scope Closure focused smoke、amd64/arm64 C++ zenoh runtime、amd64/arm64 deb
package、v0.8.3 安装版 amd64 到 arm64 cross smoke、amd64/arm64 demo smoke、
amd64/arm64 ROS2 Jazzy bridge smoke 和 amd64/arm64 ROS2 Lyrical bridge smoke 全部通过，
才会产出发布证据。

`v0.5.0 Runtime Smoke` 是面向新 runtime 能力的可诊断 gate，使用 `-j1` 分别覆盖
supervisor readiness/resource、远程参数控制面、status/hz 健康展示、scheduler
health 和 runtime introspection。它不替代 workspace 全量 Rust 测试，而是让 v0.5.0
主线能力失败时能直接定位到对应 job step。

`v0.6.0 Runtime Smoke` 使用同样的 amd64/arm64 gate，覆盖 Operation 的
RSDL/IR/validator/codegen/runtime/CLI/status 路径，以及 record format、runtime tap 和
CLI 写 MCAP 路径。安装包后的 demo smoke 会继续运行 `scripts/test-v060-installed-smoke.sh`，
验证用户从系统安装的 `flowrt` 入口使用 Operation 和 record。

`v0.7.0 External/Deploy Smoke` 覆盖 external component RSDL/IR/validator/codegen、
runtime external supervisor、bundle 和 deploy CLI 主路径。安装包后的 demo smoke 会运行
`scripts/test-v070-installed-smoke.sh`，验证安装版 `flowrt` 能检查 external package、
构建 launcher、打 bundle 并执行 deploy dry-run。

`v0.8.0 Integration Smoke` 覆盖 I/O boundary、FrameDescriptor、ROS2 typed bridge、
variable frame codegen、diagnostics、bundle 和 deploy 主路径。安装包后的 demo smoke 会运行
`scripts/test-v080-installed-smoke.sh`，验证安装版 `flowrt` 能构建 variable frame demo、
运行 I/O boundary runtime、观察 live status，并用 bundle schema v2 执行 deploy dry-run。

`v0.8.1 FrameDescriptor Smoke` 覆盖标准 64 字节 descriptor 示例、结构化 `echo`、
I/O boundary descriptor schema status、descriptor-only record 和 microbench。安装包后的
demo smoke 会运行 `scripts/test-v081-installed-smoke.sh`，验证安装版 `flowrt` 能构建
并运行 `frame_descriptor_demo`。

`v0.8.3 Cross Toolchain Smoke` 固定在 amd64 host 上验证 `linux-arm64` toolchain
profile、Rust target、C/C++ 交叉编译器、SDK overlay 配置和 CMake target SDK 参数。
`v0.8.3 Installed amd64 to arm64 Smoke` 在 package job 产出的 amd64 deb 上运行
`scripts/test-v083-installed-smoke.sh`，验证安装包内嵌完整 `linux-arm64` target SDK、
`flowrt doctor --target linux-arm64` 和真实 `flowrt build --target linux-arm64`
C++ demo，并用 ELF header 确认输出为 AArch64。

`v0.8.6 Cross UX SDK Smoke` 固定在 amd64 host 上安装 package job 产出的 amd64
deb，使用 `examples/cross_sdk_deps` 准备公开 arm64 SDK overlay，并运行
`scripts/test-v086-cross-sdk-demos.sh`。该 gate 覆盖两类真实外部依赖：平台无关 C/C++
库 `libjpeg-turbo`，以及 Arm 专用公开 SDK `Arm KleidiAI`。脚本通过
`flowrt toolchain init/show` 生成并展示最小 workspace profile，再用带 RSDL 的
`flowrt doctor <rsdl> --target linux-arm64` 检查 `component.build.pkg_config`
依赖，最后执行 `flowrt deps/build --target linux-arm64` 并检查 AArch64 ELF。CI 缓存
只保存 `.flowrt-public-sdk/v086-arm64` 和 `.flowrt-cache`，不把第三方源代码或编译产物
加入 Git。

`v0.9.0 Island Demo Smoke` 在 amd64 与 arm64 runner 上构建并运行
`examples/island_demo`，通过 `flowrt pub` 向 boundary input 注入 typed JSON，再用
`flowrt echo` 校验 boundary output。该 gate 保证 island 脚手架、边界输入输出和安装后
CLI 观测路径一起进入 release 主路径。

`v0.9.1 Island Migration Tooling Smoke` 在 amd64 与 arm64 runner 上覆盖迁移验证工具
hardening：focused crate tests 检查 `params set --file`、`flowrt pub`、显式空消息的
parser/IR/validator/codegen/CLI 路径；真实 smoke 运行
`examples/variable_frame_island_demo`，用 JSONL、`pub --file --freq` 和 `echo`
验证 variable frame boundary input 到 fixed summary output 的闭环。

`v0.9.2 Island Offline Validation Smoke` 在 amd64 与 arm64 runner 上覆盖
`flowrt check` generated handler signature、`flowrt replay`、`echo --raw`、
temporary island overlay、bundle/deploy island gate 和共享 Cargo target 下的 app
hash 隔离。真实 smoke 使用临时 RSDL 和用户代码，不新增长期示例目录，验证 strict
contract 可以通过 CLI 一次性投影成 test-only island 进行多 boundary input 离线注入。

`v0.10.2 Concurrency Hardening Smoke` 在 amd64 与 arm64 runner 上覆盖 codegen 并发
focused tests、Rust iox2 generated shell、backend route、Rust/C++ runtime executor，
并用临时复制的 `import_demo` 和 `cpp_counter_demo` 验证 generated Rust/C++ shell 可构建。
该 gate 保证 worker 只执行用户 task、scheduler 按 canonical ready order 提交 output，
以及 iox2 scheduler-local transport commit 不再把整批 task 串行化。

`v0.12.0 Authoring Smoke` 在 amd64 与 arm64 runner 上覆盖 `flowrt init` Rust/C++/C、
项目内 `flowrt add message/component`、`flowrt check`、`flowrt prepare` 和
`flowrt explain --format text/json`，确认 `init/add/prepare` 不创建或覆盖用户 `app/`，
并在临时 `import_demo`、`cpp_counter_demo` 和 `c_counter_demo` 副本上按 runner 架构改写
target platform 后运行普通 `flowrt build` / `flowrt run`。CI 容器会安装 CMake、Ninja、
g++ 和 pkg-config；低资源本地机器可用 `FLOWRT_V0120_SMOKE_DRY_RUN=1` 只验证脚本入口，
但 package/release gate 不使用 dry-run。

`v0.13.0 Robot Runtime Completion Smoke` 在 amd64 与 arm64 runner 上覆盖当前
机器人 runtime completion 主线：replay / temporary island overlay、抽象 resource
contract、external/boundary health、variable frame、params runtime apply、Operation
lifecycle、diagnostics/status/record、bundle/deploy/doctor/cross 和 C ABI。该 gate
只调用 focused crate tests，不新增长期 demo 目录；本地低资源机器可用
`FLOWRT_V0130_SMOKE_DRY_RUN=1 scripts/test-v0130-runtime-completion-smoke.sh` 验证
入口和目标平台参数。

`v0.14.0 Realtime Scheduler Smoke` 在 amd64 与 arm64 runner 上覆盖 executor
admission/completion、Rust/C++ generated scheduler 非阻塞主路径、
status/introspection timing 字段和 C ABI task timing layout。该 gate 不保留旧同步
helper 兼容路径，release package 和 release job 必须等待该 gate。

`v0.14.1 Architecture Guard` 在 amd64 与 arm64 runner 上覆盖 architecture size guard 和
已完成的大文件拆分边界。`v0.15.0 Architecture Convergence Smoke` 通过 release gate
registry 查询 focused smoke，并串联脚本语法检查、`scripts/check-architecture-size.sh`
和 `scripts/check-architecture-contract.sh`。后者检查 release gate contract、Contract IR
derived facts 和 runtime observability facts 是否已经进入 validator、codegen、status、
diagnostics 与 recorder 的生产消费路径。`v0.15.1 CI Release Evidence Smoke` 检查日常 CI
不产出 release evidence、release candidate 只在发布分支 push 上产出 evidence、tag release
只消费同一 commit SHA 的成功 evidence，并确认本地 helper 不再手工触发远端 workflow。
`v0.15.2 Scheduler Clock Smoke`
检查 realtime generated scheduler 清空 boundary/replay 样本时间戳但不把它推进
runtime scheduling time，同时确认 temporary island overlay 继续使用 fixture 时间驱动
simulated replay clock。

release readiness 还会检查 `CONTEXT.md` 的“当前 workspace 版本为 `X.Y.Z`”状态行。
发布后如果只移动 `CHANGELOG.md` 版本段而忘记更新当前上下文，脚本会拒绝通过。

发布前检查：

```bash
version=X.Y.Z
tag="v${version}"
cargo run -p flowrt-devtools -- release-gate check-registry "$version"
cargo run -p flowrt-devtools -- release-gate focused-smoke "$version"
scripts/check-architecture-contract.sh
scripts/check-release-readiness.sh "$version"
scripts/extract-release-notes.sh "$tag" CHANGELOG.md
scripts/check-release-candidate.sh "$version"
git push origin "HEAD:refs/heads/dev/v${version}"
scripts/check-release-candidate.sh "$version" --wait --ref "dev/v${version}"
```

要求：

- `CHANGELOG.md` 必须包含对应二级标题，格式为 `## vX.Y.Z - YYYY-MM-DD`。
- tag 名必须是 `vX.Y.Z`，且版本号必须与根 `Cargo.toml` 的 workspace version 一致。
- tag 只能指向已经通过同一 commit SHA `Release Evidence Gate` 的提交；不要在发布分支
  release candidate run 成功前直接推 tag。
- 对应版本段不能为空；CI 会把该段原样作为 GitHub Release 说明。
- `## 未发布` 只放尚未发布的后续变化；正式发版前把本次条目移入版本段。
- `v0.15.0` 之后，release evidence 本地预检会先运行 release readiness，再通过
  release gate registry 选择对应版本的 focused smoke；新增版本必须先登记 registry，
  不要在 release helper 里手写版本分支。

release evidence 通过后，创建并推送 tag：

```bash
git tag -a "$tag" -m "$tag"
git push origin "$tag"
```

## 提交规则

提交必须原子化。一次提交只做一类相关改动，不把多个不相关功能、修复、文档和发布
准备混入同一次提交。Agent 创建提交时不能只写标题，必须写正文说明关键动机、边界
和验证。

采用 Conventional Commits 格式，提交信息使用中文：

```text
<type>(<scope>): <中文标题>

<正文>
```

格式要求：

- `type` 和 `scope` 使用英文。
- 标题和正文使用中文。
- 标题不超过 50 个中文字符，不加句号。
- 正文每行不超过 72 个字符。
- 正文可用 `-` 列要点。
- `scope` 用小括号标注影响范围，影响全局时可省略。

常用类型：

| 类型 | 说明 | 示例 |
|---|---|---|
| `feat` | 新功能 | `feat(rsdl): 增加 module 声明解析` |
| `fix` | Bug 修复 | `fix(cli): 修复 lock 残留后的构建判断` |
| `docs` | 仅文档变更 | `docs(readme): 更新安装后使用说明` |
| `refactor` | 重构，不改变行为 | `refactor(ir): 拆分 backend 能力推导` |
| `test` | 添加或修改测试 | `test(parser): 补充 workspace 边界用例` |
| `chore` | 构建、配置或工具类 | `chore(scripts): 整理 deb smoke 脚本` |
| `perf` | 性能优化 | `perf(runtime): 减少 echo 热路径分支开销` |
| `style` | 格式调整，不影响逻辑 | `style: 统一 C++ include 顺序` |
| `ci` | CI 或发布流水线 | `ci(release): 拆分 deb 发布 gate` |
| `build` | 构建系统或打包 | `build(package): 内嵌 zenoh C++ SDK` |
| `revert` | 回滚提交 | `revert: 撤销错误的模块命名变更` |

示例：

```text
fix(validate): 拒绝缺失任务输入绑定

- 在 graph 校验中检查 active task input
- 覆盖缺失和重复 bind 的回归用例
- 验证 cargo test -p flowrt-validate
```

提交应保持原子：

- 一个提交只包含同一主题的代码、测试、文档和 changelog 更新。
- 提交前运行与改动匹配的最小验证。
- 提交前运行本地规格与生成物入库防回归检查，确认 `docs/` 下本地规格草案、`flowrt/` 和 `examples/*/flowrt/` 没有被 git tracked。
- 不把生成物、大型构建输出或未验证半成品混入提交。
- `docs/` 下被 `.gitignore` 排除的本地设计/规格文件不得加入索引。
- 禁止英文提交信息、模糊标题和没有 Conventional Commits 前缀的裸文字提交。

## backend 依赖与离线包

Rust runtime 的 iox2 支持通过 feature-gated `iceoryx2 = "0.9"` 编译。C++ iox2 binding 只有在定义 `FLOWRT_HAS_ICEORYX2_CXX` 并链接 `iceoryx2-cxx 0.9.1` 时使用真实 transport。Rust runtime 的 zenoh 支持通过 feature-gated `zenoh = "1.9"` 编译。C++ zenoh binding 只有在定义 `FLOWRT_HAS_ZENOH_CXX` 并链接基于 `zenoh-c` backend 的 `zenohcxx::zenohc` 时使用真实 transport。

FlowRT Debian 包会把锁定版本的 Rust crate vendor、`iceoryx2-cxx 0.9.1`、`zenoh-c 1.9.0`、`zenoh-cpp 1.9.0` 和第三方 license material 放入 `/opt/flowrt/<version>` 私有前缀。安装后的 `flowrt deps` 会用包内 vendor 预热全局共享 cache；`flowrt build` 会复用 cache，只编译用户项目和 generated shell，并自动把私有前缀传给 generated CMake。生成项目构建不应通过 `FetchContent`、Cargo registry 或其他外部网络路径临时解析 backend SDK。

基础 Rust/C++ inproc 验证和 `check` smoke 不要求启用 C++ iox2/zenoh 测试。直接调试 `runtime/cpp` 的 backend smoke 时，需要用 `CMAKE_PREFIX_PATH=/opt/flowrt/<version>` 或等价路径暴露 FlowRT 私有前缀。

C++ iox2 runtime smoke：

```bash
CMAKE_PREFIX_PATH=/opt/flowrt/0.2.0 \
  cmake -S runtime/cpp -B build/cpp-iox2 -G Ninja -DFLOWRT_CPP_ENABLE_IOX2_TESTS=ON
cmake --build build/cpp-iox2 --target flowrt_runtime_iox2_smoke
ctest --test-dir build/cpp-iox2 -R flowrt_runtime_iox2_smoke --output-on-failure
```

C++ zenoh runtime smoke：

```bash
CMAKE_PREFIX_PATH=/opt/flowrt/0.2.0 \
  cmake -S runtime/cpp -B build/cpp-zenoh -G Ninja -DFLOWRT_CPP_ENABLE_ZENOH_TESTS=ON
cmake --build build/cpp-zenoh --target flowrt_runtime_zenoh_smoke
ctest --test-dir build/cpp-zenoh -R flowrt_runtime_zenoh_smoke --output-on-failure
```

该 smoke 覆盖 C++ zenoh pub/sub endpoint、session 重建和 remote Operation queryable 的
真实 SDK query/reply 路径。
