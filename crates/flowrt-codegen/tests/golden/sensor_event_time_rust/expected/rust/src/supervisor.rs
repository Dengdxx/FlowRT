// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "aa5766828caa36034edd006507f8cb5a8f097462cc87a90853b1a8ef1fa10b52";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "island-sensor-rust-demo-flowrt-app",
    cpp_app_stem: "island_sensor_rust_demo_cpp_app",
    ros2_bridge_stem: "",
    package_name: "island_sensor_rust_demo",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
