// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "5204e45d4beeea7b18dfc9aa617878b42c424daba3749ff1cbb9221fc18d308a";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "iox2-operation-cpp-flowrt-app",
    cpp_app_stem: "iox2_operation_cpp_cpp_app",
    ros2_bridge_stem: "",
    package_name: "iox2_operation_cpp",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
