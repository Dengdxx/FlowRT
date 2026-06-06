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
```

仓库根目录的 `.clangd` 会让 `runtime/cpp/**` 使用 `build/cpp/compile_commands.json`，并让 `examples/*/src/cpp/**` 读取本示例自己的 `flowrt/cpp/include` 生成头。`flowrt/` 和 `examples/*/flowrt/` 仍是可删除、可重建的生成物，不入库；如果清理过这些目录，需要先重新执行对应示例的 `prepare` 或 `build`，再重启 clangd。

FlowRT demo smoke：

```bash
scripts/package-deb.sh --output-dir dist
sudo dpkg -i dist/flowrt_*_*.deb
flowrt --version

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
flowrt build --launcher examples/mixed_zenoh_demo/rsdl/robot.rsdl
FLOWRT_TICK_SLEEP_MS=5 flowrt launch --run-ticks 200 examples/mixed_zenoh_demo/rsdl/robot.rsdl
```

Debian 包和安装后用户项目 smoke：

```bash
scripts/test-package-deb.sh
scripts/test-deb-installed-user-project.sh
```

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
- `flowrt run` 和 `flowrt launch` 只读取已生成产物，不执行 prepare/build，不写 `flowrt/` 输出目录。
- 所有会写 `flowrt/` 输出目录的 CLI 命令都必须在命令级持有 OS advisory lock；`.flowrt.lock` 文件可以残留，PID 只作为诊断内容，真实占用状态必须由锁判断。`check`、`inspect`、`run`、`launch`、`list`、`nodes`、`status`、`echo` 和 `params` 不写生成物，不应获取该锁。
- 生成的 Rust/C++ runtime shell 必须把 SIGINT/SIGTERM 转成 runtime `ShutdownToken`，让长期运行的 tick loop 优雅退出，并继续执行 `shutdown` task、`on_stop` 和 `on_shutdown`。CLI 的 `--run-ticks` 只是显式运行上限，不是核心 runtime 行为来源。
- Runtime 与 codegen 不能吞掉 bind-level channel 语义：`latest` 和 `fifo` 都要保留 `overflow`、`max_age_ms` 与 `stale_policy`，inproc shell 也应使用 timestamped read/write 路径传递 freshness。
- 跨 process group 的 bind 会在该 route 的 Contract IR capability 派生中要求 `topology:multi_process`；跨 target route 还会要求 `topology:multi_host`。validator、normalizer 和 CLI 必须共享同一套 route topology 判定，不要再各自手写 process-boundary 特判。
- Task-level execution intent 也必须映射到 runtime 行为：`deadline_ms` 要进入 required capabilities，并由生成 shell 在用户回调和输出发布边界执行检查。
- 单 instance 多 task 必须由 RSDL parser、Contract IR、validator、launch manifest、self-description 和 Rust/C++ runtime shell 同步支持。旧 `[instance.<name>.task]` 单 task 写法归一化为 `main`，新 `[[instance.<name>.task]]` 必须显式声明唯一 task name。生成 shell 当前复用同一个用户组件接口，每个 task 只读取/发布自己的端口子集，并为每个 task 建立独立局部 scope；参数 pending apply 仍按 instance 每 scheduler tick 执行一次。
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

FlowRT 的 release notes 来自 `CHANGELOG.md`。推送 `v*` tag 后，CI 会等待 `guard-generated`、Rust fmt/test/clippy、C++ runtime、C++ zenoh runtime、deb package 和 demo smoke 全部通过，再创建 GitHub Release，并上传 `.deb` 与 `SHA256SUMS`。

发布前检查：

```bash
tag=v0.1.0
scripts/extract-release-notes.sh "$tag" CHANGELOG.md
```

要求：

- `CHANGELOG.md` 必须包含对应二级标题，格式为 `## vX.Y.Z - YYYY-MM-DD`。
- tag 名必须是 `vX.Y.Z`，且版本号必须与根 `Cargo.toml` 的 workspace version 一致。
- 对应版本段不能为空；CI 会把该段原样作为 GitHub Release 说明。
- `## 未发布` 只放尚未发布的后续变化；正式发版前把本次条目移入版本段。

创建并推送 tag：

```bash
git tag -a v0.1.0 -m "v0.1.0"
git push origin v0.1.0
```

## 提交规则

提交标题使用 Conventional Commits，标题正文使用中文：

```text
<type>(<scope>): <中文标题>
```

示例：

```text
docs(readme): 重写项目入口文档
fix(validate): 拒绝缺失任务输入绑定
test(abi): 补充 C++ 与 Rust 消息布局测试
```

提交应保持原子：

- 一个提交只包含同一主题的代码、测试、文档和 changelog 更新。
- 提交前运行与改动匹配的最小验证。
- 提交前运行本地规格与生成物入库防回归检查，确认 `docs/` 下本地规格草案、`flowrt/` 和 `examples/*/flowrt/` 没有被 git tracked。
- 不把生成物、大型构建输出或未验证半成品混入提交。
- `docs/` 下被 `.gitignore` 排除的本地设计/规格文件不得加入索引。

## backend 依赖与离线包

Rust runtime 的 iox2 支持通过 feature-gated `iceoryx2 = "0.9"` 编译。C++ iox2 binding 只有在定义 `FLOWRT_HAS_ICEORYX2_CXX` 并链接 `iceoryx2-cxx 0.9.1` 时使用真实 transport。Rust runtime 的 zenoh 支持通过 feature-gated `zenoh = "1.9"` 编译。C++ zenoh binding 只有在定义 `FLOWRT_HAS_ZENOH_CXX` 并链接基于 `zenoh-c` backend 的 `zenohcxx::zenohc` 时使用真实 transport。

FlowRT Debian 包会把锁定版本的 Rust crate vendor、`iceoryx2-cxx 0.9.1`、`zenoh-c 1.9.0`、`zenoh-cpp 1.9.0` 和第三方 license material 放入 `/opt/flowrt/<version>` 私有前缀。安装后的 `flowrt build` 会自动把该前缀传给 generated CMake，并为 generated Rust app 写入离线 Cargo config。生成项目构建不应通过 `FetchContent`、Cargo registry 或其他外部网络路径临时解析 backend SDK。

基础 Rust/C++ inproc 验证和 `check` smoke 不要求启用 C++ iox2/zenoh 测试。直接调试 `runtime/cpp` 的 backend smoke 时，需要用 `CMAKE_PREFIX_PATH=/opt/flowrt/<version>` 或等价路径暴露 FlowRT 私有前缀。

C++ iox2 runtime smoke：

```bash
CMAKE_PREFIX_PATH=/opt/flowrt/0.1.0 \
  cmake -S runtime/cpp -B build/cpp-iox2 -G Ninja -DFLOWRT_CPP_ENABLE_IOX2_TESTS=ON
cmake --build build/cpp-iox2 --target flowrt_runtime_iox2_smoke
ctest --test-dir build/cpp-iox2 -R flowrt_runtime_iox2_smoke --output-on-failure
```

C++ zenoh runtime smoke：

```bash
CMAKE_PREFIX_PATH=/opt/flowrt/0.1.0 \
  cmake -S runtime/cpp -B build/cpp-zenoh -G Ninja -DFLOWRT_CPP_ENABLE_ZENOH_TESTS=ON
cmake --build build/cpp-zenoh --target flowrt_runtime_zenoh_smoke
ctest --test-dir build/cpp-zenoh -R flowrt_runtime_zenoh_smoke --output-on-failure
```
