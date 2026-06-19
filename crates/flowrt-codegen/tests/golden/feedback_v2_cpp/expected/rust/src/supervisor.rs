// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "930836bc64145fe96177f02f514376591ef0bc0a392a7a692f09583a370647f1";
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
