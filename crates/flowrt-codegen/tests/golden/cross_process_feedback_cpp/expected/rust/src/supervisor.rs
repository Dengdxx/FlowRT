// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "afc5f92407309cd95dbedd6410288b709800a185fec5359b810239fadee7b336";
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
