// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "cf6954e521458a9554702d215e42533203d21735316e6eda9a2fd461c16a5687";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "zenoh-operation-cpp-flowrt-app",
    cpp_app_stem: "zenoh_operation_cpp_cpp_app",
    ros2_bridge_stem: "",
    package_name: "zenoh_operation_cpp",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
