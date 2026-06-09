use std::collections::BTreeMap;

use flowrt_rsdl::{RawDocument, RawModuleDocument};

use crate::{IrError, Result, TypeExpr};

#[derive(Debug, Clone)]
pub(super) struct SymbolInfo {
    pub(super) module: Option<String>,
    pub(super) name: String,
    pub(super) qualified_name: String,
    pub(super) generated_name: String,
}

#[derive(Debug)]
pub(super) struct NameResolver {
    type_symbols: BTreeMap<String, SymbolInfo>,
    component_symbols: BTreeMap<String, SymbolInfo>,
    type_short_names: BTreeMap<String, Vec<String>>,
    component_short_names: BTreeMap<String, Vec<String>>,
}

impl NameResolver {
    pub(super) fn new(raw_modules: &[RawModuleDocument]) -> Self {
        let mut type_symbols = BTreeMap::new();
        let mut component_symbols = BTreeMap::new();
        let mut type_short_names = BTreeMap::<String, Vec<String>>::new();
        let mut component_short_names = BTreeMap::<String, Vec<String>>::new();

        for module in raw_modules {
            let module_name = module.module.name.as_str();
            for name in module.types.keys() {
                let info = SymbolInfo::module(module_name, name);
                type_short_names
                    .entry(name.clone())
                    .or_default()
                    .push(info.qualified_name.clone());
                type_symbols.insert(info.qualified_name.clone(), info);
            }
            for name in module.components.keys() {
                let info = SymbolInfo::module(module_name, name);
                component_short_names
                    .entry(name.clone())
                    .or_default()
                    .push(info.qualified_name.clone());
                component_symbols.insert(info.qualified_name.clone(), info);
            }
        }

        Self {
            type_symbols,
            component_symbols,
            type_short_names,
            component_short_names,
        }
    }

    pub(super) fn register_document_symbols(&mut self, document: &RawDocument) {
        for name in document.types.keys() {
            let info = SymbolInfo::root(name);
            self.type_short_names
                .entry(name.clone())
                .or_default()
                .push(info.qualified_name.clone());
            self.type_symbols.insert(info.qualified_name.clone(), info);
        }
        for name in document.components.keys() {
            let info = SymbolInfo::root(name);
            self.component_short_names
                .entry(name.clone())
                .or_default()
                .push(info.qualified_name.clone());
            self.component_symbols
                .insert(info.qualified_name.clone(), info);
        }
    }

    pub(super) fn type_info_for_decl(&self, name: &str) -> SymbolInfo {
        self.type_symbols
            .get(name)
            .cloned()
            .or_else(|| {
                self.resolve_short_unique("type", name, &self.type_symbols, &self.type_short_names)
                    .ok()
            })
            .unwrap_or_else(|| SymbolInfo::root(name))
    }

    pub(super) fn component_info_for_decl(&self, name: &str) -> SymbolInfo {
        self.component_symbols
            .get(name)
            .cloned()
            .or_else(|| {
                self.resolve_short_unique(
                    "component",
                    name,
                    &self.component_symbols,
                    &self.component_short_names,
                )
                .ok()
            })
            .unwrap_or_else(|| SymbolInfo::root(name))
    }

    pub(super) fn resolve_type_expr_in_module(
        &self,
        expr: TypeExpr,
        current_module: Option<&str>,
    ) -> Result<TypeExpr> {
        match expr {
            TypeExpr::Named { name } => Ok(TypeExpr::Named {
                name: self
                    .resolve_type_in_module(&name, current_module)?
                    .qualified_name,
            }),
            TypeExpr::Array { element, len } => Ok(TypeExpr::Array {
                element: Box::new(self.resolve_type_expr_in_module(*element, current_module)?),
                len,
            }),
            TypeExpr::VarSequence { element } => Ok(TypeExpr::VarSequence {
                element: Box::new(self.resolve_type_expr_in_module(*element, current_module)?),
            }),
            TypeExpr::Primitive { .. } | TypeExpr::VarBytes | TypeExpr::VarString { .. } => {
                Ok(expr)
            }
        }
    }

    pub(super) fn resolve_component(&self, name: &str) -> Result<SymbolInfo> {
        self.resolve_symbol(
            "component",
            name,
            &self.component_symbols,
            &self.component_short_names,
        )
    }

    fn resolve_type(&self, name: &str) -> Result<SymbolInfo> {
        self.resolve_symbol("type", name, &self.type_symbols, &self.type_short_names)
    }

    fn resolve_type_in_module(
        &self,
        name: &str,
        current_module: Option<&str>,
    ) -> Result<SymbolInfo> {
        if name.contains("::") {
            return self.resolve_type(name);
        }
        if let Some(module) = current_module {
            let local_name = format!("{module}::{name}");
            if let Some(info) = self.type_symbols.get(&local_name) {
                return Ok(info.clone());
            }
        }
        self.resolve_type(name)
    }

    fn resolve_symbol(
        &self,
        kind: &'static str,
        name: &str,
        symbols: &BTreeMap<String, SymbolInfo>,
        short_names: &BTreeMap<String, Vec<String>>,
    ) -> Result<SymbolInfo> {
        if name.contains("::") {
            let Some((module, short)) = name.split_once("::") else {
                unreachable!("contains checked above")
            };
            let Some(info) = symbols.get(name).cloned() else {
                let module_exists = symbols
                    .values()
                    .any(|info| info.module.as_deref() == Some(module));
                if !module_exists {
                    return Err(IrError::UnknownModule {
                        kind,
                        name: short.to_string(),
                        module: module.to_string(),
                    });
                }
                return Err(IrError::InvalidValue {
                    context: format!("{kind} reference `{name}`"),
                    message: "qualified symbol does not exist".to_string(),
                });
            };
            return Ok(info);
        }

        self.resolve_short_unique(kind, name, symbols, short_names)
    }

    fn resolve_short_unique(
        &self,
        kind: &'static str,
        name: &str,
        symbols: &BTreeMap<String, SymbolInfo>,
        short_names: &BTreeMap<String, Vec<String>>,
    ) -> Result<SymbolInfo> {
        let Some(candidates) = short_names.get(name) else {
            return Err(IrError::InvalidValue {
                context: format!("{kind} reference `{name}`"),
                message: "symbol does not exist".to_string(),
            });
        };
        if candidates.len() != 1 {
            return Err(IrError::AmbiguousName {
                kind,
                name: name.to_string(),
                candidates: candidates.join(", "),
            });
        }
        symbols
            .get(&candidates[0])
            .cloned()
            .ok_or_else(|| IrError::InvalidValue {
                context: format!("{kind} reference `{name}`"),
                message: "symbol index is inconsistent".to_string(),
            })
    }
}

impl SymbolInfo {
    fn root(name: &str) -> Self {
        Self {
            module: None,
            name: name.to_string(),
            qualified_name: name.to_string(),
            generated_name: name.to_string(),
        }
    }

    fn module(module: &str, name: &str) -> Self {
        Self {
            module: Some(module.to_string()),
            name: name.to_string(),
            qualified_name: format!("{module}::{name}"),
            generated_name: crate::canonical_generated_symbol(Some(module), name),
        }
    }
}
