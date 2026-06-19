// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "76b98d5525a3d75d79a679420590827901e63d5081c3add2d92ecca1f036c1ff";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "service-demo-flowrt-app",
    cpp_app_stem: "service_demo_cpp_app",
    ros2_bridge_stem: "",
    package_name: "service_demo",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
