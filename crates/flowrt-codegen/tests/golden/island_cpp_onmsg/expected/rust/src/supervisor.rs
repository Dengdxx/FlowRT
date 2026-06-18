// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "608023f7dc969ff434b93a835462202b34a626c7afe0e1a96ac2bb8974c02939";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "island-cpp-demo-flowrt-app",
    cpp_app_stem: "island_cpp_demo_cpp_app",
    ros2_bridge_stem: "",
    package_name: "island_cpp_demo",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
