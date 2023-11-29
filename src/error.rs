use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use thiserror::Error;

#[derive(Clone, Debug, Error, PartialEq)]
pub enum DragoonError {
    #[error("Bad listener given")]
    BadListener(String),
    #[error("Could not dial a peer")]
    DialError(String),
    #[error("unexpected error from Dragoon")]
    UnexpectedError,
    #[error("Could not provide")]
    ProviderError(String),
    #[error("Bootstrap error")]
    BootstrapError(String),
    #[error("Peer not connected")]
    PeerNotFound,
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
            DragoonError::ProviderError(ref msg) => {
                (StatusCode::BAD_REQUEST, format!("{}: {}", self, msg))
            }
            DragoonError::BootstrapError(ref msg) => {
                (StatusCode::BAD_REQUEST, format!("{}: {}", self, msg))
            }
            DragoonError::PeerNotFound => (StatusCode::BAD_REQUEST,self.to_string())
        };
        (status, Json(format!("{}", err_msg))).into_response()
    }
}
