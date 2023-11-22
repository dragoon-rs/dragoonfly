use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use futures::channel::oneshot::{self, Canceled};
use futures::SinkExt;
use libp2p::swarm::NetworkInfo;
use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::error::Error;
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
        sender: oneshot::Sender<Result<u64, Box<dyn Error + Send>>>,
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
    Dial {
        multiaddr: String,
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>,
    },
    AddPeer {
        multiaddr: String,
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>,
    },
    StartProvide {
        key: String,
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>,
    },
    GetProviders {
        key: String,
        sender: oneshot::Sender<Result<HashSet<PeerId>, Box<dyn Error + Send>>>,
    },
    Bootstrap {
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>,
    },
}

impl std::fmt::Display for DragoonCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            DragoonCommand::Listen { .. } => write!(f, "listen"),
            DragoonCommand::GetListeners { .. } => write!(f, "get-listener"),
            DragoonCommand::GetPeerId { .. } => write!(f, "get-peer-id"),
            DragoonCommand::GetNetworkInfo { .. } => write!(f, "get-network-info"),
            DragoonCommand::RemoveListener { .. } => write!(f, "remove-listener"),
            DragoonCommand::GetConnectedPeers { .. } => write!(f, "get-connected-peers"),
            DragoonCommand::Dial { .. } => write!(f, "dial"),
            DragoonCommand::AddPeer { .. } => write!(f, "add-peer"),
            DragoonCommand::StartProvide { .. } => write!(f, "start-provide"),
            DragoonCommand::GetProviders { .. } => write!(f, "get-providers"),
            DragoonCommand::Bootstrap { .. } => write!(f, "bootstrap"),
        }
    }
}

pub(crate) async fn listen(
    Path(multiaddr): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    let (sender, receiver) = oneshot::channel();

    let cmd = DragoonCommand::Listen { multiaddr, sender };
    let cmd_name = cmd.to_string();
    send_command(cmd, state).await;

    match receiver.await {
        Err(e) => handle_canceled(e, &cmd_name),
        Ok(res) => match res {
            Err(e) => handle_dragoon_error(e, &cmd_name),
            Ok(listener_id) => (StatusCode::OK, Json(format!("{:?}", listener_id))).into_response(),
        },
    }
}

pub(crate) async fn get_listeners(State(state): State<Arc<AppState>>) -> Response {
    let (sender, receiver) = oneshot::channel();

    let cmd = DragoonCommand::GetListeners { sender };
    let cmd_name = cmd.to_string();
    send_command(cmd, state).await;

    match receiver.await {
        Err(e) => handle_canceled(e, &cmd_name),
        Ok(res) => match res {
            Err(e) => handle_dragoon_error(e, &cmd_name),
            Ok(listeners) => (StatusCode::OK, Json(listeners)).into_response(),
        },
    }
}

pub(crate) async fn get_peer_id(State(state): State<Arc<AppState>>) -> Response {
    let (sender, receiver) = oneshot::channel();

    let cmd = DragoonCommand::GetPeerId { sender };
    let cmd_name = cmd.to_string();
    send_command(cmd, state).await;

    match receiver.await {
        Err(e) => handle_canceled(e, &cmd_name),
        Ok(res) => match res {
            Err(e) => handle_dragoon_error(e, &cmd_name),
            Ok(peer_id) => (StatusCode::OK, Json(peer_id.to_base58())).into_response(),
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

    let cmd = DragoonCommand::GetNetworkInfo { sender };
    let cmd_name = cmd.to_string();
    send_command(cmd, state).await;

    match receiver.await {
        Err(e) => handle_canceled(e, &cmd_name),
        Ok(res) => match res {
            Err(e) => handle_dragoon_error(e, &cmd_name),
            Ok(network_info) => {
                let connections = network_info.connection_counters();
                (
                    StatusCode::OK,
                    Json(SerNetworkInfo {
                        peers: network_info.num_peers(),
                        pending: connections.num_pending(),
                        connections: connections.num_connections(),
                        established: connections.num_established(),
                        pending_incoming: connections.num_pending_incoming(),
                        pending_outgoing: connections.num_pending_outgoing(),
                        established_incoming: connections.num_established_incoming(),
                        established_outgoing: connections.num_established_outgoing(),
                    }),
                )
                    .into_response()
            }
        },
    }
}

pub(crate) async fn remove_listener(
    Path(listener_id): Path<u64>,
    State(state): State<Arc<AppState>>,
) -> Response {
    let (sender, receiver) = oneshot::channel();

    let cmd = DragoonCommand::RemoveListener {
        listener_id,
        sender,
    };
    let cmd_name = cmd.to_string();
    send_command(cmd, state).await;

    match receiver.await {
        Err(e) => handle_canceled(e, &cmd_name),
        Ok(res) => match res {
            Err(e) => handle_dragoon_error(e, &cmd_name),
            Ok(good) => (StatusCode::OK, Json(good)).into_response(),
        },
    }
}

pub(crate) async fn get_connected_peers(State(state): State<Arc<AppState>>) -> Response {
    let (sender, receiver) = oneshot::channel();

    let cmd = DragoonCommand::GetConnectedPeers { sender };
    let cmd_name = cmd.to_string();
    send_command(cmd, state).await;

    match receiver.await {
        Err(e) => handle_canceled(e, &cmd_name),
        Ok(res) => match res {
            Err(e) => handle_dragoon_error(e, &cmd_name),
            Ok(connected_peers) => (
                StatusCode::OK,
                Json(
                    connected_peers
                        .iter()
                        .map(|p| p.to_base58())
                        .collect::<Vec<String>>(),
                ),
            )
                .into_response(),
        },
    }
}

pub(crate) async fn dial(
    Path(multiaddr): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    let (sender, receiver) = oneshot::channel();

    let cmd = DragoonCommand::Dial { multiaddr, sender };
    let cmd_name = cmd.to_string();
    send_command(cmd, state).await;

    match receiver.await {
        Err(e) => handle_canceled(e, &cmd_name),
        Ok(res) => match res {
            Err(e) => handle_dragoon_error(e, &cmd_name),
            Ok(_) => (StatusCode::OK, Json("")).into_response(),
        },
    }
}

pub(crate) async fn add_peer(
    Path(multiaddr): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    let (sender, receiver) = oneshot::channel();

    let cmd = DragoonCommand::AddPeer { multiaddr, sender };
    let cmd_name = cmd.to_string();
    send_command(cmd, state).await;

    match receiver.await {
        Err(e) => handle_canceled(e, &cmd_name),
        Ok(res) => match res {
            Err(e) => handle_dragoon_error(e, &cmd_name),
            Ok(_) => (StatusCode::OK, Json("")).into_response(),
        },
    }
}

pub(crate) async fn start_provide(
    Path(key): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    let (sender, receiver) = oneshot::channel();
    let cmd = DragoonCommand::StartProvide { key, sender };
    let cmd_name = cmd.to_string();
    send_command(cmd, state).await;

    match receiver.await {
        Err(e) => handle_canceled(e, &cmd_name),
        Ok(res) => match res {
            Err(e) => handle_dragoon_error(e, &cmd_name),
            Ok(_) => (StatusCode::OK, Json("")).into_response(),
        },
    }
}

pub(crate) async fn get_providers(
    Path(key): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    let (sender, receiver) = oneshot::channel();
    let cmd = DragoonCommand::GetProviders { key, sender };
    let cmd_name = cmd.to_string();
    send_command(cmd, state).await;

    match receiver.await {
        Err(e) => handle_canceled(e, &cmd_name),
        Ok(res) => match res {
            Err(e) => handle_dragoon_error(e, &cmd_name),
            Ok(providers) => (
                StatusCode::OK,
                Json(
                    providers
                        .iter()
                        .map(|peer| peer.to_base58())
                        .collect::<Vec<String>>(),
                ),
            )
                .into_response(),
        },
    }
}

pub(crate) async fn bootstrap(State(state): State<Arc<AppState>>) -> Response {
    let (sender, receiver) = oneshot::channel();
    let cmd = DragoonCommand::Bootstrap { sender };
    let cmd_name = cmd.to_string();
    send_command(cmd, state).await;

    match receiver.await {
        Err(e) => handle_canceled(e, &cmd_name),
        Ok(res) => match res {
            Err(e) => handle_dragoon_error(e, &cmd_name),
            Ok(_) => (StatusCode::OK, Json("")).into_response(),
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

async fn send_command(command: DragoonCommand, state: Arc<AppState>) -> Option<Response> {
    let mut cmd_sender = state.sender.lock().await;

    let cmd_name = format!("{}", command);

    if let Err(e) = cmd_sender.send(command).await {
        error!("Cannot send command {}: {:?}", cmd_name, e);
        return Some(DragoonError::UnexpectedError.into_response());
    }

    return None;
}
