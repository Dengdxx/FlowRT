//! Supervisor engine：进程编排、依赖排序、重启策略和健康监控。
//!
//! 本模块把 supervisor 核心逻辑从 codegen 字符串下沉为可独立测试的 runtime 深模块。
//! 生成物只保留 manifest 常量、binary name stems 和 self-description hash 的薄 glue。

mod command;
mod dependency;
mod global_tick;
mod launch_loop;
mod manifest;
mod readiness;
pub mod resource_placement;
mod resources;
mod time;
mod zenoh;

pub use command::{
    ExternalExecutableResolution, app_executable_for_runtime, build_external_process_command,
    build_process_command, build_process_command_with_status_out, external_app_executable,
    spawn_flowrt_process,
};
pub use dependency::{
    PropagatableChild, collect_propagated_failures, process_dependencies_satisfied,
    resolve_dependency_order,
};
pub use global_tick::{GlobalTickCoordinator, TickCoordinatorEvent, TickDone, TickGrant};
pub use launch_loop::{SupervisorConfig, launch};
pub use manifest::{
    DEFAULT_RESTART_POLICY, LaunchArtifact, LaunchBoundaryEndpoint, LaunchClock, LaunchDeterminism,
    LaunchExternalHealth, LaunchExternalProcess, LaunchExternalWorkingDir, LaunchGraph,
    LaunchIoBoundary, LaunchIoResource, LaunchManifest, LaunchProcess, LaunchProfileMode,
    LaunchResourceContract, LaunchResourceDescriptor, LaunchResourceProvider,
    LaunchResourceRequirement, LaunchResourceSatisfaction, LaunchService, ReadinessGate,
    RestartPolicy, RestartPolicyKind,
};
pub use zenoh::{
    ZenohLaunchEnv, ZenohLaunchPlan, should_auto_configure_zenoh, zenoh_launch_env_for_graph,
};

#[cfg(test)]
mod tests;
