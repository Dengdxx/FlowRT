// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "18d540b3802f5d645822861ea20c57230761bdadd4892156b61a3ec13ed8a005";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "instance-fault-demo-flowrt-app",
    cpp_app_stem: "instance_fault_demo_cpp_app",
    ros2_bridge_stem: "",
    package_name: "instance_fault_demo",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
