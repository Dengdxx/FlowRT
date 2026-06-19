// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "6488078acc04181aa645efdc5dcd24ea65906c6c4d9e32738c64c6917b6c4e6c";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "iox2-service-cpp-flowrt-app",
    cpp_app_stem: "iox2_service_cpp_cpp_app",
    ros2_bridge_stem: "",
    package_name: "iox2_service_cpp",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
