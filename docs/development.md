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
```

仓库根目录的 `.clangd` 会让 `runtime/cpp/**` 使用 `build/cpp/compile_commands.json`，并让 `examples/*/src/cpp/**` 读取本示例自己的 `flowrt/cpp/include` 生成头。`flowrt/` 和 `examples/*/flowrt/` 仍是可删除、可重建的生成物，不入库；如果清理过这些目录，需要先重新执行对应示例的 `prepare` 或 `build`，再重启 clangd。

FlowRT demo smoke：

```bash
export FLOWRT_RUN_TICKS=5
cargo run -p flowrt-cli -- build examples/cpp_counter_demo/rsdl/robot.rsdl
cargo run -p flowrt-cli -- run examples/cpp_counter_demo/rsdl/robot.rsdl --process control
cargo run -p flowrt-cli -- launch examples/cpp_counter_demo/rsdl/robot.rsdl
cargo run -p flowrt-cli -- build examples/imu_demo/rsdl/robot.rsdl
cargo run -p flowrt-cli -- run examples/import_demo/rsdl/robot.rsdl --process main
cargo run -p flowrt-cli -- launch examples/import_demo/rsdl/robot.rsdl
cargo run -p flowrt-cli -- check examples/mixed_iox2_demo/rsdl/robot.rsdl
cargo run -p flowrt-cli -- check examples/imu_demo_iox2/rsdl/robot.rsdl
cargo run -p flowrt-cli -- check examples/profile_switch_demo/rsdl/robot.rsdl
cargo run -p flowrt-cli -- run --profile iox2 examples/profile_switch_demo/rsdl/robot.rsdl
```

Mixed contract 的跨语言 Message ABI roundtrip 可用生成工程直接验证。先构建含 C++ 和 Rust 消息生成物的示例，CMake 会在构建 `message_abi` target 后写出 C++ sample bytes fixture；再运行生成 Rust crate 的 `message_abi` 测试读取并重建这些 fixture：

```bash
cargo run -p flowrt-cli -- build examples/imu_demo/rsdl/robot.rsdl
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

仓库开发可以使用 `cargo run -p flowrt-cli -- ...`，但面向用户的 README、文档、示例和最终说明应使用安装后的 `flowrt ...`。

## 代码与生成物边界

- `flowrt/` 和 `examples/*/flowrt/` 是生成物目录，不入库。
- 生成目录可以删除并重新生成。
- 用户算法代码应放在示例或项目自己的 `src/` 目录，不写进生成文件。
- FlowRT 管理代码只做 glue：消息、接口、runtime shell、backend 绑定、启动配置和构建文件。
- Codegen 入口必须只消费通过 validator 的 Contract IR；crate public API 也要重新校验传入 IR，避免调用方绕过 CLI 后生成半成品或触发 panic。
- 静态自描述产物必须来自已验证、已投影的 Contract IR。`flowrt/selfdesc/selfdesc.json` 只作为可读 sidecar 和测试辅助；部署后的事实源是生成应用二进制中的 `.flowrt.selfdesc` section。自描述 JSON 要包含静态拓扑、process、channel、profile/target/deployment 和 Message ABI layout，供后续 CLI 在没有 RSDL 源文件时自查。
- runtime introspection socket 路径只用于发现候选进程，真实身份必须来自 handshake。默认路径优先使用 `$XDG_RUNTIME_DIR/flowrt/<pid>.sock`，fallback 要按当前用户隔离；CLI status 连接后再验证 PID、process、runtime 和 self-description hash。Rust runtime 的 live state 是 `IntrospectionState`，status 响应从该 state 取 tick/channel 摘要；生成的 Rust runtime shell 要为当前 process 的 active channel 注册 canonical channel 名和 message type，并在成功发布输出后用同一 tick timestamp 记录 raw ABI payload。channel snapshot 请求返回 raw ABI bytes，CLI 展示必须结合静态 self-description 的 Message ABI layout，不要在 runtime 层重复定义业务 payload schema。
- C++ runtime introspection API 要保持与 Rust JSON-line wire 格式兼容：`status` 返回 handshake、tick 和 channel 摘要，`channel_snapshot` 只返回 raw ABI bytes、发布计数和发布时间，未知 channel 返回结构化 error。generated Rust/C++ shell 都应启动 PID 命名 socket、注册当前 process active channel，并在成功发布输出后记录 live state。
- C++ only contract 的 `flowrt build` / `flowrt run` 走 CMake app 路径，不依赖 Cargo app。
- C++ only contract 的 `flowrt launch` 会生成 supervisor-only Rust crate；该 crate 只负责编排 C++ app，不生成 Rust runtime shell 或 Rust app binary。
- 所有会写 `flowrt/` 输出目录的 CLI 命令都必须在命令级持有输出目录锁；`check` 和 `inspect` 不写生成物，不应获取该锁。
- Runtime 与 codegen 不能吞掉 bind-level channel 语义：`latest` 和 `fifo` 都要保留 `overflow`、`max_age_ms` 与 `stale_policy`，inproc shell 也应使用 timestamped read/write 路径传递 freshness。
- 跨 process group 的 bind 会在 Contract IR capability 派生中要求 `topology:multi_process`；validator、normalizer 和 CLI 必须共享同一套 deployment 判定，不要再各自手写 process-boundary 特判。
- Task-level execution intent 也必须映射到 runtime 行为：`deadline_ms` 要进入 required capabilities，并由生成 shell 在用户回调和输出发布边界执行检查。
- Message ABI v0.1 必须保持 fixed-size plain data。未来 `bytes<max=N>`、`string<max=N>` 和 `sequence<T,max=N>` 可以进入 Contract IR 表达层，但 validator、conformance helper 和 codegen public 入口必须明确拒绝，直到 Variable Frame ABI runtime 语义落地。
- Mixed contract 的 Message ABI conformance 不能只依赖同一生成器内嵌的 expected bytes；C++ test 写出的 fixture 和 Rust test 读取后的 typed roundtrip 都应保持可运行。
- 扩展 backend capability 时，先在 `flowrt-ir` 的 typed capability catalog 中维护全局 canonical 顺序，再由 `backend_capabilities`、`channel_capabilities`、`trigger_capability` 或 message ABI 推导函数输出既有 `CapabilityAtom` 字符串。凡是 backend、target、deployment、channel 的 capability 组合，都要先去重再按该 catalog 顺序输出，不能依赖声明顺序或首次出现顺序；新增或重排 catalog 都会改变 canonical IR 顺序，因此必须同步补顺序独立测试。不要在 validator、normalizer 或 codegen 中散落新 capability 字符串。
- Rust/C++ runtime 的 backend capability 报告顺序也必须跟随同一个 catalog；runtime smoke test 应精确断言顺序，避免自描述、诊断和跨语言对比输出出现漂移。
- deployment satisfaction 只能通过 `flowrt-ir` 的集中 typed decision 推导；normalizer 和 validator 必须复用同一 decision 入口，不能各自复制 unknown backend、target 未声明支持、missing required capabilities 或 satisfied 的判断逻辑，也不能把 `TargetIr.capabilities` 或 `DeploymentIr.satisfied` 当作事实源。

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

## 可选 iox2 依赖

Rust runtime 的 iox2 支持通过 feature-gated `iceoryx2 = "0.9"` 编译。C++ iox2 binding 只有在定义 `FLOWRT_HAS_ICEORYX2_CXX` 并链接 `iceoryx2-cxx 0.9.1` 时使用真实 transport。

没有安装 `iceoryx2-cxx` 时，基础 Rust/C++ inproc 验证和 `check` smoke 仍应可运行；含 C++ iox2 组件的构建应由 CMake 依赖解析明确失败，而不是静默退回 inproc。
