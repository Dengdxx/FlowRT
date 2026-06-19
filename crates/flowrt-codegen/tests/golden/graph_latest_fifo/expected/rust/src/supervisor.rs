// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "0d1f6d877e0ca2ed36d015df04a7829d76ea35bd64e4d93ed87705e23b5d7df6";
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
