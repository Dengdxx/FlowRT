use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use super::names::pascal_case;
use super::types::{PortSpec, cpp_type, named_rust_message_imports, parse_type, rust_type};

pub(super) fn merge_rust_component_skeleton(
    app_root: &Path,
    component_name: &str,
    inputs: &[PortSpec],
    outputs: &[PortSpec],
) -> Result<()> {
    let path = app_root.join("app/rust/mod.rs");
    let content = if path.exists() {
        fs::read_to_string(&path).with_context(|| format!("failed to read `{}`", path.display()))?
    } else {
        String::new()
    };
    let component_ty = pascal_case(component_name);
    let impl_ty = format!("{component_ty}Impl");
    if content.contains(&format!("pub struct {impl_ty}"))
        || content.contains(&format!("impl {component_ty} for {impl_ty}"))
    {
        anyhow::bail!(
            "user file `{}` already contains component skeleton `{impl_ty}`",
            path.display()
        );
    }

    let mut updated = ensure_rust_import(&content, "crate::components", &component_ty);
    for ty in named_rust_message_imports(inputs, outputs)? {
        updated = ensure_rust_import(&updated, "crate::messages", &ty);
    }
    if !updated.ends_with('\n') && !updated.is_empty() {
        updated.push('\n');
    }
    updated.push_str(&rust_component_skeleton(component_name, inputs, outputs)?);
    updated = merge_rust_build_app(&updated, &format!("Box::new({impl_ty}::default())"))?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create `{}`", parent.display()))?;
    }
    fs::write(&path, updated).with_context(|| format!("failed to write `{}`", path.display()))
}

fn ensure_rust_import(content: &str, module: &str, item: &str) -> String {
    let line = format!("use {module}::{item};\n");
    if content.contains(&line) {
        return content.to_string();
    }
    let mut output = String::with_capacity(content.len() + line.len());
    output.push_str(&line);
    output.push_str(content);
    output
}

fn rust_component_skeleton(
    component_name: &str,
    inputs: &[PortSpec],
    outputs: &[PortSpec],
) -> Result<String> {
    let component_ty = pascal_case(component_name);
    let impl_ty = format!("{component_ty}Impl");
    let args = rust_callback_args(inputs, outputs)?;
    let body = rust_on_tick_body(inputs, outputs)?;
    if args.is_empty() {
        Ok(format!(
            "\n#[derive(Default)]\npub struct {impl_ty};\n\nimpl {component_ty} for {impl_ty} {{\n    fn on_tick(&mut self) -> flowrt::Status {{\n{body}    }}\n}}\n"
        ))
    } else {
        Ok(format!(
            "\n#[derive(Default)]\npub struct {impl_ty};\n\nimpl {component_ty} for {impl_ty} {{\n    fn on_tick(&mut self, {args}) -> flowrt::Status {{\n{body}    }}\n}}\n"
        ))
    }
}

fn rust_callback_args(inputs: &[PortSpec], outputs: &[PortSpec]) -> Result<String> {
    let mut args = Vec::new();
    for input in inputs {
        args.push(format!(
            "{}: flowrt::Latest<'_, {}>",
            input.name,
            rust_type(&parse_type(&input.ty)?)
        ));
    }
    for output in outputs {
        args.push(format!(
            "{}: &mut flowrt::Output<{}>",
            output.name,
            rust_type(&parse_type(&output.ty)?)
        ));
    }
    Ok(args.join(", "))
}

fn rust_on_tick_body(inputs: &[PortSpec], outputs: &[PortSpec]) -> Result<String> {
    let mut body = String::new();
    for input in inputs {
        body.push_str(&format!("        let _ = {};\n", input.name));
    }
    for output in outputs {
        body.push_str(&format!(
            "        {}.write({}::default());\n",
            output.name,
            rust_type(&parse_type(&output.ty)?)
        ));
    }
    body.push_str("        flowrt::Status::Ok\n");
    Ok(body)
}

fn merge_rust_build_app(content: &str, new_arg: &str) -> Result<String> {
    if content.contains(new_arg) {
        return Ok(content.to_string());
    }
    let Some(start) = content.find("crate::App::new(") else {
        anyhow::bail!("user Rust file has no `crate::App::new(...)` call to merge safely");
    };
    let open = start + "crate::App::new".len();
    let close = matching_paren(content, open)
        .context("user Rust file has malformed `crate::App::new(...)` call")?;
    let existing = content[open + 1..close].trim().trim_end_matches(',');
    let replacement = if existing.is_empty() {
        format!("crate::App::new({new_arg})")
    } else {
        format!("crate::App::new(\n        {existing},\n        {new_arg},\n    )")
    };
    Ok(format!(
        "{}{}{}",
        &content[..start],
        replacement,
        &content[close + 1..]
    ))
}

pub(super) fn merge_cpp_component_skeleton(
    app_root: &Path,
    component_name: &str,
    inputs: &[PortSpec],
    outputs: &[PortSpec],
) -> Result<()> {
    let path = app_root.join("app/cpp/components.cpp");
    let content = if path.exists() {
        fs::read_to_string(&path).with_context(|| format!("failed to read `{}`", path.display()))?
    } else {
        r#"#include "flowrt_app/runtime_shell.hpp"

#include <memory>

namespace {

}  // namespace

namespace flowrt_user {

flowrt_app::App build_app() {
    return flowrt_app::App();
}

}  // namespace flowrt_user
"#
        .to_string()
    };
    let component_ty = pascal_case(component_name);
    if content.contains(&format!("class {component_ty} final"))
        || content.contains(&format!("std::make_unique<{component_ty}>()"))
    {
        anyhow::bail!(
            "user file `{}` already contains component skeleton `{component_ty}`",
            path.display()
        );
    }

    let mut updated = insert_before_namespace_close(
        &content,
        "\n}  // namespace\n",
        &cpp_component_skeleton(component_name, inputs, outputs)?,
    )?;
    updated = merge_cpp_build_app(&updated, &format!("std::make_unique<{component_ty}>()"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create `{}`", parent.display()))?;
    }
    fs::write(&path, updated).with_context(|| format!("failed to write `{}`", path.display()))
}

pub(super) fn merge_c_component_skeleton(
    app_root: &Path,
    component_name: &str,
    inputs: &[PortSpec],
    outputs: &[PortSpec],
) -> Result<()> {
    let path = app_root.join("app/c").join(format!("{component_name}.c"));
    if path.exists() {
        anyhow::bail!("refusing to overwrite existing file `{}`", path.display());
    }
    let content = c_component_skeleton(component_name, inputs, outputs);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create `{}`", parent.display()))?;
    }
    write_new_file(&path, &content)
}

fn c_component_skeleton(
    component_name: &str,
    _inputs: &[PortSpec],
    outputs: &[PortSpec],
) -> String {
    let run_periodic = format!("{component_name}_run_periodic");
    let callback_factory = format!("flowrt_app_{component_name}_callbacks");
    let inputs_line = "    (void)inputs;\n";
    let output_body = if outputs.is_empty() {
        "    (void)outputs;\n".to_string()
    } else {
        r#"    if (outputs == NULL || (outputs->len > 0U && outputs->data == NULL)) {
        return FLOWRT_STATUS_ERROR;
    }
    for (size_t index = 0U; index < outputs->len; ++index) {
        flowrt_c_output_slot_t *slot = &outputs->data[index];
        if (slot->data == NULL || slot->capacity < slot->size_bytes) {
            return FLOWRT_STATUS_ERROR;
        }
        memset(slot->data, 0, slot->size_bytes);
        slot->written_len = slot->size_bytes;
        slot->status = FLOWRT_C_OUTPUT_WRITTEN;
    }
"#
        .to_string()
    };

    format!(
        r#"#include "flowrt_app/c_components.h"

#include <stddef.h>
#include <stdint.h>
#include <string.h>

#ifndef FLOWRT_ABI_FEATURE_C_COMPONENT_CALLBACKS_V0
#error "FlowRT C component callback ABI v0 is required"
#endif

static flowrt_status_t {run_periodic}(void *user_data,
                                      const flowrt_c_component_context_t *context,
                                      const flowrt_c_input_array_view_t *inputs,
                                      flowrt_c_output_array_view_t *outputs) {{
    (void)user_data;
    (void)context;
{inputs_line}{output_body}    return FLOWRT_STATUS_OK;
}}

const flowrt_c_component_callback_table_t *{callback_factory}(void) {{
    static const flowrt_c_component_callback_table_t callbacks = {{
        .size = (uint32_t)sizeof(flowrt_c_component_callback_table_t),
        .version_major = FLOWRT_C_COMPONENT_CALLBACK_ABI_VERSION_MAJOR,
        .version_minor = FLOWRT_C_COMPONENT_CALLBACK_ABI_VERSION_MINOR,
        .reserved0 = 0U,
        .feature_flags = FLOWRT_ABI_FEATURE_C_COMPONENT_CALLBACKS_V0,
        .user_data = NULL,
        .on_init = NULL,
        .on_start = NULL,
        .on_stop = NULL,
        .on_shutdown = NULL,
        .run_periodic = {run_periodic},
        .run_on_message = NULL,
        .run_startup = NULL,
        .run_shutdown = NULL,
        .reserved = {{0U}},
    }};
    return &callbacks;
}}
"#
    )
}

fn write_new_file(path: &Path, content: &str) -> Result<()> {
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => {
            use std::io::Write;
            file.write_all(content.as_bytes())
                .with_context(|| format!("failed to write `{}`", path.display()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            anyhow::bail!("refusing to overwrite existing file `{}`", path.display())
        }
        Err(error) => Err(error).with_context(|| format!("failed to create `{}`", path.display())),
    }
}

fn insert_before_namespace_close(content: &str, marker: &str, insertion: &str) -> Result<String> {
    let Some(index) = content.find(marker) else {
        anyhow::bail!("user C++ file has no anonymous namespace close marker to merge safely");
    };
    Ok(format!(
        "{}{}{}",
        &content[..index],
        insertion,
        &content[index..]
    ))
}

fn cpp_component_skeleton(
    component_name: &str,
    inputs: &[PortSpec],
    outputs: &[PortSpec],
) -> Result<String> {
    let component_ty = pascal_case(component_name);
    let args = cpp_callback_args(inputs, outputs)?;
    let body = cpp_on_tick_body(inputs, outputs)?;
    if args.is_empty() {
        Ok(format!(
            "\nclass {component_ty} final : public flowrt_app::{component_ty}Interface {{\npublic:\n    flowrt::Status on_tick() override {{\n{body}    }}\n}};\n"
        ))
    } else {
        Ok(format!(
            "\nclass {component_ty} final : public flowrt_app::{component_ty}Interface {{\npublic:\n    flowrt::Status on_tick({args}) override {{\n{body}    }}\n}};\n"
        ))
    }
}

fn cpp_callback_args(inputs: &[PortSpec], outputs: &[PortSpec]) -> Result<String> {
    let mut args = Vec::new();
    for input in inputs {
        args.push(format!(
            "const flowrt::Latest<{}>& {}",
            cpp_type(&parse_type(&input.ty)?),
            input.name
        ));
    }
    for output in outputs {
        args.push(format!(
            "flowrt::Output<{}>& {}",
            cpp_type(&parse_type(&output.ty)?),
            output.name
        ));
    }
    Ok(args.join(", "))
}

fn cpp_on_tick_body(inputs: &[PortSpec], outputs: &[PortSpec]) -> Result<String> {
    let mut body = String::new();
    for input in inputs {
        body.push_str(&format!("        (void){};\n", input.name));
    }
    for output in outputs {
        body.push_str(&format!(
            "        {}.write({}{{}});\n",
            output.name,
            cpp_type(&parse_type(&output.ty)?)
        ));
    }
    body.push_str("        return flowrt::Status::Ok;\n");
    Ok(body)
}

fn merge_cpp_build_app(content: &str, new_arg: &str) -> Result<String> {
    if content.contains(new_arg) {
        return Ok(content.to_string());
    }
    let Some(start) = content.find("flowrt_app::App(") else {
        anyhow::bail!("user C++ file has no `flowrt_app::App(...)` call to merge safely");
    };
    let open = start + "flowrt_app::App".len();
    let close = matching_paren(content, open)
        .context("user C++ file has malformed `flowrt_app::App(...)` call")?;
    let existing = content[open + 1..close].trim().trim_end_matches(',');
    let replacement = if existing.is_empty() {
        format!("flowrt_app::App({new_arg})")
    } else {
        format!("flowrt_app::App(\n        {existing},\n        {new_arg},\n    )")
    };
    Ok(format!(
        "{}{}{}",
        &content[..start],
        replacement,
        &content[close + 1..]
    ))
}

fn matching_paren(content: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, ch) in content[open..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
}
