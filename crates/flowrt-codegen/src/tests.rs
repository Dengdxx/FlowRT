use flowrt_ir::{hash_source, normalize_document, normalize_loaded_document};
use flowrt_rsdl::parse_str;

use super::*;

mod artifacts;
mod backend;
mod channels;
mod introspection;
mod launch;
mod messages;
mod operation;
mod params;
mod ros2_bridge;
mod services;
mod tasks;

fn contract_from_source(source: &str) -> ContractIr {
    let raw = parse_str(source).unwrap();
    normalize_document(&raw, hash_source(source)).unwrap()
}

fn contract_from_file(path: &std::path::Path) -> ContractIr {
    let loaded = flowrt_rsdl::load_file(path).unwrap();
    normalize_loaded_document(&loaded, hash_source(&loaded.source_bundle_text())).unwrap()
}

fn unique_temp_dir() -> std::path::PathBuf {
    let suffix = format!(
        "flowrt-codegen-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    std::env::temp_dir().join(suffix)
}

fn artifact_content<'a>(bundle: &'a ArtifactBundle, path: &str) -> &'a str {
    bundle
        .artifacts
        .iter()
        .find(|artifact| artifact.relative_path.as_path() == std::path::Path::new(path))
        .map(|artifact| artifact.content.as_str())
        .unwrap()
}

fn generated_function_block<'a>(source: &'a str, function: &str) -> &'a str {
    let start = source
        .find(function)
        .expect("generated function must exist");
    let rest = &source[start..];
    let next = ["\n    fn ", "\nflowrt::Status App::"]
        .iter()
        .filter_map(|marker| {
            rest[function.len()..]
                .find(marker)
                .map(|offset| function.len() + offset)
        })
        .min()
        .unwrap_or(rest.len());
    &rest[..next]
}

fn extract_probe_field_for_registration<'a>(
    source: &'a str,
    registration_marker: &str,
) -> Option<&'a str> {
    let marker_at = source.find(registration_marker)?;
    let before = &source[..marker_at];
    before
        .rsplit_once(|ch: char| ch.is_whitespace())
        .map(|(_, probe)| probe.trim())
        .filter(|probe| probe.starts_with("introspection_probe_bind_"))
        .or_else(|| {
            before
                .rsplit_once("self.")
                .map(|(_, probe)| probe.trim())
                .filter(|probe| probe.starts_with("introspection_probe_bind_"))
        })
        .or_else(|| {
            before
                .rsplit_once("this->")
                .map(|(_, probe)| probe.trim())
                .filter(|probe| probe.starts_with("introspection_probe_bind_"))
        })
}

#[test]
fn float_literal_rejects_non_finite_values() {
    assert_eq!(float_literal(1.0), "1.0");
    assert!(std::panic::catch_unwind(|| float_literal(f64::NAN)).is_err());
    assert!(std::panic::catch_unwind(|| float_literal(f64::INFINITY)).is_err());
}
