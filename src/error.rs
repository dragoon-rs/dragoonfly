use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use thiserror::Error;

use crate::send_strategy::SendId;

#[derive(Clone, Debug, Error, PartialEq)]
pub enum DragoonError {
    #[error("Bad listener given")]
    BadListener(String),
    #[error("Could not dial a peer")]
    DialError(String),
    #[error("unexpected error from Dragoon")]
    UnexpectedError(String),
    #[error("Could not provide")]
    ProviderError(String),
    #[error("Bootstrap error")]
    BootstrapError(String),
    #[error("The parent directory of the block directory (block_dir: {0}) either doesn't exist, or permissions are insufficient to write")]
    NoParentDirectory(String),
    #[error("The block response of block {0} for file {1} through channel {2} could not be sent (channel closed due to a timeout or the connection was closed)")]
    CouldNotSendBlockResponse(String, String, String),
    #[error("The peer block info response for file {0} through channel {1} could not be sent (channel closed due to a timeout or the connection was closed)")]
    CouldNotSendInfoResponse(String, String),
    #[error("The block {} of file {} could not be sent to {}", send_id.block_hash, send_id.file_hash, send_id.peer_id)]
    SendBlockToError { send_id: SendId },
    #[error("This SendBlockTo request to {:?} for file hash {} / block hash {} is already being handled", send_id.peer_id, send_id.file_hash, send_id.block_hash)]
    SendBlockToAlreadyStarted { send_id: SendId },
    #[error(
        "Send block list failed with a final block distribution of {:?}, due to {}",
        final_block_distribution,
        context
    )]
    SendBlockListFailed {
        final_block_distribution: Vec<SendId>,
        context: String,
    },
}

impl IntoResponse for DragoonError {
    fn into_response(self) -> Response {
        let (status, err_msg) = match self {
            DragoonError::UnexpectedError(ref msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("{}: {}", self, msg),
            ),
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
            DragoonError::NoParentDirectory(ref msg) => {
                (StatusCode::BAD_REQUEST, format!("{}: {}", self, msg))
            }
            DragoonError::CouldNotSendBlockResponse(block_hash, file_hash, channel_string) => {
                (StatusCode::REQUEST_TIMEOUT, format!("The block response of block {0} for file {1} through channel {2} could not be sent (channel closed due to a timeout or the connection was closed)", block_hash, file_hash, channel_string))
            }
            DragoonError::CouldNotSendInfoResponse(file_hash, channel_string) => {
                (StatusCode::REQUEST_TIMEOUT, format!("The peer block info response for file {0} through channel {1} could not be sent (channel closed due to a timeout or the connection was closed)", file_hash, channel_string))
            }
            DragoonError::SendBlockToError{send_id} => {
                (StatusCode::INTERNAL_SERVER_ERROR, format!("The block {} of file {} could not be sent to {}", send_id.block_hash, send_id.file_hash, send_id.peer_id))
            }
            DragoonError::SendBlockToAlreadyStarted{send_id} => {
                (StatusCode::TOO_MANY_REQUESTS, format!("This SendBlockTo request to {:?} for file hash {} / block hash {} is already being handled", send_id.peer_id, send_id.file_hash, send_id.block_hash))
            }
            DragoonError::SendBlockListFailed{final_block_distribution, context} => {
                (StatusCode::INTERNAL_SERVER_ERROR, format!("Send block list failed with a final block distribution of {:?}, due to {}", final_block_distribution, context))
            }
        };
        (status, Json(err_msg.to_string())).into_response()
    }
}
