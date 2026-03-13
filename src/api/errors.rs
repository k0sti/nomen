//! Structured error model for the Nomen API v2.

use std::fmt;

#[derive(Debug)]
pub struct ApiError {
    code: ErrorCode,
    message: String,
}

#[derive(Debug, Clone, Copy)]
pub enum ErrorCode {
    InvalidParams,
    InvalidScope,
    NotFound,
    Unauthorized,
    RateLimited,
    InternalError,
    UnknownAction,
}

impl ErrorCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InvalidParams => "invalid_params",
            Self::InvalidScope => "invalid_scope",
            Self::NotFound => "not_found",
            Self::Unauthorized => "unauthorized",
            Self::RateLimited => "rate_limited",
            Self::InternalError => "internal_error",
            Self::UnknownAction => "unknown_action",
        }
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for ApiError {}

impl ApiError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn code(&self) -> &str {
        self.code.as_str()
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    // Convenience constructors

    pub fn invalid_params(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidParams, msg)
    }

    pub fn invalid_scope(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidScope, msg)
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, msg)
    }

    pub fn unauthorized(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::Unauthorized, msg)
    }

    pub fn rate_limited(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::RateLimited, msg)
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::InternalError, msg)
    }

    pub fn unknown_action(action: &str) -> Self {
        Self::new(
            ErrorCode::UnknownAction,
            format!("Unknown action: {action}"),
        )
    }

    /// Convert from anyhow::Error, preserving message.
    pub fn from_anyhow(err: anyhow::Error) -> Self {
        Self::internal(err.to_string())
    }
}
