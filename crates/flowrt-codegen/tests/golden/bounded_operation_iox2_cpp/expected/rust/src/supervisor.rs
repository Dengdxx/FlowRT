// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "76749d07a7bfdec8e87500666778a176d8e54d688250009c6c4f4d8c4df6ecc2";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "bounded-operation-iox2-cpp-flowrt-app",
    cpp_app_stem: "bounded_operation_iox2_cpp_cpp_app",
    ros2_bridge_stem: "",
    package_name: "bounded_operation_iox2_cpp",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
