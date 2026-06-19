// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "bcfa4794b237450e92c146b4afe476d7711272d01a914c0f0b43c5da136d1341";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "zenoh-operation-rust-flowrt-app",
    cpp_app_stem: "zenoh_operation_rust_cpp_app",
    ros2_bridge_stem: "",
    package_name: "zenoh_operation_rust",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
