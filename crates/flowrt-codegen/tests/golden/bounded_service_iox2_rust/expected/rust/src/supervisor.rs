// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "59203645af27681df769a77056176bd0f3295a727e944eb5002996a29a6b1d1b";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "bounded-service-iox2-rust-flowrt-app",
    cpp_app_stem: "bounded_service_iox2_rust_cpp_app",
    ros2_bridge_stem: "",
    package_name: "bounded_service_iox2_rust",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
