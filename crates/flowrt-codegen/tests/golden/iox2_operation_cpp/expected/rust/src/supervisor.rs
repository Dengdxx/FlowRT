// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "b2f00591952996e25a5f61bc283c317fcaa40ac26ef6b990c9cce17936684987";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "iox2-operation-cpp-flowrt-app",
    cpp_app_stem: "iox2_operation_cpp_cpp_app",
    ros2_bridge_stem: "",
    package_name: "iox2_operation_cpp",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
