use std::fmt;

/// Extension 执行错误。
/// 单个 extension 错误不会中断管道——registry 内部 catch 并 log::warn。
#[derive(Debug)]
pub struct ExtensionError {
    pub extension_name: String,
    pub message: String,
    pub kind: ExtensionErrorKind,
}

#[derive(Debug)]
pub enum ExtensionErrorKind {
    /// JSON 解析/结构访问失败
    Json(String),
    /// 业务逻辑错误
    Logic(String),
    /// I/O 错误（如文件写入失败）
    Io(std::io::Error),
}

impl fmt::Display for ExtensionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] {}: {}",
            self.extension_name,
            match &self.kind {
                ExtensionErrorKind::Json(_) => "JSON",
                ExtensionErrorKind::Logic(_) => "Logic",
                ExtensionErrorKind::Io(_) => "IO",
            },
            self.message
        )
    }
}

impl std::error::Error for ExtensionError {}

impl ExtensionError {
    pub fn json(name: &str, msg: impl Into<String>) -> Self {
        let msg = msg.into();
        Self {
            extension_name: name.to_string(),
            message: msg.clone(),
            kind: ExtensionErrorKind::Json(msg),
        }
    }

    pub fn logic(name: &str, msg: impl Into<String>) -> Self {
        let msg = msg.into();
        Self {
            extension_name: name.to_string(),
            message: msg,
            kind: ExtensionErrorKind::Logic("".into()),
        }
    }

    pub fn io(name: &str, err: std::io::Error) -> Self {
        Self {
            extension_name: name.to_string(),
            message: err.to_string(),
            kind: ExtensionErrorKind::Io(err),
        }
    }
}
