use flowrt_rsdl::RawDocument;

use crate::{EntityId, ImportIr, PackageIr};

use super::ids::entity_id;

pub(super) struct NormalizedPackage {
    pub id: EntityId,
    pub package: PackageIr,
}

pub(super) fn normalize_package(document: &RawDocument) -> NormalizedPackage {
    let qualified_name = format!(
        "{}@{}",
        document.package.name,
        document.package.version.as_deref().unwrap_or("0.0.0")
    );
    let id = entity_id("package", &qualified_name);

    let package = PackageIr {
        name: document.package.name.clone(),
        version: document.package.version.clone(),
        rsdl_version: document.package.rsdl_version.clone(),
        imports: document
            .package
            .imports
            .iter()
            .map(|(kind, patterns)| {
                let mut patterns = patterns.clone();
                patterns.sort();
                ImportIr {
                    kind: kind.clone(),
                    patterns,
                }
            })
            .collect(),
    };

    NormalizedPackage { id, package }
}
