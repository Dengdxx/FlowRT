// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "d15edb38db76485db58e6a44aec6504c46e998d20b2bd172c5a20c72b70c3eae";
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
