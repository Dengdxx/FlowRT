//! RSDL v0.1 parser 和源 AST。
//!
//! v0.1 的 `.rsdl` 是 TOML 子集。本 crate 只负责把源文本解析成保留语法结构的
//! `RawDocument`，不在这里做跨表引用解析或语义归一化。

mod ast;
mod error;
mod parser;

pub use ast::*;
pub use error::{Result, RsdlError};
pub use parser::{parse_file, parse_str};
