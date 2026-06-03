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

    #[error("missing required table `[package]`")]
    MissingPackage,

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
