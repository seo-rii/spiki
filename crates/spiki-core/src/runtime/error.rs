use serde_json::json;
use thiserror::Error;

use crate::model::ExecutionError;

pub type SpikiResult<T> = Result<T, SpikiError>;

#[derive(Debug, Error, Clone)]
#[error("{code}: {message}")]
pub struct SpikiError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy)]
pub enum SpikiCode {
    InvalidRequest,
    Forbidden,
    NotFound,
    StalePlan,
    Conflict,
    Unsupported,
    Internal,
}

pub fn spiki_error(code: SpikiCode, message: impl Into<String>) -> SpikiError {
    let (code, retryable) = match code {
        SpikiCode::InvalidRequest => ("AE_INVALID_REQUEST", false),
        SpikiCode::Forbidden => ("AE_FORBIDDEN", false),
        SpikiCode::NotFound => ("AE_NOT_FOUND", false),
        SpikiCode::StalePlan => ("AE_STALE_PLAN", true),
        SpikiCode::Conflict => ("AE_CONFLICT", true),
        SpikiCode::Unsupported => ("AE_UNSUPPORTED", false),
        SpikiCode::Internal => ("AE_INTERNAL", true),
    };

    SpikiError {
        code: code.to_string(),
        message: message.into(),
        retryable,
        details: Some(json!({})),
    }
}

impl From<SpikiError> for ExecutionError {
    fn from(error: SpikiError) -> Self {
        ExecutionError {
            code: error.code,
            message: error.message,
            retryable: error.retryable,
            details: error.details,
        }
    }
}
