// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "2403363ebd07a13a2d22f16f1ae7bef0f506b6092c23bbdb00e258d492d46b89";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "fault-injection-degrade-demo-flowrt-app",
    cpp_app_stem: "fault_injection_degrade_demo_cpp_app",
    ros2_bridge_stem: "",
    package_name: "fault_injection_degrade_demo",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
