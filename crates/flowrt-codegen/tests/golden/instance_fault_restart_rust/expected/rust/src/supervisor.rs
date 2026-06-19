// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "12a898695bf0b185bced0f7bf631d0c1da55ee6b37a24a12bf1c33f39e7d89f4";
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
