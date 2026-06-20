// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "2a909e3a12f7eb02d19c62d66183f87896a3152bcb2d48271a06878f71cd9e2a";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "bounded-channel-iox2-rust-flowrt-app",
    cpp_app_stem: "bounded_channel_iox2_rust_cpp_app",
    ros2_bridge_stem: "",
    package_name: "bounded_channel_iox2_rust",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
