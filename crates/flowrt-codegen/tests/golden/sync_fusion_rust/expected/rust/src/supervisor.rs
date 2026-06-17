// FlowRT 管理产物。不要手工修改。

const LAUNCH_MANIFEST_HASH: &str = "340c5b11ca568914fb9294d6404bd8521294f267bfc4fbf9357068859d08e566";
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

static SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {
    manifest_json: LAUNCH_MANIFEST,
    rust_app_stem: "sync-fusion-rust-demo-flowrt-app",
    cpp_app_stem: "sync_fusion_rust_demo_cpp_app",
    ros2_bridge_stem: "",
    package_name: "sync_fusion_rust_demo",
    self_description_hash: crate::selfdesc::self_description_hash,
};

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let _ = LAUNCH_MANIFEST_HASH;
    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)
}
