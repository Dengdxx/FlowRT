use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use crate::ast::*;
use crate::{Result, RsdlError};

use super::ParsedDocument;

pub(super) fn read_source(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|source| RsdlError::Io {
        path: path.to_path_buf(),
        source,
    })
}

pub(super) fn load_import_document(
    path: &Path,
    package_root: &Path,
    sources: &mut Vec<LoadedSource>,
) -> Result<ParsedDocument> {
    let source = read_source(path)?;
    sources.push(LoadedSource {
        path: logical_source_path(path, package_root),
        content: source.clone(),
    });
    super::parse_source(&source, false)
}

pub(super) fn expand_imports(
    document: &mut RawDocument,
    importer: &Path,
    package_root: &Path,
    loaded_paths: &mut std::collections::BTreeSet<PathBuf>,
    sources: &mut Vec<LoadedSource>,
) -> Result<()> {
    let imports = document.package.imports.clone();
    for pattern in imports.values().flatten() {
        let matches = expand_import_pattern(importer, pattern)?;
        for path in matches {
            let path = canonicalize_existing(&path)?;
            if !loaded_paths.insert(path.clone()) {
                continue;
            }

            let imported = load_import_document(&path, package_root, sources)?;
            let nested_imports = imported
                .package
                .as_ref()
                .map(|package| package.imports.clone())
                .unwrap_or_default();
            merge_imported_document(document, imported)?;
            expand_nested_imports(
                document,
                &path,
                package_root,
                nested_imports,
                loaded_paths,
                sources,
            )?;
        }
    }
    Ok(())
}

fn expand_nested_imports(
    document: &mut RawDocument,
    importer: &Path,
    package_root: &Path,
    imports: BTreeMap<String, Vec<String>>,
    loaded_paths: &mut std::collections::BTreeSet<PathBuf>,
    sources: &mut Vec<LoadedSource>,
) -> Result<()> {
    for pattern in imports.values().flatten() {
        let matches = expand_import_pattern(importer, pattern)?;
        for path in matches {
            let path = canonicalize_existing(&path)?;
            if !loaded_paths.insert(path.clone()) {
                continue;
            }
            let imported = load_import_document(&path, package_root, sources)?;
            let nested_imports = imported
                .package
                .as_ref()
                .map(|package| package.imports.clone())
                .unwrap_or_default();
            merge_imported_document(document, imported)?;
            expand_nested_imports(
                document,
                &path,
                package_root,
                nested_imports,
                loaded_paths,
                sources,
            )?;
        }
    }
    Ok(())
}

pub(super) fn merge_imported_document(
    document: &mut RawDocument,
    imported: ParsedDocument,
) -> Result<()> {
    merge_named_map("type", &mut document.types, imported.types)?;
    merge_named_map("component", &mut document.components, imported.components)?;
    merge_named_map("instance", &mut document.instances, imported.instances)?;
    document.processes.extend(imported.processes);
    document
        .external_processes
        .extend(imported.external_processes);
    document.binds.extend(imported.binds);
    document.service_binds.extend(imported.service_binds);
    document.operation_binds.extend(imported.operation_binds);
    document.ros2_bridges.extend(imported.ros2_bridges);
    document.sync_groups.extend(imported.sync_groups);
    merge_named_vec(
        "boundary.input",
        &mut document.boundary_inputs,
        imported.boundary_inputs,
        |endpoint| &endpoint.name,
    )?;
    merge_named_vec(
        "boundary.output",
        &mut document.boundary_outputs,
        imported.boundary_outputs,
        |endpoint| &endpoint.name,
    )?;
    merge_named_map("profile", &mut document.profiles, imported.profiles)?;
    merge_named_map("target", &mut document.targets, imported.targets)?;
    Ok(())
}

pub(super) fn merge_named_map<T>(
    kind: &'static str,
    target: &mut BTreeMap<String, T>,
    imported: BTreeMap<String, T>,
) -> Result<()> {
    for (name, value) in imported {
        if target.contains_key(&name) {
            return Err(RsdlError::DuplicateSymbol { kind, name });
        }
        target.insert(name, value);
    }
    Ok(())
}

pub(super) fn merge_named_vec<T>(
    kind: &'static str,
    target: &mut Vec<T>,
    imported: Vec<T>,
    name_of: impl Fn(&T) -> &str,
) -> Result<()> {
    let mut names = target
        .iter()
        .map(|item| name_of(item).to_string())
        .collect::<std::collections::BTreeSet<_>>();
    for item in imported {
        let name = name_of(&item).to_string();
        if !names.insert(name.clone()) {
            return Err(RsdlError::DuplicateSymbol { kind, name });
        }
        target.push(item);
    }
    Ok(())
}

pub(super) fn expand_import_pattern(importer: &Path, pattern: &str) -> Result<Vec<PathBuf>> {
    let importer_dir = importer.parent().unwrap_or_else(|| Path::new("."));
    validate_relative_import_path(importer, pattern)?;
    let components = pattern.split('/').collect::<Vec<_>>();
    let mut matches = Vec::new();
    expand_import_components(importer_dir, importer, pattern, &components, &mut matches)?;
    matches.sort();
    matches.dedup();
    if matches.is_empty() {
        return Err(RsdlError::ImportPatternNoMatches {
            importer: importer.to_path_buf(),
            pattern: pattern.to_string(),
        });
    }
    Ok(matches)
}

fn expand_import_components(
    base: &Path,
    importer: &Path,
    pattern: &str,
    components: &[&str],
    matches: &mut Vec<PathBuf>,
) -> Result<()> {
    if !base.exists() {
        return Ok(());
    }

    let Some((component, rest)) = components.split_first() else {
        if base.extension() == Some(std::ffi::OsStr::new("rsdl")) {
            matches.push(base.to_path_buf());
        }
        return Ok(());
    };

    if component.contains('*') {
        let mut entries = std::fs::read_dir(base)
            .map_err(|source| RsdlError::Io {
                path: base.to_path_buf(),
                source,
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|source| RsdlError::Io {
                path: base.to_path_buf(),
                source,
            })?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if wildcard_match(component, &name) {
                expand_import_components(&entry.path(), importer, pattern, rest, matches)?;
            }
        }
        return Ok(());
    }

    let next = base.join(component);
    if rest.is_empty() && !next.exists() {
        return Err(RsdlError::ImportPatternNoMatches {
            importer: importer.to_path_buf(),
            pattern: pattern.to_string(),
        });
    }
    expand_import_components(&next, importer, pattern, rest, matches)
}

fn validate_relative_import_path(importer: &Path, pattern: &str) -> Result<()> {
    let path = Path::new(pattern);
    if path.is_absolute() {
        return Err(RsdlError::InvalidImportPath {
            importer: importer.to_path_buf(),
            pattern: pattern.to_string(),
            message: "absolute paths are not allowed".to_string(),
        });
    }
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            _ => {
                return Err(RsdlError::InvalidImportPath {
                    importer: importer.to_path_buf(),
                    pattern: pattern.to_string(),
                    message: "only normal relative path components are allowed".to_string(),
                });
            }
        }
    }
    Ok(())
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    let parts = pattern.split('*').collect::<Vec<_>>();
    if parts.len() == 1 {
        return pattern == value;
    }

    let mut rest = value;
    if let Some(first) = parts.first()
        && !first.is_empty()
    {
        let Some(next) = rest.strip_prefix(first) else {
            return false;
        };
        rest = next;
    }

    for part in parts.iter().skip(1).take(parts.len().saturating_sub(2)) {
        if part.is_empty() {
            continue;
        }
        let Some(index) = rest.find(part) else {
            return false;
        };
        rest = &rest[index + part.len()..];
    }

    if let Some(last) = parts.last()
        && !last.is_empty()
    {
        return rest.ends_with(last);
    }
    true
}

pub(super) fn canonicalize_existing(path: &Path) -> Result<PathBuf> {
    std::fs::canonicalize(path).map_err(|source| RsdlError::Io {
        path: path.to_path_buf(),
        source,
    })
}

pub(super) fn logical_source_path(path: &Path, package_root: &Path) -> PathBuf {
    path.strip_prefix(package_root)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| {
            path.file_name()
                .map(PathBuf::from)
                .unwrap_or_else(|| path.to_path_buf())
        })
}
