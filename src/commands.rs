//! Define all the commands that can be used by the network

use anyhow::{self, format_err, Error, Result};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use libp2p::swarm::NetworkInfo;
use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{
    mpsc,
    oneshot::{self, error::RecvError},
};
use tracing::{error, info};

use crate::app::AppState;
use crate::dragoon_network::BlockResponse;
use crate::error::DragoonError;
use crate::peer_block_info::PeerBlockInfo;
use crate::send_strategy::SendId;
use crate::send_strategy_impl::StrategyName;
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

pub(crate) type SenderOneS<T, E = Error> = oneshot::Sender<Result<T, E>>;
pub(crate) type SenderMPSC<T, E = Error> = mpsc::UnboundedSender<Result<T, E>>;

#[derive(Debug)]
pub(crate) enum Sender<T, E = Error> {
    SenderOneS(SenderOneS<T, E>),
    SenderMPSC(SenderMPSC<T, E>),
}

pub(crate) fn sender_send_match<T, E>(
    sender: Sender<T, E>,
    res: Result<T, E>,
    operation_name: String,
) where
    E: std::fmt::Debug,
{
    if match sender {
        Sender::SenderMPSC(sender) => sender.send(res).map_err(|_| format_err!("")),
        Sender::SenderOneS(sender) => sender.send(res).map_err(|_| format_err!("")),
    }
    .is_err()
    {
        error!(
            "Could not send the result of the {} operation",
            operation_name
        )
    }
}

#[derive(Debug)]
pub(crate) enum DragoonCommand {
    AddPeer {
        multiaddr: String,
        sender: Sender<()>,
    },
    Bootstrap {
        sender: Sender<()>,
    },
    ChangeAvailableSendStorage {
        new_storage_size: usize,
        sender: Sender<String>,
    },
    DecodeBlocks {
        block_dir: String,
        block_hashes: Vec<String>,
        output_filename: String,
        sender: Sender<()>,
    },
    DialMultiple {
        list_multiaddr: Vec<String>,
        sender: Sender<()>,
    },
    DialSingle {
        multiaddr: String,
        sender: Sender<()>,
    },
    // DragoonPeers {
    //     sender: Sender<HashSet<PeerId>>,
    // },
    // DragoonSend {
    //     block_hash: String,
    //     block_path: String,
    //     peerid: String,
    //     sender: Sender<()>,
    // },
    EncodeFile {
        file_path: String,
        replace_blocks: bool,
        encoding_method: EncodingMethod,
        encode_mat_k: usize,
        encode_mat_n: usize,
        sender: Sender<(String, String)>,
    },
    GetAvailableStorage {
        sender: Sender<usize>,
    },
    GetBlockDir {
        file_hash: String,
        sender: Sender<PathBuf>,
    },
    GetBlockFrom {
        peer_id: PeerId,
        file_hash: String,
        block_hash: String,
        save_to_disk: bool,
        sender: Sender<Option<BlockResponse>>,
    },
    GetBlocksInfoFrom {
        peer_id: PeerId,
        file_hash: String,
        sender: Sender<PeerBlockInfo>,
    },
    GetBlockList {
        file_hash: String,
        sender: Sender<Vec<String>>,
    },
    GetConnectedPeers {
        sender: Sender<Vec<PeerId>>,
    },
    GetFile {
        file_hash: String,
        output_filename: String,
        sender: Sender<PathBuf>,
    },
    GetFileDir {
        file_hash: String,
        sender: Sender<PathBuf>,
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
        sender: Sender<Vec<PeerId>>,
    },
    Listen {
        multiaddr: String,
        sender: Sender<u64>,
    },
    NodeInfo {
        sender: Sender<(PeerId, String)>,
    },
    RemoveEntryFromSendBlockToSet {
        peer_id: PeerId,
        block_hash: String,
        sender: Sender<()>,
    },
    RemoveListener {
        listener_id: u64,
        sender: Sender<bool>,
    },
    SendBlockList {
        strategy_name: StrategyName,
        file_hash: String,
        block_list: Vec<String>,
        sender: Sender<Vec<SendId>, DragoonError>,
    },
    SendBlockTo {
        peer_id: PeerId,
        file_hash: String,
        block_hash: String,
        sender: Sender<(bool, SendId), DragoonError>,
    },
    StartProvide {
        key: String,
        sender: Sender<()>,
    },
    StopProvide {
        key: String,
        sender: Sender<()>,
    },
}

impl std::fmt::Display for DragoonCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            DragoonCommand::AddPeer { .. } => write!(f, "add-peer"),
            DragoonCommand::Bootstrap { .. } => write!(f, "bootstrap"),
            DragoonCommand::ChangeAvailableSendStorage { .. } => {
                write!(f, "change-available-send-storage")
            }
            DragoonCommand::DecodeBlocks { .. } => write!(f, "decode-blocks"),
            DragoonCommand::DialMultiple { .. } => write!(f, "dial-multiple"),
            DragoonCommand::DialSingle { .. } => write!(f, "dial-single"),
            DragoonCommand::EncodeFile { .. } => write!(f, "encode-file"),
            DragoonCommand::GetAvailableStorage { .. } => write!(f, "get-available-storage"),
            DragoonCommand::GetBlockDir { .. } => write!(f, "get-block-dir"),
            DragoonCommand::GetBlockFrom { .. } => write!(f, "get-block-from"),
            DragoonCommand::GetBlocksInfoFrom { .. } => write!(f, "get-blocks-info-from"),
            DragoonCommand::GetBlockList { .. } => write!(f, "get-block-list"),
            DragoonCommand::GetConnectedPeers { .. } => write!(f, "get-connected-peers"),
            DragoonCommand::GetFile { .. } => write!(f, "get-file"),
            DragoonCommand::GetFileDir { .. } => write!(f, "get-file-dir"),
            DragoonCommand::GetListeners { .. } => write!(f, "get-listener"),
            DragoonCommand::GetPeerId { .. } => write!(f, "get-peer-id"),
            DragoonCommand::GetNetworkInfo { .. } => write!(f, "get-network-info"),
            DragoonCommand::GetProviders { .. } => write!(f, "get-providers"),
            DragoonCommand::Listen { .. } => write!(f, "listen"),
            DragoonCommand::NodeInfo { .. } => write!(f, "node-info"),
            DragoonCommand::RemoveEntryFromSendBlockToSet { .. } => {
                write!(f, "remove-entry-from-send-block-to-set")
            }
            DragoonCommand::RemoveListener { .. } => write!(f, "remove-listener"),
            DragoonCommand::SendBlockList { .. } => write!(f, "send-block-list"),
            DragoonCommand::SendBlockTo { .. } => write!(f, "send-block-to"),
            DragoonCommand::StartProvide { .. } => write!(f, "start-provide"),
            DragoonCommand::StopProvide { .. } => write!(f, "stop-provide"),
        }
    }
}

async fn command_res_match<E>(
    receiver: oneshot::Receiver<Result<impl ConvertSer, E>>,
    cmd_name: String,
) -> Response
where
    E: std::fmt::Debug + Send + Sync + 'static,
{
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

        let sender = Sender::SenderOneS(sender);

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

pub(crate) async fn create_cmd_change_available_send_storage(
    Path(new_storage_size): Path<usize>,
    State(state): State<Arc<AppState>>,
) -> Response {
    info!("running command `change_available_send_storage`");
    dragoon_command!(state, ChangeAvailableSendStorage, new_storage_size)
}

pub(crate) async fn create_cmd_decode_blocks(
    Path((block_dir, block_hashes_json, output_filename)): Path<(String, String, String)>,
    State(state): State<Arc<AppState>>,
) -> Response {
    let block_hashes = serde_json::from_str(&block_hashes_json).expect(
        "Could not parse user input as a valid list of block hashes when trying to decode blocks",
    );
    info!("running command `decode_blocks");
    dragoon_command!(
        state,
        DecodeBlocks,
        block_dir,
        block_hashes,
        output_filename
    )
}

pub(crate) async fn create_cmd_dial_multiple(
    Path(list_multiaddr_json): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    let list_multiaddr = serde_json::from_str(&list_multiaddr_json).expect(
        "Could not parse user input as a valid list of mutliaddr when trying to dial multiple peers",
    );
    info!("running command `dial-multiple`");
    dragoon_command!(state, DialMultiple, list_multiaddr)
}

pub(crate) async fn create_cmd_dial_single(
    Path(multiaddr): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    info!("running command `dial-single`");
    dragoon_command!(state, DialSingle, multiaddr)
}

// pub(crate) async fn create_cmd_dragoon_peers(State(state): State<Arc<AppState>>) -> Response {
//     info!("running command `dragoon_peers`");
//     dragoon_command!(state, DragoonPeers)
// }

// pub(crate) async fn create_cmd_dragoon_send(
//     Path((peerid, block_hash, block_path)): Path<(String, String, String)>,
//     State(state): State<Arc<AppState>>,
// ) -> Response {
//     info!("running command `dragoon_send`");
//     dragoon_command!(state, DragoonSend, block_hash, block_path, peerid)
// }

pub(crate) async fn create_cmd_encode_file(
    Path((file_path, replace_blocks, encoding_method, encode_mat_k, encode_mat_n)): Path<(
        String,
        bool,
        EncodingMethod,
        usize,
        usize,
    )>,
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
        encode_mat_n
    )
}

pub(crate) async fn create_cmd_get_available_storage(
    State(state): State<Arc<AppState>>,
) -> Response {
    info!("running command `get_available_storage`");
    dragoon_command!(state, GetAvailableStorage)
}

pub(crate) async fn create_cmd_get_block_from(
    Path((peer_id_base_58, file_hash, block_hash, save_to_disk)): Path<(
        String,
        String,
        String,
        bool,
    )>,
    State(state): State<Arc<AppState>>,
) -> Response {
    info!("running command `get_block_from`");
    let bytes = bs58::decode(peer_id_base_58).into_vec().unwrap();
    let peer_id = PeerId::from_bytes(&bytes).unwrap();
    dragoon_command!(
        state,
        GetBlockFrom,
        peer_id,
        file_hash,
        block_hash,
        save_to_disk
    )
}

pub(crate) async fn create_cmd_get_blocks_info_from(
    Path((peer_id_base_58, file_hash)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
) -> Response {
    info!("running command `get_blocks_info_from`");
    let bytes = bs58::decode(peer_id_base_58).into_vec().unwrap();
    let peer_id = PeerId::from_bytes(&bytes).unwrap();
    dragoon_command!(state, GetBlocksInfoFrom, peer_id, file_hash)
}

pub(crate) async fn create_cmd_get_block_list(
    Path(file_hash): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    info!("running command `get_block_list");
    dragoon_command!(state, GetBlockList, file_hash)
}

pub(crate) async fn create_cmd_get_connected_peers(State(state): State<Arc<AppState>>) -> Response {
    info!("running command `get_connected_peers`");
    dragoon_command!(state, GetConnectedPeers)
}

pub(crate) async fn create_cmd_get_file(
    Path((file_hash, output_filename)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
) -> Response {
    info!("running command get_file");
    dragoon_command!(state, GetFile, file_hash, output_filename)
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

pub(crate) async fn create_cmd_listen(
    Path(multiaddr): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    info!("running command `listen`");
    dragoon_command!(state, Listen, multiaddr)
}

pub(crate) async fn create_cmd_node_info(State(state): State<Arc<AppState>>) -> Response {
    info!("running command `node_info`");
    dragoon_command!(state, NodeInfo)
}

pub(crate) async fn create_cmd_remove_listener(
    Path(listener_id): Path<u64>,
    State(state): State<Arc<AppState>>,
) -> Response {
    info!("running command `remove_listener`");
    dragoon_command!(state, RemoveListener, listener_id)
}

pub(crate) async fn create_cmd_send_block_list(
    Path((strategy_name, file_hash, block_hashes_json)): Path<(StrategyName, String, String)>,
    State(state): State<Arc<AppState>>,
) -> Response {
    let block_list = serde_json::from_str(&block_hashes_json).expect(
        "Could not parse user input as a valid list of block hashes when trying to decode blocks",
    );
    info!("running command `send_block_list`");
    dragoon_command!(state, SendBlockList, strategy_name, file_hash, block_list)
}

pub(crate) async fn create_cmd_send_block_to(
    Path((peer_id_base_58, file_hash, block_hash)): Path<(String, String, String)>,
    State(state): State<Arc<AppState>>,
) -> Response {
    info!("running command `send_block_to`");
    let bytes = bs58::decode(peer_id_base_58).into_vec().unwrap();
    let peer_id = PeerId::from_bytes(&bytes).unwrap();
    dragoon_command!(state, SendBlockTo, peer_id, block_hash, file_hash)
}

pub(crate) async fn create_cmd_start_provide(
    Path(key): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    info!("running command `start_provide`");
    dragoon_command!(state, StartProvide, key)
}

pub(crate) async fn create_cmd_stop_provide(
    Path(key): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    info!("running command `stop_provide`");
    dragoon_command!(state, StopProvide, key)
}

// End of dragoon command implementation

fn handle_dragoon_error<E>(err: E, command: &str) -> Response
where
    E: std::fmt::Debug + Send + Sync + 'static,
{
    let err_msg = format!("Got error from command `{}`: {:?}", command, err);
    error!(err_msg);
    DragoonError::UnexpectedError(err_msg).into_response()
}

fn handle_canceled(err: RecvError, command: &str) -> Response {
    error!(
        "Could not receive a return from command `{}`: {:?}",
        command, err
    );
    DragoonError::UnexpectedError("Command was canceled".to_string()).into_response()
}

async fn send_command(command: DragoonCommand, state: Arc<AppState>) -> Option<Response> {
    let cmd_sender = state.cmd_sender.clone();

    let cmd_name = format!("{}", command);

    info!("Sending command `{:?}`", command);

    if let Err(e) = cmd_sender.send(command) {
        let err = format!("Could not send command `{}`: {:?}", cmd_name, e);
        error!(err);
        return Some(DragoonError::UnexpectedError(err).into_response());
    }

    None
}
