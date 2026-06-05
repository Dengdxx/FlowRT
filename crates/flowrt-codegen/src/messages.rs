use std::collections::{BTreeMap, BTreeSet};

use flowrt_conformance::{MessageAbiExpectation, message_abi_expectations};
use flowrt_ir::{ContractIr, FieldIr, LanguageKind, PrimitiveType, TypeExpr, TypeIr};

use crate::{
    Result, has_language, managed_header, rust_string_literal, selected_backend_name,
    snake_identifier, type_by_name,
};

fn ordered_types(contract: &ContractIr) -> Vec<&flowrt_ir::TypeIr> {
    let type_map = contract
        .types
        .iter()
        .map(|ty| (ty.name.as_str(), ty))
        .collect::<BTreeMap<_, _>>();
    let mut visited = BTreeSet::new();
    let mut visiting = BTreeSet::new();
    let mut order = Vec::with_capacity(contract.types.len());

    for ty in &contract.types {
        visit_type(ty, &type_map, &mut visited, &mut visiting, &mut order);
    }

    order
}

fn visit_type<'a>(
    ty: &'a flowrt_ir::TypeIr,
    type_map: &BTreeMap<&str, &'a flowrt_ir::TypeIr>,
    visited: &mut BTreeSet<String>,
    visiting: &mut BTreeSet<String>,
    order: &mut Vec<&'a flowrt_ir::TypeIr>,
) {
    if visited.contains(&ty.name) {
        return;
    }
    if !visiting.insert(ty.name.clone()) {
        panic!("validated contract must not contain recursive message types");
    }

    let mut deps = BTreeSet::new();
    for field in &ty.fields {
        collect_type_dependencies(&field.ty, &mut deps);
    }
    for dep in deps {
        if let Some(next) = type_map.get(dep.as_str()) {
            visit_type(next, type_map, visited, visiting, order);
        }
    }

    visiting.remove(&ty.name);
    visited.insert(ty.name.clone());
    order.push(ty);
}

fn collect_type_dependencies(expr: &TypeExpr, dependencies: &mut BTreeSet<String>) {
    match expr {
        TypeExpr::Primitive { .. } => {}
        TypeExpr::Named { name } => {
            dependencies.insert(name.clone());
        }
        TypeExpr::Array { element, .. } => collect_type_dependencies(element, dependencies),
        TypeExpr::VarBytes { .. } | TypeExpr::VarString { .. } => {}
        TypeExpr::VarSequence { element, .. } => collect_type_dependencies(element, dependencies),
    }
}

pub(crate) fn emit_cpp_messages(contract: &ContractIr) -> String {
    let mut output = managed_header();
    output.push_str("#pragma once\n\n");
    output.push_str(
        "#include <algorithm>\n#include <array>\n#include <cstddef>\n#include <cstdint>\n#include <limits>\n#include <span>\n#include <vector>\n\n#include <flowrt/runtime.hpp>\n\n",
    );
    output.push_str("namespace flowrt_app {\n\n");
    let needs_iox2_type_name = selected_backend_name(contract) == "iox2";
    let needs_wire_codec =
        selected_backend_name(contract) == "zenoh" || contract_has_variable_messages(contract);
    for ty in ordered_types(contract) {
        let variable_message = type_contains_variable_data(
            contract,
            &TypeExpr::Named {
                name: ty.name.clone(),
            },
        );
        output.push_str(&format!("struct {} {{\n", ty.name));
        if needs_iox2_type_name && !variable_message {
            output.push_str(&format!(
                "    static constexpr const char* IOX2_TYPE_NAME = \"{}\";\n",
                ty.name
            ));
        }
        for field in &ty.fields {
            output.push_str(&format!(
                "    {} {}{{}};\n",
                cpp_type(&field.ty),
                field.name
            ));
        }
        if variable_message {
            output.push_str(&cpp_frame_codec_methods(contract, ty));
        } else if needs_wire_codec {
            output.push_str(&cpp_wire_codec_methods(contract, ty));
        }
        output.push_str("};\n\n");
        if needs_iox2_type_name && variable_message {
            output.push_str(&cpp_iox2_frame_slot_type(ty));
        }
    }
    output.push_str("}  // namespace flowrt_app\n");
    output
}

pub(crate) fn emit_rust_messages(contract: &ContractIr) -> String {
    let mut output = managed_header();
    output.push('\n');
    let needs_iox2_type_name = selected_backend_name(contract) == "iox2";
    let needs_wire_codec =
        selected_backend_name(contract) == "zenoh" || contract_has_variable_messages(contract);
    let zero_copy_derive = if needs_iox2_type_name {
        output.push_str("use flowrt::ZeroCopySend;\n\n");
        ", flowrt::ZeroCopySend"
    } else {
        ""
    };
    for ty in ordered_types(contract) {
        let variable_message = type_contains_variable_data(
            contract,
            &TypeExpr::Named {
                name: ty.name.clone(),
            },
        );
        if !variable_message {
            output.push_str("#[repr(C)]\n");
        }
        let copy_derive = if variable_message { "" } else { ", Copy" };
        let zero_copy_derive = if variable_message {
            ""
        } else {
            zero_copy_derive
        };
        output.push_str(&format!(
            "#[derive(Clone{copy_derive}, Debug, PartialEq{zero_copy_derive})]\n"
        ));
        if needs_iox2_type_name && !variable_message {
            output.push_str(&format!(
                "#[type_name({})]\n",
                rust_string_literal(&ty.name)
            ));
        }
        output.push_str(&format!("pub struct {} {{\n", ty.name));
        for field in &ty.fields {
            output.push_str(&format!(
                "    pub {}: {},\n",
                field.name,
                rust_type(&field.ty)
            ));
        }
        output.push_str("}\n\n");
        output.push_str(&rust_default_impl(ty, variable_message));
        if variable_message {
            output.push_str(&rust_frame_codec_impl(contract, ty));
            if needs_iox2_type_name {
                output.push_str(&rust_iox2_frame_slot_type(contract, ty));
            }
        } else if needs_wire_codec {
            output.push_str(&rust_wire_codec_impl(contract, ty));
        }
    }
    output
}

fn rust_default_impl(ty: &TypeIr, variable_message: bool) -> String {
    let mut output = String::new();
    output.push_str(&format!("impl Default for {} {{\n", ty.name));
    output.push_str("    fn default() -> Self {\n");
    if variable_message {
        output.push_str("        Self {\n");
        for field in &ty.fields {
            output.push_str(&format!(
                "            {}: Default::default(),\n",
                field.name
            ));
        }
        output.push_str("        }\n");
    } else {
        output.push_str(
            "        // Safety：FlowRT Message ABI v0.1 只允许拥有有效全零位模式的 plain-data 类型。\n",
        );
        output.push_str("        unsafe { std::mem::zeroed() }\n");
    }
    output.push_str("    }\n");
    output.push_str("}\n\n");
    output
}

pub(crate) fn fixed_message_abi_expectations(
    contract: &ContractIr,
) -> Result<Vec<MessageAbiExpectation>> {
    let mut fixed_contract = contract.clone();
    fixed_contract.types = contract
        .types
        .iter()
        .filter(|ty| {
            !type_contains_variable_data(
                contract,
                &TypeExpr::Named {
                    name: ty.name.clone(),
                },
            )
        })
        .cloned()
        .collect();
    Ok(message_abi_expectations(&fixed_contract)?)
}

fn contract_has_variable_messages(contract: &ContractIr) -> bool {
    contract.types.iter().any(|ty| {
        type_contains_variable_data(
            contract,
            &TypeExpr::Named {
                name: ty.name.clone(),
            },
        )
    })
}

pub(crate) fn type_contains_variable_data(contract: &ContractIr, expr: &TypeExpr) -> bool {
    type_contains_variable_data_inner(contract, expr, &mut BTreeSet::new())
}

fn type_contains_variable_data_inner(
    contract: &ContractIr,
    expr: &TypeExpr,
    visiting: &mut BTreeSet<String>,
) -> bool {
    match expr {
        TypeExpr::Primitive { .. } => false,
        TypeExpr::VarBytes { .. } | TypeExpr::VarString { .. } | TypeExpr::VarSequence { .. } => {
            true
        }
        TypeExpr::Array { element, .. } => {
            type_contains_variable_data_inner(contract, element, visiting)
        }
        TypeExpr::Named { name } => {
            if !visiting.insert(name.clone()) {
                return false;
            }
            let contains = type_by_name(contract, name)
                .fields
                .iter()
                .any(|field| type_contains_variable_data_inner(contract, &field.ty, visiting));
            visiting.remove(name);
            contains
        }
    }
}

pub(crate) fn frame_header_size_for_type(contract: &ContractIr, ty: &TypeIr) -> usize {
    ty.fields
        .iter()
        .map(|field| frame_header_size_for_expr(contract, &field.ty))
        .sum()
}

pub(crate) fn frame_header_size_for_expr(contract: &ContractIr, expr: &TypeExpr) -> usize {
    match expr {
        TypeExpr::VarBytes { .. } | TypeExpr::VarString { .. } | TypeExpr::VarSequence { .. } => 8,
        _ => rust_wire_size(contract, expr),
    }
}

pub(crate) fn frame_max_size_for_type(contract: &ContractIr, ty: &TypeIr) -> usize {
    frame_header_size_for_type(contract, ty)
        + ty.fields
            .iter()
            .filter_map(|field| variable_tail_max_size(contract, &field.ty))
            .sum::<usize>()
}

pub(crate) fn variable_tail_max_size(contract: &ContractIr, expr: &TypeExpr) -> Option<usize> {
    match expr {
        TypeExpr::VarBytes { max_len } | TypeExpr::VarString { max_len, .. } => {
            Some(*max_len as usize)
        }
        TypeExpr::VarSequence { element, max_len } => {
            Some(rust_wire_size(contract, element) * (*max_len as usize))
        }
        _ => None,
    }
}

fn message_sample_bytes(
    contract: &ContractIr,
    expectation: &MessageAbiExpectation,
    expectations_by_name: &BTreeMap<&str, &MessageAbiExpectation>,
) -> Vec<u8> {
    let ty = type_by_name(contract, &expectation.type_name);
    let mut bytes = vec![0u8; expectation.size_bytes];
    for (index, field) in ty.fields.iter().enumerate() {
        let field_expectation = &expectation.fields[index];
        let field_bytes =
            sample_bytes_for_expr(contract, expectations_by_name, &field.ty, index + 1);
        debug_assert_eq!(field_bytes.len(), field_expectation.size_bytes);
        let start = field_expectation.offset_bytes;
        let end = start + field_bytes.len();
        bytes[start..end].copy_from_slice(&field_bytes);
    }
    bytes
}

fn message_wire_sample_bytes(contract: &ContractIr, ty: &TypeIr) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (index, field) in ty.fields.iter().enumerate() {
        bytes.extend_from_slice(&wire_sample_bytes_for_expr(contract, &field.ty, index + 1));
    }
    bytes
}

fn sample_bytes_for_expr(
    contract: &ContractIr,
    expectations_by_name: &BTreeMap<&str, &MessageAbiExpectation>,
    expr: &TypeExpr,
    seed: usize,
) -> Vec<u8> {
    match expr {
        TypeExpr::Primitive { name } => primitive_sample_bytes(*name, seed),
        TypeExpr::Named { name } => {
            let expectation = expectations_by_name
                .get(name.as_str())
                .copied()
                .expect("ABI expectation must exist for named message type");
            message_sample_bytes(contract, expectation, expectations_by_name)
        }
        TypeExpr::Array { element, len } => {
            let element_bytes =
                sample_bytes_for_expr(contract, expectations_by_name, element, seed);
            let mut bytes = Vec::with_capacity(element_bytes.len() * *len);
            for _ in 0..*len {
                bytes.extend_from_slice(&element_bytes);
            }
            bytes
        }
        TypeExpr::VarBytes { .. } | TypeExpr::VarString { .. } | TypeExpr::VarSequence { .. } => {
            panic!(
                "validated Message ABI v0.1 contract must not contain {}",
                expr.canonical_syntax()
            )
        }
    }
}

fn wire_sample_bytes_for_expr(contract: &ContractIr, expr: &TypeExpr, seed: usize) -> Vec<u8> {
    match expr {
        TypeExpr::Primitive { name } => primitive_sample_bytes(*name, seed),
        TypeExpr::Named { name } => {
            message_wire_sample_bytes(contract, type_by_name(contract, name))
        }
        TypeExpr::Array { element, len } => {
            let element_bytes = wire_sample_bytes_for_expr(contract, element, seed);
            let mut bytes = Vec::with_capacity(element_bytes.len() * *len);
            for _ in 0..*len {
                bytes.extend_from_slice(&element_bytes);
            }
            bytes
        }
        TypeExpr::VarBytes { .. } | TypeExpr::VarString { .. } | TypeExpr::VarSequence { .. } => {
            panic!(
                "validated Message ABI v0.1 contract must not contain {}",
                expr.canonical_syntax()
            )
        }
    }
}

fn primitive_sample_bytes(primitive: PrimitiveType, seed: usize) -> Vec<u8> {
    let value = ((seed % 9) + 1) as u128;
    match primitive {
        PrimitiveType::Bool => vec![1],
        PrimitiveType::U8 => vec![value as u8],
        PrimitiveType::U16 => (value as u16).to_le_bytes().to_vec(),
        PrimitiveType::U32 => (value as u32).to_le_bytes().to_vec(),
        PrimitiveType::U64 => (value as u64).to_le_bytes().to_vec(),
        PrimitiveType::U128 => value.to_le_bytes().to_vec(),
        PrimitiveType::I8 => vec![-(value as i8) as u8],
        PrimitiveType::I16 => (-(value as i16)).to_le_bytes().to_vec(),
        PrimitiveType::I32 => (-(value as i32)).to_le_bytes().to_vec(),
        PrimitiveType::I64 => (-(value as i64)).to_le_bytes().to_vec(),
        PrimitiveType::I128 => (-(value as i128)).to_le_bytes().to_vec(),
        PrimitiveType::F32 => ((value as f32) + 0.25).to_le_bytes().to_vec(),
        PrimitiveType::F64 => ((value as f64) + 0.25).to_le_bytes().to_vec(),
    }
}

fn byte_array_literal(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn expected_bytes_const_name(type_name: &str) -> String {
    format!(
        "EXPECTED_{}_BYTES",
        snake_identifier(type_name).to_uppercase()
    )
}

pub(crate) fn emit_rust_message_abi_tests(
    contract: &ContractIr,
    expectations: &[MessageAbiExpectation],
) -> String {
    let mut output = managed_header();
    let needs_wire_codec = selected_backend_name(contract) == "zenoh";
    let reads_cpp_fixtures = has_language(contract, LanguageKind::Cpp);
    let expectations_by_name = expectations
        .iter()
        .map(|expectation| (expectation.type_name.as_str(), expectation))
        .collect::<BTreeMap<_, _>>();
    if needs_wire_codec {
        output.push_str("\nuse flowrt::WireCodec;\n");
    }
    output.push_str(
        "\nfn bytes_of<T>(value: &T) -> Vec<u8> {\n    let mut bytes = vec![0u8; std::mem::size_of::<T>()];\n    // Safety：生成测试只传入 FlowRT ABI v0.1 plain-data 消息，且 padding 已初始化。\n    unsafe {\n        std::ptr::copy_nonoverlapping(\n            (value as *const T).cast::<u8>(),\n            bytes.as_mut_ptr(),\n            bytes.len(),\n        );\n    }\n    bytes\n}\n\nfn assert_default_bytes_zero<T: Copy + Default>() {\n    let value = T::default();\n    assert_eq!(bytes_of(&value), vec![0u8; std::mem::size_of::<T>()]);\n}\n\nfn assert_byte_roundtrip<T: Copy + Default>(value: T) {\n    let bytes = bytes_of(&value);\n    let mut roundtrip = T::default();\n    // Safety：`roundtrip` 是有效 plain-data 存储，`bytes` 长度等于 `size_of::<T>()`。\n    unsafe {\n        std::ptr::copy_nonoverlapping(\n            bytes.as_ptr(),\n            (&mut roundtrip as *mut T).cast::<u8>(),\n            bytes.len(),\n        );\n    }\n    assert_eq!(bytes_of(&roundtrip), bytes);\n}\n\n",
    );
    if reads_cpp_fixtures {
        output.push_str(
            "fn assert_cpp_fixture_roundtrip<T: Copy + Default>(name: &str, expected: &[u8]) {\n    let path = std::path::Path::new(env!(\"CARGO_MANIFEST_DIR\"))\n        .join(\"abi-fixtures\")\n        .join(\"cpp\")\n        .join(name);\n    let bytes = std::fs::read(&path).unwrap_or_else(|error| {\n        panic!(\"failed to read C++ ABI fixture `{}`: {error}\", path.display())\n    });\n    assert_eq!(bytes, expected);\n    assert_eq!(bytes.len(), std::mem::size_of::<T>());\n    let mut value = T::default();\n    // Safety：C++ fixture bytes 已按同一 Contract IR 的 Message ABI v0.1 写出。\n    unsafe {\n        std::ptr::copy_nonoverlapping(\n            bytes.as_ptr(),\n            (&mut value as *mut T).cast::<u8>(),\n            bytes.len(),\n        );\n    }\n    assert_eq!(bytes_of(&value), expected);\n}\n\n",
        );
    }
    output.push_str(
        "fn assert_sample_bytes<T: Copy>(value: T, expected: &[u8]) {\n    assert_eq!(bytes_of(&value), expected);\n}\n\n",
    );

    for expectation in expectations {
        let bytes = message_sample_bytes(contract, expectation, &expectations_by_name);
        output.push_str(&format!(
            "const {}: &[u8] = &[{}];\n",
            expected_bytes_const_name(&expectation.type_name),
            byte_array_literal(&bytes)
        ));
    }
    output.push('\n');

    for ty in ordered_types(contract) {
        output.push_str(&format!(
            "fn {}() -> flowrt_app::messages::{} {{\n",
            sample_function_name(&ty.name),
            ty.name
        ));
        output.push_str(&format!(
            "    let mut value = flowrt_app::messages::{}::default();\n",
            ty.name
        ));
        for (index, field) in ty.fields.iter().enumerate() {
            output.push_str(&format!(
                "    value.{} = {};\n",
                field.name,
                rust_sample_expr(&field.ty, index + 1)
            ));
        }
        output.push_str("    value\n}\n\n");
    }

    for expectation in expectations {
        let ty = format!("flowrt_app::messages::{}", expectation.type_name);
        output.push_str("#[test]\n");
        output.push_str(&format!(
            "fn {}_message_abi() {{\n",
            snake_identifier(&expectation.type_name)
        ));
        output.push_str(&format!(
            "    assert_eq!(std::mem::size_of::<{}>(), {});\n",
            ty, expectation.size_bytes
        ));
        output.push_str(&format!(
            "    assert_eq!(std::mem::align_of::<{}>(), {});\n",
            ty, expectation.align_bytes
        ));
        output.push_str(&format!("    assert_default_bytes_zero::<{}>();\n", ty));
        for field in &expectation.fields {
            output.push_str(&format!(
                "    assert_eq!(std::mem::offset_of!({}, {}), {});\n",
                ty, field.name, field.offset_bytes
            ));
        }
        output.push_str(&format!(
            "    assert_byte_roundtrip({}());\n",
            sample_function_name(&expectation.type_name)
        ));
        output.push_str(&format!(
            "    assert_sample_bytes({}(), {});\n",
            sample_function_name(&expectation.type_name),
            expected_bytes_const_name(&expectation.type_name)
        ));
        if reads_cpp_fixtures {
            output.push_str(&format!(
                "    assert_cpp_fixture_roundtrip::<{}>(\"{}.bin\", {});\n",
                ty,
                snake_identifier(&expectation.type_name),
                expected_bytes_const_name(&expectation.type_name)
            ));
        }
        output.push_str("}\n\n");
        if needs_wire_codec {
            let message = type_by_name(contract, &expectation.type_name);
            let wire_bytes = message_wire_sample_bytes(contract, message);
            output.push_str("#[test]\n");
            output.push_str(&format!(
                "fn {}_wire_codec_omits_native_padding() {{\n",
                snake_identifier(&expectation.type_name)
            ));
            output.push_str(&format!(
                "    let value = {}();\n",
                sample_function_name(&expectation.type_name)
            ));
            output.push_str("    let wire = value.to_wire_vec().unwrap();\n");
            output.push_str(&format!(
                "    assert_eq!(wire, vec![{}]);\n",
                byte_array_literal(&wire_bytes)
            ));
            output.push_str(&format!(
                "    assert_eq!(flowrt_app::messages::{}::decode_wire(&wire).unwrap(), value);\n",
                expectation.type_name
            ));
            output.push_str("}\n\n");
        }
    }

    output
}

pub(crate) fn emit_cpp_message_abi_tests(
    contract: &ContractIr,
    expectations: &[MessageAbiExpectation],
) -> String {
    let mut output = managed_header();
    let needs_wire_codec = selected_backend_name(contract) == "zenoh";
    let expectations_by_name = expectations
        .iter()
        .map(|expectation| (expectation.type_name.as_str(), expectation))
        .collect::<BTreeMap<_, _>>();
    output.push_str(
        "\n#include <array>\n#include <cassert>\n#include <cstddef>\n#include <cstdint>\n#include <cstring>\n#include <filesystem>\n#include <fstream>\n#include <stdexcept>\n#include <string>\n#include <string_view>\n#include <type_traits>\n\n#include \"flowrt_app/messages.hpp\"\n\nnamespace {\n\ntemplate <typename T>\nstd::array<std::uint8_t, sizeof(T)> bytes_of(const T& value) {\n    std::array<std::uint8_t, sizeof(T)> bytes{};\n    std::memcpy(bytes.data(), &value, bytes.size());\n    return bytes;\n}\n\ntemplate <typename T>\nvoid assert_default_bytes_zero() {\n    T value{};\n    std::array<std::uint8_t, sizeof(T)> expected{};\n    assert(bytes_of(value) == expected);\n}\n\ntemplate <typename T>\nvoid assert_byte_roundtrip(const T& value) {\n    const auto bytes = bytes_of(value);\n    T roundtrip{};\n    std::memset(&roundtrip, 0, sizeof(roundtrip));\n    std::memcpy(&roundtrip, bytes.data(), bytes.size());\n    assert(std::memcmp(&roundtrip, &value, sizeof(T)) == 0);\n}\n\ntemplate <typename T, std::size_t N>\nvoid assert_sample_bytes(const T& value, const std::array<std::uint8_t, N>& expected) {\n    static_assert(sizeof(T) == N);\n    assert(bytes_of(value) == expected);\n}\n\ntemplate <std::size_t N>\nvoid write_fixture(std::string_view name, const std::array<std::uint8_t, N>& bytes) {\n#ifdef FLOWRT_ABI_FIXTURE_DIR\n    std::filesystem::create_directories(FLOWRT_ABI_FIXTURE_DIR);\n    auto path = std::filesystem::path(FLOWRT_ABI_FIXTURE_DIR) / std::string(name);\n    std::ofstream output(path, std::ios::binary);\n    if (!output) {\n        throw std::runtime_error(\"failed to open ABI fixture output\");\n    }\n    output.write(reinterpret_cast<const char*>(bytes.data()), static_cast<std::streamsize>(bytes.size()));\n    if (!output) {\n        throw std::runtime_error(\"failed to write ABI fixture output\");\n    }\n#else\n    (void)name;\n    (void)bytes;\n#endif\n}\n\n",
    );

    for expectation in expectations {
        let bytes = message_sample_bytes(contract, expectation, &expectations_by_name);
        output.push_str(&format!(
            "constexpr std::array<std::uint8_t, {}> {}{{{{{}}}}};\n",
            expectation.size_bytes,
            expected_bytes_const_name(&expectation.type_name),
            byte_array_literal(&bytes)
        ));
    }
    output.push('\n');

    for ty in ordered_types(contract) {
        output.push_str(&format!(
            "flowrt_app::{} {}() {{\n",
            ty.name,
            sample_function_name(&ty.name)
        ));
        output.push_str(&format!("    flowrt_app::{} value{{}};\n", ty.name));
        output.push_str("    std::memset(&value, 0, sizeof(value));\n");
        for (index, field) in ty.fields.iter().enumerate() {
            output.push_str(&format!(
                "    value.{} = {};\n",
                field.name,
                cpp_sample_expr(&field.ty, index + 1)
            ));
        }
        output.push_str("    return value;\n}\n\n");
    }

    for expectation in expectations {
        let ty = format!("flowrt_app::{}", expectation.type_name);
        output.push_str(&format!(
            "void test_{}_message_abi() {{\n",
            snake_identifier(&expectation.type_name)
        ));
        output.push_str(&format!(
            "    static_assert(std::is_standard_layout_v<{}>);\n",
            ty
        ));
        output.push_str(&format!(
            "    static_assert(std::is_trivially_copyable_v<{}>);\n",
            ty
        ));
        output.push_str(&format!(
            "    static_assert(sizeof({}) == {});\n",
            ty, expectation.size_bytes
        ));
        output.push_str(&format!(
            "    static_assert(alignof({}) == {});\n",
            ty, expectation.align_bytes
        ));
        output.push_str(&format!("    assert_default_bytes_zero<{}>();\n", ty));
        for field in &expectation.fields {
            output.push_str(&format!(
                "    static_assert(offsetof({}, {}) == {});\n",
                ty, field.name, field.offset_bytes
            ));
        }
        output.push_str(&format!(
            "    assert_byte_roundtrip({}());\n",
            sample_function_name(&expectation.type_name)
        ));
        output.push_str(&format!(
            "    assert_sample_bytes({}(), {});\n",
            sample_function_name(&expectation.type_name),
            expected_bytes_const_name(&expectation.type_name)
        ));
        output.push_str(&format!(
            "    write_fixture(\"{}.bin\", bytes_of({}()));\n",
            snake_identifier(&expectation.type_name),
            sample_function_name(&expectation.type_name)
        ));
        output.push_str("}\n\n");
        if needs_wire_codec {
            let message = type_by_name(contract, &expectation.type_name);
            let wire_bytes = message_wire_sample_bytes(contract, message);
            output.push_str(&format!(
                "void test_{}_wire_codec_omits_native_padding() {{\n",
                snake_identifier(&expectation.type_name)
            ));
            output.push_str(&format!(
                "    const auto value = {}();\n",
                sample_function_name(&expectation.type_name)
            ));
            output.push_str(&format!(
                "    std::array<std::uint8_t, flowrt_app::{}::wire_size()> wire{{}};\n",
                expectation.type_name
            ));
            output.push_str("    value.encode_wire(wire);\n");
            output.push_str(&format!(
                "    const std::array<std::uint8_t, flowrt_app::{}::wire_size()> expected_wire{{{}}};\n",
                expectation.type_name,
                byte_array_literal(&wire_bytes)
            ));
            output.push_str("    assert(wire == expected_wire);\n");
            output.push_str(&format!(
                "    const auto decoded = flowrt_app::{}::decode_wire(wire);\n",
                expectation.type_name
            ));
            output.push_str("    assert(bytes_of(decoded) == bytes_of(value));\n");
            output.push_str("}\n\n");
        }
    }

    output.push_str("}  // namespace\n\nint main() {\n");
    for expectation in expectations {
        output.push_str(&format!(
            "    test_{}_message_abi();\n",
            snake_identifier(&expectation.type_name)
        ));
        if needs_wire_codec {
            output.push_str(&format!(
                "    test_{}_wire_codec_omits_native_padding();\n",
                snake_identifier(&expectation.type_name)
            ));
        }
    }
    output.push_str("    return 0;\n}\n");
    output
}

pub(crate) fn cpp_type(expr: &TypeExpr) -> String {
    match expr {
        TypeExpr::Primitive { name } => cpp_primitive(*name).to_string(),
        TypeExpr::Named { name } => name.clone(),
        TypeExpr::Array { element, len } => {
            format!("std::array<{}, {}>", cpp_type(element), len)
        }
        TypeExpr::VarBytes { max_len } => {
            format!("flowrt::BoundedBytes<{max_len}>")
        }
        TypeExpr::VarString { max_len, .. } => {
            format!("flowrt::BoundedString<{max_len}>")
        }
        TypeExpr::VarSequence { element, max_len } => {
            format!(
                "flowrt::BoundedSequence<{}, {}>",
                cpp_type(element),
                max_len
            )
        }
    }
}

fn cpp_primitive(primitive: PrimitiveType) -> &'static str {
    match primitive {
        PrimitiveType::Bool => "bool",
        PrimitiveType::U8 => "std::uint8_t",
        PrimitiveType::U16 => "std::uint16_t",
        PrimitiveType::U32 => "std::uint32_t",
        PrimitiveType::U64 => "std::uint64_t",
        PrimitiveType::U128 => "unsigned __int128",
        PrimitiveType::I8 => "std::int8_t",
        PrimitiveType::I16 => "std::int16_t",
        PrimitiveType::I32 => "std::int32_t",
        PrimitiveType::I64 => "std::int64_t",
        PrimitiveType::I128 => "__int128",
        PrimitiveType::F32 => "float",
        PrimitiveType::F64 => "double",
    }
}

pub(crate) fn rust_type(expr: &TypeExpr) -> String {
    match expr {
        TypeExpr::Primitive { name } => rust_primitive(*name).to_string(),
        TypeExpr::Named { name } => name.clone(),
        TypeExpr::Array { element, len } => format!("[{}; {}]", rust_type(element), len),
        TypeExpr::VarBytes { max_len } => {
            format!("flowrt::BoundedBytes<{max_len}>")
        }
        TypeExpr::VarString { max_len, .. } => {
            format!("flowrt::BoundedString<{max_len}>")
        }
        TypeExpr::VarSequence { element, max_len } => {
            format!(
                "flowrt::BoundedSequence<{}, {}>",
                rust_type(element),
                max_len
            )
        }
    }
}

fn rust_wire_codec_impl(contract: &ContractIr, ty: &TypeIr) -> String {
    let mut output = String::new();
    output.push_str(&format!("impl flowrt::WireCodec for {} {{\n", ty.name));
    output.push_str(&format!(
        "    const WIRE_SIZE: usize = {};\n\n",
        rust_wire_size(
            contract,
            &TypeExpr::Named {
                name: ty.name.clone()
            }
        )
    ));
    output.push_str(
        "    fn encode_wire(&self, output: &mut [u8]) -> Result<(), flowrt::WireCodecError> {\n        if output.len() != Self::WIRE_SIZE {\n            return Err(flowrt::WireCodecError::wrong_size(Self::WIRE_SIZE, output.len()));\n        }\n        let mut cursor = 0usize;\n",
    );
    for field in &ty.fields {
        output.push_str(&rust_wire_encode_expr(
            &field.ty,
            &format!("self.{}", field.name),
            "output",
            8,
        ));
    }
    output.push_str("        Ok(())\n    }\n\n");
    output.push_str(
        "    fn decode_wire(input: &[u8]) -> Result<Self, flowrt::WireCodecError> {\n        if input.len() != Self::WIRE_SIZE {\n            return Err(flowrt::WireCodecError::wrong_size(Self::WIRE_SIZE, input.len()));\n        }\n        let mut cursor = 0usize;\n",
    );
    for field in &ty.fields {
        output.push_str(&rust_wire_decode_expr(&field.ty, &field.name, "input", 8));
    }
    output.push_str("        Ok(Self {\n");
    for field in &ty.fields {
        output.push_str(&format!("            {},\n", field.name));
    }
    output.push_str("        })\n    }\n}\n\n");
    output
}

fn rust_frame_codec_impl(contract: &ContractIr, ty: &TypeIr) -> String {
    let header_size = frame_header_size_for_type(contract, ty);
    let max_size = frame_max_size_for_type(contract, ty);
    let mut output = String::new();
    output.push_str(&format!("impl flowrt::FrameCodec for {} {{\n", ty.name));
    output.push_str(&format!(
        "    fn encoded_frame_size(&self) -> usize {{\n        {header_size}{}    }}\n\n",
        rust_dynamic_tail_size_exprs(contract, ty)
    ));
    output.push_str(
        "    fn encode_frame(&self, output: &mut [u8]) -> Result<(), flowrt::WireCodecError> {\n",
    );
    output.push_str("        let mut tail = Vec::<u8>::new();\n");
    for field in &ty.fields {
        output.push_str(&rust_frame_prepare_tail_field(contract, field));
    }
    output.push_str(
        "        if output.len() != self.encoded_frame_size() {\n            return Err(flowrt::WireCodecError::wrong_size(self.encoded_frame_size(), output.len()));\n        }\n        let mut cursor = 0usize;\n",
    );
    for field in &ty.fields {
        output.push_str(&rust_frame_encode_header_field(field));
    }
    output.push_str(&format!(
        "        output[{header_size}..].copy_from_slice(&tail);\n        let _ = cursor;\n        Ok(())\n    }}\n\n"
    ));
    output
        .push_str("    fn decode_frame(input: &[u8]) -> Result<Self, flowrt::WireCodecError> {\n");
    output.push_str(&format!(
        "        if input.len() < {header_size} {{\n            return Err(flowrt::WireCodecError::wrong_size({header_size}, input.len()));\n        }}\n        if input.len() > {max_size} {{\n            return Err(flowrt::WireCodecError::invalid_frame(\"canonical frame exceeds declared maximum size\"));\n        }}\n        let mut cursor = 0usize;\n"
    ));
    for field in &ty.fields {
        output.push_str(&rust_frame_decode_header_field(field));
    }
    output.push_str(&format!(
        "        let _ = cursor;\n        let mut decoder = flowrt::FrameDecoder::new(&input[{header_size}..]);\n"
    ));
    for field in &ty.fields {
        output.push_str(&rust_frame_decode_tail_field(contract, field));
    }
    output.push_str("        decoder.finish()?;\n        Ok(Self {\n");
    for field in &ty.fields {
        output.push_str(&format!("            {},\n", field.name));
    }
    output.push_str("        })\n    }\n}\n\n");
    output
}

fn iox2_frame_slot_type_name(type_name: &str) -> String {
    format!("{type_name}Iox2Frame")
}

pub(crate) fn iox2_frame_slot_type_for_expr(expr: &TypeExpr) -> String {
    match expr {
        TypeExpr::Named { name } => iox2_frame_slot_type_name(name),
        other => panic!(
            "validated iox2 variable frame channel must use a named message type, got `{}`",
            other.canonical_syntax()
        ),
    }
}

fn rust_iox2_frame_slot_type(contract: &ContractIr, ty: &TypeIr) -> String {
    let slot_name = iox2_frame_slot_type_name(&ty.name);
    let max_size = frame_max_size_for_type(contract, ty);
    format!(
        "#[repr(C)]\n#[derive(Clone, Copy, Debug, PartialEq, flowrt::ZeroCopySend)]\n#[type_name({type_name})]\npub struct {slot_name} {{\n    len: u32,\n    bytes: [u8; {max_size}],\n}}\n\nimpl Default for {slot_name} {{\n    fn default() -> Self {{\n        Self {{\n            len: 0,\n            bytes: [0u8; {max_size}],\n        }}\n    }}\n}}\n\nimpl flowrt::iox2::Iox2FrameSlot<{message_name}> for {slot_name} {{\n    fn try_from_message(value: &{message_name}) -> Result<Self, flowrt::WireCodecError> {{\n        let frame = flowrt::FrameCodec::to_frame_vec(value)?;\n        if frame.len() > {max_size} {{\n            return Err(flowrt::WireCodecError::invalid_frame(\"iox2 frame exceeds fixed slot capacity\"));\n        }}\n        let len = u32::try_from(frame.len())\n            .map_err(|_| flowrt::WireCodecError::invalid_frame(\"iox2 frame length exceeds u32\"))?;\n        let mut slot = Self::default();\n        slot.len = len;\n        slot.bytes[..frame.len()].copy_from_slice(&frame);\n        Ok(slot)\n    }}\n\n    fn decode_message(&self) -> Result<{message_name}, flowrt::WireCodecError> {{\n        let len = self.len as usize;\n        if len > {max_size} {{\n            return Err(flowrt::WireCodecError::invalid_frame(\"iox2 frame slot length exceeds fixed capacity\"));\n        }}\n        <{message_name} as flowrt::FrameCodec>::decode_frame(&self.bytes[..len])\n    }}\n}}\n\n",
        type_name = rust_string_literal(&ty.name),
        message_name = ty.name,
    )
}

fn rust_dynamic_tail_size_exprs(contract: &ContractIr, ty: &TypeIr) -> String {
    let mut output = String::new();
    for field in &ty.fields {
        match &field.ty {
            TypeExpr::VarBytes { .. } => {
                output.push_str(&format!(" + self.{}.len()\n", field.name));
            }
            TypeExpr::VarString { .. } => {
                output.push_str(&format!(" + self.{}.len()\n", field.name));
            }
            TypeExpr::VarSequence { element, .. } => {
                output.push_str(&format!(
                    " + self.{}.len() * {}\n",
                    field.name,
                    rust_wire_size(contract, element)
                ));
            }
            _ => {}
        }
    }
    if output.is_empty() {
        "\n".to_string()
    } else {
        output
    }
}

fn rust_frame_prepare_tail_field(contract: &ContractIr, field: &FieldIr) -> String {
    match &field.ty {
        TypeExpr::VarBytes { .. } => format!(
            "        let {name}_span = flowrt::append_tail_block(&mut tail, self.{name}.as_slice())?;\n",
            name = field.name
        ),
        TypeExpr::VarString { .. } => format!(
            "        let {name}_span = flowrt::append_tail_block(&mut tail, self.{name}.as_bytes())?;\n",
            name = field.name
        ),
        TypeExpr::VarSequence { element, .. } => {
            let element_size = rust_wire_size(contract, element);
            let mut code = format!(
                "        let mut {name}_tail = Vec::<u8>::with_capacity(self.{name}.len() * {element_size});\n        for element in self.{name}.as_slice() {{\n            let start = {name}_tail.len();\n            {name}_tail.resize(start + {element_size}, 0);\n",
                name = field.name
            );
            code.push_str("            let mut cursor = start;\n");
            code.push_str(&rust_wire_encode_expr(
                element,
                "*element",
                &format!("{}_tail", field.name),
                12,
            ));
            code.push_str("            let _ = cursor;\n");
            code.push_str(&format!(
                "        }}\n        let {name}_span = flowrt::append_tail_block(&mut tail, &{name}_tail)?;\n",
                name = field.name
            ));
            code
        }
        _ => String::new(),
    }
}

fn rust_frame_encode_header_field(field: &FieldIr) -> String {
    match &field.ty {
        TypeExpr::VarBytes { .. } | TypeExpr::VarString { .. } | TypeExpr::VarSequence { .. } => {
            format!(
                "        {name}_span.encode(&mut output[cursor..cursor + flowrt::VAR_SPAN_WIRE_SIZE])?;\n        cursor += flowrt::VAR_SPAN_WIRE_SIZE;\n",
                name = field.name
            )
        }
        _ => rust_wire_encode_expr(&field.ty, &format!("self.{}", field.name), "output", 8),
    }
}

fn rust_frame_decode_header_field(field: &FieldIr) -> String {
    match &field.ty {
        TypeExpr::VarBytes { .. } | TypeExpr::VarString { .. } | TypeExpr::VarSequence { .. } => {
            format!(
                "        let {name}_span = flowrt::VarSpan::decode(&input[cursor..cursor + flowrt::VAR_SPAN_WIRE_SIZE])?;\n        cursor += flowrt::VAR_SPAN_WIRE_SIZE;\n",
                name = field.name
            )
        }
        _ => rust_wire_decode_expr(&field.ty, &field.name, "input", 8),
    }
}

fn rust_frame_decode_tail_field(contract: &ContractIr, field: &FieldIr) -> String {
    match &field.ty {
        TypeExpr::VarBytes { max_len } => format!(
            "        let {name} = flowrt::BoundedBytes::<{max_len}>::try_from_slice(decoder.read_block({name}_span, {max_len})?)?;\n",
            name = field.name
        ),
        TypeExpr::VarString { max_len, .. } => format!(
            "        let {name} = flowrt::BoundedString::<{max_len}>::try_from_utf8(decoder.read_block({name}_span, {max_len})?)?;\n",
            name = field.name
        ),
        TypeExpr::VarSequence { element, max_len } => {
            let element_size = rust_wire_size(contract, element);
            let element_ty = rust_type(element);
            let max_tail = element_size * (*max_len as usize);
            format!(
                "        let {name}_block = decoder.read_block({name}_span, {max_tail})?;\n        if {name}_block.len() % {element_size} != 0 {{\n            return Err(flowrt::WireCodecError::invalid_frame(\"bounded sequence byte length is not divisible by element wire size\"));\n        }}\n        let mut {name}_values = Vec::<{element_ty}>::with_capacity({name}_block.len() / {element_size});\n        for chunk in {name}_block.chunks_exact({element_size}) {{\n            {name}_values.push(<{element_ty} as flowrt::WireCodec>::decode_wire(chunk)?);\n        }}\n        let {name} = flowrt::BoundedSequence::<{element_ty}, {max_len}>::try_from_vec({name}_values)?;\n",
                name = field.name
            )
        }
        _ => String::new(),
    }
}

pub(crate) fn rust_wire_size(contract: &ContractIr, expr: &TypeExpr) -> usize {
    match expr {
        TypeExpr::Primitive { name } => primitive_wire_size(*name),
        TypeExpr::Named { name } => type_by_name(contract, name)
            .fields
            .iter()
            .map(|field| rust_wire_size(contract, &field.ty))
            .sum(),
        TypeExpr::Array { element, len } => rust_wire_size(contract, element) * *len,
        TypeExpr::VarBytes { .. } | TypeExpr::VarString { .. } | TypeExpr::VarSequence { .. } => {
            panic!(
                "validated Message ABI v0.1 contract must not contain {}",
                expr.canonical_syntax()
            )
        }
    }
}

fn primitive_wire_size(primitive: PrimitiveType) -> usize {
    match primitive {
        PrimitiveType::Bool | PrimitiveType::U8 | PrimitiveType::I8 => 1,
        PrimitiveType::U16 | PrimitiveType::I16 => 2,
        PrimitiveType::U32 | PrimitiveType::I32 | PrimitiveType::F32 => 4,
        PrimitiveType::U64 | PrimitiveType::I64 | PrimitiveType::F64 => 8,
        PrimitiveType::U128 | PrimitiveType::I128 => 16,
    }
}

fn rust_wire_encode_expr(expr: &TypeExpr, value: &str, output: &str, indent: usize) -> String {
    let pad = " ".repeat(indent);
    match expr {
        TypeExpr::Primitive { name } => rust_wire_encode_primitive(*name, value, output, indent),
        TypeExpr::Named { name } => format!(
            "{pad}{value}.encode_wire(&mut {output}[cursor..cursor + {name}::WIRE_SIZE])?;\n{pad}cursor += {name}::WIRE_SIZE;\n"
        ),
        TypeExpr::Array { element, .. } => {
            let mut code = format!("{pad}for element in &{value} {{\n");
            code.push_str(&rust_wire_encode_expr(
                element,
                "*element",
                output,
                indent + 4,
            ));
            code.push_str(&format!("{pad}}}\n"));
            code
        }
        TypeExpr::VarBytes { .. } | TypeExpr::VarString { .. } | TypeExpr::VarSequence { .. } => {
            panic!(
                "validated Message ABI v0.1 contract must not contain {}",
                expr.canonical_syntax()
            )
        }
    }
}

fn rust_wire_encode_primitive(
    primitive: PrimitiveType,
    value: &str,
    output: &str,
    indent: usize,
) -> String {
    let pad = " ".repeat(indent);
    match primitive {
        PrimitiveType::Bool => {
            format!("{pad}{output}[cursor] = {value} as u8;\n{pad}cursor += 1;\n")
        }
        PrimitiveType::U8 | PrimitiveType::I8 => {
            format!("{pad}{output}[cursor] = {value} as u8;\n{pad}cursor += 1;\n")
        }
        PrimitiveType::U16
        | PrimitiveType::U32
        | PrimitiveType::U64
        | PrimitiveType::U128
        | PrimitiveType::I16
        | PrimitiveType::I32
        | PrimitiveType::I64
        | PrimitiveType::I128
        | PrimitiveType::F32
        | PrimitiveType::F64 => {
            let size = primitive_wire_size(primitive);
            format!(
                "{pad}{output}[cursor..cursor + {size}].copy_from_slice(&({value}).to_le_bytes());\n{pad}cursor += {size};\n"
            )
        }
    }
}

fn rust_wire_decode_expr(expr: &TypeExpr, local: &str, input: &str, indent: usize) -> String {
    let pad = " ".repeat(indent);
    match expr {
        TypeExpr::Primitive { name } => rust_wire_decode_primitive(*name, local, input, indent),
        TypeExpr::Named { name } => format!(
            "{pad}let {local} = {name}::decode_wire(&{input}[cursor..cursor + {name}::WIRE_SIZE])?;\n{pad}cursor += {name}::WIRE_SIZE;\n"
        ),
        TypeExpr::Array { element, len } => {
            let element_ty = rust_type(element);
            let mut code = format!(
                "{pad}let mut {local} = [{}::default(); {len}];\n{pad}for element in &mut {local} {{\n",
                element_ty
            );
            code.push_str(&rust_wire_decode_expr(
                element,
                "decoded_element",
                input,
                indent + 4,
            ));
            code.push_str(&format!("{pad}    *element = decoded_element;\n{pad}}}\n"));
            code
        }
        TypeExpr::VarBytes { .. } | TypeExpr::VarString { .. } | TypeExpr::VarSequence { .. } => {
            panic!(
                "validated Message ABI v0.1 contract must not contain {}",
                expr.canonical_syntax()
            )
        }
    }
}

fn rust_wire_decode_primitive(
    primitive: PrimitiveType,
    local: &str,
    input: &str,
    indent: usize,
) -> String {
    let pad = " ".repeat(indent);
    match primitive {
        PrimitiveType::Bool => {
            format!("{pad}let {local} = {input}[cursor] != 0;\n{pad}cursor += 1;\n")
        }
        PrimitiveType::U8 => {
            format!("{pad}let {local} = {input}[cursor];\n{pad}cursor += 1;\n")
        }
        PrimitiveType::I8 => {
            format!("{pad}let {local} = {input}[cursor] as i8;\n{pad}cursor += 1;\n")
        }
        PrimitiveType::U16 => rust_wire_decode_le("u16", local, input, 2, indent),
        PrimitiveType::U32 => rust_wire_decode_le("u32", local, input, 4, indent),
        PrimitiveType::U64 => rust_wire_decode_le("u64", local, input, 8, indent),
        PrimitiveType::U128 => rust_wire_decode_le("u128", local, input, 16, indent),
        PrimitiveType::I16 => rust_wire_decode_le("i16", local, input, 2, indent),
        PrimitiveType::I32 => rust_wire_decode_le("i32", local, input, 4, indent),
        PrimitiveType::I64 => rust_wire_decode_le("i64", local, input, 8, indent),
        PrimitiveType::I128 => rust_wire_decode_le("i128", local, input, 16, indent),
        PrimitiveType::F32 => rust_wire_decode_le("f32", local, input, 4, indent),
        PrimitiveType::F64 => rust_wire_decode_le("f64", local, input, 8, indent),
    }
}

fn rust_wire_decode_le(ty: &str, local: &str, input: &str, size: usize, indent: usize) -> String {
    let pad = " ".repeat(indent);
    let indexes = (0..size)
        .map(|offset| {
            if offset == 0 {
                format!("{input}[cursor]")
            } else {
                format!("{input}[cursor + {offset}]")
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("{pad}let {local} = {ty}::from_le_bytes([{indexes}]);\n{pad}cursor += {size};\n")
}

fn cpp_wire_codec_methods(contract: &ContractIr, ty: &TypeIr) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "\n    static constexpr std::size_t wire_size() noexcept {{ return {}; }}\n\n",
        rust_wire_size(
            contract,
            &TypeExpr::Named {
                name: ty.name.clone()
            }
        )
    ));
    output.push_str(
        "    void encode_wire(std::span<std::uint8_t> output) const {\n        flowrt::ensure_wire_size(wire_size(), output.size());\n        std::size_t cursor = 0;\n",
    );
    for field in &ty.fields {
        output.push_str(&cpp_wire_encode_expr(
            contract,
            &field.ty,
            &field.name,
            "output",
            8,
        ));
    }
    output.push_str("    }\n\n");
    output.push_str(&format!(
        "    static {} decode_wire(std::span<const std::uint8_t> input) {{\n        flowrt::ensure_wire_size(wire_size(), input.size());\n        std::size_t cursor = 0;\n        {} value{{}};\n",
        ty.name, ty.name
    ));
    for field in &ty.fields {
        output.push_str(&cpp_wire_decode_expr(
            contract,
            &field.ty,
            &format!("value.{}", field.name),
            "input",
            8,
        ));
    }
    output.push_str("        return value;\n    }\n");
    output
}

fn cpp_frame_codec_methods(contract: &ContractIr, ty: &TypeIr) -> String {
    let header_size = frame_header_size_for_type(contract, ty);
    let max_size = frame_max_size_for_type(contract, ty);
    let mut output = String::new();
    output.push_str(&format!(
        "\n    std::size_t encoded_frame_size() const noexcept {{ return {header_size}{}; }}\n\n",
        cpp_dynamic_tail_size_exprs(contract, ty)
    ));
    output.push_str(&format!(
        "    static constexpr std::size_t max_frame_size() noexcept {{ return {max_size}; }}\n\n"
    ));
    output.push_str(
        "    void encode_frame(std::span<std::uint8_t> output) const {\n        std::vector<std::uint8_t> tail;\n",
    );
    for field in &ty.fields {
        output.push_str(&cpp_frame_prepare_tail_field(contract, field));
    }
    output.push_str(
        "        flowrt::ensure_wire_size(encoded_frame_size(), output.size());\n        std::size_t cursor = 0;\n",
    );
    for field in &ty.fields {
        output.push_str(&cpp_frame_encode_header_field(contract, field));
    }
    output.push_str(&format!(
        "        std::copy(tail.begin(), tail.end(), output.begin() + {header_size});\n    }}\n\n"
    ));
    output.push_str(&format!(
        "    static {} decode_frame(std::span<const std::uint8_t> input) {{\n        if (input.size() < {header_size}) {{\n            throw flowrt::WireCodecError({header_size}, input.size());\n        }}\n        if (input.size() > max_frame_size()) {{\n            throw flowrt::WireCodecError(\"canonical frame exceeds declared maximum size\");\n        }}\n        std::size_t cursor = 0;\n        {} value{{}};\n",
        ty.name, ty.name
    ));
    for field in &ty.fields {
        output.push_str(&cpp_frame_decode_header_field(contract, field));
    }
    output.push_str(&format!(
        "        flowrt::FrameDecoder decoder(input.subspan({header_size}));\n"
    ));
    for field in &ty.fields {
        output.push_str(&cpp_frame_decode_tail_field(contract, field));
    }
    output.push_str("        decoder.finish();\n        return value;\n    }\n");
    output
}

fn cpp_iox2_frame_slot_type(ty: &TypeIr) -> String {
    let slot_name = iox2_frame_slot_type_name(&ty.name);
    format!(
        "struct {slot_name} {{\n    static constexpr const char* IOX2_TYPE_NAME = \"{message_name}\";\n\n    std::uint32_t len{{}};\n    std::array<std::uint8_t, {message_name}::max_frame_size()> bytes{{}};\n\n    static {slot_name} from_message(const {message_name}& value) {{\n        {slot_name} slot{{}};\n        const auto size = value.encoded_frame_size();\n        if (size > slot.bytes.size()) {{\n            throw flowrt::WireCodecError(\"iox2 frame exceeds fixed slot capacity\");\n        }}\n        if (size > static_cast<std::size_t>(std::numeric_limits<std::uint32_t>::max())) {{\n            throw flowrt::WireCodecError(\"iox2 frame length exceeds u32\");\n        }}\n        slot.len = static_cast<std::uint32_t>(size);\n        value.encode_frame(std::span<std::uint8_t>{{slot.bytes.data(), size}});\n        return slot;\n    }}\n\n    {message_name} decode_message() const {{\n        if (len > bytes.size()) {{\n            throw flowrt::WireCodecError(\"iox2 frame slot length exceeds fixed capacity\");\n        }}\n        return {message_name}::decode_frame(std::span<const std::uint8_t>{{bytes.data(), len}});\n    }}\n}};\n\n",
        message_name = ty.name,
    )
}

fn cpp_dynamic_tail_size_exprs(contract: &ContractIr, ty: &TypeIr) -> String {
    let mut output = String::new();
    for field in &ty.fields {
        match &field.ty {
            TypeExpr::VarBytes { .. } => {
                output.push_str(&format!(" + {}.size()", field.name));
            }
            TypeExpr::VarString { .. } => {
                output.push_str(&format!(" + {}.size()", field.name));
            }
            TypeExpr::VarSequence { element, .. } => {
                output.push_str(&format!(
                    " + {}.size() * {}",
                    field.name,
                    rust_wire_size(contract, element)
                ));
            }
            _ => {}
        }
    }
    output
}

fn cpp_frame_prepare_tail_field(contract: &ContractIr, field: &FieldIr) -> String {
    match &field.ty {
        TypeExpr::VarBytes { .. } => format!(
            "        const auto {name}_span = flowrt::append_tail_block(tail, {name}.as_span());\n",
            name = field.name
        ),
        TypeExpr::VarString { .. } => format!(
            "        const auto {name}_span = flowrt::append_tail_block(tail, {name}.bytes());\n",
            name = field.name
        ),
        TypeExpr::VarSequence { element, .. } => {
            let element_size = rust_wire_size(contract, element);
            let mut code = format!(
                "        std::vector<std::uint8_t> {name}_tail;\n        {name}_tail.resize({name}.size() * {element_size});\n        std::size_t {name}_cursor = 0;\n        for (const auto& element : {name}.as_span()) {{\n            std::size_t cursor = {name}_cursor;\n",
                name = field.name
            );
            code.push_str(&cpp_wire_encode_expr(
                contract,
                element,
                "element",
                &format!(
                    "std::span<std::uint8_t>{{{}_tail.data(), {}_tail.size()}}",
                    field.name, field.name
                ),
                12,
            ));
            code.push_str(&format!(
                "            {name}_cursor += {element_size};\n        }}\n        const auto {name}_span = flowrt::append_tail_block(tail, std::span<const std::uint8_t>{{{name}_tail.data(), {name}_tail.size()}});\n",
                name = field.name
            ));
            code
        }
        _ => String::new(),
    }
}

fn cpp_frame_encode_header_field(contract: &ContractIr, field: &FieldIr) -> String {
    match &field.ty {
        TypeExpr::VarBytes { .. } | TypeExpr::VarString { .. } | TypeExpr::VarSequence { .. } => {
            format!(
                "        flowrt::write_var_span(output.subspan(cursor, flowrt::VAR_SPAN_WIRE_SIZE), {name}_span);\n        cursor += flowrt::VAR_SPAN_WIRE_SIZE;\n",
                name = field.name
            )
        }
        _ => cpp_wire_encode_expr(contract, &field.ty, &field.name, "output", 8),
    }
}

fn cpp_frame_decode_header_field(contract: &ContractIr, field: &FieldIr) -> String {
    match &field.ty {
        TypeExpr::VarBytes { .. } | TypeExpr::VarString { .. } | TypeExpr::VarSequence { .. } => {
            format!(
                "        const auto {name}_span = flowrt::read_var_span(input.subspan(cursor, flowrt::VAR_SPAN_WIRE_SIZE));\n        cursor += flowrt::VAR_SPAN_WIRE_SIZE;\n",
                name = field.name
            )
        }
        _ => cpp_wire_decode_expr(
            contract,
            &field.ty,
            &format!("value.{}", field.name),
            "input",
            8,
        ),
    }
}

fn cpp_frame_decode_tail_field(contract: &ContractIr, field: &FieldIr) -> String {
    match &field.ty {
        TypeExpr::VarBytes { max_len } => format!(
            "        value.{name} = flowrt::BoundedBytes<{max_len}>::from(decoder.read_block({name}_span, {max_len}));\n",
            name = field.name
        ),
        TypeExpr::VarString { max_len, .. } => format!(
            "        value.{name} = flowrt::BoundedString<{max_len}>::from_utf8(decoder.read_block({name}_span, {max_len}));\n",
            name = field.name
        ),
        TypeExpr::VarSequence { element, max_len } => {
            let element_size = rust_wire_size(contract, element);
            let element_ty = cpp_type(element);
            let max_tail = element_size * (*max_len as usize);
            let decode_element = if matches!(**element, TypeExpr::Primitive { .. }) {
                format!(
                    "flowrt::read_wire_le<{element_ty}>({name}_block, index)",
                    element_ty = element_ty,
                    name = field.name
                )
            } else {
                format!(
                    "{element_ty}::decode_wire({name}_block.subspan(index, {element_size}))",
                    element_ty = element_ty,
                    name = field.name
                )
            };
            format!(
                "        const auto {name}_block = decoder.read_block({name}_span, {max_tail});\n        if ({name}_block.size() % {element_size} != 0) {{\n            throw flowrt::WireCodecError(\"bounded sequence byte length is not divisible by element wire size\");\n        }}\n        std::vector<{element_ty}> {name}_values;\n        {name}_values.reserve({name}_block.size() / {element_size});\n        for (std::size_t index = 0; index < {name}_block.size(); index += {element_size}) {{\n            {name}_values.push_back({decode_element});\n        }}\n        value.{name} = flowrt::BoundedSequence<{element_ty}, {max_len}>::from(std::span<const {element_ty}>{{{name}_values.data(), {name}_values.size()}});\n",
                name = field.name,
                decode_element = decode_element
            )
        }
        _ => String::new(),
    }
}

fn cpp_wire_encode_expr(
    contract: &ContractIr,
    expr: &TypeExpr,
    value: &str,
    output: &str,
    indent: usize,
) -> String {
    let pad = " ".repeat(indent);
    match expr {
        TypeExpr::Primitive { .. } => {
            let size = rust_wire_size(contract, expr);
            format!(
                "{pad}flowrt::write_wire_le({output}, cursor, {value});\n{pad}cursor += {size};\n"
            )
        }
        TypeExpr::Named { name } => format!(
            "{pad}{value}.encode_wire({output}.subspan(cursor, {name}::wire_size()));\n{pad}cursor += {name}::wire_size();\n"
        ),
        TypeExpr::Array { element, .. } => {
            let mut code = format!("{pad}for (const auto& element : {value}) {{\n");
            code.push_str(&cpp_wire_encode_expr(
                contract,
                element,
                "element",
                output,
                indent + 4,
            ));
            code.push_str(&format!("{pad}}}\n"));
            code
        }
        TypeExpr::VarBytes { .. } | TypeExpr::VarString { .. } | TypeExpr::VarSequence { .. } => {
            panic!(
                "validated Message ABI v0.1 contract must not contain {}",
                expr.canonical_syntax()
            )
        }
    }
}

fn cpp_wire_decode_expr(
    contract: &ContractIr,
    expr: &TypeExpr,
    target: &str,
    input: &str,
    indent: usize,
) -> String {
    let pad = " ".repeat(indent);
    match expr {
        TypeExpr::Primitive { .. } => {
            let size = rust_wire_size(contract, expr);
            format!(
                "{pad}{target} = flowrt::read_wire_le<{}>({input}, cursor);\n{pad}cursor += {size};\n",
                cpp_type(expr)
            )
        }
        TypeExpr::Named { name } => format!(
            "{pad}{target} = {name}::decode_wire({input}.subspan(cursor, {name}::wire_size()));\n{pad}cursor += {name}::wire_size();\n"
        ),
        TypeExpr::Array { element, len } => {
            let mut code = format!("{pad}for (std::size_t index = 0; index < {len}; ++index) {{\n");
            code.push_str(&cpp_wire_decode_expr(
                contract,
                element,
                &format!("{target}[index]"),
                input,
                indent + 4,
            ));
            code.push_str(&format!("{pad}}}\n"));
            code
        }
        TypeExpr::VarBytes { .. } | TypeExpr::VarString { .. } | TypeExpr::VarSequence { .. } => {
            panic!(
                "validated Message ABI v0.1 contract must not contain {}",
                expr.canonical_syntax()
            )
        }
    }
}

fn rust_primitive(primitive: PrimitiveType) -> &'static str {
    match primitive {
        PrimitiveType::Bool => "bool",
        PrimitiveType::U8 => "u8",
        PrimitiveType::U16 => "u16",
        PrimitiveType::U32 => "u32",
        PrimitiveType::U64 => "u64",
        PrimitiveType::U128 => "u128",
        PrimitiveType::I8 => "i8",
        PrimitiveType::I16 => "i16",
        PrimitiveType::I32 => "i32",
        PrimitiveType::I64 => "i64",
        PrimitiveType::I128 => "i128",
        PrimitiveType::F32 => "f32",
        PrimitiveType::F64 => "f64",
    }
}

fn rust_sample_expr(expr: &TypeExpr, seed: usize) -> String {
    match expr {
        TypeExpr::Primitive { name } => rust_primitive_sample(*name, seed),
        TypeExpr::Named { name } => format!("{}()", sample_function_name(name)),
        TypeExpr::Array { element, len } => {
            format!("[{}; {}]", rust_sample_expr(element, seed), len)
        }
        TypeExpr::VarBytes { .. } | TypeExpr::VarString { .. } | TypeExpr::VarSequence { .. } => {
            panic!(
                "validated Message ABI v0.1 contract must not contain {}",
                expr.canonical_syntax()
            )
        }
    }
}

fn rust_primitive_sample(primitive: PrimitiveType, seed: usize) -> String {
    let value = (seed % 9) + 1;
    match primitive {
        PrimitiveType::Bool => "true".to_string(),
        PrimitiveType::U8 => format!("{value}u8"),
        PrimitiveType::U16 => format!("{value}u16"),
        PrimitiveType::U32 => format!("{value}u32"),
        PrimitiveType::U64 => format!("{value}u64"),
        PrimitiveType::U128 => format!("{value}u128"),
        PrimitiveType::I8 => format!("-{value}i8"),
        PrimitiveType::I16 => format!("-{value}i16"),
        PrimitiveType::I32 => format!("-{value}i32"),
        PrimitiveType::I64 => format!("-{value}i64"),
        PrimitiveType::I128 => format!("-{value}i128"),
        PrimitiveType::F32 => format!("{value}.25f32"),
        PrimitiveType::F64 => format!("{value}.25f64"),
    }
}

fn cpp_sample_expr(expr: &TypeExpr, seed: usize) -> String {
    match expr {
        TypeExpr::Primitive { name } => cpp_primitive_sample(*name, seed),
        TypeExpr::Named { name } => format!("{}()", sample_function_name(name)),
        TypeExpr::Array { element, len: _ } => {
            format!(
                "[] {{ auto value = {}{{}}; value.fill({}); return value; }}()",
                cpp_type(expr),
                cpp_sample_expr(element, seed)
            )
        }
        TypeExpr::VarBytes { .. } | TypeExpr::VarString { .. } | TypeExpr::VarSequence { .. } => {
            panic!(
                "validated Message ABI v0.1 contract must not contain {}",
                expr.canonical_syntax()
            )
        }
    }
}

fn cpp_primitive_sample(primitive: PrimitiveType, seed: usize) -> String {
    let value = (seed % 9) + 1;
    match primitive {
        PrimitiveType::Bool => "true".to_string(),
        PrimitiveType::U8 => format!("std::uint8_t{{{value}}}"),
        PrimitiveType::U16 => format!("std::uint16_t{{{value}}}"),
        PrimitiveType::U32 => format!("std::uint32_t{{{value}}}"),
        PrimitiveType::U64 => format!("std::uint64_t{{{value}}}"),
        PrimitiveType::U128 => format!("static_cast<unsigned __int128>({value})"),
        PrimitiveType::I8 => format!("std::int8_t{{-{value}}}"),
        PrimitiveType::I16 => format!("std::int16_t{{-{value}}}"),
        PrimitiveType::I32 => format!("std::int32_t{{-{value}}}"),
        PrimitiveType::I64 => format!("std::int64_t{{-{value}}}"),
        PrimitiveType::I128 => format!("static_cast<__int128>(-{value})"),
        PrimitiveType::F32 => format!("{value}.25F"),
        PrimitiveType::F64 => format!("{value}.25"),
    }
}

fn sample_function_name(type_name: &str) -> String {
    format!("sample_{}", snake_identifier(type_name))
}
