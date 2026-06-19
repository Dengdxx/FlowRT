// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "746473aeb8bbbca2dc8ef2bd05cb794750d8d77d00df39ac3e7ca2eab8732f05";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "graph-health-stop-demo-flowrt-app",
    cpp_app_stem: "graph_health_stop_demo_cpp_app",
    ros2_bridge_stem: "",
    package_name: "graph_health_stop_demo",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
