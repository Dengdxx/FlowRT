// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "1ebab7db5376f7b2de8e63d391ddc4a1a20fa06969de8343b25288e31ccb7d9f";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "feedback-v2-rust-flowrt-app",
    cpp_app_stem: "feedback_v2_rust_cpp_app",
    ros2_bridge_stem: "",
    package_name: "feedback_v2_rust",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
