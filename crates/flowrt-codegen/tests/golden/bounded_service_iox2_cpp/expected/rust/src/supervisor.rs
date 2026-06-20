// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "8301330a4e4aeb494142a250eb1e60fb658e7adf572fbb1d7d51bea28f760b48";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "bounded-service-iox2-cpp-flowrt-app",
    cpp_app_stem: "bounded_service_iox2_cpp_cpp_app",
    ros2_bridge_stem: "",
    package_name: "bounded_service_iox2_cpp",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
