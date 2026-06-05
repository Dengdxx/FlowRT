# 示例矩阵

本仓库的 `examples/` 目录用于验证 RSDL、Contract IR、validator、codegen、runtime 和 CLI 的端到端切片。每个示例都尽量覆盖一个明确边界，不把所有能力塞进同一个 demo。

## 示例列表

| 示例 | Runtime | Backend | 推荐命令 | 用途 |
| --- | --- | --- | --- | --- |
| `examples/import_demo` | Rust | `inproc` | `flowrt build --launcher examples/import_demo/rsdl/robot.rsdl` | 验证 `[package.imports]`、Rust codegen、inproc run 和 launch manifest |
| `examples/cpp_counter_demo` | C++ | `inproc` | `flowrt build --launcher examples/cpp_counter_demo/rsdl/robot.rsdl` | 验证 C++ only CMake app 路径、用户工厂、C++ runtime shell 和 supervisor 启动 |
| `examples/imu_demo` | Rust + C++ | `inproc` 声明用于 build smoke | `flowrt build examples/imu_demo/rsdl/robot.rsdl` | 验证 mixed contract 的接口、消息和生成物边界；不伪装为 mixed inproc 可运行 |
| `examples/profile_switch_demo` | Rust | `inproc` / `iox2` | `flowrt build --profile iox2 examples/profile_switch_demo/rsdl/robot.rsdl` | 验证同一份 RSDL 通过 profile 切换 backend |
| `examples/mixed_iox2_demo` | Rust + C++ | `iox2` | `flowrt check examples/mixed_iox2_demo/rsdl/robot.rsdl` | 验证 Rust source 与 C++ sink 通过 iox2 分进程连接的 contract |
| `examples/imu_demo_iox2` | Rust + C++ | `iox2` | `flowrt check examples/imu_demo_iox2/rsdl/robot.rsdl` | 验证主 demo 的语言分离 iox2 运行变体 |
| `examples/variable_iox2_demo` | Rust + C++ | `iox2` | `flowrt build --launcher examples/variable_iox2_demo/rsdl/robot.rsdl` | 验证 bounded variable frame 经 iox2 fixed slot 跨语言传递 |
| `examples/mixed_zenoh_demo` | Rust + C++ | `zenoh` | `flowrt build --launcher examples/mixed_zenoh_demo/rsdl/robot.rsdl` | 验证 bounded variable frame、zenoh 跨主机 transport 和 mixed launch 路径 |

## `import_demo`

入口文件：

```text
examples/import_demo/rsdl/robot.rsdl
```

该文件只声明 package 和 imports：

```toml
[package.imports]
types = ["types/*.rsdl"]
components = ["components/*.rsdl"]
graphs = ["graphs/*.rsdl"]
profiles = ["profiles/*.rsdl"]
targets = ["targets/*.rsdl"]
```

它用于证明 v0.1 可以把 `types`、`components`、`graphs`、`profiles` 和 `targets` 拆成多个 RSDL 片段，同时仍归一化到同一份 Contract IR。

常用命令：

```bash
flowrt check examples/import_demo/rsdl/robot.rsdl
flowrt build --launcher examples/import_demo/rsdl/robot.rsdl
flowrt run examples/import_demo/rsdl/robot.rsdl --process main
flowrt launch examples/import_demo/rsdl/robot.rsdl
```

## `cpp_counter_demo`

入口文件：

```text
examples/cpp_counter_demo/rsdl/robot.rsdl
```

该示例是 C++ only inproc contract：

```text
counter_source.count -> counter_sink.count
```

它验证：

- C++ message codegen。
- C++ interface codegen。
- C++ inproc runtime shell。
- `flowrt_user::build_app()` 用户工厂入口。
- C++ only 普通 `flowrt build` / `flowrt run` 走 CMake app 路径。
- C++ only `flowrt build --launcher` 会显式构建 generated supervisor；`flowrt launch` 只执行已有 supervisor。

常用命令：

```bash
flowrt build --launcher examples/cpp_counter_demo/rsdl/robot.rsdl
flowrt run examples/cpp_counter_demo/rsdl/robot.rsdl --process control
flowrt launch examples/cpp_counter_demo/rsdl/robot.rsdl
```

## `imu_demo`

入口文件：

```text
examples/imu_demo/rsdl/robot.rsdl
```

该示例表达主线数据流：

```text
imu_sim -> estimator -> controller -> monitor
```

它用于验证 mixed contract 的生成能力，包括 C++/Rust message、接口和构建产物。当前规则要求 mixed contract 保持语言边界诚实：Rust codegen 不为 C++ component 伪造 Rust trait，C++ codegen 不为 Rust component 伪造 C++ interface。

基础 smoke：

```bash
flowrt build examples/imu_demo/rsdl/robot.rsdl
```

## `profile_switch_demo`

入口文件：

```text
examples/profile_switch_demo/rsdl/robot.rsdl
```

该示例用于验证 `--profile`：

```bash
flowrt check examples/profile_switch_demo/rsdl/robot.rsdl
flowrt build --profile iox2 examples/profile_switch_demo/rsdl/robot.rsdl
flowrt run --profile iox2 examples/profile_switch_demo/rsdl/robot.rsdl
```

`build --profile <name>` 会先投影 Contract IR，再做 validation 和 codegen。`run --profile <name>` 只校验已生成产物的 profile 是否匹配，不会临时重生成。默认 profile 仍是 `default` 或首个 profile。未在 `bind.dataflow` 上显式声明的 channel policy 会随选中 profile 的默认值一起投影；显式 bind policy 不会被 profile 覆盖。

## iox2 mixed 示例

入口文件：

```text
examples/mixed_iox2_demo/rsdl/robot.rsdl
examples/imu_demo_iox2/rsdl/robot.rsdl
examples/variable_iox2_demo/rsdl/robot.rsdl
```

这些示例验证 language-separated mixed contract over `iox2`：

- process group 必须按语言拆分。
- selected backend 必须是 `iox2`。
- launch manifest 中的 channel 必须暴露 canonical service name。
- Rust 和 C++ shell 消费同一份 Contract IR-derived transport 契约。
- bounded variable frame 会通过 codegen 生成的 fixed-size iox2 slot 承载，用户组件接口仍使用结构化消息。

`mixed_iox2_demo` 和 `imu_demo_iox2` 的基础 smoke 仍以 `check` 为主。`variable_iox2_demo` 会在 CI 中构建并有限 tick 运行，用 marker 文件证明 C++ sink 实际收到 Rust source 发出的 bounded variable frame：

```bash
flowrt build --launcher examples/variable_iox2_demo/rsdl/robot.rsdl
rm -f /tmp/flowrt-variable-iox2-saw-packet
FLOWRT_TICK_SLEEP_MS=5 FLOWRT_VARIABLE_IOX2_SAW_PACKET_PATH=/tmp/flowrt-variable-iox2-saw-packet \
  flowrt launch --run-ticks 200 examples/variable_iox2_demo/rsdl/robot.rsdl
test -s /tmp/flowrt-variable-iox2-saw-packet
```

含 C++ iox2 组件的生成 CMake 会先查找本机 `iceoryx2-cxx 0.9.1`。未安装时默认通过 `FetchContent` 拉取 `iceoryx2` v0.9.1，并调用 Cargo 构建其 Rust FFI 部分；网络不可用时，应预先安装并通过 `CMAKE_PREFIX_PATH` 暴露依赖。

## zenoh mixed 示例

入口文件：

```text
examples/mixed_zenoh_demo/rsdl/robot.rsdl
```

该示例验证 language-separated mixed contract over `zenoh`，同时覆盖 bounded variable frame：

- `bytes<max=N>`
- `string<max=N>`
- `sequence<T,max=N>`

推荐命令：

```bash
flowrt build --launcher examples/mixed_zenoh_demo/rsdl/robot.rsdl
FLOWRT_TICK_SLEEP_MS=5 flowrt launch --run-ticks 200 examples/mixed_zenoh_demo/rsdl/robot.rsdl
```

构建前提是本机已安装 `zenohc 1.9.0` 和 `zenohcxx 1.9.0`，并通过 `CMAKE_PREFIX_PATH` 暴露给生成的 CMake 工程。

如果要跨机器运行，需要让两个进程分别拿到对应的 zenoh session 配置，例如通过 `FLOWRT_ZENOH_CONNECT` 和 `FLOWRT_ZENOH_LISTEN` 注入端点；如果要在本机观察足够多的样本，`FLOWRT_TICK_SLEEP_MS` 可以把同步 tick 拉长。

## 添加新示例

新增示例时应明确它验证的边界：

- RSDL 语法或 import 行为。
- validator 规则。
- Rust/C++ codegen 边界。
- runtime channel 或 lifecycle 行为。
- backend capability 或 launch 行为。

不要新增只展示目录结构、但没有可验证命令的空示例。示例如果引入新语义、命令或生成物边界，应同步更新 README、本文档和 `CHANGELOG.md`。
