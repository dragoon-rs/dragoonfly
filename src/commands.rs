use axum::extract::{Path, State};
use axum::response::{IntoResponse, Json, Response};
use futures::channel::oneshot;
use futures::SinkExt;
use libp2p_core::transport::ListenerId;
use libp2p_core::Multiaddr;
use libp2p_core::PeerId;
use libp2p_swarm::NetworkInfo;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt::Debug;
use std::sync::Arc;
use axum::http::StatusCode;
use serde_json::json;
use tracing::error;

use crate::app::AppState;
use crate::error::DragoonError;

// Potential other commands:
// - dial
//
// - external_addresses
// - add_external_address
// - remove_external_address
//
// - ban_peer_id
// - unban_peer_id
// - disconnect_peer_id
//
// - connected_peers
//
// - behaviour
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
    GetNetworkInfo {
        sender: oneshot::Sender<Result<NetworkInfo, Box<dyn Error + Send>>>,
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
        return DragoonError::UnexpectedError.into_response();
    }

    match receiver.await {
        Err(e) => {
            error!("Cannot receive a return from command Listen: {:?}", e);
            DragoonError::UnexpectedError.into_response()
        }
        Ok(res) => match res {
            Err(e) => {
                if let Ok(dragoon_error)= e.downcast::<DragoonError>() {
                    dragoon_error.into_response()
                } else {
                    error!("cannot get return message from command Listen");
                    DragoonError::UnexpectedError.into_response()
                }
            }
            Ok(listener_id) =>  {
                let str_id = format!("{:?}",listener_id);
                (StatusCode::OK,Json(json!({"result":str_id}))).into_response()
            },
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

#[derive(Serialize, Deserialize)]
struct SerNetworkInfo {
    peers: usize,
    pending: u32,
    connections: u32,
    established: u32,
    pending_incoming: u32,
    pending_outgoing: u32,
    established_incoming: u32,
    established_outgoing: u32,
}

pub async fn get_network_info(State(state): State<Arc<AppState>>) -> Response {
    let (sender, receiver) = oneshot::channel();

    let mut cmd_sender = state.sender.lock().await;

    if let Err(e) = cmd_sender
        .send(DragoonCommand::GetNetworkInfo { sender })
        .await
    {
        error!("Cannot send command GetNetworkInfo: {:?}", e);
    }

    match receiver.await {
        Err(e) => {
            error!(
                "Cannot receive a return from command GetNetworkInfo: {:?}",
                e
            );
            Json("").into_response()
        }
        Ok(res) => match res {
            Err(e) => {
                error!("GetNetworkInfo returned an error: {:?}", e);
                Json("").into_response()
            }
            Ok(network_info) => {
                let connections = network_info.connection_counters();
                Json(SerNetworkInfo {
                    peers: network_info.num_peers(),
                    pending: connections.num_pending(),
                    connections: connections.num_connections(),
                    established: connections.num_established(),
                    pending_incoming: connections.num_pending_incoming(),
                    pending_outgoing: connections.num_pending_outgoing(),
                    established_incoming: connections.num_established_incoming(),
                    established_outgoing: connections.num_established_outgoing(),
                })
            }
            .into_response(),
        },
    }
}
