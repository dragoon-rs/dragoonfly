//! Define all the commands that can be used by the network

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use futures::channel::oneshot::{self, Canceled};
use futures::SinkExt;
use libp2p::swarm::NetworkInfo;
use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::HashSet;
use std::error::Error;
use std::sync::Arc;
use tracing::{error, info};

use crate::app::AppState;
use crate::error::DragoonError;
use crate::to_serialize::{ConvertSer, JsonWrapper};

// use komodo::linalg::Matrix;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) enum EncodingMethod {
    Vandermonde,
    Random,
}
//TODO impl Display to convert from String for axum when doing http-get requests ?

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

pub(crate) type Sender<T> = oneshot::Sender<Result<T, Box<dyn Error + Send>>>;
#[derive(Debug)]
pub(crate) enum DragoonCommand {
    AddPeer {
        multiaddr: String,
        sender: Sender<()>,
    },
    Bootstrap {
        sender: Sender<()>,
    },
    DecodeBlocks {
        block_dir: String,
        block_hashes: Vec<String>,
        output_filename: String,
        sender: Sender<()>,
    },
    Dial {
        multiaddr: String,
        sender: Sender<()>,
    },
    DragoonGet {
        peerid: String,
        key: String,
        sender: Sender<Vec<u8>>,
    },
    DragoonPeers {
        sender: Sender<HashSet<PeerId>>,
    },
    DragoonSend {
        block_hash: String,
        block_path: String,
        peerid: String,
        sender: Sender<()>,
    },
    EncodeFile {
        file_path: String,
        replace_blocks: bool,
        encoding_method: EncodingMethod,
        encode_mat_k: usize,
        encode_mat_n: usize,
        powers_path: String,
        sender: Sender<String>,
    },
    GetConnectedPeers {
        sender: Sender<Vec<PeerId>>,
    },
    GetListeners {
        sender: Sender<Vec<Multiaddr>>,
    },
    GetNetworkInfo {
        sender: Sender<NetworkInfo>,
    },
    GetPeerId {
        sender: Sender<PeerId>,
    },
    GetProviders {
        key: String,
        sender: Sender<HashSet<PeerId>>,
    },
    GetRecord {
        key: String,
        sender: Sender<Vec<u8>>,
    },
    Listen {
        multiaddr: String,
        sender: Sender<u64>,
    },
    PutRecord {
        block_hash: String,
        block_dir: String,
        sender: Sender<()>,
    },
    RemoveListener {
        listener_id: u64,
        sender: Sender<bool>,
    },
    StartProvide {
        key: String,
        sender: Sender<()>,
    },
}

impl std::fmt::Display for DragoonCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            DragoonCommand::AddPeer { .. } => write!(f, "add-peer"),
            DragoonCommand::Bootstrap { .. } => write!(f, "bootstrap"),
            DragoonCommand::DecodeBlocks { .. } => write!(f, "decode-blocks"),
            DragoonCommand::Dial { .. } => write!(f, "dial"),
            DragoonCommand::DragoonGet { .. } => write!(f, "dragoon-get"),
            DragoonCommand::DragoonPeers { .. } => write!(f, "dragoon-peers"),
            DragoonCommand::DragoonSend { .. } => write!(f, "dragoon-send"),
            DragoonCommand::EncodeFile { .. } => write!(f, "encode-file"),
            DragoonCommand::GetConnectedPeers { .. } => write!(f, "get-connected-peers"),
            DragoonCommand::GetListeners { .. } => write!(f, "get-listener"),
            DragoonCommand::GetPeerId { .. } => write!(f, "get-peer-id"),
            DragoonCommand::GetNetworkInfo { .. } => write!(f, "get-network-info"),
            DragoonCommand::GetProviders { .. } => write!(f, "get-providers"),
            DragoonCommand::GetRecord { .. } => write!(f, "get-record"),
            DragoonCommand::Listen { .. } => write!(f, "listen"),
            DragoonCommand::PutRecord { .. } => write!(f, "put-record"),
            DragoonCommand::RemoveListener { .. } => write!(f, "remove-listener"),
            DragoonCommand::StartProvide { .. } => write!(f, "start-provide"),
        }
    }
}

async fn command_res_match(
    receiver: oneshot::Receiver<Result<impl ConvertSer, Box<dyn Error + Send>>>,
    cmd_name: String,
) -> Response {
    match receiver.await {
        Err(e) => handle_canceled(e, &cmd_name),
        Ok(res) => match res {
            Err(e) => handle_dragoon_error(e, &cmd_name),
            Ok(convertable) => (
                StatusCode::OK,
                JsonWrapper(
                    Json(convertable.convert_ser()), // into_response is implement for Json<T> where T: Serialize
                                                     // so we convert everything to a Serialize
                                                     // see to_serialize to check how the conversion is done
                )
                .into_response(),
            )
                .into_response(),
        },
    }
}

/// Used to factorise the code of all the commands, since they basically do the same thing
macro_rules! dragoon_command {
    ($state:expr, // the current state we are in
        $variant:ident // the type of DragoonCommand we want to use
        $(,)? // optional comma, allows to not leave a trailing comma when there is nothing behind
        $($variant_args:ident),*) // the list of the parameters for the given variant, separated by comma, 0 or more of them
        // note that the sender is automatically added, since it's common to all the variants
         => {
        {
        let (sender, receiver) = oneshot::channel(); // create a channel

        let cmd = DragoonCommand::$variant {$($variant_args,)* sender}; // build the command
        // for example, when calling `dragoon_command!(state, Listen, multiaddr)` the expanded result will be:
        // `let cmd = DragoonCommand::Listen {multiaddr, sender}`
        // note that as variant and all the t are captured as ident, there is no need to write the corresponding field name for each variable
        // because Rust will infere when the name of the variable is the same as the field
        let cmd_name = cmd.to_string();
        send_command(cmd, $state).await;

        command_res_match(receiver, cmd_name).await
        }
    };
}

// dragoon_command(state, DragoonCommand::Something, peerid, data)
// Implementation of dragoon commands

pub(crate) async fn create_cmd_add_peer(
    Path(multiaddr): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    info!("running command `add_peer`");
    dragoon_command!(state, AddPeer, multiaddr)
}

pub(crate) async fn create_cmd_bootstrap(State(state): State<Arc<AppState>>) -> Response {
    info!("running command `bootstrap`");
    dragoon_command!(state, Bootstrap)
}

pub(crate) async fn create_cmd_decode_blocks(
    Path((block_dir, block_hashes_json, output_filename)): Path<(String, String, String)>,
    State(state): State<Arc<AppState>>,
) -> Response {
    info!("running command `decode_blocks");
    let block_hashes = serde_json::from_str(&block_hashes_json).expect(
        "Could not parse user input as a valid list of block hashes when trying to decode blocks",
    );
    dragoon_command!(
        state,
        DecodeBlocks,
        block_dir,
        block_hashes,
        output_filename
    )
}

pub(crate) async fn create_cmd_dial(
    Path(multiaddr): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    info!("running command `dial`");
    dragoon_command!(state, Dial, multiaddr)
}

pub(crate) async fn create_cmd_dragoon_get(
    Path((peerid, key)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
) -> Response {
    info!("running command `dragoon_get`");
    dragoon_command!(state, DragoonGet, peerid, key)
}

pub(crate) async fn create_cmd_dragoon_peers(State(state): State<Arc<AppState>>) -> Response {
    info!("running command `dragoon_peers`");
    dragoon_command!(state, DragoonPeers)
}

pub(crate) async fn create_cmd_dragoon_send(
    Path((peerid, block_hash, block_path)): Path<(String, String, String)>,
    State(state): State<Arc<AppState>>,
) -> Response {
    info!("running command `dragoon_send`");
    dragoon_command!(state, DragoonSend, block_hash, block_path, peerid)
}

pub(crate) async fn create_cmd_encode_file(
    Path((file_path, replace_blocks, encoding_method, encode_mat_k, encode_mat_n, powers_path)): Path<(String, bool, EncodingMethod, usize, usize, String)>,
    State(state): State<Arc<AppState>>,
) -> Response {
    info!("running command `encode_file`");
    dragoon_command!(
        state,
        EncodeFile,
        file_path,
        replace_blocks,
        encoding_method,
        encode_mat_k,
        encode_mat_n,
        powers_path
    )
}

pub(crate) async fn create_cmd_get_connected_peers(State(state): State<Arc<AppState>>) -> Response {
    info!("running command `get_connected_peers`");
    dragoon_command!(state, GetConnectedPeers)
}

pub(crate) async fn create_cmd_get_listeners(State(state): State<Arc<AppState>>) -> Response {
    info!("running command `get_listeners`");
    dragoon_command!(state, GetListeners)
}

pub(crate) async fn create_cmd_get_peer_id(State(state): State<Arc<AppState>>) -> Response {
    info!("running command `get_peer_id`");
    dragoon_command!(state, GetPeerId)
}

pub(crate) async fn create_cmd_get_providers(
    Path(key): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    info!("running command `get_providers`");
    dragoon_command!(state, GetProviders, key)
}

#[derive(Serialize, Deserialize)]
pub(crate) struct SerNetworkInfo {
    peers: usize,
    pending: u32,
    connections: u32,
    established: u32,
    pending_incoming: u32,
    pending_outgoing: u32,
    established_incoming: u32,
    established_outgoing: u32,
}
// as each field of SerNetworkInfo is Serialize, SerNetworkInfo becomes Serialize by extension

impl SerNetworkInfo {
    pub(crate) fn new(network_info: &NetworkInfo) -> Self {
        let connections = network_info.connection_counters();
        SerNetworkInfo {
            peers: network_info.num_peers(),
            pending: connections.num_pending(),
            connections: connections.num_connections(),
            established: connections.num_established(),
            pending_incoming: connections.num_pending_incoming(),
            pending_outgoing: connections.num_pending_outgoing(),
            established_incoming: connections.num_established_incoming(),
            established_outgoing: connections.num_established_outgoing(),
        }
    }
}

pub(crate) async fn create_cmd_get_network_info(State(state): State<Arc<AppState>>) -> Response {
    info!("running command `get_network_info`");
    dragoon_command!(state, GetNetworkInfo)
}

pub(crate) async fn create_cmd_get_record(
    Path(key): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    info!("running command `get_record`");
    dragoon_command!(state, GetRecord, key)
}

pub(crate) async fn create_cmd_listen(
    Path(multiaddr): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    info!("running command `listen`");
    dragoon_command!(state, Listen, multiaddr)
}

pub(crate) async fn create_cmd_put_record(
    Path((block_hash, block_dir)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
) -> Response {
    info!("running command `put_record`");
    dragoon_command!(state, PutRecord, block_hash, block_dir)
}

pub(crate) async fn create_cmd_remove_listener(
    Path(listener_id): Path<u64>,
    State(state): State<Arc<AppState>>,
) -> Response {
    info!("running command `remove_listener`");
    dragoon_command!(state, RemoveListener, listener_id)
}

pub(crate) async fn create_cmd_start_provide(
    Path(key): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    info!("running command `start_provide`");
    dragoon_command!(state, StartProvide, key)
}

// End of dragoon command implementation

fn handle_dragoon_error(err: Box<dyn Error + Send>, command: &str) -> Response {
    format!("Got error from command `{}`: {}", command, err.to_string()).into_response()
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

    None
}
