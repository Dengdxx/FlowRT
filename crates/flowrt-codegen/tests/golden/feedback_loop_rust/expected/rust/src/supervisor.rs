// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "234d29570dd40c5f520208d7d57cdce752e9400a715c3bd3a39da0d5cf15c913";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "feedback-loop-rust-demo-flowrt-app",
    cpp_app_stem: "feedback_loop_rust_demo_cpp_app",
    ros2_bridge_stem: "",
    package_name: "feedback_loop_rust_demo",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
