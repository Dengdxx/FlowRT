// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "c5c5711284b0fcac54eec896832f6986b6fd6a4078035b9fa39771eb1c1c1dcd";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "instance-fault-cpp-demo-flowrt-app",
    cpp_app_stem: "instance_fault_cpp_demo_cpp_app",
    ros2_bridge_stem: "",
    package_name: "instance_fault_cpp_demo",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
