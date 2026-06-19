// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "f134713723e76dbb644e06a621e18de6a19f36b661707ba3d22f2c311fbd1708";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "iox2-service-rust-flowrt-app",
    cpp_app_stem: "iox2_service_rust_cpp_app",
    ros2_bridge_stem: "",
    package_name: "iox2_service_rust",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
