// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "1c4f8da37c9130ae6af309540390fba61e857108698ed93441954b1a71030c73";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "graph-demo-flowrt-app",
    cpp_app_stem: "graph_demo_cpp_app",
    ros2_bridge_stem: "",
    package_name: "graph_demo",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
