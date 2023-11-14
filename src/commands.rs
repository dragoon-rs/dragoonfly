use axum::extract::{Path, State};
use axum::response::{IntoResponse, Json, Response};
use futures::SinkExt;
use futures::channel::oneshot;
use libp2p_core::transport::ListenerId;
use libp2p_core::Multiaddr;
use libp2p_core::PeerId;
use std::error::Error;
use std::sync::Arc;
use tracing::error;

use crate::app::AppState;

#[derive(Debug)]
pub enum DragoonCommand {
    Listen {
        multiaddr: String,
        sender: oneshot::Sender<Result<ListenerId, Box<dyn Error + Send>>>,
    },
    GetListeners {
        sender: oneshot::Sender<Result<Vec<Multiaddr>, Box<dyn Error + Send>>>,
    },
    GetPeerId {
        sender: oneshot::Sender<Result<PeerId, Box<dyn Error + Send>>>,
    },
}

pub async fn listen(Path(multiaddr): Path<String>, State(state): State<Arc<AppState>>) -> Response {
    let (sender, receiver) = oneshot::channel();

    let mut cmd_sender = state.sender.lock().await;

    if let Err(e) = cmd_sender
        .send(DragoonCommand::Listen { multiaddr, sender })
        .await
    {
        error!("Cannot send command Listen: {:?}", e);
    }

    match receiver.await {
        Err(e) => {
            error!("Cannot receive a return from command Listen: {:?}", e);
            Json("").into_response()
        }
        Ok(res) => match res {
            Err(e) => {
                error!("Listen returned an error: {:?}", e);
                Json("").into_response()
            }
            Ok(listener_id) => Json(format!("{:?}", listener_id)).into_response(),
        },
    }
}

pub async fn get_listeners(State(state): State<Arc<AppState>>) -> Response {
    let (sender, receiver) = oneshot::channel();

    let mut cmd_sender = state.sender.lock().await;

    if let Err(e) = cmd_sender
        .send(DragoonCommand::GetListeners { sender })
        .await
    {
        error!("Cannot send command GetListener: {:?}", e);
    }

    match receiver.await {
        Err(e) => {
            error!("Cannot receive a return from command GetListener: {:?}", e);
            Json("").into_response()
        }
        Ok(res) => match res {
            Err(e) => {
                error!("GetListener returned an error: {:?}", e);
                Json("").into_response()
            }
            Ok(listeners) => Json(listeners).into_response(),
        },
    }
}

pub async fn get_peer_id(State(state): State<Arc<AppState>>) -> Response {
    let (sender, receiver) = oneshot::channel();

    let mut cmd_sender = state.sender.lock().await;

    if let Err(e) = cmd_sender.send(DragoonCommand::GetPeerId { sender }).await {
        error!("Cannot send command GetPeerId: {:?}", e);
    }

    match receiver.await {
        Err(e) => {
            error!("Cannot receive a return from command GetPeerId: {:?}", e);
            Json("").into_response()
        }
        Ok(res) => match res {
            Err(e) => {
                error!("GetPeerId returned an error: {:?}", e);
                Json("").into_response()
            }
            Ok(peer_id) => Json(peer_id.to_base58()).into_response(),
        },
    }
}
