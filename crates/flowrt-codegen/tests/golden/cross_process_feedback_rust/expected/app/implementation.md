# FlowRT App API 实现清单

FlowRT 管理产物，可删除后由 `flowrt prepare` 重建。用户业务代码仍放在项目 `app/` 目录；本目录下 `stubs/` 只提供参考模板，不会被自动复制。

- App API manifest: `flowrt/app/app_api.json`
- package: `xproc_feedback_rust`
- graph: `default` mode=`strict` profile=`default` backend=`zenoh`

## Runtime Context

- task context timing: `context.timing()`
- C callback context: `context->has_timing / context->timing`
- 不改变用户 handler 签名；已有 `Context` 或 C callback context 指针用于读取 timing。
- realtime 运行时读取 runtime observed scheduling time；`observed_delta_ms` 表示相邻 observed 时间差。
- replay / temporary island 使用 fixture 驱动的 deterministic timing。
- 生命周期 context 默认不携带 timing；读取前需判断 `Option`、指针或 `has_timing`。
- fields: `scheduled_time_ms`, `observed_time_ms`, `scheduled_delta_ms`, `observed_delta_ms`, `lateness_ms`, `missed_periods`, `deadline_missed`, `overrun`
- non-goals: 不承诺硬实时，不定义 sensor timestamp / event-time、clock domain、PTP、NTP 或 approximate sync。

## Components

### `controller`

- language: `rust`
- kind: `native`
- user file: `app/rust/mod.rs`
- reference stub: `app/stubs/rust/controller.rs`
- handlers:
  - `on_init`: `fn on_init(&mut self, context: &mut flowrt::Context) -> flowrt::Status`
  - `on_start`: `fn on_start(&mut self, context: &mut flowrt::Context) -> flowrt::Status`
  - `on_stop`: `fn on_stop(&mut self, context: &mut flowrt::Context) -> flowrt::Status`
  - `on_shutdown`: `fn on_shutdown(&mut self, context: &mut flowrt::Context) -> flowrt::Status`
  - `on_tick`: `fn on_tick(&mut self, state: flowrt::Latest<'_, State>, cmd: &mut flowrt::Output<Cmd>) -> flowrt::Status`

### `plant`

- language: `rust`
- kind: `native`
- user file: `app/rust/mod.rs`
- reference stub: `app/stubs/rust/plant.rs`
- handlers:
  - `on_init`: `fn on_init(&mut self, context: &mut flowrt::Context) -> flowrt::Status`
  - `on_start`: `fn on_start(&mut self, context: &mut flowrt::Context) -> flowrt::Status`
  - `on_stop`: `fn on_stop(&mut self, context: &mut flowrt::Context) -> flowrt::Status`
  - `on_shutdown`: `fn on_shutdown(&mut self, context: &mut flowrt::Context) -> flowrt::Status`
  - `on_tick`: `fn on_tick(&mut self, cmd: flowrt::Latest<'_, Cmd>, state: &mut flowrt::Output<State>) -> flowrt::Status`
