# 项目布局

本文说明 FlowRT app 的已落地目录边界。它是用户项目和示例的主路径，不是设计草案。

## 标准布局

FlowRT app 项目采用固定分层。`flowrt init` 只创建项目入口和最小 RSDL；用户实现目录在
参考 `flowrt/app/stubs/` 后手写或复制：

```text
my_robot/
  flowrt.toml
  rsdl/
    robot.rsdl
  app/                       # 用户业务代码，按需手写或复制参考 stub
    rust/mod.rs
    cpp/
    c/
  flowrt/
    app/
      app_api.json
      implementation.md
      stubs/
```

- `flowrt.toml` 只记录项目入口，当前格式为：

  ```toml
  [project]
  main = "rsdl/robot.rsdl"
  ```
- `rsdl/` 放系统契约，声明 package、message、component、instance、task、bind、profile
  和 target。
- `app/` 放用户业务算法。Rust 用户入口是 `app/rust/mod.rs`；C++ 用户代码位于
  `app/cpp/**`；C callback v0 用户代码位于 `app/c/**`。
- `flowrt/app/app_api.json`、`flowrt/app/implementation.md` 和 `flowrt/app/stubs/` 是
  `flowrt prepare` 生成的 App API manifest、实现清单和参考模板。
- `flowrt/` 是 FlowRT 管理的生成目录，可删除、可重建，不放用户业务代码。

## 入口发现

在含 `flowrt.toml` 的项目根或子目录中，以下命令可以省略 RSDL 路径：

```bash
flowrt check
flowrt explain
flowrt deps
flowrt prepare
flowrt build
flowrt run --process main
flowrt doctor
```

CLI 会从当前目录向上查找最近的 `flowrt.toml`，读取相对 manifest 所在目录的
`project.main`。显式传入 RSDL 路径时，显式路径优先。

`launch`、`bundle` 和 `deploy` 不做默认 RSDL 发现。需要 supervisor 路径时，先构建
launcher，再显式传入 RSDL：

```bash
flowrt build --launcher
flowrt launch rsdl/robot.rsdl
```

## 用户代码边界

`flowrt init` 只创建 `flowrt.toml` 和最小 `rsdl/robot.rsdl`。`flowrt add` 只编辑 RSDL：
`add message` 追加 type，`add module` 追加 module 文件和 workspace 注册，
`add component` 追加 component、同名 instance 和最小 task；它们都不创建、追加或覆盖
用户 `app/`。

`flowrt prepare` 只生成 `flowrt/` 管理产物，其中 `flowrt/app/stubs/` 是参考模板，不会被
自动复制到用户 `app/`。用户参考后在 `app/` 中手写或复制需要保留的算法实现；后续
`prepare`、`build`、`run` 和 `launch` 不会把手写业务逻辑写进 `flowrt/`。

Rust component 通过 generated trait 接入。C++ component 通过 generated interface 和
`flowrt_user::build_app()` 用户工厂接入。C component v0 通过
`flowrt_app/c_components.h` 声明的 callback table factory 接入 generated C++ runtime
shell。

## C callback v0

C callback v0 是跨语言 C ABI 边界的一条最小可用路径，不是完整 C runtime。

当前支持：

- `native` component。
- fixed-size plain data message。
- `inproc` demo。
- 普通 `flowrt build` / `flowrt run`。
- `flowrt build --launcher` 后由 `flowrt launch` 启动 generated supervisor。

当前不支持：

- params。
- service。
- operation。
- variable frame。
- `io_boundary`。
- `external`。
- `pkg_config`。
- 动态加载。
- 独立 C runtime。
- Python binding。

`examples/c_counter_demo` 展示该路径：`rsdl/robot.rsdl` 声明 `language = "c"` 的
`counter_source` 和 `counter_sink`，用户实现位于 `app/c/`，运行时由 generated C++
runtime shell 静态链接 callback table factory。
