// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "fb4abd2bc81598fb1977184718a6522eb27789582fef7eee74e55e6ab9305a7e";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "feedback-v2-cpp-flowrt-app",
    cpp_app_stem: "feedback_v2_cpp_cpp_app",
    ros2_bridge_stem: "",
    package_name: "feedback_v2_cpp",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
