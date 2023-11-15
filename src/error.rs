use std::fmt::{Debug, Formatter};
use axum::http::StatusCode;
use axum::Json;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use thiserror::Error;

#[derive(Clone, Error, PartialEq)]
pub enum DragoonError {
    #[error("Bad listener given")]
    BadListener,
    #[error("unexpected error from Dragoon")]
    UnexpectedError
}

impl Debug for DragoonError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}", self)?;
        Ok(())
    }
}

impl IntoResponse for DragoonError {
    fn into_response(self) -> Response {
        let (status, err_msg) = match self {
            DragoonError::UnexpectedError => (StatusCode::BAD_REQUEST, self.to_string()),
            DragoonError::BadListener => (StatusCode::BAD_REQUEST, self.to_string()),
        };
        (status, Json(json!({"error": err_msg}))).into_response()
    }
}