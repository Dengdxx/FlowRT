// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "d1609ab4e9cf0e6cad85eedff36b96e49c28461fde5b0e55f2352bc8b19b7463";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "bounded-channel-iox2-cpp-flowrt-app",
    cpp_app_stem: "bounded_channel_iox2_cpp_cpp_app",
    ros2_bridge_stem: "",
    package_name: "bounded_channel_iox2_cpp",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
