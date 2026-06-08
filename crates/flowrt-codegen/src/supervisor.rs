use flowrt_ir::ContractIr;
use sha2::{Digest, Sha256};

use crate::ros2_bridge::ros2_bridge_stem;
use crate::{contract_has_ros2_bridge, managed_header, rust_string_literal, sanitize_package_name};

pub(crate) fn emit_rust_supervisor_main() -> String {
    let mut output = managed_header();
    output.push_str(
        "\nfn main() {\n    let mut args = std::env::args().skip(1);\n    let mut run_ticks = None;\n    while let Some(arg) = args.next() {\n        match arg.as_str() {\n            \"--flowrt-run-ticks\" | \"--flowrt-run-steps\" => {\n                let Some(raw_ticks) = args.next() else {\n                    eprintln!(\"missing value for {arg}\");\n                    std::process::exit(2);\n                };\n                match raw_ticks.parse::<usize>() {\n                    Ok(ticks) if ticks > 0 => run_ticks = Some(ticks),\n                    _ => {\n                        eprintln!(\"invalid value for {arg}: {raw_ticks}\");\n                        std::process::exit(2);\n                    }\n                }\n            }\n            _ => {\n                eprintln!(\"unknown FlowRT supervisor argument: {arg}\");\n                std::process::exit(2);\n            }\n        }\n    }\n\n    match flowrt_app::supervisor::launch(run_ticks) {\n        Ok(()) => std::process::exit(0),\n        Err(error) => {\n            eprintln!(\"FlowRT supervisor failed: {error}\");\n            std::process::exit(1);\n        }\n    }\n}\n",
    );
    output
}

pub(crate) fn emit_rust_supervisor(contract: &ContractIr, launch_manifest: &str) -> String {
    let mut output = managed_header();
    let launch_manifest_hash = hex_sha256(launch_manifest);
    output.push_str(&format!(
        "\nconst LAUNCH_MANIFEST_HASH: &str = {};\nconst LAUNCH_MANIFEST: &str = include_str!(\"../../launch/launch.json\");\n\nstatic SUPERVISOR_CONFIG: flowrt::supervisor::SupervisorConfig = flowrt::supervisor::SupervisorConfig {{\n    manifest_json: LAUNCH_MANIFEST,\n    rust_app_stem: {},\n    cpp_app_stem: {},\n    ros2_bridge_stem: {},\n    package_name: {},\n    self_description_hash: crate::selfdesc::self_description_hash,\n}};\n\npub fn launch(run_ticks: Option<usize>) -> Result<(), String> {{\n    let _ = LAUNCH_MANIFEST_HASH;\n    flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)\n}}\n",
        rust_string_literal(&launch_manifest_hash),
        rust_string_literal(&rust_app_stem(contract)),
        rust_string_literal(&cpp_app_stem(contract)),
        rust_string_literal(&ros2_bridge_app_stem(contract)),
        rust_string_literal(&contract.package.name),
    ));
    output
}

fn hex_sha256(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn rust_app_stem(contract: &ContractIr) -> String {
    format!(
        "{}-flowrt-app",
        sanitize_package_name(&contract.package.name).replace('_', "-")
    )
}

fn cpp_app_stem(contract: &ContractIr) -> String {
    format!(
        "{}_cpp_app",
        sanitize_package_name(&contract.package.name).replace('-', "_")
    )
}

fn ros2_bridge_app_stem(contract: &ContractIr) -> String {
    if contract_has_ros2_bridge(contract) {
        ros2_bridge_stem(contract)
    } else {
        String::new()
    }
}
