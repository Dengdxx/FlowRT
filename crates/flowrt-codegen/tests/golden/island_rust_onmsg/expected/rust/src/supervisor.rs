// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "a639762e5d52db7541ba4a1668515ae41044e106c1faee5b7805f91afcc0aa8c";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "island-rust-demo-flowrt-app",
    cpp_app_stem: "island_rust_demo_cpp_app",
    ros2_bridge_stem: "",
    package_name: "island_rust_demo",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
