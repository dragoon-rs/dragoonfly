use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use futures::channel::oneshot::{self, Canceled};
use futures::SinkExt;
use libp2p_core::transport::ListenerId;
use libp2p_core::Multiaddr;
use libp2p_core::PeerId;
use libp2p_swarm::NetworkInfo;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt::Debug;
use std::sync::Arc;
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
// - is_connected
//
// - behaviour
#[derive(Debug)]
pub(crate) enum DragoonCommand {
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
    RemoveListener {
        listener_id: u64,
        sender: oneshot::Sender<Result<bool, Box<dyn Error + Send>>>,
    },
    GetConnectedPeers {
        sender: oneshot::Sender<Result<Vec<PeerId>, Box<dyn Error + Send>>>,
    },
}

pub(crate) async fn listen(
    Path(multiaddr): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    let (sender, receiver) = oneshot::channel();

    send_command(
        DragoonCommand::Listen { multiaddr, sender },
        "listen",
        state,
    )
    .await;

    match receiver.await {
        Err(e) => handle_canceled(e, "listen"),
        Ok(res) => match res {
            Err(e) => handle_dragoon_error(e, "listen"),
            Ok(listener_id) => (StatusCode::OK, Json(format!("{:?}", listener_id))).into_response(),
        },
    }
}

pub(crate) async fn get_listeners(State(state): State<Arc<AppState>>) -> Response {
    let (sender, receiver) = oneshot::channel();

    send_command(
        DragoonCommand::GetListeners { sender },
        "get-listeners",
        state,
    )
    .await;

    match receiver.await {
        Err(e) => handle_canceled(e, "get-listeners"),
        Ok(res) => match res {
            Err(e) => handle_dragoon_error(e, "get-listeners"),
            Ok(listeners) => Json(listeners).into_response(),
        },
    }
}

pub(crate) async fn get_peer_id(State(state): State<Arc<AppState>>) -> Response {
    let (sender, receiver) = oneshot::channel();

    send_command(DragoonCommand::GetPeerId { sender }, "get-peer-id", state).await;

    match receiver.await {
        Err(e) => handle_canceled(e, "get-peer-id"),
        Ok(res) => match res {
            Err(e) => handle_dragoon_error(e, "get-peer-id"),
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

pub(crate) async fn get_network_info(State(state): State<Arc<AppState>>) -> Response {
    let (sender, receiver) = oneshot::channel();

    send_command(
        DragoonCommand::GetNetworkInfo { sender },
        "get-network-info",
        state,
    )
    .await;

    match receiver.await {
        Err(e) => handle_canceled(e, "get-network-info"),
        Ok(res) => match res {
            Err(e) => handle_dragoon_error(e, "get-network-info"),
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

pub(crate) async fn remove_listener(
    Path(listener_id): Path<u64>,
    State(state): State<Arc<AppState>>,
) -> Response {
    let (sender, receiver) = oneshot::channel();

    send_command(
        DragoonCommand::RemoveListener {
            listener_id,
            sender,
        },
        "remove-listener",
        state,
    )
    .await;

    match receiver.await {
        Err(e) => handle_canceled(e, "remove-listener"),
        Ok(res) => match res {
            Err(e) => handle_dragoon_error(e, "remove-listener"),
            Ok(good) => Json(good).into_response(),
        },
    }
}

pub(crate) async fn get_connected_peers(State(state): State<Arc<AppState>>) -> Response {
    let (sender, receiver) = oneshot::channel();

    send_command(
        DragoonCommand::GetConnectedPeers { sender },
        "get-connected-peers",
        state,
    )
    .await;

    match receiver.await {
        Err(e) => handle_canceled(e, "get-connected-peers"),
        Ok(res) => match res {
            Err(e) => handle_dragoon_error(e, "get-listener"),
            Ok(connected_peers) => Json(
                connected_peers
                    .iter()
                    .map(|p| p.to_base58())
                    .collect::<Vec<String>>(),
            )
            .into_response(),
        },
    }
}

fn handle_dragoon_error(err: Box<dyn Error + Send>, command: &str) -> Response {
    if let Ok(dragoon_error) = err.downcast::<DragoonError>() {
        dragoon_error.into_response()
    } else {
        error!("cannot get return message from command {}", command);
        DragoonError::UnexpectedError.into_response()
    }
}

fn handle_canceled(err: Canceled, command: &str) -> Response {
    error!(
        "Cannot receive a return from command {}: {:?}",
        command, err
    );
    DragoonError::UnexpectedError.into_response()
}

async fn send_command(
    dragoon_command: DragoonCommand,
    command: &str,
    state: Arc<AppState>,
) -> Option<Response> {
    let mut cmd_sender = state.sender.lock().await;

    if let Err(e) = cmd_sender.send(dragoon_command).await {
        error!("Cannot send command {}: {:?}", command, e);
        return Some(DragoonError::UnexpectedError.into_response());
    }

    return None;
}
