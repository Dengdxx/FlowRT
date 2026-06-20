// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "70553656ed8fb6880c0910f52e0567f3655aa86020d9a6d34ac01901211856c5";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "service-iox2-dynamic-fallback-flowrt-app",
    cpp_app_stem: "service_iox2_dynamic_fallback_cpp_app",
    ros2_bridge_stem: "",
    package_name: "service_iox2_dynamic_fallback",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
