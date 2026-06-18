use toml::value::Table;

use crate::{Result, RsdlError};

pub(super) fn validate_top_level_sections(root: &Table) -> Result<()> {
    const ALLOWED_SECTIONS: &[&str] = &[
        "package",
        "workspace",
        "module",
        "type",
        "component",
        "instance",
        "graph",
        "process",
        "external_process",
        "resource",
        "bind",
        "bridge",
        "boundary",
        "sync",
        "redundancy",
        "profile",
        "target",
    ];

    for section in root.keys() {
        if !ALLOWED_SECTIONS.contains(&section.as_str()) {
            return Err(RsdlError::UnknownTopLevelSection {
                section: section.clone(),
            });
        }
    }
    Ok(())
}

pub(super) fn validate_known_fields(
    table: &Table,
    context: &str,
    allowed_fields: &[&str],
) -> Result<()> {
    for field in table.keys() {
        if !allowed_fields.contains(&field.as_str()) {
            return Err(RsdlError::UnknownField {
                context: context.to_string(),
                field: field.clone(),
            });
        }
    }
    Ok(())
}
