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

VSCode / clangd：

```bash
cmake -S runtime/cpp -B build/cpp
cargo run -p flowrt-cli -- prepare examples/cpp_counter_demo/rsdl/robot.rsdl
cargo run -p flowrt-cli -- prepare examples/imu_demo_iox2/rsdl/robot.rsdl
cargo run -p flowrt-cli -- prepare examples/mixed_iox2_demo/rsdl/robot.rsdl
cargo run -p flowrt-cli -- prepare examples/mixed_zenoh_demo/rsdl/robot.rsdl
cargo run -p flowrt-cli -- prepare examples/ros2_bridge_demo/rsdl/robot.rsdl
```

仓库根目录的 `.clangd` 会让 `runtime/cpp/**` 使用 `build/cpp/compile_commands.json`，并让 `examples/*/src/cpp/**` 读取本示例自己的 `flowrt/cpp/include` 生成头。`flowrt/` 和 `examples/*/flowrt/` 仍是可删除、可重建的生成物，不入库；如果清理过这些目录，需要先重新执行对应示例的 `prepare` 或 `build`，再重启 clangd。

FlowRT demo smoke：

```bash
scripts/package-deb.sh --output-dir dist
sudo dpkg -i dist/flowrt_*_*.deb
flowrt --version

flowrt build --launcher examples/cpp_counter_demo/rsdl/robot.rsdl
flowrt run --run-steps 5 examples/cpp_counter_demo/rsdl/robot.rsdl --process control
flowrt launch --run-steps 5 examples/cpp_counter_demo/rsdl/robot.rsdl
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

`scripts/test-deb-installed-user-project.sh` 会从 deb 中解包 `flowrt`，在临时用户项目目录里构建示例，验证生成项目不依赖 FlowRT 源码树。ROS2 bridge 安装后 smoke 由 `scripts/test-ros2-bridge-installed.sh` 单独负责；它要求本机已经安装对应 ROS2 发行版、`rmw_zenoh_cpp`，并且 PATH 中的 `flowrt` 来自 FlowRT 安装包或解包后的安装前缀。该脚本会构建并运行 `ros2_bridge_demo`，确认 generated adapter 链接 ROS2 自带 `zenoh_cpp_vendor` 并能被 `ros2 topic echo /flowrt/text --once` 观察到。CI 会在官方 `ros:jazzy-ros-base-noble` 基础容器中运行常规 Linux job，并额外并行运行 `ros2-jazzy-bridge` 与 `ros2-lyrical-bridge` 两个强制 bridge smoke。

Mixed contract 的跨语言 Message ABI roundtrip 可用生成工程直接验证。先构建含 C++ 和 Rust 消息生成物的示例，CMake 会在构建 `message_abi` target 后写出 C++ sample bytes fixture；再运行生成 Rust crate 的 `message_abi` 测试读取并重建这些 fixture：

```bash
flowrt build examples/imu_demo/rsdl/robot.rsdl
cargo test --manifest-path examples/imu_demo/flowrt/build/Cargo.toml --test message_abi
```

本地规格与生成物入库防回归：

```bash
tracked=$(
  git ls-files -- \
    docs/architecture-plan.md \
    docs/backend-contract.md \
    docs/contract-ir-v0.1.md \
    docs/message-abi-v0.1.md \
    docs/project-layout.md \
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

仓库开发可以使用 `cargo run -p flowrt-cli -- ...`，但完整 smoke、面向用户的 README、文档、示例和最终说明应使用系统安装后的 `flowrt ...`。

## 代码与生成物边界

- `flowrt/` 和 `examples/*/flowrt/` 是生成物目录，不入库。
- 生成目录可以删除并重新生成。
- 用户算法代码应放在示例或项目自己的 `src/` 目录，不写进生成文件。
- FlowRT 管理代码只做 glue：消息、接口、runtime shell、backend 绑定、启动配置和构建文件。
- Codegen 入口必须只消费通过 validator 的 Contract IR；crate public API 也要重新校验传入 IR，避免调用方绕过 CLI 后生成半成品或触发 panic。
- 静态自描述产物必须来自已验证、已投影的 Contract IR。`flowrt/selfdesc/selfdesc.json` 只作为可读 sidecar 和测试辅助；部署后的事实源是生成应用二进制中的 `.flowrt.selfdesc` section。自描述 JSON 要包含静态拓扑、process、channel、profile/target/deployment 和 Message ABI layout，供后续 CLI 在没有 RSDL 源文件时自查。
- runtime introspection socket 路径只用于发现候选进程，真实身份必须来自 handshake。默认路径优先使用 `$XDG_RUNTIME_DIR/flowrt/<pid>.sock`，fallback 要按当前用户隔离；CLI status 连接后再验证 PID、process、runtime 和 self-description hash。runtime 启动 status socket 时必须先探测同路径 socket 是否仍可连接：live socket 不得覆盖，不可连接的 stale socket 可以回收。Rust runtime 的 live state 是 `IntrospectionState`，status 响应从该 state 取 tick/channel 摘要；`IntrospectionState` 必须在 mutex poison 后恢复访问，避免单个异常连接或线程把全局 live state 带崩。
- generated supervisor 要作为独立控制面进程暴露 `runtime=supervisor` 的 introspection socket，并把 `flowrt/launch/launch.json` 中的子进程作为健康观测对象。supervisor health 要采集 heartbeat、tick stale、exit、restart count 和当前状态，并在 `flowrt status` 展示。当前内置 `on-failure` restart policy 只处理异常退出，最多重启 3 次，退避 100ms 起步、上限 1000ms；正常退出不重启。后续如果把 policy 暴露到 RSDL/IR，必须先明确生命周期、退出码和依赖进程传播语义。
- runtime introspection 控制面常驻，数据面按需启用。生成的 Rust/C++ runtime shell 要为当前 process 的 active channel 预注册 canonical channel 名、message type 和 probe 容量，并注册编译期 self-description JSON；`flowrt echo` 打开 `observe_channel` 连接后，发布路径才允许在成功发布输出后 best-effort 记录 latest payload。无观察者时，热路径最多做 channel-local 原子检查，不做 payload 拷贝、variable frame 编码或 socket 写入；观察连接断开后 probe 必须自动回收。`status` 可以报告 active observer 和 probe drop 计数，`channel_snapshot` 返回 raw/canonical bytes、发布计数和发布时间；CLI 展示必须结合 self-description 的 Message ABI layout，不要在 runtime 层重复定义业务 payload schema。
- C++ runtime introspection API 要保持与 Rust JSON-line wire 格式兼容：`status`、`self_description`、`channel_snapshot`、`observe_channel` 和结构化 error 的字段语义必须一致。generated Rust/C++ shell 都应启动 PID 命名 socket、注册当前 process active channel，并使用同一套按需 probe 规则。
- C++ only contract 的普通 `flowrt build` / `flowrt run` 走 CMake app 路径，不依赖 Cargo app。
- C++ only contract 的 `flowrt build --launcher` 会生成并构建 supervisor-only Rust crate；该 crate 只负责编排 C++ app，不生成 Rust runtime shell 或 Rust app binary。
- `[[bridge.ros2]]` 会生成 FlowRT 管理的 C++ ROS2 adapter process；CLI `build` 必须构建 CMake bridge target，即使 contract 没有 C++ 用户 component。FlowRT 与 ROS2 的唯一 bridge backend 是 `zenoh`，ROS2 侧必须使用 `rmw_zenoh_cpp`，不得添加 DDS fallback。
- `flowrt run` 和 `flowrt launch` 只读取已生成产物，不执行 prepare/build，不写 `flowrt/` 输出目录。
- 所有会写 `flowrt/` 输出目录的 CLI 命令都必须在命令级持有 OS advisory lock；`.flowrt.lock` 文件可以残留，PID 只作为诊断内容，真实占用状态必须由锁判断。`check`、`inspect`、`run`、`launch`、`list`、`nodes`、`status`、`echo` 和 `params` 不写生成物，不应获取该锁。
- 生成的 Rust/C++ runtime shell 必须把 SIGINT/SIGTERM 转成 runtime `ShutdownToken`，让长期运行的 scheduler loop 优雅退出，并继续执行 `shutdown` task、`on_stop` 和 `on_shutdown`。CLI 的 `--run-steps` 只是显式运行上限，`--run-ticks` 是兼容别名，二者都不是核心 runtime 行为来源。
- Scheduler v2 以 task 为调度单元：`periodic` 由 timer 唤醒，`on_message` 由输入 revision 或 FIFO backlog 唤醒。前序 task 在同一 step 发布数据后，依赖它的 `on_message` task 必须在同一 drain loop 中继续执行，不能退回“下一轮 polling 才看见”。阻塞等待前的 data-generation barrier 必须在 drain loop 的最后一轮 wake probe 前刷新，避免同一 step 内部发布的数据把 scheduler 自己再次无意义唤醒。transport backend 的 wake probe 只能刷新 endpoint cache，task 输入读取必须使用 cached latest view，避免探测本身消费掉用户回调需要的样本。iox2 typed pub/sub 不直接暴露 sample-arrival waitable，因此 FlowRT 使用同名 iox2 event service 做 sideband wake；zenoh 使用 subscriber callback 唤醒 scheduler。
- Runtime 与 codegen 不能吞掉 bind-level channel 语义：`latest` 和 `fifo` 都要保留 `overflow`、`max_age_ms` 与 `stale_policy`，inproc shell 也应使用 timestamped read/write 路径传递 freshness。
- 跨 process group 的 bind 会在该 route 的 Contract IR capability 派生中要求 `topology:multi_process`；跨 target route 还会要求 `topology:multi_host`。validator、normalizer 和 CLI 必须共享同一套 route topology 判定，不要再各自手写 process-boundary 特判。
- Task-level execution intent 也必须映射到 runtime 行为：`deadline_ms` 要进入 required capabilities，并由生成 shell 在用户回调和输出发布边界执行检查。
- 单 instance 多 task 必须由 RSDL parser、Contract IR、validator、launch manifest、self-description 和 Rust/C++ runtime shell 同步支持。旧 `[instance.<name>.task]` 单 task 写法归一化为 `main`，新 `[[instance.<name>.task]]` 必须显式声明唯一 task name。生成 shell 当前复用同一个用户组件接口，每个 task 只读取/发布自己的端口子集，并为每个 task 建立独立局部 scope；参数 pending apply 仍按 instance 每 Scheduler v2 step 执行一次。Rust/C++ runtime 可以提供 `WorkerPool` 和 coroutine/future substrate，但 generated shell 在没有明确 lane ownership 和用户 API 边界前仍同步调用普通用户组件。
- Message ABI v0.1 的 native ABI 基线仍是 fixed-size plain data；`bytes`、`string` 和 `sequence<T>` 已作为无界 variable frame 落地。backend 支持必须通过 `abi:variable_payload_frame` 与 `allocation:unbounded_dynamic` capability 明确声明；`iox2` 不承载 variable frame。profile 默认 backend 为 `iox2` 且 route 使用 variable frame 时，normalizer 会把该 route 自动选择到支持变长消息的 backend（当前为 `zenoh`），fixed-size route 仍继续走 `iox2`。
- iox2/zenoh endpoint 需要保持 peer endpoint 重建后的继续收发回归测试。Runtime 提供 C ABI 友好形状的 `BackendHealthState`、`BackendHealthSnapshot`、`ReconnectPolicy` 和 `BackendHealthTracker`：状态只表达 `ready`、`degraded`、`reconnecting`、`failed`，退避策略使用毫秒和 attempt 数，后续 C、Python 或更多语言 runtime 应复用该稳定形状。`iox2` 和 `zenoh` endpoint 已接入自动恢复，本地 transport 资源丢失或操作失败会重建本地 publisher/subscriber/session；codec/schema 错误不触发重连。恢复逻辑必须留在 backend endpoint 层，不要在 generated shell 中临时吞掉错误。
- Mixed contract 的 Message ABI conformance 不能只依赖同一生成器内嵌的 expected bytes；C++ test 写出的 fixture 和 Rust test 读取后的 typed roundtrip 都应保持可运行。
- 扩展 backend capability 时，先在 `flowrt-ir` 的 typed capability catalog 中维护全局 canonical 顺序，再由 `backend_capabilities`、`channel_route_capabilities`、`channel_capabilities`、`trigger_capability` 或 message ABI 推导函数输出既有 `CapabilityAtom` 字符串。凡是 backend、target、deployment、route、channel 的 capability 组合，都要先去重再按该 catalog 顺序输出，不能依赖声明顺序或首次出现顺序；新增或重排 catalog 都会改变 canonical IR 顺序，因此必须同步补顺序独立测试。不要在 validator、normalizer 或 codegen 中散落新 capability 字符串。
- Rust/C++ runtime 的 backend capability 报告顺序也必须跟随同一个 catalog；runtime smoke test 应精确断言顺序，避免自描述、诊断和跨语言对比输出出现漂移。
- deployment satisfaction 和 route backend satisfaction 都只能通过 `flowrt-ir` 的集中 typed decision 推导；normalizer 和 validator 必须复用同一 decision 入口，不能各自复制 unknown backend、target 未声明支持、missing required capabilities 或 satisfied 的判断逻辑，也不能把 `TargetIr.capabilities`、`ChannelEdgeIr.capability_requirements` 或 `DeploymentIr.satisfied` 当作事实源。

## 文档维护

必须入库：

- `README.md`
- `CHANGELOG.md`
- `AGENTS.md`
- `docs/README.md`
- `docs/getting-started.md`
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
- `docs/project-layout.md`

判断规则：

- 已落地的用户流程、命令、示例、验证方法和维护规则属于配套文档，应入库。
- 未冻结的架构计划、语义规格草案和本地设计推演属于设计文档，不入库。
- 任何影响语义、接口、目录结构、命令或生成物边界的变更，都要同步更新配套文档或在最终说明中解释为什么不需要更新。

文档正文使用中文；代码标识符、命令、配置键、协议名和必要专有名词可以保留英文。

## 发布流程

FlowRT 的 release notes 来自 `CHANGELOG.md`。推送 `v*` tag 后，CI 会等待 `guard-generated`、Rust fmt/test/clippy、C++ runtime、C++ zenoh runtime、deb package、demo smoke、ROS2 Jazzy bridge smoke 和 ROS2 Lyrical bridge smoke 全部通过，再创建 GitHub Release，并上传 `.deb` 与 `SHA256SUMS`。

发布前检查：

```bash
tag=v0.2.0
scripts/extract-release-notes.sh "$tag" CHANGELOG.md
```

要求：

- `CHANGELOG.md` 必须包含对应二级标题，格式为 `## vX.Y.Z - YYYY-MM-DD`。
- tag 名必须是 `vX.Y.Z`，且版本号必须与根 `Cargo.toml` 的 workspace version 一致。
- 对应版本段不能为空；CI 会把该段原样作为 GitHub Release 说明。
- `## 未发布` 只放尚未发布的后续变化；正式发版前把本次条目移入版本段。

创建并推送 tag：

```bash
git tag -a v0.2.0 -m "v0.2.0"
git push origin v0.2.0
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

FlowRT Debian 包会把锁定版本的 Rust crate vendor、`iceoryx2-cxx 0.9.1`、`zenoh-c 1.9.0`、`zenoh-cpp 1.9.0` 和第三方 license material 放入 `/opt/flowrt/<version>` 私有前缀。安装后的 `flowrt build` 会自动把该前缀传给 generated CMake，并为 generated Rust app 写入离线 Cargo config。生成项目构建不应通过 `FetchContent`、Cargo registry 或其他外部网络路径临时解析 backend SDK。

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
