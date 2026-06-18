// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "d7c96aaedcc344cfd7f2e42a754bf852a6c9f1452b547b9552e78de61d8f19ff";
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
