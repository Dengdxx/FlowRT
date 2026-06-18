// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "59c48e0ed55f08f36923b309b11708d70e3df1f93a1f70b0686f8335d8b0df2f";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "feedback-loop-cpp-demo-flowrt-app",
    cpp_app_stem: "feedback_loop_cpp_demo_cpp_app",
    ros2_bridge_stem: "",
    package_name: "feedback_loop_cpp_demo",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
