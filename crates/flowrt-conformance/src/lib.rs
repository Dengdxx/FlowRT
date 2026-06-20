//! Message ABI 和 layout expectations 的共享 conformance helper。
//!
//! 本 crate 从 Contract IR 推导 C++/Rust 都必须满足的消息布局期望，用于生成跨语言
//! conformance tests。它不读取语言源码，也不依赖具体 runtime backend。

use std::collections::{BTreeMap, BTreeSet};

use flowrt_ir::{ContractIr, PrimitiveType, TypeExpr, TypeIr};

/// conformance helper 返回的结果类型。
pub type Result<T> = std::result::Result<T, AbiError>;

/// 推导 ABI expectations 时产生的错误。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AbiError {
    #[error("unknown message type `{type_name}` referenced from `{context}`")]
    UnknownType { context: String, type_name: String },

    #[error("recursive message type `{type_name}` detected")]
    RecursiveType { type_name: String },

    #[error("type expression `{type_expr}` in `{context}` requires future {required_abi}")]
    UnsupportedFutureType {
        context: String,
        type_expr: String,
        required_abi: &'static str,
    },
}

/// 单个字段的 layout expectation。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldLayoutExpectation {
    pub name: String,
    pub offset_bytes: usize,
    pub size_bytes: usize,
    pub align_bytes: usize,
}

/// 单个消息类型的 ABI expectation。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageAbiExpectation {
    pub type_name: String,
    pub size_bytes: usize,
    pub align_bytes: usize,
    pub fields: Vec<FieldLayoutExpectation>,
}

/// 单个字段的 canonical frame expectation。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameFieldExpectation {
    pub name: String,
    pub header_offset_bytes: usize,
    pub header_size_bytes: usize,
    pub tail_max_bytes: Option<usize>,
}

/// 单个消息类型的 canonical frame expectation。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageFrameExpectation {
    pub type_name: String,
    pub header_size_bytes: usize,
    pub max_size_bytes: Option<usize>,
    pub variable: bool,
    pub fields: Vec<FrameFieldExpectation>,
}

/// iox2 有界变长 frame slot 的容量 expectation。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Iox2FrameSlotExpectation {
    pub type_name: String,
    pub cap_bytes: usize,
}

/// 返回必须参与 ABI conformance tests 的消息类型。
pub fn message_types(contract: &ContractIr) -> impl Iterator<Item = &TypeIr> {
    contract.types.iter()
}

/// 现有测试和文档使用的简化 layout expectation。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutExpectation {
    pub type_name: String,
    pub fields: Vec<String>,
}

/// 从 Contract IR 推导字段名 expectation。
pub fn layout_expectations(contract: &ContractIr) -> Result<Vec<LayoutExpectation>> {
    message_abi_expectations(contract)?;
    Ok(contract
        .types
        .iter()
        .map(|ty| LayoutExpectation {
            type_name: ty.generated_name.clone(),
            fields: ty.fields.iter().map(|field| field.name.clone()).collect(),
        })
        .collect())
}

/// 为 contract 中所有消息类型推导确定性的 ABI expectations。
pub fn message_abi_expectations(contract: &ContractIr) -> Result<Vec<MessageAbiExpectation>> {
    let type_map = contract
        .types
        .iter()
        .map(|ty| (ty.qualified_name.as_str(), ty))
        .collect::<BTreeMap<_, _>>();
    let mut cache = BTreeMap::new();
    let mut expectations = Vec::with_capacity(contract.types.len());

    for ty in &contract.types {
        let layout = message_layout(ty, &type_map, &mut cache, &mut BTreeSet::new())?;
        expectations.push(layout);
    }

    Ok(expectations)
}

/// 为 contract 中所有消息类型推导 canonical frame expectations。
pub fn message_frame_expectations(contract: &ContractIr) -> Result<Vec<MessageFrameExpectation>> {
    let type_map = contract
        .types
        .iter()
        .map(|ty| (ty.qualified_name.as_str(), ty))
        .collect::<BTreeMap<_, _>>();
    contract
        .types
        .iter()
        .map(|ty| message_frame_expectation(ty, &type_map))
        .collect()
}

/// 为全部有界变长消息推导 iox2 frame slot CAP。
pub fn iox2_frame_slot_expectations(
    contract: &ContractIr,
) -> Result<Vec<Iox2FrameSlotExpectation>> {
    Ok(message_frame_expectations(contract)?
        .into_iter()
        .filter(|frame| frame.variable)
        .filter_map(|frame| {
            frame
                .max_size_bytes
                .map(|cap_bytes| Iox2FrameSlotExpectation {
                    type_name: frame.type_name,
                    cap_bytes,
                })
        })
        .collect())
}

fn message_frame_expectation(
    ty: &TypeIr,
    type_map: &BTreeMap<&str, &TypeIr>,
) -> Result<MessageFrameExpectation> {
    let mut header_offset = 0usize;
    let mut fields = Vec::with_capacity(ty.fields.len());
    for field in &ty.fields {
        let header_size = frame_header_size_for_expr(
            &field.ty,
            type_map,
            &mut BTreeSet::new(),
            &ty.qualified_name,
        )?;
        let tail_max = if type_contains_variable_data(
            &field.ty,
            type_map,
            &mut BTreeSet::new(),
            &ty.qualified_name,
        )? {
            variable_tail_max_size(
                &field.ty,
                type_map,
                &mut BTreeSet::new(),
                &ty.qualified_name,
            )?
        } else {
            None
        };
        fields.push(FrameFieldExpectation {
            name: field.name.clone(),
            header_offset_bytes: header_offset,
            header_size_bytes: header_size,
            tail_max_bytes: tail_max,
        });
        header_offset = header_offset.saturating_add(header_size);
    }
    let variable = type_contains_variable_data(
        &TypeExpr::Named {
            name: ty.qualified_name.clone(),
        },
        type_map,
        &mut BTreeSet::new(),
        &ty.qualified_name,
    )?;
    let tail_max = variable_tail_max_size(
        &TypeExpr::Named {
            name: ty.qualified_name.clone(),
        },
        type_map,
        &mut BTreeSet::new(),
        &ty.qualified_name,
    )?;
    Ok(MessageFrameExpectation {
        type_name: ty.generated_name.clone(),
        header_size_bytes: header_offset,
        max_size_bytes: tail_max.map(|tail| header_offset.saturating_add(tail)),
        variable,
        fields,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Layout {
    size_bytes: usize,
    align_bytes: usize,
}

fn message_layout(
    ty: &TypeIr,
    type_map: &BTreeMap<&str, &TypeIr>,
    cache: &mut BTreeMap<String, Layout>,
    visiting: &mut BTreeSet<String>,
) -> Result<MessageAbiExpectation> {
    let layout = struct_layout(&ty.qualified_name, &ty.fields, type_map, cache, visiting)?;
    Ok(MessageAbiExpectation {
        type_name: ty.generated_name.clone(),
        size_bytes: layout.layout.size_bytes,
        align_bytes: layout.layout.align_bytes,
        fields: layout.fields,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StructLayout {
    layout: Layout,
    fields: Vec<FieldLayoutExpectation>,
}

fn struct_layout(
    type_name: &str,
    fields: &[flowrt_ir::FieldIr],
    type_map: &BTreeMap<&str, &TypeIr>,
    cache: &mut BTreeMap<String, Layout>,
    visiting: &mut BTreeSet<String>,
) -> Result<StructLayout> {
    if !visiting.insert(type_name.to_string()) {
        return Err(AbiError::RecursiveType {
            type_name: type_name.to_string(),
        });
    }

    let mut offset_bytes = 0usize;
    let mut struct_align = 1usize;
    let mut field_layouts = Vec::with_capacity(fields.len());

    for field in fields {
        let field_layout = type_layout(&field.ty, type_map, cache, visiting, type_name)?;
        struct_align = struct_align.max(field_layout.align_bytes);
        offset_bytes = round_up(offset_bytes, field_layout.align_bytes);
        field_layouts.push(FieldLayoutExpectation {
            name: field.name.clone(),
            offset_bytes,
            size_bytes: field_layout.size_bytes,
            align_bytes: field_layout.align_bytes,
        });
        offset_bytes = offset_bytes.saturating_add(field_layout.size_bytes);
    }

    let struct_size = round_up(offset_bytes, struct_align);
    visiting.remove(type_name);
    cache.insert(
        type_name.to_string(),
        Layout {
            size_bytes: struct_size,
            align_bytes: struct_align,
        },
    );

    Ok(StructLayout {
        layout: Layout {
            size_bytes: struct_size,
            align_bytes: struct_align,
        },
        fields: field_layouts,
    })
}

fn type_layout(
    expr: &TypeExpr,
    type_map: &BTreeMap<&str, &TypeIr>,
    cache: &mut BTreeMap<String, Layout>,
    visiting: &mut BTreeSet<String>,
    context: &str,
) -> Result<Layout> {
    match expr {
        TypeExpr::Primitive { name } => Ok(primitive_layout(*name)),
        TypeExpr::Named { name } => {
            if let Some(layout) = cache.get(name).copied() {
                return Ok(layout);
            }
            let Some(ty) = type_map.get(name.as_str()).copied() else {
                return Err(AbiError::UnknownType {
                    context: context.to_string(),
                    type_name: name.clone(),
                });
            };
            if visiting.contains(name) {
                return Err(AbiError::RecursiveType {
                    type_name: name.clone(),
                });
            }
            let StructLayout { layout, .. } =
                struct_layout(&ty.qualified_name, &ty.fields, type_map, cache, visiting)?;
            Ok(layout)
        }
        TypeExpr::Array { element, len } => {
            let element_layout = type_layout(element, type_map, cache, visiting, context)?;
            Ok(Layout {
                size_bytes: element_layout.size_bytes.saturating_mul(*len),
                align_bytes: element_layout.align_bytes,
            })
        }
        TypeExpr::VarBytes { .. } | TypeExpr::VarString { .. } | TypeExpr::VarSequence { .. } => {
            Err(AbiError::UnsupportedFutureType {
                context: context.to_string(),
                type_expr: expr.canonical_syntax(),
                required_abi: expr.required_future_abi().unwrap_or("future ABI"),
            })
        }
    }
}

fn primitive_layout(primitive: PrimitiveType) -> Layout {
    match primitive {
        PrimitiveType::Bool => Layout {
            size_bytes: 1,
            align_bytes: 1,
        },
        PrimitiveType::U8 => Layout {
            size_bytes: 1,
            align_bytes: 1,
        },
        PrimitiveType::U16 => Layout {
            size_bytes: 2,
            align_bytes: 2,
        },
        PrimitiveType::U32 | PrimitiveType::I32 | PrimitiveType::F32 => Layout {
            size_bytes: 4,
            align_bytes: 4,
        },
        PrimitiveType::U64 | PrimitiveType::I64 | PrimitiveType::F64 => Layout {
            size_bytes: 8,
            align_bytes: 8,
        },
        PrimitiveType::U128 | PrimitiveType::I128 => Layout {
            size_bytes: 16,
            align_bytes: 16,
        },
        PrimitiveType::I8 => Layout {
            size_bytes: 1,
            align_bytes: 1,
        },
        PrimitiveType::I16 => Layout {
            size_bytes: 2,
            align_bytes: 2,
        },
    }
}

fn type_contains_variable_data(
    expr: &TypeExpr,
    type_map: &BTreeMap<&str, &TypeIr>,
    visiting: &mut BTreeSet<String>,
    context: &str,
) -> Result<bool> {
    match expr {
        TypeExpr::Primitive { .. } => Ok(false),
        TypeExpr::VarBytes { .. } | TypeExpr::VarString { .. } | TypeExpr::VarSequence { .. } => {
            Ok(true)
        }
        TypeExpr::Array { element, .. } => {
            type_contains_variable_data(element, type_map, visiting, context)
        }
        TypeExpr::Named { name } => {
            if !visiting.insert(name.clone()) {
                return Err(AbiError::RecursiveType {
                    type_name: name.clone(),
                });
            }
            let ty = type_by_name(type_map, name, context)?;
            let contains = ty.fields.iter().try_fold(false, |contains, field| {
                Ok::<_, AbiError>(
                    contains
                        || type_contains_variable_data(
                            &field.ty,
                            type_map,
                            visiting,
                            &ty.qualified_name,
                        )?,
                )
            })?;
            visiting.remove(name);
            Ok(contains)
        }
    }
}

fn frame_header_size_for_expr(
    expr: &TypeExpr,
    type_map: &BTreeMap<&str, &TypeIr>,
    visiting: &mut BTreeSet<String>,
    context: &str,
) -> Result<usize> {
    match expr {
        TypeExpr::VarBytes { .. } | TypeExpr::VarString { .. } | TypeExpr::VarSequence { .. } => {
            Ok(8)
        }
        TypeExpr::Primitive { name } => Ok(primitive_layout(*name).size_bytes),
        TypeExpr::Array { element, len } => Ok(frame_header_size_for_expr(
            element, type_map, visiting, context,
        )?
        .saturating_mul(*len)),
        TypeExpr::Named { name } => {
            if !visiting.insert(name.clone()) {
                return Err(AbiError::RecursiveType {
                    type_name: name.clone(),
                });
            }
            let ty = type_by_name(type_map, name, context)?;
            let mut header = 0usize;
            for field in &ty.fields {
                header = header.saturating_add(frame_header_size_for_expr(
                    &field.ty,
                    type_map,
                    visiting,
                    &ty.qualified_name,
                )?);
            }
            visiting.remove(name);
            Ok(header)
        }
    }
}

fn variable_tail_max_size(
    expr: &TypeExpr,
    type_map: &BTreeMap<&str, &TypeIr>,
    visiting: &mut BTreeSet<String>,
    context: &str,
) -> Result<Option<usize>> {
    match expr {
        TypeExpr::Primitive { .. } => Ok(Some(0)),
        TypeExpr::VarBytes { max_len } | TypeExpr::VarString { max_len, .. } => {
            Ok(max_len.map(|n| n as usize))
        }
        TypeExpr::VarSequence { element, max_len } => {
            let Some(count) = max_len else {
                return Ok(None);
            };
            let Some(unit) = frame_unit_size(element, type_map, visiting, context)? else {
                return Ok(None);
            };
            Ok(Some((*count as usize).saturating_mul(unit)))
        }
        TypeExpr::Array { element, len } => Ok(variable_tail_max_size(
            element, type_map, visiting, context,
        )?
        .map(|tail| tail.saturating_mul(*len))),
        TypeExpr::Named { name } => {
            if !visiting.insert(name.clone()) {
                return Err(AbiError::RecursiveType {
                    type_name: name.clone(),
                });
            }
            let ty = type_by_name(type_map, name, context)?;
            let mut tail = 0usize;
            for field in &ty.fields {
                let Some(field_tail) =
                    variable_tail_max_size(&field.ty, type_map, visiting, &ty.qualified_name)?
                else {
                    visiting.remove(name);
                    return Ok(None);
                };
                tail = tail.saturating_add(field_tail);
            }
            visiting.remove(name);
            Ok(Some(tail))
        }
    }
}

fn frame_unit_size(
    expr: &TypeExpr,
    type_map: &BTreeMap<&str, &TypeIr>,
    visiting: &mut BTreeSet<String>,
    context: &str,
) -> Result<Option<usize>> {
    let Some(tail) = variable_tail_max_size(expr, type_map, visiting, context)? else {
        return Ok(None);
    };
    Ok(Some(
        frame_header_size_for_expr(expr, type_map, visiting, context)?.saturating_add(tail),
    ))
}

fn type_by_name<'a>(
    type_map: &BTreeMap<&str, &'a TypeIr>,
    name: &str,
    context: &str,
) -> Result<&'a TypeIr> {
    type_map
        .get(name)
        .copied()
        .ok_or_else(|| AbiError::UnknownType {
            context: context.to_string(),
            type_name: name.to_string(),
        })
}

fn round_up(value: usize, align: usize) -> usize {
    if align <= 1 {
        return value;
    }
    let remainder = value % align;
    if remainder == 0 {
        value
    } else {
        value + (align - remainder)
    }
}

#[cfg(test)]
mod tests {
    use flowrt_ir::{hash_source, normalize_document};
    use flowrt_rsdl::parse_str;

    use super::*;

    #[test]
    fn derives_layout_expectations_from_contract() {
        let source = r#"
[package]
name = "demo"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"
ax = "f32"
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let expectations = layout_expectations(&ir).unwrap();
        assert_eq!(expectations[0].fields, ["timestamp", "ax"]);
    }

    #[test]
    fn computes_struct_layouts_with_padding() {
        let source = r#"
[package]
name = "demo"
rsdl_version = "0.1"

[type.Inner]
left = "u8"
right = "u32"

[type.Outer]
head = "u16"
inner = "Inner"
tail = "u8"
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let expectations = message_abi_expectations(&ir).unwrap();
        let outer = expectations
            .iter()
            .find(|item| item.type_name == "Outer")
            .unwrap();
        assert_eq!(outer.align_bytes, 4);
        assert_eq!(outer.size_bytes, 16);
        assert_eq!(outer.fields[0].offset_bytes, 0);
        assert_eq!(outer.fields[1].offset_bytes, 4);
        assert_eq!(outer.fields[2].offset_bytes, 12);
    }

    #[test]
    fn rejects_recursive_types() {
        let source = r#"
[package]
name = "demo"
rsdl_version = "0.1"

[type.Node]
next = "Node"
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let error = message_abi_expectations(&ir).unwrap_err();
        assert!(matches!(error, AbiError::RecursiveType { .. }));
    }

    #[test]
    fn rejects_variable_frame_types_for_message_abi_v0_1_layout() {
        let source = r#"
[package]
name = "demo"
rsdl_version = "0.1"

[type.Packet]
payload = "bytes"
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let error = message_abi_expectations(&ir).unwrap_err();

        assert!(matches!(
            error,
            AbiError::UnsupportedFutureType { context, ref type_expr, required_abi }
                if context == "Packet"
                    && type_expr == "bytes"
                    && required_abi == "Variable Frame ABI"
        ));
    }

    #[test]
    fn rejects_variable_frame_types_for_field_name_layout_expectations() {
        let source = r#"
[package]
name = "demo"
rsdl_version = "0.1"

[type.Packet]
payload = "bytes"
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let error = layout_expectations(&ir).unwrap_err();

        assert!(matches!(
            error,
            AbiError::UnsupportedFutureType { context, ref type_expr, required_abi }
                if context == "Packet"
                    && type_expr == "bytes"
                    && required_abi == "Variable Frame ABI"
        ));
    }

    #[test]
    fn derives_bounded_variable_frame_and_iox2_slot_expectations() {
        let source = r#"
[package]
name = "demo"
rsdl_version = "0.1"

[type.Packet]
payload = "bytes<max=8>"
label = "string<max=12>"
samples = "sequence<u32,max=4>"
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();

        let frames = message_frame_expectations(&ir).unwrap();
        let packet = frames
            .iter()
            .find(|frame| frame.type_name == "Packet")
            .unwrap();
        assert!(packet.variable);
        assert_eq!(packet.header_size_bytes, 24);
        assert_eq!(packet.max_size_bytes, Some(60));
        assert_eq!(packet.fields[0].tail_max_bytes, Some(8));
        assert_eq!(packet.fields[1].tail_max_bytes, Some(12));
        assert_eq!(packet.fields[2].tail_max_bytes, Some(16));

        let slots = iox2_frame_slot_expectations(&ir).unwrap();
        assert_eq!(
            slots,
            [Iox2FrameSlotExpectation {
                type_name: "Packet".to_string(),
                cap_bytes: 60,
            }]
        );
    }

    #[test]
    fn leaves_unbounded_variable_frames_without_iox2_slot_expectation() {
        let source = r#"
[package]
name = "demo"
rsdl_version = "0.1"

[type.Packet]
payload = "bytes"
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();

        let frames = message_frame_expectations(&ir).unwrap();
        assert_eq!(frames[0].type_name, "Packet");
        assert!(frames[0].variable);
        assert_eq!(frames[0].header_size_bytes, 8);
        assert_eq!(frames[0].max_size_bytes, None);
        assert_eq!(iox2_frame_slot_expectations(&ir).unwrap(), []);
    }
}
