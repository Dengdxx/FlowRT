// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "d92adc6feea352a2fef7a762972adcffd2bc5e8d6eee670e628e74f5628928cc";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "graph-health-stop-cpp-demo-flowrt-app",
    cpp_app_stem: "graph_health_stop_cpp_demo_cpp_app",
    ros2_bridge_stem: "",
    package_name: "graph_health_stop_cpp_demo",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
