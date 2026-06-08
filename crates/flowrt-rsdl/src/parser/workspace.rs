use std::path::{Path, PathBuf};

use crate::ast::*;
use crate::{Result, RsdlError};

use super::ParsedDocument;
use super::imports::{
    canonicalize_existing, expand_import_pattern, load_import_document, logical_source_path,
    merge_named_map,
};

pub(super) fn expand_workspace(
    document: &mut RawDocument,
    root_path: &Path,
    package_root: &Path,
    loaded_paths: &mut std::collections::BTreeSet<PathBuf>,
    sources: &mut Vec<LoadedSource>,
    modules: &mut Vec<RawModuleDocument>,
    compositions: &mut Vec<RawCompositionDocument>,
) -> Result<()> {
    let workspace = document
        .workspace
        .clone()
        .expect("caller checked workspace presence");
    let mut module_names = std::collections::BTreeSet::new();

    for pattern in &workspace.modules {
        for path in expand_import_pattern(root_path, pattern)? {
            let path = canonicalize_existing(&path)?;
            if !loaded_paths.insert(path.clone()) {
                continue;
            }
            let parsed = load_import_document(&path, package_root, sources)?;
            let module = parsed
                .module
                .clone()
                .ok_or_else(|| RsdlError::MissingModule {
                    path: logical_source_path(&path, package_root),
                })?;
            validate_module_document(&path, package_root, &module, &parsed)?;
            if !module_names.insert(module.name.clone()) {
                return Err(RsdlError::DuplicateModule {
                    module: module.name,
                });
            }
            modules.push(RawModuleDocument {
                module,
                types: parsed.types,
                components: parsed.components,
                source: logical_source_path(&path, package_root),
            });
        }
    }

    for pattern in &workspace.compositions {
        for path in expand_import_pattern(root_path, pattern)? {
            let path = canonicalize_existing(&path)?;
            if !loaded_paths.insert(path.clone()) {
                continue;
            }
            let parsed = load_import_document(&path, package_root, sources)?;
            if parsed.module.is_some() {
                return Err(RsdlError::UnexpectedModule {
                    path: logical_source_path(&path, package_root),
                });
            }
            let composition = RawCompositionDocument {
                instances: parsed.instances.clone(),
                processes: parsed.processes.clone(),
                external_processes: parsed.external_processes.clone(),
                binds: parsed.binds.clone(),
                service_binds: parsed.service_binds.clone(),
                operation_binds: parsed.operation_binds.clone(),
                ros2_bridges: parsed.ros2_bridges.clone(),
                profiles: parsed.profiles.clone(),
                targets: parsed.targets.clone(),
                source: logical_source_path(&path, package_root),
            };
            merge_composition_document(document, parsed)?;
            compositions.push(composition);
        }
    }

    modules.sort_by(|left, right| left.module.name.cmp(&right.module.name));
    compositions.sort_by(|left, right| left.source.cmp(&right.source));
    Ok(())
}

fn validate_module_document(
    path: &Path,
    package_root: &Path,
    module: &RawModule,
    parsed: &ParsedDocument,
) -> Result<()> {
    let invalid = [
        (!parsed.instances.is_empty(), "instance"),
        (!parsed.processes.is_empty(), "process"),
        (!parsed.external_processes.is_empty(), "external_process"),
        (!parsed.binds.is_empty(), "bind"),
        (!parsed.service_binds.is_empty(), "bind.service"),
        (!parsed.operation_binds.is_empty(), "bind.operation"),
        (!parsed.ros2_bridges.is_empty(), "bridge"),
        (!parsed.profiles.is_empty(), "profile"),
        (!parsed.targets.is_empty(), "target"),
        (parsed.workspace.is_some(), "workspace"),
        (parsed.package.is_some(), "package"),
    ]
    .into_iter()
    .find_map(|(present, section)| present.then_some(section));

    if let Some(section) = invalid {
        return Err(RsdlError::InvalidModuleSection {
            path: logical_source_path(path, package_root),
            module: module.name.clone(),
            section: section.to_string(),
        });
    }

    Ok(())
}

fn merge_composition_document(
    document: &mut RawDocument,
    composition: ParsedDocument,
) -> Result<()> {
    merge_named_map("instance", &mut document.instances, composition.instances)?;
    document.processes.extend(composition.processes);
    document
        .external_processes
        .extend(composition.external_processes);
    document.binds.extend(composition.binds);
    document.service_binds.extend(composition.service_binds);
    document.operation_binds.extend(composition.operation_binds);
    document.ros2_bridges.extend(composition.ros2_bridges);
    merge_named_map("profile", &mut document.profiles, composition.profiles)?;
    merge_named_map("target", &mut document.targets, composition.targets)?;
    Ok(())
}
