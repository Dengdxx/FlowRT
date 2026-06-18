// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "586cf566d11273524661805ccab0e9236bb93a990a1d20214be1f11aacf76481";
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
