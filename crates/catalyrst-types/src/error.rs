use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use thiserror::Error;

use crate::entity::{EntityId, EntityType, Pointer};

#[derive(Debug, Error)]
pub enum ContentError {
    #[error("entity validation failed: {reason}")]
    ValidationFailed { reason: String },

    #[error("missing content files: {hashes:?}")]
    MissingContent { hashes: Vec<String> },

    #[error("authentication failed: {reason}")]
    AuthenticationFailed { reason: String },

    #[error("entity {entity_id} is older than the current entity for pointers {pointers:?}")]
    EntityIsOlder {
        entity_id: EntityId,
        pointers: Vec<Pointer>,
    },

    #[error("unknown entity type: {entity_type}")]
    UnknownEntityType { entity_type: String },

    #[error("rate limited for entity type {entity_type}")]
    RateLimited { entity_type: EntityType },

    #[error("server is in read-only mode")]
    ReadOnly,

    #[error("entity not found: {entity_id}")]
    EntityNotFound { entity_id: EntityId },

    #[error("entity {entity_id} is denylisted")]
    Denylisted { entity_id: EntityId },

    #[error("storage error: {0}")]
    Storage(String),

    #[error("database error: {0}")]
    Database(String),

    #[error("internal error: {0}")]
    Internal(String),
}

pub type ContentResult<T> = Result<T, ContentError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailedDeploymentReason {
    BlockchainAccessCheck,
    ContentDownloadFailed,
    ValidationFailed,
    Other(String),
}

impl std::fmt::Display for FailedDeploymentReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FailedDeploymentReason::BlockchainAccessCheck => {
                write!(f, "blockchain_access_check")
            }
            FailedDeploymentReason::ContentDownloadFailed => {
                write!(f, "content_download_failed")
            }
            FailedDeploymentReason::ValidationFailed => write!(f, "validation_failed"),
            FailedDeploymentReason::Other(s) => write!(f, "{}", s),
        }
    }
}

#[derive(Debug, Error)]
#[error("The value of the {parameter} parameter is invalid: {value}")]
pub struct InvalidParameterError {
    pub parameter: String,
    pub value: String,
}

impl InvalidParameterError {
    pub fn new(parameter: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            parameter: parameter.into(),
            value: value.into(),
        }
    }
}

#[derive(Debug, Error)]
#[error("{message}")]
pub struct HttpError {
    pub code: u16,
    pub message: String,
}

impl HttpError {
    pub fn new(code: u16, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

#[derive(Debug, Error)]
pub enum MarketplaceApiError {
    #[error(transparent)]
    Http(#[from] HttpError),

    #[error(transparent)]
    InvalidParameter(#[from] InvalidParameterError),

    #[cfg(feature = "sqlx")]
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("{0}")]
    Internal(String),
}

impl MarketplaceApiError {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        MarketplaceApiError::Http(HttpError::new(400, msg))
    }
    pub fn not_found(msg: impl Into<String>) -> Self {
        MarketplaceApiError::Http(HttpError::new(404, msg))
    }
    pub fn internal(msg: impl Into<String>) -> Self {
        MarketplaceApiError::Internal(msg.into())
    }
}

impl IntoResponse for MarketplaceApiError {
    fn into_response(self) -> Response {
        let (code, message) = match &self {
            MarketplaceApiError::Http(HttpError { code, message }) => (*code, message.clone()),
            MarketplaceApiError::InvalidParameter(e) => (400u16, e.to_string()),
            #[cfg(feature = "sqlx")]
            MarketplaceApiError::Database(e) => {
                tracing::error!(error = %e, "sqlx error");
                (500, "database error".to_string())
            }
            MarketplaceApiError::Internal(s) => (500, s.clone()),
        };
        let status = StatusCode::from_u16(code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let body = json!({ "ok": false, "message": message });
        (status, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_error_display() {
        let err = ContentError::ValidationFailed {
            reason: "bad metadata".into(),
        };
        assert_eq!(err.to_string(), "entity validation failed: bad metadata");
    }

    #[test]
    fn failed_deployment_reason_display() {
        assert_eq!(
            FailedDeploymentReason::BlockchainAccessCheck.to_string(),
            "blockchain_access_check"
        );
    }

    #[test]
    fn invalid_parameter_display() {
        let err = InvalidParameterError::new("first", "abc");
        assert_eq!(
            err.to_string(),
            "The value of the first parameter is invalid: abc"
        );
    }

    #[test]
    fn http_error_display() {
        let err = HttpError::new(404, "missing");
        assert_eq!(err.to_string(), "missing");
        assert_eq!(err.code, 404);
    }

    #[test]
    fn marketplace_api_error_helpers() {
        let bad = MarketplaceApiError::bad_request("oops");
        let nf = MarketplaceApiError::not_found("gone");
        let int = MarketplaceApiError::internal("boom");
        assert!(matches!(
            bad,
            MarketplaceApiError::Http(HttpError { code: 400, .. })
        ));
        assert!(matches!(
            nf,
            MarketplaceApiError::Http(HttpError { code: 404, .. })
        ));
        assert!(matches!(int, MarketplaceApiError::Internal(_)));
    }
}
