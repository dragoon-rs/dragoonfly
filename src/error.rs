use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use std::fmt::{Debug, Formatter};
use thiserror::Error;

#[derive(Clone, Error, PartialEq)]
pub enum DragoonError {
    #[error("Bad listener given")]
    BadListener(String),
    #[error("Could not dial a peer")]
    DialError(String),
    #[error("unexpected error from Dragoon")]
    UnexpectedError,
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
            DragoonError::UnexpectedError => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            DragoonError::BadListener(ref msg) => {
                (StatusCode::BAD_REQUEST, format!("{}: {}", self, msg))
            }
            DragoonError::DialError(ref msg) => {
                (StatusCode::BAD_REQUEST, format!("{}: {}", self, msg))
            }
        };
        (status, Json(format!("{}", err_msg))).into_response()
    }
}

