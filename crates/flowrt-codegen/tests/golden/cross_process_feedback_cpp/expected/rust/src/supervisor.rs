// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "933d32505fef60e99ed8bceb3ecb51e5afaa9a1170d542d50e5a3204941b311d";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "xproc-feedback-cpp-flowrt-app",
    cpp_app_stem: "xproc_feedback_cpp_cpp_app",
    ros2_bridge_stem: "",
    package_name: "xproc_feedback_cpp",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
