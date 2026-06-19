// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "8098ed2ea7ea32b1565aa729fafa4c1c7e05d9c1fff87df86bb4d31d96601824";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "zenoh-service-cpp-flowrt-app",
    cpp_app_stem: "zenoh_service_cpp_cpp_app",
    ros2_bridge_stem: "",
    package_name: "zenoh_service_cpp",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
