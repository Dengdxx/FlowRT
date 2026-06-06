use std::path::PathBuf;

/// RSDL parser 使用的结果类型。
pub type Result<T> = std::result::Result<T, RsdlError>;

/// 解析 `.rsdl` 源文件时产生的错误。
#[derive(Debug, thiserror::Error)]
pub enum RsdlError {
    #[error("failed to read `{path}`: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse TOML: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("invalid import path `{pattern}` in `{importer}`: {message}")]
    InvalidImportPath {
        importer: PathBuf,
        pattern: String,
        message: String,
    },

    #[error("import pattern `{pattern}` in `{importer}` did not match any .rsdl file")]
    ImportPatternNoMatches { importer: PathBuf, pattern: String },

    #[error("duplicate `{kind}` symbol `{name}` while merging imported RSDL")]
    DuplicateSymbol { kind: &'static str, name: String },

    #[error("duplicate module `{module}` while loading workspace")]
    DuplicateModule { module: String },

    #[error("module source `{path}` must declare `[module]`")]
    MissingModule { path: PathBuf },

    #[error(
        "module `{module}` in `{path}` may only declare type and component tables; found `{section}`"
    )]
    InvalidModuleSection {
        path: PathBuf,
        module: String,
        section: String,
    },

    #[error("composition source `{path}` must not declare `[module]`")]
    UnexpectedModule { path: PathBuf },

    #[error("missing required table `[package]`")]
    MissingPackage,

    #[error("unknown top-level RSDL section `{section}`")]
    UnknownTopLevelSection { section: String },

    #[error("unknown field `{field}` in `{context}`")]
    UnknownField { context: String, field: String },

    #[error("missing required field `{field}` in `{context}`")]
    MissingField {
        context: String,
        field: &'static str,
    },

    #[error("invalid field `{field}` in `{context}`: expected {expected}")]
    InvalidFieldType {
        context: String,
        field: String,
        expected: &'static str,
    },

    #[error("invalid value in `{context}`: {message}")]
    InvalidValue { context: String, message: String },

    #[error("invalid port descriptor `{descriptor}`; expected `<port_name>:<type_expr>`")]
    InvalidPortDescriptor { descriptor: String },
}
