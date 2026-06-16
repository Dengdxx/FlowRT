// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "f10f887a9bbfe7c6f60d84b12de6cddb863d00b5a5f3e2d42d31ce49488cef2c";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "island-sensor-cpp-demo-flowrt-app",
    cpp_app_stem: "island_sensor_cpp_demo_cpp_app",
    ros2_bridge_stem: "",
    package_name: "island_sensor_cpp_demo",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
