# CLI 参考

`flowrt` 是 FlowRT 面向用户的命令入口。仓库内可以用 `cargo run -p flowrt-cli -- ...` 调试，但对外文档和示例默认使用安装后的 `flowrt ...`。

## 命令概览

```bash
flowrt check <path/to/robot.rsdl>
flowrt prepare <path/to/robot.rsdl> [--out-dir flowrt] [--profile <name>]
flowrt deps [path/to/robot.rsdl] [--backend <inproc|iox2|zenoh|all>] [--profile <name>] [--target <linux-amd64|linux-arm64>] [--build-mode <release|debug>] [--check]
flowrt cache status
flowrt cache clean [--target <linux-amd64|linux-arm64>] [--build-mode <release|debug>] [--dry-run] [--flowrt-deps] [--project-build] [--incremental] [--stale-temp]
flowrt doctor [path/to/robot.rsdl] [--target <linux-amd64|linux-arm64>]
flowrt toolchain show --target <linux-amd64|linux-arm64>
flowrt toolchain init --target <linux-arm64> [--sdk-overlay <path>] [--force]
flowrt external check <path/to/external-package>
flowrt external list --path <path/to/search-root>
flowrt build <path/to/robot.rsdl> [--out-dir flowrt] [--profile <name>] [--target <linux-amd64|linux-arm64>] [--launcher] [--build-mode <release|debug>]
flowrt run <path/to/robot.rsdl> [--out-dir flowrt] [--profile <name>] [--process <name>] [--run-steps <N>] [--build-mode <release|debug>]
flowrt launch <path/to/robot.rsdl> [--out-dir flowrt] [--profile <name>] [--run-steps <N>] [--build-mode <release|debug>]
flowrt bundle <path/to/robot.rsdl> [--out-dir flowrt] --output <path/to/bundle-dir> [--profile <name>] [--build-mode <release|debug>]
flowrt deploy <path/to/bundle-dir> --host <user@host> --target <target-name> --remote-dir <dir> [--dry-run]
flowrt inspect <path/to/flowrt/contract/contract.ir.json>
flowrt list <path/to/generated-app-or-selfdesc.json>
flowrt nodes <path/to/generated-app-or-selfdesc.json>
flowrt echo <channel> [--socket <path>] [--image <path/to/generated-app-or-selfdesc.json>] [--follow] [--interval-ms <ms>]
flowrt echo <path/to/generated-app-or-selfdesc.json> <channel> [--socket <path>] [--follow] [--interval-ms <ms>]
flowrt params list --image <path> [--socket <path>] [--remote] [--runtime <key_expr>] [--timeout-ms <ms>]
flowrt params get <instance.param> --image <path> [--socket <path>] [--remote] [--runtime <key_expr>] [--timeout-ms <ms>]
flowrt params set <instance.param> <json-value> --image <path> [--socket <path>] [--remote] [--runtime <key_expr>] [--timeout-ms <ms>]
flowrt op list [--image <path>] [--socket <path>]
flowrt op status [operation] [--socket <path>]
flowrt op cancel <operation_id> [--socket <path>]
flowrt status [--live-only]
flowrt hz [channel] [--socket <path>] [--window-ms <ms>]
flowrt record --output <path/to/run.mcap> [--socket <path>] [--duration <10s|500ms|2m>] [--channel <name>] [--operation <name>] [--all] [--force]
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

## `deps`

```bash
flowrt deps examples/import_demo/rsdl/robot.rsdl
flowrt deps --backend all
flowrt deps examples/profile_switch_demo/rsdl/robot.rsdl --profile iox2
flowrt deps examples/external_driver_demo/rsdl/robot.rsdl --target linux-arm64
flowrt deps --backend zenoh --build-mode release --check
```

`deps` 负责补全并预热 FlowRT 底层依赖缓存。它只编译 FlowRT runtime 依赖，不生成用户项目产物。`flowrt build` 会复用该 cache；如果缺少匹配的 ready marker，会直接失败并提示先运行 `flowrt deps`。

缓存根目录按以下顺序选择：

- `FLOWRT_CACHE_DIR`
- `XDG_CACHE_HOME/flowrt`
- `~/.cache/flowrt`

cache key 包含 FlowRT 版本、Rust toolchain identity、target triple、vendor hash、build mode 和 backend feature 组合。`--target <platform>` 会按 toolchain profile 解析 Rust target triple，并把 Cargo prewarm 输出隔离到对应 cache key 和 ready marker 下；省略 `--target` 且 RSDL 已声明 target platform 时使用 Contract IR 的 platform，仍无 platform 时保持 native 构建。`--backend all` 预热的 cache 可以被 `inproc`、`iox2` 或 `zenoh` 子集复用；安装后推荐在项目内运行 `flowrt deps <rsdl>`，CI 或离线镜像准备阶段可以用 `flowrt deps --backend all` 一次性补全。

当前支持的 platform 为 `linux-amd64` 和 `linux-arm64`。当 Cargo 需要交叉 target 时，CLI 会传递 `--target <rust-target-triple>`；如果 rustup 未安装该 target 或 Cargo 构建失败，错误会指出 Rust target triple，并提示先执行 `rustup target add <triple>` 或配置对应 Rust toolchain。FlowRT 不会自动下载系统交叉编译器或板级私有 SDK。

`--build-mode` 默认是 `release`。`debug` 只用于本地调试，不能和 release 产物混用。

## `cache`

```bash
flowrt cache status
flowrt cache clean --dry-run --flowrt-deps --target linux-arm64 --build-mode release
flowrt cache clean --project-build
flowrt cache clean --incremental --stale-temp
```

`cache status` 用于解释磁盘占用，`cache clean` 用于显式、安全地删除可重建目录。CLI 会把输出分成四类：

- `默认可清`：FlowRT deps cache、`deps-workspaces/`、ready marker、显式选择的
  incremental cache、已确认 stale 的临时 socket/zenoh 目录、项目 `flowrt/build/cmake`。
- `条件可清`：当前项目 `flowrt/build`、`flowrt/build/bin/...`、当前 git worktree。
- `仅展示`：FlowRT 仓库开发 `target/`、用户 `.mcap` 录制产物和日志目录。
- `永不自动清`：`.flowrt/toolchains.toml`、live runtime socket、`sdk_overlays`
  指向的外部目录。

`cache status` 会展示：

- FlowRT cache root；
- `flowrt deps` 的 ready marker，按 target/build mode/backend feature 列出；
- `cargo-target/`、`deps-workspaces/` 和 incremental 占用；
- 当前项目 `flowrt/build/`、`flowrt/build/bin/...`、`flowrt/build/cmake/...`；
- 当前目录位于 FlowRT 仓库时的开发 `target/`；
- stale `/tmp` 候选和 live runtime socket 区分结果；
- SDK overlay 占用提示。

`cache clean` 只会清理显式选中的范围：

- `--flowrt-deps`：清理 FlowRT cache root 下的 deps target、deps workspace、ready marker
  和 lock 文件。带 `--target` / `--build-mode` 时只匹配对应 cache key。
- `--project-build`：清理当前项目 `flowrt/build` 可重建目录。没有过滤条件时删除整个
  `flowrt/build`；带 `--target` / `--build-mode` 时只删除匹配的 `bin/` 和 `cmake/`
  子目录。
- `--incremental`：只删 Cargo incremental 目录，保留其余 fingerprint 和 link input。
- `--stale-temp`：只删已确认 stale 的 FlowRT runtime socket 或带 dead PID 的 zenoh
  临时目录；不能证明 stale 的项只展示，不自动删。
- `--dry-run`：先输出计划删除路径，不执行实际删除。实际删除时 CLI 会逐条打印删除路径。

安全边界：

- CLI 只会删除 FlowRT cache root、当前项目 `flowrt/build`，以及经 PID 校验确认 stale
  的临时候选；遇到 symlink、空路径或父路径逃逸会直接拒绝。
- SDK overlay 只展示占用，默认不删除；未来若支持清理，必须要求显式路径和显式确认。
- 安装前缀 `/opt/flowrt/<version>`、已安装 CLI 路径（例如 `/usr/bin/flowrt`）、
  用户源码、`.flowrt/toolchains.toml`、live runtime socket、`.mcap` 和日志都不属于
  `cache clean` 自动删除范围。
- `iox2` / `zenoh` 共享内存或运行期资源不靠路径模式批量清理；无法证明资源已经脱离
  live 进程时，CLI 只会展示。

关于占用和增量缓存：

- 用户最终二进制通常只有 `1MB - 15MB`；GB 级空间大头通常是 `.rlib`、`.rmeta`、
  build-script、proc-macro、多 target、多 feature 和 vendor hash 组合下的中间产物。
- `flowrt deps` 的共享语义是复用编译产物作为 link input，不是让不同用户程序动态共享
  同一个最终 FRT 依赖二进制；Rust 用户程序仍会把实际用到的 runtime/dependency code
  链接进自己的最终 executable。
- CI 若不需要本地增量构建收益，可以设置 `CARGO_INCREMENTAL=0` 减少无价值缓存；本地
  开发默认保留 incremental 以换取重复编译速度。
- 多 worktree 的本地开发可以显式共享 `CARGO_TARGET_DIR` 来复用仓库开发 `target/`
  产物，但 FlowRT 不会强制写入仓库配置。

## `build`

```bash
flowrt build examples/cpp_counter_demo/rsdl/robot.rsdl
flowrt build --launcher examples/external_driver_demo/rsdl/robot.rsdl --target linux-arm64
flowrt build examples/cpp_counter_demo/rsdl/robot.rsdl --target linux-arm64
```

`build` 先执行 `prepare`，再构建生成应用。默认 build mode 是 `release`。

规则：

- `build` 只编译用户项目和生成 shell，不负责隐式预热 FlowRT 底层依赖。首次构建、切换 backend、切换 FlowRT 版本或清理 cache 后，应先运行匹配的 `flowrt deps`。
- Rust/Cargo 构建会复用 `flowrt deps` 准备好的共享 target cache。该 cache 中的依赖、
  用户代码增量产物和 Cargo fingerprint 对后续构建有复用价值，默认不会被自动清理。
- 最终运行二进制会复制到项目自己的 `flowrt/build/bin/...`；用户项目工作区不维护一份
  独立的大型 Rust target 目录。
- Rust-only 或含 Rust component 的 contract 当前会触发 Cargo app 构建。
- C++ only contract 走 CMake app 路径，不依赖 Cargo app。
- `--launcher` 会额外构建 `flowrt launch` 需要的 generated supervisor；省略时只构建可由 `flowrt run` 直接执行的 app。
- `--target <platform>` 选择交叉编译 toolchain platform。Rust/Cargo 路径会使用对应
  Rust target triple；C++/CMake 路径会消费对应 `ToolchainProfile` 和 target SDK。
- `build` 成功后会输出一段简短 summary。显式 `--target` 或由 Contract IR 选中的 target
  构建会显示 target platform、build mode、Rust target triple、C/C++ compiler、
  runtime dependency policy、SDK overlay、选中的 `pkg_config` 模块和最终二进制路径；
  native 路径只保留必要字段，避免普通本机构建输出过于嘈杂。
- 含 C++ component 时，生成的 CMake 工程会构建 managed shell、app target 和 ABI conformance test target。
- 选择 `iox2` 且 contract 含 C++ component 时，CMake 会查找 `iceoryx2-cxx 0.9.1` 的 `iceoryx2-cxx::static-lib-cxx` 目标。
- 选择 `zenoh` 且 contract 含 C++ component 时，CMake 会查找 `zenohcxx 1.9.0` 的 `zenohcxx::zenohc` 目标。
- 声明 `[[bridge.ros2]]` 时，`build` 会额外构建 FlowRT 管理的 C++ ROS2 adapter target；即使没有 C++ 用户 component，也会运行生成 CMake。

构建出的用户项目二进制包括 Rust app、generated supervisor、C++ app 和 ROS2 bridge
adapter。native 构建或未使用 Cargo cross target triple 时继续写入兼容路径
`flowrt/build/bin/<release|debug>/`；实际 cross target 构建会写入
`flowrt/build/bin/<platform>/<release|debug>/`，例如
`flowrt/build/bin/linux-arm64/release/`，避免不同 target 的同名二进制互相覆盖。
`flowrt/build/build-info.json` 记录本次构建的 FlowRT 版本、profile、build mode、
target 名称、platform、target identity、Rust target triple、host triple、依赖 target
目录和相对 executable 路径。

`--target <platform>` 显式选择构建目标 platform，优先级高于 Contract IR target
platform；省略时，`build` 使用选定 Contract IR target 的 platform，仍无 platform
时保持 native 旧行为。Rust app、generated supervisor 和 deps cache 会使用同一个
Rust target triple，并按 Cargo 的 cross target 输出路径定位二进制。`run`、`launch`
和 `bundle` 不硬编码旧 bin 目录，而是读取 `build-info.json` 中的 executable
相对路径。

Debian 包会把 FlowRT 锁定版本的 Rust crate vendor、`iceoryx2-cxx`、`zenoh-c` 和 `zenoh-cpp` 安装到 `/opt/flowrt/<version>`。安装后的 `flowrt deps` / `flowrt build` 会使用该私有前缀和包内 vendor；生成项目构建不需要联网拉取 backend 依赖。源码树内直接调试生成 CMake 时，可以用 `FLOWRT_CPP_RUNTIME_DIR` 或 `CMAKE_PREFIX_PATH` 指向同一私有前缀。

`/opt/flowrt/<version>` 保留兼容的 `include/`、`lib/` 和 `lib/<multiarch>/` 查找路径，
同时提供 `targets/<platform>/` SDK 布局。amd64 安装包内嵌两个完整 SDK：
`targets/linux-amd64` 是本机 mirror，`targets/linux-arm64` 是交叉 target SDK。两者都在
`flowrt-target-sdk.toml` 中记录 `platform`、`multiarch`、`components` 和
`complete = true`。arm64 安装包当前只保证 `targets/linux-arm64` 完整；不承诺反向
`linux-arm64 -> linux-amd64` 交叉编译，`targets/linux-amd64` 可以只是 `complete = false`
的 marker。

执行 `flowrt build --target <platform>` 且构建 C++/CMake 产物时，CLI 会先解析
toolchain profile，再从安装私有前缀或 `FLOWRT_CPP_RUNTIME_DIR` 指向的位置查找
`targets/<platform>/flowrt-target-sdk.toml`。只有 `complete = true` 的 target SDK
会进入 CMake；manifest 缺失或 `complete = false` 会 fail-fast，并提示安装内嵌该
target SDK 的 FlowRT 包或显式配置完整 SDK。CMake 的 `CMAKE_PREFIX_PATH` 会优先包含
target SDK root 及其 `cmake/` 目录，避免只使用 host 私有前缀。profile 声明
`cmake_toolchain` 时会传 `-DCMAKE_TOOLCHAIN_FILE=...`；否则会传
`-DCMAKE_C_COMPILER=...`、`-DCMAKE_CXX_COMPILER=...`，有 `sysroot` 时同时传
`-DCMAKE_SYSROOT=...`。profile 或 target SDK 提供 `pkgconfig/` 时，configure 环境会
设置 `PKG_CONFIG_LIBDIR`，避免 cross build 误用 host pkg-config 搜索路径。

在真正进入 `cmake configure` 之前，CLI 会对 selected target 下 C++ component 的
`build.pkg_config` 依赖做一次 fail-fast 预检。若 target SDK 缺失、`complete = false`
或 `pkg-config` 模块不可见，错误文案会直接带出当前 target、当前
`PKG_CONFIG_LIBDIR`、缺失模块或 target SDK 搜索路径，并建议先执行
`flowrt doctor <rsdl> --target <platform>`；底层 CMake/Cargo 原始输出仍保持直出，
CLI 只在前后补充上下文，不会隐式下载或修复任何外部 SDK。

toolchain profile 由系统、用户、workspace 和 CLI override 按优先级合并。默认路径为
`/etc/flowrt/toolchains.toml`、`~/.config/flowrt/toolchains.toml` 和项目
`.flowrt/toolchains.toml`。板级私有 SDK 不写进 RSDL，可通过 profile 接入：

```toml
[toolchain.linux-arm64]
sysroot = "/opt/vendor/sysroots/linux-arm64"
sdk_overlays = ["/opt/vendor/rknn"]
cmake_prefix_paths = ["/opt/vendor/rknn"]
pkg_config_libdirs = ["/opt/vendor/rknn/lib/aarch64-linux-gnu/pkgconfig"]
runtime_dependency_policy = "external"
```

`sdk_overlays` 会作为额外 CMake prefix，并派生常见 pkg-config 目录
`pkgconfig`、`lib/pkgconfig` 和 `lib/<multiarch>/pkgconfig`。`runtime_dependency_policy`
当前接受 `bundle`、`system`、`external`：`bundle` 是默认值，表示优先使用 FlowRT
安装包内嵌 SDK；`external` 用于板级私有 SDK 或用户自行管理的运行时依赖；
`system` 保留给目标系统已经提供对应依赖的场景。

## `doctor`

```bash
flowrt doctor --target linux-arm64
flowrt doctor examples/libjpeg_cross_demo/rsdl/robot.rsdl --target linux-arm64
```

`doctor` 预检本机或交叉编译环境。指定 `--target` 后，它会解析 toolchain profile，
检查 Rust target、C/C++ 编译器、pkg-config、完整 target SDK、显式 sysroot、
CMake toolchain file 和 SDK overlay。缺少 Rust target、交叉编译器、完整 target SDK
或显式 overlay 时会以非零状态退出，并给出可执行修复提示。

提供 RSDL 路径时，`doctor` 会走与 `check` / `prepare` 一致的主路径：读取 RSDL、
归一化并校验 Contract IR、选中默认 profile，然后按 selected target 的 C++ component
`build.pkg_config` 依赖逐项执行 `pkg-config` 查询。查询环境会显式设置 target profile
语义下的 `PKG_CONFIG_LIBDIR`，不会借用 host 的默认搜索路径。输出会列出：

- `component=<name>`：依赖所属的 C++ component。
- `module=<pkg-config-name>`：声明在 `component.build.pkg_config` 中的模块名。
- `status=found|missing`：当前 target profile 下是否可见。
- `pc=<path>`：命中的 `.pc` 文件路径。
- `include_dirs=` / `lib_dirs=`：从 pkg-config 解出的 include / library 目录摘要。

当模块缺失时，`doctor` 会同时输出当前 `pkg_config_libdirs`、派生后的 overlay 搜索路径、
当前 `sdk_overlays`，并提示先显式 prepare 外部 SDK；如果 SDK 已经落在 overlay 中，
可执行 `flowrt toolchain init --target <platform> --sdk-overlay <path>` 生成 workspace
配置。`doctor` 不会触发 `flowrt build` 隐式下载或拉取任何第三方 SDK。

默认情况下，`flowrt build` 和生成 CMake 不会回退到 FlowRT 源码树 `runtime/cpp`。在 FlowRT 仓库内开发时，设置环境变量 `FLOWRT_ALLOW_REPO_RUNTIME_FALLBACK=1`，CLI 会同时把 `-DFLOWRT_ALLOW_REPO_RUNTIME_FALLBACK=ON` 传给 CMake，启用源码树回退。正式用户路径不应依赖此选项。

## `toolchain`

```bash
flowrt toolchain show --target linux-arm64
flowrt toolchain init --target linux-arm64 --sdk-overlay /opt/vendor/rknn
flowrt toolchain init --target linux-arm64 --force
```

`toolchain show` 展示指定 platform 的合并后 toolchain profile。输出包含每个字段的值和来源标注（`builtin`、`system`、`user`、`workspace`），以及配置层优先级说明。适用于诊断 toolchain 配置是否符合预期，或确认板级 SDK overlay 是否被正确加载。

`toolchain init` 在当前 workspace 下生成 `.flowrt/toolchains.toml`。默认不覆盖已有配置；传入 `--force` 会截断重写。`--sdk-overlay <path>` 可重复传入，相对路径按当前 workspace 解析后写入配置，生成的 TOML 会包含 `sdk_overlays` 列表。生成的配置保持最小可读格式，用户可以后续手动编辑补充 `sysroot`、`c_compiler`、`runtime_dependency_policy` 等字段。

不支持的 platform 会以非零状态退出并报错；当前只承诺 `linux-amd64` 和 `linux-arm64`。

## `external`

```bash
flowrt external check examples/external_driver_demo/external/fake_sensor_driver
flowrt external list --path examples/external_driver_demo/external
```

`external` 子命令用于检查和列出 external package。external package 是独立目录，必须包含 `flowrt-external.toml`：

```toml
[package]
name = "fake_sensor_driver"
version = "0.1.0"
flowrt_version = "0.7"
license = "MIT"

[[executable]]
name = "driver"
path = "bin/driver"
platforms = ["linux-amd64", "linux-arm64"]
backends = ["zenoh"]
health = "process_started"
```

`check` 会校验 package metadata、executable 路径、platform、backend 和 health 字段；不会启动进程，也不会隐式编译 external package。platform 当前支持 `linux-amd64` / `linux-arm64`，`linux-x86_64` / `linux-aarch64` 只作为旧输入别名接受，`list --path <dir>` 输出 canonical 名称。

RSDL 通过 `language = "external"` 和 graph 级 `[[external_process]]` 引用该 package：

```toml
[component.sensor]
language = "external"
kind = "external"
output = ["value:u32"]

[[external_process]]
process = "sensor_proc"
package = "fake_sensor_driver"
executable = "bin/driver"
args = ["--mode", "smoke"]
health = "process_started"
required_backends = ["zenoh"]
```

external route 默认不走 `inproc`。当前 auto resolver 会把涉及 external component 的 dataflow/service/operation route 选择到 `zenoh`；显式 `inproc` 会被拒绝，`iox2` 在 external package 能力和固定大小约束未完整建模前默认拒绝。

## `run`

```bash
flowrt run examples/import_demo/rsdl/robot.rsdl --process main
flowrt run examples/cpp_counter_demo/rsdl/robot.rsdl --process control
```

`run` 只读取 `flowrt/contract/contract.ir.json`、`flowrt/build/build-info.json` 和已构建的 generated app，然后运行单个 process group。它不会执行 `prepare`、不会构建、不会写 `flowrt/` 目录。首次运行或修改 RSDL、profile、生成模板、用户代码后，应先执行匹配 profile 的 `flowrt build`。

`--process <name>` 运行一个 RSDL process group。process 名称来自 `instance.<name>.process`，未声明时默认属于 `main`；RSDL process label 必须使用 `snake_case`，并且不得使用大小写不敏感的保留 `flowrt` 前缀。

`--run-steps <N>` 是 CLI 的显式运行上限，主要用于 smoke test 和调试观察。省略时，生成应用会持续运行，直到收到 SIGINT/SIGTERM 或 runtime shell 返回 `Error`。SIGINT/SIGTERM 会触发 runtime shutdown token，生成应用退出 scheduler loop 后继续执行 `shutdown` task、`on_stop` 和 `on_shutdown`。`--run-ticks <N>` 作为兼容别名保留；CLI 会把上限转换为生成应用的内部 `--flowrt-run-steps` 参数，核心 runtime scheduler 不读取 CLI 环境变量。

如果传入 `--profile <name>`，`run` 只校验已生成产物是否使用同名 profile；不匹配时会要求重新执行 `flowrt build --profile <name>`。如果传入 `--build-mode <mode>`，`run` 会校验 `build-info.json` 中的模式匹配；省略时使用最近一次成功 build 的模式。

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

`launch` 只运行已构建的 generated supervisor。supervisor 读取 `flowrt/launch/launch.json`，遍历 graph 中的 process group，并按 `runtime_kind` 启动 Rust app executable、C++ app executable 或 ROS2 bridge adapter。首次 launch 或修改 RSDL、profile、生成模板、用户代码后，应先执行匹配 profile 的 `flowrt build --launcher`。

含 C++ component 的 contract 需要先通过 `flowrt build --launcher` 显式构建 CMake app 和 generated supervisor；C++ only contract 的 supervisor-only Rust crate 只负责编排 C++ app，不生成 Rust runtime shell 或 Rust app binary。

含 external component 的 contract 也会生成 supervisor-only Rust crate。supervisor 按以下顺序解析 external package：

1. `FLOWRT_EXTERNAL_PATH` 中的目录或目录下同名 package。
2. `/opt/flowrt/external/<package>`。
3. 当前项目的 `external/<package>`。

external executable 不接收生成 app 的 `--process` 参数；FlowRT 通过环境变量传递上下文：`FLOWRT_PROCESS`、`FLOWRT_BACKEND`、`FLOWRT_EXTERNAL_PACKAGE`、`FLOWRT_EXTERNAL_EXECUTABLE`、`FLOWRT_EXTERNAL_PACKAGE_ROOT`、`FLOWRT_WORKSPACE_ROOT` 和可选 `FLOWRT_RUN_STEPS`。`health = "process_started"` 表示 spawn 成功即通过 readiness；默认 `runtime_socket` 会等待 external process 暴露 FlowRT introspection socket。

`inproc` 是单进程 backend。`launch` 如果发现 dataflow bind 跨越两个 RSDL process group，会拒绝该 contract；需要跨 process 通信时应选择 `iox2` 或 `zenoh` backend，或把相关 instance 放回同一 process group。

`--run-steps <N>` 会传给 supervisor，再由 supervisor 转发给每个生成应用 process；supervisor 也会把 live tick 快照作为上限信号，达到上限后终止仍在运行的子进程。省略时全部 process 按长期运行模式启动，并通过生成应用自己的 shutdown token 响应 SIGINT/SIGTERM。`--run-ticks <N>` 仍可作为兼容别名使用。

如果传入 `--profile <name>`，`launch` 只校验已生成产物是否使用同名 profile；不匹配时会要求重新执行 `flowrt build --launcher --profile <name>`。如果传入 `--build-mode <mode>`，`launch` 会校验 `build-info.json` 中的模式匹配；省略时使用最近一次成功 build 的模式。

launch manifest 的关键字段包括：

- process group 的 `runtimes`、`runtime_kind`、`depends_on`、`restart`、`failure`、`readiness`、`startup_delay_ms`、`env`、`cpu_affinity`、`nice`、`rt_policy` 和 `rt_priority`
- graph instance 的 `runtime`
- task 的 `name`、`trigger`、`period_ms`、`deadline_ms`、`priority`、`inputs` 和 `outputs`
- graph `channels`
- graph `services`
- graph `ros2_bridges`
- graph `external_processes`
- iox2 channel 的 canonical service name
- zenoh channel 的 deterministic key expression

## `bundle`

```bash
flowrt bundle examples/external_driver_demo/rsdl/robot.rsdl --output dist/external-demo
```

`bundle` 只读取已生成、已构建产物，不隐式运行 `deps`、`prepare` 或 `build`。缺少 `flowrt/build/build-info.json`、generated supervisor 或记录的 app binary 时会要求先运行 `flowrt build --launcher`。

bundle 输出是目录，包含：

- `bundle.toml`：FlowRT 版本、package、profile、target、platform、build mode、入口 binary、external package 摘要和 `artifacts` 列表；artifact 记录 kind、target、platform、相对路径和 sha256，是后续多目标 deploy 的事实源。
- `bin/`：本项目已构建二进制。native 或无 platform 的 bundle 继续使用 `bin/<filename>`；带 target platform 的 bundle 使用 `bin/<platform>/<filename>`，避免不同 target 同名二进制覆盖。复制到 bundle 后会对 ELF 可执行文件 best-effort 运行 `strip --strip-unneeded`；非 ELF 文件跳过，strip 不可用或失败时在命令摘要中累计 `strip_warnings`，不修改用户工作区原始产物。
- `flowrt/contract/contract.ir.json`、`flowrt/launch/launch.json`、`flowrt/selfdesc/selfdesc.json` 和 `flowrt/build/build-info.json`。
- `external/<package>`：随项目携带的 external package 副本。

输出目录必须不存在或为空，避免覆盖已有部署内容。bundle 会按 target platform 校验 external executable 的支持矩阵。bundle 不包含 FlowRT 源码仓库、不包含 Cargo target cache，也不内嵌系统 FlowRT runtime；目标机器应安装同版本 FlowRT deb。

## `deploy`

```bash
flowrt deploy dist/external-demo --host user@host --target edge --remote-dir /opt/external-demo
flowrt deploy dist/external-demo --host user@host --target edge --remote-dir /opt/external-demo --dry-run
```

`deploy` 读取 `bundle.toml`，不回读源码或 RSDL。schema v2 bundle 以 `artifacts` 列表作为部署事实源：dry-run 和真实部署都会按请求 `target` 选择 artifact，并校验 platform、相对路径、文件存在性和 sha256；如果 artifact platform、`bin/<platform>/` 路径层级或 hash 不一致，会提示重新执行对应的 `flowrt build --target <platform> --launcher` 后再 bundle。schema v1 bundle 继续按顶层 `target` 字段兼容。非 dry-run 时通过 `ssh <host> flowrt --version` 检查远端存在同一 `major.minor` 的 FlowRT，再用 `scp -r` 上传 bundle 到 `remote-dir`。它不做交叉编译、不安装系统 deb、不管理远端 supervisor 服务，这些属于后续多目标部署深化。

## `[[process]]` 编排字段

RSDL 支持 graph 级 `[[process]]` 声明进程编排策略：

```toml
[[process]]
name = "sensors"
restart = "on_failure"
max_restarts = 5
initial_delay_ms = 50
max_delay_ms = 500
failure = "propagate"
readiness = "runtime_ready"
startup_delay_ms = 200
cpu_affinity = [0, 1]
nice = -10
rt_policy = "fifo"
rt_priority = 50
env = { FLOWRT_LOG_LEVEL = "info" }

[[process]]
name = "control"
depends_on = ["sensors"]
restart = "never"
failure = "isolate"
readiness = "service_ready"
```

| 字段 | 类型 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `name` | string | — | process group 名称，必须与 `instance.<name>.process` 对应。 |
| `depends_on` | [string] | [] | 依赖的 process group 列表，supervisor 按依赖顺序启动。 |
| `restart` | string | `on_failure` | 重启策略：`never`、`on_failure`、`always`。 |
| `max_restarts` | u32 | 3 | 最大重启次数。 |
| `initial_delay_ms` | u64 | 100 | 首次重启退避。 |
| `max_delay_ms` | u64 | 1000 | 重启退避上限。 |
| `failure` | string | `propagate` | 失败传播：`propagate`（终止依赖进程）或 `isolate`（只记录当前进程失败）。 |
| `readiness` | string | `process_started` | readiness gate：`process_started`、`runtime_ready`、`service_ready`。 |
| `startup_delay_ms` | u64 | 0 | 进程启动后的错峰延迟毫秒数。 |
| `cpu_affinity` | [u32] | [] | 绑定到指定 CPU 核心列表。 |
| `nice` | i32 | — | 进程 nice 值（-20..=19）。 |
| `rt_policy` | string | — | 可选 Linux RT 调度策略：`fifo` 或 `round_robin`。 |
| `rt_priority` | u32 | — | RT 优先级（1..=99），需配合 `rt_policy`。 |
| `env` | table | {} | 注入子进程的环境变量键值对。 |

未声明 `[[process]]` 的实际 process group 仍使用默认策略。`depends_on` 只依赖同一 graph 内已由 instance 使用的 process group。`runtime_ready` 通过 introspection socket 握手判断 runtime 存活；`service_ready` 额外检查所有 service endpoint 就绪。readiness 超时或进程意外退出时，supervisor 会终止子进程并结构化报错。`cpu_affinity` 使用 `sched_setaffinity` 绑核；`rt_policy` 使用 Linux `SCHED_FIFO`/`SCHED_RR`。权限不足时 `flowrt status` 会展示结构化诊断而非静默忽略。

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
flowrt launch --run-steps 200 examples/ros2_bridge_demo/rsdl/robot.rsdl
```

观察 ROS2 topic 时如果遇到 daemon 旧缓存，先执行 `ros2 daemon stop` 后重试。

## `inspect`

```bash
flowrt inspect examples/import_demo/flowrt/contract/contract.ir.json
```

`inspect` 会先校验已落盘 Contract IR JSON，再显示摘要，用于确认 package、type、component、instance、task、bind、profile、target 和 deployment 是否符合预期。当前工具链不支持的 `ir_version`、`schema_version` 或 package `rsdl_version` 会被明确拒绝。

## RSDL Service 写法

Service 是 request/response 语义，不是 dataflow channel。RSDL 声明 service 端口和
bind 后，codegen 生成 typed client handle 和 server handler trait，用户只接触 typed
API，不直接调用 backend。

```toml
[type.PlanRequest]
goal = "u32"

[type.PlanResponse]
accepted = "bool"

[component.client]
language = "rust"
service_client = ["plan:PlanRequest->PlanResponse"]

[component.server]
language = "rust"
service_server = ["plan:PlanRequest->PlanResponse"]

[[bind.service]]
client = "client.plan"
server = "server.plan"
backend = "inproc"
timeout_ms = 1000
queue_depth = 16
overflow = "busy"
```

### Service policy 字段

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `backend` | string | auto | 传输后端：native generated Service 当前支持 `inproc`；`zenoh` runtime 已实现但 generated Service/Operation 尚未接线，native endpoint 会在 codegen fail-fast；external endpoint 可在 manifest 中选择 `zenoh` |
| `timeout_ms` | u64 | 5000 | 请求超时毫秒 |
| `queue_depth` | u32 | 32 | pending request 队列深度 |
| `overflow` | string | "busy" | 队列满策略：`busy` 或 `error` |
| `lane` | string | auto | server 所在 lane 名称 |
| `max_in_flight` | u32 | 64 | 并发处理中请求上限 |

### Rust 用户 API

codegen 为每个 service client 生成 `ServiceClient_{instance}_{port}` handle：

```rust
// 同步阻塞调用
let result = client.call(request, Duration::from_secs(1));
match result {
    flowrt::ServiceResult::Ok(response) => { /* 处理响应 */ }
    flowrt::ServiceResult::Err(code, msg) => { /* 处理错误 */ }
}

// 非阻塞调用
let handle = client.start_call(request, Duration::from_secs(1));
if handle.poll() {
    let result = handle.complete();
}
```

codegen 为有 service server 端口的 component trait 生成 handler 方法：

```rust
impl PlanService for MyPlanService {
    fn on_plan_request(&mut self, request: &PlanRequest) -> flowrt::ServiceResult<PlanResponse> {
        flowrt::ServiceResult::ok(PlanResponse { accepted: true })
    }
}
```

### C++ 用户 API

codegen 为有 service server 端口的 C++ component interface 生成虚方法：

```cpp
class PlanServiceInterface {
    virtual flowrt::ServiceResult<PlanResponse> on_plan_request(const PlanRequest& request) {
        return flowrt::ServiceResult<PlanResponse>::err(flowrt::ServiceError::HandlerError);
    }
};
```

validator 会要求：

- `client` 指向 component 的 `service_client` 端口。
- `server` 指向 component 的 `service_server` 端口。
- request 和 response 类型完全匹配。
- 同一个 client service 端口只能绑定一次。

### 错误语义

`flowrt::ServiceResult<T>` 携带 `ServiceError` 错误码：

| 错误码 | 含义 |
|--------|------|
| `Timeout` | 请求超时（`timeout_ms` 到期） |
| `Busy` | 服务队列满，请求被限流 |
| `Unavailable` | server 未注册或已销毁 |
| `WouldDeadlock` | 同 lane 阻塞调用会死锁 |
| `HandlerError` | 用户 handler 返回的业务错误 |
| `Backend` | 后端传输错误 |

`flowrt status` 输出每个 service 的运行态健康指标：`ready`、`in_flight`、`queued`、
`total_requests`、`timeout`、`busy`、`unavailable`、`late_drop`。

## `list` / `nodes`

```bash
flowrt list path/to/generated-app
flowrt nodes path/to/generated-app
```

`list` 和 `nodes` 读取生成应用二进制中的 `.flowrt.selfdesc` section，直接输出组件视图或 instance 列表；也可以读取 `flowrt/selfdesc/selfdesc.json` 作为调试辅助。它们不需要 RSDL 源文件，适合部署后在目标机器上确认 package、graph、component type、instance、task、channel、service、operation 和 params 是否与预期一致。

`list` 的摘要行包含 `component_types=<N>`、`services=<N>` 和 `operations=<N>` 计数。每个 graph 内先展示 component type 声明，再按 instance 展示其 tasks、channel endpoints、service endpoints、operation endpoints 和 params：

```text
package=robot_demo selfdesc=0.1 source_hash=abc graphs=1 component_types=2 instances=2 tasks=3 channels=2 services=1 operations=1 messages=1
graph default
  component planner language=rust kind=native
    service_clients: plan:PlanRequest->PlanResponse
    operation_clients: navigate:NavGoal->NavFeedback->NavResult
    params: goal_x:f64 update=on_tick
  component executor language=rust kind=native
    service_servers: execute:PlanRequest->PlanResponse
    operation_servers: navigate:NavGoal->NavFeedback->NavResult
  instance planner component=planner process=main runtime=rust
    task plan_task trigger=on_message lane=plan_lane
    channel planner.cmd -> executor.cmd type=Cmd backend=inproc
    service planner.plan_to_executor.execute client=planner.plan server=executor.execute request=PlanRequest response=PlanResponse backend=inproc
    operation planner.navigate client=planner.navigate server=executor.navigate goal=NavGoal feedback=NavFeedback result=NavResult backend=inproc
    param goal_x:f64 update=on_tick current=1.0
  instance executor component=executor process=main runtime=rust
    task exec_task trigger=on_message
    channel planner.cmd -> executor.cmd type=Cmd backend=inproc
    service planner.plan_to_executor.execute client=planner.plan server=executor.execute request=PlanRequest response=PlanResponse backend=inproc
    operation planner.navigate client=planner.navigate server=executor.navigate goal=NavGoal feedback=NavFeedback result=NavResult backend=inproc
```

`nodes` 输出 instance 列表，当 self-description 包含 component type 信息时会附加 `kind=` 字段：

```text
graph default
planner process=main runtime=rust component=planner kind=native
executor process=main runtime=rust component=executor kind=native
```

当前这两个命令只读取编译期静态自描述；运行态 socket 由 `status`、`echo`、`params` 和 `op` 使用。

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

标准 FrameDescriptor 是 fixed-size ABI 的特殊展示路径。消息 layout 如果是 64 字节、
字段为 `resource_id_hash`、`slot`、`generation`、`size_bytes`、
`timestamp_unix_ns`、`width`、`height`、`stride_bytes`、`format_id`、
`encoding_id` 和 `flags`，`echo` 会输出 `descriptor=frame` 与
`frame_descriptor={...}`，便于观察图像/大 payload 的 descriptor，而不复制真实 payload。

如果 runtime 还没有该 channel 的 payload，例如当前进程尚未发布该 channel 的样本，输出会包含 `payload_len=0 no payload`。

默认情况下，`echo` 只读取一次 latest snapshot。传入 `--follow` 后，CLI 会按 `--interval-ms <ms>` 指定的间隔持续轮询同一 runtime socket；第一条 snapshot 一定输出，后续只在 `published_count`、`published_at_ms` 或 raw payload 变化时输出，避免没有新发布时重复刷屏。默认轮询间隔是 250 ms。

生成的 Rust/C++ runtime shell 会为当前 process 的 active channel 预注册 live 摘要。控制面常驻，数据面 probe 按需启用：`flowrt echo` 打开 `observe_channel` 连接后，发布路径才会 best-effort 记录 latest payload；连接断开后自动关闭。无观察者时发布热路径只做 channel-local 原子检查，不做 payload 拷贝、frame 编码或 socket 写入。

## `params`

```bash
# 本机 socket 路径
flowrt params list --image path/to/generated-app
flowrt params get --image path/to/generated-app controller.kp
flowrt params set --image path/to/generated-app controller.kp 2.5
flowrt params set --image path/to/generated-app controller.mode '"safe"'

# 跨机 zenoh control-plane 路径
flowrt params list --image path/to/generated-app --remote
flowrt params get --image path/to/generated-app controller.kp --remote
flowrt params set --image path/to/generated-app controller.kp 2.5 --remote
flowrt params list --image path/to/generated-app --remote --runtime flowrt/params/robot/hash/12345
```

`params` 操作运行态参数控制面。静态 self-description 用于确认参数属于该应用，并通过 `self_description_hash` 选择匹配的 live runtime；实际值来自 runtime。

**本机路径**（默认）：`--image` 指定生成应用二进制或 `selfdesc.json`，可选 `--socket` 指定 Unix socket。省略 `--socket` 时使用与 `echo` 相同的自动发现规则，多个同 hash 进程同时存在时需要显式传入 `--socket <path>`。

**远程路径**（`--remote`）：通过 zenoh control-plane 发现远端 runtime。CLI 按 `flowrt/params/{package}/{selfdesc_hash}/{pid}` 格式的 key expression 查询所有远程参数端点，筛选与 `--image` 自描述 hash 匹配的 runtime。多个匹配时要求用户用 `--runtime <key_expr>` 显式选择；无匹配时报错。`--socket` 只表示本机 Unix socket，不能和 `--remote` 同用。`--timeout-ms` 控制发现和请求超时，默认 5000ms。CLI 会在 stderr 输出 `target:` 行，明确告知命令打到了哪个 runtime。

参数不是 dataflow channel。RSDL/Contract IR 声明参数 schema，生成 shell 持有 typed params 快照，并在 scheduler tick 边界把 `on_tick` 参数的 pending 值应用到用户组件。用户组件可以实现默认提供的 `on_params_update(old, new, context)` 钩子；该钩子返回 `Ok` 后，新参数才会提交并反映到后续 `on_tick`。

`set` 的值必须是合法 JSON：数字写 `2.5`，布尔写 `true`，字符串需要带 JSON 引号，例如 shell 中常写成 `'"safe"'`。`startup` 参数运行时不可修改；`on_tick` 参数可以提交 pending 值，由生成 shell 在下一个 tick 边界应用。输出中的 `runtime_update=startup-only` 表示该参数只能在进程启动前确定，运行中只能读取当前值。输出格式是行式摘要：

```text
controller.kp type=f32 update=on_tick current=1.0 pending=2.5 min=0.0 max=5.0 choices=[] runtime_update=pending-on-tick
```

`zenoh` 示例在跨机时通常需要为两个进程分别注入连接信息。常用环境变量是：

- `FLOWRT_ZENOH_CONNECT`
- `FLOWRT_ZENOH_LISTEN`
- `FLOWRT_ZENOH_MODE`
- `FLOWRT_ZENOH_NO_MULTICAST`
- `FLOWRT_TICK_SLEEP_MS`

前四个用于给 runtime session 注入 zenoh 网络配置。`flowrt launch` 在这些变量都未显式设置时，会为同一个 supervisor 本机启动的 zenoh process 自动分配 `127.0.0.1` TCP mesh；只要设置了任一 `FLOWRT_ZENOH_MODE` / `FLOWRT_ZENOH_LISTEN` / `FLOWRT_ZENOH_CONNECT`，就视为用户接管 session 配置。`FLOWRT_TICK_SLEEP_MS` 用于把 demo 的同步调度步间隔拉长到可观察窗口。运行上限由 `flowrt run --run-steps <N>` 或 `flowrt launch --run-steps <N>` 显式传入，不进入核心 runtime scheduler。

## `op`

```bash
flowrt op list --image path/to/generated-app
flowrt op list --socket /run/user/1000/flowrt/12345.sock
flowrt op status
flowrt op status controller.plan --socket /run/user/1000/flowrt/12345.sock
flowrt op cancel 111:7:3 --socket /run/user/1000/flowrt/12345.sock
```

`op` 面向 Operation 的观测和基础控制。Operation 是 typed long-running command，不是 Service 别名；生成器会把 Operation lower 成内部 start/cancel/status service 与 feedback/result channel，但用户和 CLI 的主视图仍是 Operation。

当前 generated Operation runtime 只支持单个运行中的 invocation：policy 必须是 `concurrency = "reject"`、`preempt = "reject"`、`max_in_flight = 1`。第二个 start 会返回 `Busy`，直到当前 invocation 进入终态。`queue`、`cancel_running` 和多 in-flight 策略属于长期 IR 语义，runtime 完整实现前由 validator 拒绝。

`op list` 读取 self-description：传入 `--image` 时读取生成应用二进制或 `selfdesc.json`，省略 `--image` 时通过 live socket 请求当前进程嵌入的 self-description。输出包含 Operation name、canonical id、client/server 端口、goal/feedback/result 类型、backend 和 policy 摘要。

`op status` 读取 live runtime status。省略 operation 名称时输出所有 live Operation；传入 `<client_instance>.<client_port>` 时只输出该 Operation。输出格式与 `status` 的 operation 行一致：

```text
operation=controller.plan ready=true running=1 queued=0 current_operation_ids=[111:7:3] total_started=1 succeeded=0 failed=0 canceled=0 timeout=0 preempted=0 last_transition_ms=1717800000000 socket=...
```

`op cancel <operation_id>` 通过 runtime introspection socket 发送 `operation_cancel` 请求。`operation_id` 来自 `op status` 的 `current_operation_ids` 字段。省略 `--socket` 时 CLI 会先通过 `status` 无副作用筛选唯一 runtime；如果多个进程都报告同一个 ID，会要求显式传入 `--socket <path>`，不会广播取消。

## `status`

```bash
flowrt status
flowrt status --live-only
```

`status` 扫描当前用户 runtime socket 目录中的 FlowRT 进程，并通过 handshake 验证 PID、package、process、runtime、静态自描述 hash 和 tick/channel 摘要。socket 路径只作为发现入口；CLI 不把文件名当作进程身份事实。

默认输出会保留 stale socket 诊断，便于排查 SIGKILL、异常退出或非 FlowRT socket。`--live-only` 只输出成功返回 live status 的 runtime；如果没有 live runtime，会输出 `no live FlowRT processes`。

当前 Rust/C++ 生成应用都会启动 status socket，路径优先使用 `$XDG_RUNTIME_DIR/flowrt/<pid>.sock`，没有 `XDG_RUNTIME_DIR` 时使用 `/tmp/flowrt.<uid>/<pid>.sock` 风格的当前用户目录。生成 shell 会把 scheduler tick 计数、active channel 摘要、发布计数、active echo observer 数量和 probe drop 计数写入 live status；payload 只在 echo 数据面 probe 启用期间 best-effort 记录。

generated supervisor 会启动自己的 status socket，`runtime=supervisor`，`process=flowrt_supervisor`。它会按子进程 PID socket 轮询 live status，并额外输出 `supervisor_process=<name>` 行，字段包括 `state`、`pid`、`restarts`、`ticks`、`last_seen_ms`、`tick_stale` 和 `exit_code`。`state` 当前取值包括 `starting`、`running`、`stale`、`waiting_dependencies`、`restarting`、`completed`、`shutdown`、`exited` 或 `failed`。内置 restart policy 是 `on-failure`：子进程异常退出时最多重启 3 次，退避 100ms 起步、上限 1000ms；正常退出不重启。

当进程正在等待 readiness gate 时，supervisor_process 行会包含 `readiness_wait=<gate>` 字段。当进程配置了资源提示时，会包含 `resource_placement=<json>` 字段，展示 `desired`（RSDL 声明）和 `applied`（实际生效）的 `cpu_affinity`、`nice`、`rt_policy` 和 `rt_priority`。

```text
supervisor_process=control state=running pid=12345 restarts=0 ticks=1000 last_seen_ms=... tick_stale=false exit_code=none readiness_wait=service_ready resource_placement={"desired":{"cpu_affinity":[0,1],"nice":-10,"rt_policy":"fifo","rt_priority":50},"applied":{"cpu_affinity":[0,1],"nice":-10,"rt_policy":"fifo","rt_priority":50}} socket=...
```

runtime 启动时可以预注册 service endpoint，`status` 会输出每个 service 的运行态健康行，并通过 self-description 关联 client/server instance：

```text
service=planner.plan_to_executor.execute client_instance=planner server_instance=executor ready=true in_flight=2 queued=1 total_requests=100 timeout=3 busy=1 unavailable=0 late_drop=2 socket=/run/user/1000/flowrt/12345.sock
```

字段说明：`client_instance`/`server_instance` 是从 self-description 关联的 service endpoint 参与方；`ready` 表示 service 是否就绪；`in_flight` 是当前正在处理的请求数；`queued` 是排队中的请求数；`total_requests` 是累计请求总数；`timeout`/`busy`/`unavailable`/`late_drop` 分别是超时、繁忙拒绝、不可用和迟到响应/丢弃的累计计数。

runtime 也可以预注册 Operation endpoint，`status` 会输出每个 Operation 的运行态健康行：

```text
operation=controller.plan ready=true running=1 queued=0 current_operation_ids=[111:7:3] total_started=1 succeeded=0 failed=0 canceled=0 timeout=0 preempted=0 last_transition_ms=1717800000000 socket=/run/user/1000/flowrt/12345.sock
```

字段说明：`ready` 表示 Operation endpoint 是否可用；`running` / `queued` 是当前运行和排队 invocation 数；`current_operation_ids` 是可用于 `flowrt op cancel` 的非终态 ID；`total_started`、`succeeded`、`failed`、`canceled`、`timeout` 和 `preempted` 是累计计数；`last_transition_ms` 为最近状态转换时间戳，`none` 表示尚无状态转换。

录制开启或存在累计 recorder 计数时，`status` 会输出 recorder 行：

```text
recorder enabled=true output=run.mcap dropped_count=0 bytes_written=128 queued_events=0 active_filters=[channel:source.imu_to_sink.imu] socket=/run/user/1000/flowrt/12345.sock
```

字段说明：`enabled` 表示数据面 tap 是否开启；`dropped_count` 是 recorder 有界队列满时丢弃的事件数；`bytes_written` 是 runtime 已接受的事件 payload 字节数；`queued_events` 是等待 CLI drain 的事件数；`active_filters` 是当前 recorder 过滤条件。

runtime 启动 status socket 时会先探测同路径 socket 是否仍可连接：仍可连接时拒绝覆盖，避免同机多个进程互相抢占；不可连接时按 stale socket 回收，处理 SIGKILL 后遗留的 socket 文件。

### 调度健康指标

`status` 会输出 task 级和 lane 级调度健康行：

```text
task_health=fast_loop lane=sensor_lane deadline_missed=0 stale_input=2 backpressure=0 overflow=0 fairness_violations=0 runs=1000 successes=998 consecutive_failures=0 last_run_ms=1717800000000 last_success_ms=1717800000000 socket=...
lane_health=sensor_lane queue_depth=0 dispatched_count=1000 fairness_violations=0 socket=...
```

task 健康字段：

| 字段 | 说明 |
| --- | --- |
| `deadline_missed` | task 执行超过 `deadline_ms` 的累计次数，超限时阻止 late output 发布。 |
| `stale_input` | 输入数据超过 `max_age_ms` 的累计次数。 |
| `backpressure` | 下游队列满导致的背压事件累计次数。 |
| `overflow` | channel 溢出事件累计次数。 |
| `fairness_violations` | lane 饥饿公平性违规累计次数。 |
| `runs` / `successes` | 累计运行次数和成功次数。 |
| `consecutive_failures` | 连续失败次数。 |
| `last_run_ms` / `last_success_ms` | 最近运行/成功时间戳，`none` 表示尚未运行。 |

lane 健康字段：

| 字段 | 说明 |
| --- | --- |
| `queue_depth` | lane 当前排队任务数。 |
| `dispatched_count` | lane 累计调度次数。 |
| `fairness_violations` | lane 间饥饿违规累计次数。 |

这些指标由 runtime 内置的调度健康策略自动采集。所有健康字段使用 `serde(default)` 保证前向兼容，旧版 JSON 不含健康字段时解析为零值。

## `hz`

```bash
flowrt hz
flowrt hz source.imu_to_sink.imu
flowrt hz source.imu_to_sink.imu --socket /run/user/1000/flowrt/12345.sock --window-ms 500
```

`hz` 通过 live status 控制面读取 channel `published_count`，等待一个采样窗口后再次读取，并用计数差除以实际 elapsed time 得到发布频率。它不打开 `observe_channel`，不读取 payload，不启用 echo 数据面 probe，因此不会让发布热路径做 payload 拷贝或 frame 编码。

省略 channel 时输出所有 live channel；传入 channel 时只输出完全匹配的 canonical channel 名。省略 `--socket` 时扫描当前用户 runtime socket 目录；多个进程同时存在时会分别输出并带上 socket 路径。`--window-ms` 默认 1000，必须大于 0。

## `record`

```bash
flowrt record --output run.mcap --duration 10s --all
flowrt record --output imu.mcap --channel source.imu_to_sink.imu --socket /run/user/1000/flowrt/12345.sock
flowrt record --output op.mcap --operation controller.plan --force
```

`record` 通过 live runtime introspection socket 按需开启 recorder tap，把 FlowRT 事件写入 MCAP 文件。它不需要 RSDL 源文件，也不需要生成应用二进制；事件 schema 使用 FlowRT 自有 `RecordEnvelope` v1，覆盖 channel sample、参数控制面、Service、Operation、scheduler/time 和 runtime/process metadata。

省略 `--socket` 时，CLI 扫描当前用户 runtime socket 并要求恰好一个 live FlowRT 进程；如果同机有多个 runtime，必须显式传入 `--socket <path>`。`--duration` 省略时持续录制到 SIGINT/SIGTERM；收到信号后 CLI 会请求 runtime 停止 recorder、drain 剩余事件并关闭 MCAP。已有输出文件默认拒绝覆盖，传入 `--force` 后才会截断重写。

过滤规则：

- 不传 `--channel` / `--operation` 且不传 `--all` 时，默认录制所有支持的 FlowRT 事件。
- `--channel <name>` 可重复，只录制匹配的 channel sample。
- `--operation <name>` 可重复，只录制匹配的 Operation event。
- `--all` 表示录制全部事件，不能与 `--channel` 或 `--operation` 混用。

完成后输出最小摘要：

```text
recorded output=run.mcap socket=/run/user/1000/flowrt/12345.sock event_count=42 dropped_count=0 bytes_written=4096 active_filters=[all]
```

`event_count` 是写入 MCAP 的 envelope 数量；`dropped_count` 是 runtime recorder 队列丢弃计数；`bytes_written` 是 runtime 已接受的事件 payload 字节数。默认未录制时，生成的 Rust/C++ runtime shell 只在热路径做轻量开关判断，不持续复制 payload。

FrameDescriptor 事件默认按 descriptor-only 录制；CLI 摘要会输出
`descriptor_payload=descriptor_only`。这表示 MCAP 中记录的是 descriptor 和事件元数据，
不是 side-channel 中的真实图像 payload。

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

profile 还可以声明 graph 完整性模式：

```toml
[profile.dev]
mode = "island"
backend = "inproc"
```

`mode` 只允许 `strict` 或 `island`，省略时等同 `strict`。`strict` 是生产默认模式，task 的 active input 必须由普通 dataflow bind 满足；`island` 是可拆卸脚手架模式，用于单功能单位开发或旧系统逐包迁移。island 模式下，外部输入和待对比输出通过 typed boundary endpoint 表达：

```toml
[[boundary.input]]
name = "scan_in"
port = "planner.scan"
type = "Scan"

[[boundary.output]]
name = "cmd_out"
port = "planner.cmd"
type = "ControlCommand"
```

boundary endpoint 绑定真实 component port，不是传输后端、ROS2 topic 或 transport API。开发或迁移完成后，应删除 boundary endpoint，改用普通 `[[bind.dataflow]]`，并把 profile 切回 `strict`。

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
