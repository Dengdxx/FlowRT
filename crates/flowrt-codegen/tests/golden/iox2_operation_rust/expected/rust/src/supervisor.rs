// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "f281234861dfbf2235d5c399e68ab73a9e55305fc21c9d0d5f71df7569e58384";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "iox2-operation-rust-flowrt-app",
    cpp_app_stem: "iox2_operation_rust_cpp_app",
    ros2_bridge_stem: "",
    package_name: "iox2_operation_rust",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
