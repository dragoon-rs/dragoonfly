use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use futures::channel::oneshot::{self, Canceled};
use futures::SinkExt;
#[cfg(feature = "file-sharing")]
use futures::StreamExt;
#[cfg(feature = "file-sharing")]
use libp2p::request_response::ResponseChannel;
use libp2p::swarm::NetworkInfo;
use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::error::Error;
use std::sync::Arc;
#[cfg(feature = "file-sharing")]
use tracing::debug;
use tracing::{error, info};

use crate::app::AppState;
#[cfg(feature = "file-sharing")]
use crate::dragoon_network::{DragoonEvent, FileResponse};
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
    #[cfg(feature = "file-sharing")]
    GetFile {
        key: String,
        peer: PeerId,
        sender: oneshot::Sender<Result<Vec<u8>, Box<dyn Error + Send>>>,
    },
    #[cfg(feature = "file-sharing")]
    AddFile {
        file: Vec<u8>,
        channel: ResponseChannel<FileResponse>,
    },
    PutRecord {
        key: String,
        value: Vec<u8>,
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>,
    },
    GetRecord {
        key: String,
        sender: oneshot::Sender<Result<Vec<u8>, Box<dyn Error + Send>>>,
    },
    DragoonPeers {
        sender: oneshot::Sender<Result<HashSet<PeerId>, Box<dyn Error + Send>>>,
    },
    DragoonSend {
        data: String,
        peerid: String,
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>,
    },
    DragoonGet {
        peerid: String,
        key: String,
        sender: oneshot::Sender<Result<Vec<u8>, Box<dyn Error + Send>>>,
    }
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
            #[cfg(feature = "file-sharing")]
            DragoonCommand::GetFile { .. } => write!(f, "get-file"),
            #[cfg(feature = "file-sharing")]
            DragoonCommand::AddFile { .. } => write!(f, "add-file"),
            DragoonCommand::PutRecord { .. } => write!(f, "put-record"),
            DragoonCommand::GetRecord { .. } => write!(f, "get-record"),
            DragoonCommand::DragoonPeers { .. } => write!(f, "dragoon-peers"),
            DragoonCommand::DragoonSend { .. } => write!(f, "dragoon-send"),
            DragoonCommand::DragoonGet { .. } => write!(f, "dragoon-get"),
        }
    }
}

pub(crate) async fn listen(
    Path(multiaddr): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    info!("running command `listen`");
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
    info!("running command `get_listeners`");
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
    info!("running command `get_peer_id`");
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
    info!("running command `get_network_info`");
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
    info!("running command `remove_listener`");
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
    info!("running command `get_connected_peers`");
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
    info!("running command `dial`");
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
    info!("running command `add_peer`");
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
    info!("running command `start_provide`");
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

pub(crate) async fn dragoon_peers(State(state): State<Arc<AppState>>) -> Response {
    let (sender, receiver) = oneshot::channel();
    let cmd = DragoonCommand::DragoonPeers { sender };
    let cmd_name = cmd.to_string();
    send_command(cmd, state).await;

    match receiver.await {
        Ok(res) => match res {
            Err(e) => handle_dragoon_error(e, &cmd_name),
            Ok(peers) => (
                StatusCode::OK,
                Json(
                    peers
                        .iter()
                        .map(|peer| peer.to_base58())
                        .collect::<Vec<String>>(),
                ),
            )
                .into_response(),
        },
        Err(e) => handle_canceled(e, &cmd_name),
    }
}

pub(crate) async fn dragoon_send(
    Path((peerid, data)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
) -> Response {
    let (sender, receiver) = oneshot::channel();
    let cmd = DragoonCommand::DragoonSend {
        data,
        peerid,
        sender,
    };
    let cmd_name = cmd.to_string();
    send_command(cmd, state).await;

    match receiver.await {
        Ok(res) => match res {
            Ok(_) => (StatusCode::OK, Json("")).into_response(),
            Err(e) => handle_dragoon_error(e, &cmd_name),
        },
        Err(e) => handle_canceled(e, &cmd_name),
    }
}

pub(crate) async fn dragoon_get(
    Path((peerid, key)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
) -> Response {
    let (sender, receiver) = oneshot::channel();
    let cmd = DragoonCommand::DragoonGet {
        peerid,
        key,
        sender,
    };
    let cmd_name = cmd.to_string();
    send_command(cmd, state).await;

    match receiver.await {
        Ok(res) => match res {
            Ok(bytes) => (StatusCode::OK, Json(bytes)).into_response(),
            Err(e) => handle_dragoon_error(e, &cmd_name),
        },
        Err(e) => handle_canceled(e, &cmd_name),
    }
}

pub(crate) async fn get_providers(
    Path(key): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    info!("running command `get_providers`");
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
    info!("running command `bootstrap`");
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

#[cfg(feature = "file-sharing")]
pub(crate) async fn get_file(
    Path(key): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    info!("running command `get_file`");
    let providers = {
        let (sender, receiver) = oneshot::channel();

        let cmd = DragoonCommand::GetProviders {
            key: key.clone(),
            sender,
        };
        let cmd_name = cmd.to_string();
        send_command(cmd, state.clone()).await;

        match receiver.await {
            Err(e) => return handle_canceled(e, &cmd_name),
            Ok(res) => match res {
                Err(e) => return handle_dragoon_error(e, &cmd_name),
                Ok(providers) => providers,
            },
        }
    };

    let (sender, receiver) = oneshot::channel();

    let cmd = DragoonCommand::GetFile {
        key: key.clone(),
        // FIXME: should use all the providers here instead of just the first one,
        // run the requests on all of them and then "future select" the first one to complete
        // successfully.
        peer: *providers.into_iter().collect::<Vec<_>>().get(0).unwrap(),
        sender,
    };
    let cmd_name = cmd.to_string();
    send_command(cmd, state).await;

    match receiver.await {
        Err(e) => handle_canceled(e, &cmd_name),
        Ok(res) => match res {
            Err(e) => handle_dragoon_error(e, &cmd_name),
            Ok(content) => Json(content).into_response(),
        },
    }
}

#[cfg(feature = "file-sharing")]
pub(crate) async fn add_file(
    Path((key, content)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
) -> Response {
    info!(
        "running command `add_file`: key = {}, content = {}",
        key, content
    );
    let mut event_receiver = state.event_receiver.lock().await;

    loop {
        match event_receiver.next().await {
            Some(DragoonEvent::InboundRequest { channel, request }) => {
                debug!("add_file: request '{}'", request);
                if request == key {
                    debug!("add_file: request accepted");
                    let cmd = DragoonCommand::AddFile {
                        file: content.as_bytes().to_vec(),
                        channel,
                    };
                    send_command(cmd, state.clone()).await;
                }
            }
            e => todo!("{:?}", e),
        }
    }
}

pub(crate) async fn put_record(
    Path((key, value)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
) -> Response {
    info!("running command `put_record`");
    let (sender, receiver) = oneshot::channel();

    let cmd = DragoonCommand::PutRecord {
        key,
        value: value.as_bytes().to_vec(),
        sender,
    };
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

pub(crate) async fn get_record(
    Path(key): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    info!("running command `get_record`");
    let (sender, receiver) = oneshot::channel();

    let cmd = DragoonCommand::GetRecord { key, sender };
    let cmd_name = cmd.to_string();
    send_command(cmd, state).await;

    match receiver.await {
        Err(e) => handle_canceled(e, &cmd_name),
        Ok(res) => match res {
            Err(e) => handle_dragoon_error(e, &cmd_name),
            Ok(bytes) => (StatusCode::OK, Json(bytes)).into_response(),
        },
    }
}

fn handle_dragoon_error(err: Box<dyn Error + Send>, command: &str) -> Response {
    if let Ok(dragoon_error) = err.downcast::<DragoonError>() {
        dragoon_error.into_response()
    } else {
        error!("Could not get return message from command `{}`", command);
        DragoonError::UnexpectedError(format!("could not convert error for {}", command))
            .into_response()
    }
}

fn handle_canceled(err: Canceled, command: &str) -> Response {
    error!(
        "Could not receive a return from command `{}`: {:?}",
        command, err
    );
    DragoonError::UnexpectedError("Command was canceled".to_string()).into_response()
}

async fn send_command(command: DragoonCommand, state: Arc<AppState>) -> Option<Response> {
    let mut cmd_sender = state.sender.lock().await;

    let cmd_name = format!("{}", command);

    info!("Sending command `{:?}`", command);

    if let Err(e) = cmd_sender.send(command).await {
        let err = format!("Could not send command `{}`: {:?}", cmd_name, e);
        error!(err);
        return Some(DragoonError::UnexpectedError(err).into_response());
    }

    return None;
}
