// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "172e5e4b070a667f8478e0f4e5d86f100525db4e8dfb1480aa0dad7a8e25bb50";
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
