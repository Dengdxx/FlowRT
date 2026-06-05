/// Contract IR 归一化使用的结果类型。
pub type Result<T> = std::result::Result<T, IrError>;

/// 构建 normalized Contract IR 时产生的错误。
#[derive(Debug, thiserror::Error)]
pub enum IrError {
    #[error("invalid `{kind}` value `{value}` in `{context}`")]
    InvalidEnum {
        context: String,
        kind: &'static str,
        value: String,
    },

    #[error("invalid type expression `{expr}`: {message}")]
    InvalidTypeExpr { expr: String, message: String },

    #[error("unknown component `{component}` referenced by instance `{instance}`")]
    UnknownComponent { instance: String, component: String },

    #[error("unknown target `{target}` referenced by instance `{instance}`")]
    UnknownTarget { instance: String, target: String },

    #[error("unknown profile `{profile}` referenced by contract")]
    UnknownProfile { profile: String },

    #[error(
        "unknown parameter `{param}` override on instance `{instance}` of component `{component}`"
    )]
    UnknownParamOverride {
        instance: String,
        component: String,
        param: String,
    },

    #[error(
        "incompatible parameter `{param}` override on instance `{instance}` of component `{component}`: expected {expected}, got {actual}"
    )]
    IncompatibleParamOverride {
        instance: String,
        component: String,
        param: String,
        expected: &'static str,
        actual: &'static str,
    },

    #[error("invalid parameter `{param}` schema on component `{component}`: {message}")]
    InvalidParamSchema {
        component: String,
        param: String,
        message: String,
    },

    #[error("invalid port endpoint `{endpoint}`; expected `<instance>.<port>`")]
    InvalidPortEndpoint { endpoint: String },

    #[error("unknown instance `{instance}` referenced by endpoint `{endpoint}`")]
    UnknownEndpointInstance { endpoint: String, instance: String },
}
