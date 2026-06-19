// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "991b5ca2c204262826438feee8564df2ca3237f19b2b99a684c1beb9155e360b";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "zenoh-service-rust-flowrt-app",
    cpp_app_stem: "zenoh_service_rust_cpp_app",
    ros2_bridge_stem: "",
    package_name: "zenoh_service_rust",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
