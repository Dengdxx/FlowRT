// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "9115602a2219c00123bd63211e3ee3ffb7d94567c131c9338c68467bf58f1b99";
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
