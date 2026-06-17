// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "7cd79f0fc775acdb9ae71d33a880e6363bd010e53f7fa7852f7122851f275f65";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "instance-degrade-demo-flowrt-app",
    cpp_app_stem: "instance_degrade_demo_cpp_app",
    ros2_bridge_stem: "",
    package_name: "instance_degrade_demo",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
