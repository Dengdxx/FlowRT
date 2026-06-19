// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "7b598e1b7305b08ad3a96cdb98a8e2a5f9ac1c97e1e78d600989ac12166b8246";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "fault-injection-restart-demo-flowrt-app",
    cpp_app_stem: "fault_injection_restart_demo_cpp_app",
    ros2_bridge_stem: "",
    package_name: "fault_injection_restart_demo",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
