use flowrt_rsdl::RawDocument;

use crate::{BackendName, LanguageKind, Result, TargetIr, target_capabilities};

use super::ids::entity_id;
use super::modules::parse_language;

pub(super) fn normalize_targets(document: &RawDocument) -> Result<Vec<TargetIr>> {
    document
        .targets
        .iter()
        .map(|(name, raw)| {
            let mut backends = raw
                .backends
                .iter()
                .cloned()
                .map(BackendName)
                .collect::<Vec<_>>();
            backends.sort();
            Ok(TargetIr {
                id: entity_id("target", name),
                name: name.clone(),
                platform: raw.platform.clone(),
                runtime: normalize_target_runtime(name, raw)?,
                capabilities: target_capabilities(&backends),
                backends,
            })
        })
        .collect()
}

fn normalize_target_runtime(
    target_name: &str,
    raw: &flowrt_rsdl::RawTarget,
) -> Result<Vec<LanguageKind>> {
    let mut runtime = raw
        .runtime
        .iter()
        .map(|language| parse_language(&format!("target.{target_name}.runtime"), language))
        .collect::<Result<Vec<_>>>()?;
    runtime.sort_by_key(|language| match language {
        LanguageKind::Cpp => 0,
        LanguageKind::Rust => 1,
        LanguageKind::External => 2,
    });
    Ok(runtime)
}
