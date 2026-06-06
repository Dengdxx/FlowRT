use flowrt_ir::{hash_source, normalize_document};
use flowrt_rsdl::parse_str;

use super::*;

mod artifacts;
mod backend;
mod channels;
mod introspection;
mod launch;
mod messages;
mod params;
mod ros2_bridge;
mod tasks;

fn contract_from_source(source: &str) -> ContractIr {
    let raw = parse_str(source).unwrap();
    normalize_document(&raw, hash_source(source)).unwrap()
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
