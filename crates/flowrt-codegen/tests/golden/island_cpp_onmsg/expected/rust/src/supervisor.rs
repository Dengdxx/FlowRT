// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "4d51533b60a6785cb84fab3fe4f3643d2786b27595ffb907f7226f41291b616d";
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
