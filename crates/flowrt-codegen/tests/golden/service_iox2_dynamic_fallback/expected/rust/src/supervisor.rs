// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "d1e2883a4425995de8db6ba3cc5558c34c67719b4df26b2fe9724bdbfc9d340f";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "service-iox2-dynamic-fallback-flowrt-app",
    cpp_app_stem: "service_iox2_dynamic_fallback_cpp_app",
    ros2_bridge_stem: "",
    package_name: "service_iox2_dynamic_fallback",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
