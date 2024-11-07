use anyhow::{self, format_err, Result};
use futures::pin_mut;
use futures::prelude::*;
use futures::stream::{self as f_stream, BoxStream, FusedStream};
use libp2p::core::ConnectedPoint;
use tokio::fs as tfs;
use tokio::io::AsyncWriteExt;
use tokio::sync::{
    mpsc::{self, UnboundedReceiver, UnboundedSender},
    oneshot,
};
use tokio::time;

use libp2p::core::transport::ListenerId;
use libp2p::identity::Keypair;
use libp2p::kad::{QueryId, QueryResult};
use libp2p::request_response::{Event, Message, OutboundRequestId, ResponseChannel};
use libp2p::{
    core::Multiaddr,
    identify, kad,
    multiaddr::Protocol,
    noise,
    request_response::{self, ProtocolSupport},
    swarm::{NetworkBehaviour, Swarm, SwarmEvent},
    tcp, yamux, PeerId, StreamProtocol, TransportError,
};
use libp2p_stream as stream;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs as sfs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;
use tracing::{debug, error, info, warn};

use crate::commands::{sender_send_match, DragoonCommand, EncodingMethod, Sender, SenderMPSC};
use crate::error::DragoonError::{
    self, BadListener, BootstrapError, CouldNotSendBlockResponse, CouldNotSendInfoResponse,
    DialError, NoParentDirectory, ProviderError, SendBlockToAlreadyStarted, SendBlockToError,
};
use crate::peer_block_info::PeerBlockInfo;
use crate::send_block_to::{self, SendBlockHandler};
use crate::send_strategy::{SendId, SendStrategy};
use crate::send_strategy_impl::{self, StrategyName};

use komodo::{
    self,
    algebra::linalg::Matrix,
    fec::{self, Shard},
    fs,
    semi_avid::{verify, Block},
    zk::Powers,
};

use resolve_path::PathResolveExt;
use rs_merkle::{algorithms::Sha256, Hasher};

use ark_ec::CurveGroup;
use ark_ff::PrimeField;
use ark_poly::DenseUVPolynomial;
use ark_serialize::{CanonicalDeserialize, Compress, Validate};
use ark_std::ops::Div;

const SEND_BLOCK_PROTOCOL: StreamProtocol = StreamProtocol::new("/send-block/1.0.0");
pub(crate) const SEND_BLOCK_FILE_NAME: &str = "send_block_list.txt";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BlockRequest {
    file_hash: String,
    block_hash: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BlockResponse {
    pub(crate) file_hash: String,
    pub(crate) block_hash: String,
    pub(crate) block_data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PeerBlockInfoRequest {
    file_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PeerBlockInfoResponse(PeerBlockInfo);

pub(crate) async fn create_swarm(id_keys: Keypair) -> Result<Swarm<DragoonBehaviour>> {
    let peer_id = id_keys.public().to_peer_id();

    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(id_keys)
        .with_async_std()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_behaviour(|key| DragoonBehaviour {
            kademlia: kad::Behaviour::new(
                peer_id,
                kad::store::MemoryStore::new(key.public().to_peer_id()),
            ),
            identify: identify::Behaviour::new(identify::Config::new(
                "/ipfs/id/1.0.0".to_string(),
                key.public(),
            )),
            request_block: request_response::cbor::Behaviour::new(
                [(
                    StreamProtocol::new("/block-exchange/1"),
                    ProtocolSupport::Full,
                )],
                request_response::Config::default(),
            ),
            request_info: request_response::cbor::Behaviour::new(
                [(StreamProtocol::new("/peer-info/1"), ProtocolSupport::Full)],
                request_response::Config::default(),
            ),
            send_block: stream::Behaviour::new(),
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60 * 60)))
        .build();

    swarm
        .behaviour_mut()
        .kademlia
        .set_mode(Some(kad::Mode::Server));

    Ok(swarm)
}

#[derive(NetworkBehaviour)]
pub(crate) struct DragoonBehaviour {
    request_block: request_response::cbor::Behaviour<BlockRequest, BlockResponse>,
    request_info: request_response::cbor::Behaviour<PeerBlockInfoRequest, PeerBlockInfoResponse>,
    identify: identify::Behaviour,
    kademlia: kad::Behaviour<kad::store::MemoryStore>,
    send_block: stream::Behaviour,
}

pub(crate) struct DragoonNetwork {
    swarm: Swarm<DragoonBehaviour>,
    label: String,
    command_receiver: mpsc::UnboundedReceiver<DragoonCommand>,
    command_sender: mpsc::UnboundedSender<DragoonCommand>,
    listeners: HashMap<u64, ListenerId>,
    file_dir: PathBuf,
    powers_path: PathBuf,
    current_available_storage_for_send: Arc<AtomicUsize>,
    current_total_size_of_blocks_on_disk: Arc<AtomicUsize>,
    known_peer_id: HashSet<PeerId>,
    pending_dial: HashMap<String, Sender<()>>,
    pending_send_block_to: HashSet<(PeerId, String)>,
    pending_start_providing: HashMap<kad::QueryId, Sender<()>>,
    pending_get_providers: HashMap<kad::QueryId, SenderMPSC<HashSet<PeerId>>>,
    pending_request_block_info: HashMap<OutboundRequestId, Sender<PeerBlockInfo>>,
    pending_request_block: HashMap<OutboundRequestId, (bool, Sender<Option<BlockResponse>>)>,
    //TODO add a pending_request_file using the hash as a key
}

impl DragoonNetwork {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        swarm: Swarm<DragoonBehaviour>,
        command_receiver: mpsc::UnboundedReceiver<DragoonCommand>,
        command_sender: mpsc::UnboundedSender<DragoonCommand>,
        powers_path: PathBuf,
        total_available_storage_for_send: usize,
        peer_id: PeerId,
        maybe_label: Option<String>,
        replace: bool,
    ) -> Self {
        let label = if let Some(label) = maybe_label {
            label
        } else {
            peer_id.to_base58()
        };
        Self {
            swarm,
            label,
            command_receiver,
            command_sender,
            listeners: HashMap::new(),
            file_dir: Self::create_block_dir(peer_id, replace).unwrap(),
            powers_path,
            current_available_storage_for_send: Arc::new(AtomicUsize::new(
                total_available_storage_for_send,
            )),
            current_total_size_of_blocks_on_disk: Arc::new(AtomicUsize::new(0)),
            known_peer_id: Default::default(),
            pending_dial: Default::default(),
            pending_send_block_to: Default::default(),
            pending_start_providing: Default::default(),
            pending_get_providers: Default::default(),
            pending_request_block_info: Default::default(),
            pending_request_block: Default::default(),
        }
    }

    fn create_block_dir(peer_id: PeerId, replace: bool) -> std::io::Result<PathBuf> {
        // * change the replace bool to be read from CLI
        let base_path = format!("~/.share/dragoonfly/{}/files", peer_id.to_base58())
            .resolve()
            .into_owned(); // * needs to be changed to allow taking the base path as argument from CLI
        if replace {
            let _ = sfs::remove_dir_all(&base_path); // ignore the error if the directory does not exist
        }
        if sfs::metadata(&base_path).is_err() {
            sfs::create_dir_all(&base_path)?;
            info!(
                "Created the directory for node {} at {:?}",
                peer_id, base_path
            );
        } else {
            warn!(
                "Directory for the node {} already exists at {:?}, skipping creation of a new one",
                peer_id, base_path
            );
        }

        Ok(base_path)
    }

    fn get_current_available_storage(&mut self) -> Result<(Arc<AtomicUsize>, Arc<AtomicUsize>)> {
        let current_available_storage = self.current_available_storage_for_send.clone();
        let total_block_size_on_disk = self.current_total_size_of_blocks_on_disk.clone();
        let send_block_file_list: PathBuf =
            [self.file_dir.clone(), PathBuf::from(SEND_BLOCK_FILE_NAME)]
                .iter()
                .collect();
        fn create_new_send_list(mut file: sfs::File) {
            file.write_all(format!("Total: {}\n", 0).as_bytes())
                .unwrap();
        }

        match sfs::File::create_new(send_block_file_list.clone()) {
            Ok(file) => create_new_send_list(file),
            Err(_) => {
                // * the file already exists
                match BufReader::new(sfs::File::open(send_block_file_list.clone()).unwrap())
                    .lines()
                    .nth(0)
                {
                    Some(first_line) => {
                        let first_line = first_line.unwrap();
                        let re = regex::Regex::new(r"Total: ([0-9]*)$").unwrap();
                        let already_used_size = re
                            .captures(&first_line)
                            .unwrap()
                            .get(1)
                            .unwrap()
                            .as_str()
                            .parse::<usize>()
                            .unwrap();
                        total_block_size_on_disk.store(already_used_size, Ordering::SeqCst);
                        let total_size = current_available_storage.load(Ordering::SeqCst);
                        match total_size.checked_sub(already_used_size) {
                            Some(new_size) => {info!("The total available storage is {} after deducting the already used storage", new_size); current_available_storage.store(new_size, Ordering::SeqCst);},
                            None => panic!("The total size allowed for send blocks is already smaller than the total size used by blocks received by send, that are currently stored on disk"),
                        }
                    }
                    None => {
                        // the file exists but for some reason we didn't find the first line
                        // move the existing file to a .old to prevent erasing a file, then create a new one and write into it
                        let mut new_path = send_block_file_list.clone();
                        new_path.push(PathBuf::from(".old"));
                        sfs::rename(send_block_file_list.clone(), new_path).unwrap();
                        let file = sfs::File::create_new(send_block_file_list).unwrap();
                        create_new_send_list(file)
                    }
                }
            }
        }

        Ok((current_available_storage, total_block_size_on_disk))
    }

    pub async fn run<F, G, P>(mut self)
    where
        F: PrimeField,
        G: CurveGroup<ScalarField = F>,
        P: DenseUVPolynomial<F>,
        for<'a, 'b> &'a P: Div<&'b P, Output = P>,
    {
        info!("Starting Dragoon Network");
        let incoming_send_streams = self
            .swarm
            .behaviour()
            .send_block
            .new_control()
            .accept(SEND_BLOCK_PROTOCOL)
            .unwrap();
        let (current_available_storage, total_block_size_on_disk) =
            match self.get_current_available_storage() {
                Ok(val) => val,
                Err(e) => {
                    error!("{:?}", e);
                    panic!()
                }
            };

        // starts a new task to handle the receiving end of sending blocks
        SendBlockHandler::run::<F, G, P>(
            incoming_send_streams,
            self.powers_path.clone(),
            self.file_dir.clone(),
            current_available_storage,
            total_block_size_on_disk,
        )
        .unwrap();
        loop {
            tokio::select! {
                e = self.swarm.next() => self.handle_event(e.expect("Swarm stream to be infinite.")).await,
                cmd = self.command_receiver.recv() =>  match cmd {
                    Some(c) => self.handle_command::<F,G,P>(c).await,
                    None => return,
                }
            }
        }
    }

    async fn handle_query_result(&mut self, result: QueryResult, id: QueryId) {
        match result {
            kad::QueryResult::StartProviding(Ok(result_ok)) => {
                info!("Started providing {:?}", result_ok);
                if let Some(sender) = self.pending_start_providing.remove(&id) {
                    debug!("Sending empty response");
                    sender_send_match(sender, Ok(()), String::from("StartProviding"));
                } else {
                    warn!("Could not find id = {} in the start providers", id);
                }
            }
            kad::QueryResult::GetProviders(get_providers_result) => {
                if let Ok(res) = get_providers_result {
                    match res {
                        kad::GetProvidersOk::FoundProviders { providers, .. } => {
                            if let Some(sender) = self.pending_get_providers.get(&id) {
                                if sender.send(Ok(providers)).is_err() {
                                    error!("Could not send the result of the kademlia Found Providers query result");
                                }
                            }
                        }
                        kad::GetProvidersOk::FinishedWithNoAdditionalRecord { .. } => {
                            info!("kad finished get providers ");
                            if let Some(sender) = self.pending_get_providers.remove(&id) {
                                debug!(
                                    "Closing the channel for getting new providers for id {:?}",
                                    id
                                );
                                drop(sender);
                            } else {
                                error!("could not find {} in the providers query list", id);
                            }
                        }
                    }
                } else {
                    info!("Could not get the providers");
                    if let Some(sender) = self.pending_get_providers.remove(&id) {
                        if let Some(mut query_id) =
                            self.swarm.behaviour_mut().kademlia.query_mut(&id)
                        {
                            query_id.finish();
                            debug!("Sending empty providers");
                            if sender.send(Ok(HashSet::default())).is_err() {
                                error!("Could not send empty result for the kademlia GetProviders query result");
                            }
                        } else {
                            error!("could not find {} in the query ids", id);
                            let err =
                                ProviderError(format!("could not find {} in the query ids", id));
                            debug!("Sending error");
                            if sender.send(Err(format_err!(err))).is_err() {
                                error!("Could not send error for the kademlia GetProviders query result");
                            }
                        }
                    } else {
                        error!("could not find {} in the providers", id);
                    }
                }
            }
            e => warn!("[unknown event] {:?}", e),
        }
    }

    async fn handle_event(&mut self, event: SwarmEvent<DragoonBehaviourEvent>) {
        debug!("[event] {:?}", event);
        match event {
            SwarmEvent::Behaviour(DragoonBehaviourEvent::Kademlia(
                kad::Event::InboundRequest { request },
            )) => match request {
                kad::InboundRequest::GetRecord {
                    num_closer_peers,
                    present_locally,
                } => info!("closer: {}, present: {}", num_closer_peers, present_locally),
                kad::InboundRequest::PutRecord {
                    source,
                    connection,
                    record,
                } => info!(
                    "source: {}, connection: {}, record: {:?}",
                    source, connection, record
                ),
                _ => {}
            },
            SwarmEvent::Behaviour(DragoonBehaviourEvent::Kademlia(
                kad::Event::OutboundQueryProgressed { id, result, .. },
            )) => {
                debug!("outbound query progressed");
                self.handle_query_result(result, id).await
            }
            SwarmEvent::Behaviour(DragoonBehaviourEvent::Identify(identify::Event::Sent {
                peer_id,
                ..
            })) => info!("Sent identify info to {}", peer_id),
            SwarmEvent::Behaviour(DragoonBehaviourEvent::Identify(identify::Event::Received {
                peer_id,
                info,
            })) => {
                info!("Received identify info '{:?}' from {}", info, peer_id);
                if let Some(addr) = info.listen_addrs.first() {
                    self.swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, addr.clone());
                    self.known_peer_id.insert(peer_id);
                    info!("Added peer {}", peer_id);
                } else {
                    error!("Peer {} not added, no listen address", peer_id);
                }
            }
            SwarmEvent::Behaviour(DragoonBehaviourEvent::RequestBlock(Event::Message {
                peer: _,
                message,
            })) => match message {
                Message::Request {
                    request, channel, ..
                } => {
                    if let Err(e) = self.message_request(request, channel).await {
                        error!("{}", e)
                    }
                }
                Message::Response {
                    request_id,
                    response,
                } => {
                    if let Some((save_to_disk, sender)) =
                        self.pending_request_block.remove(&request_id)
                    {
                        if save_to_disk {
                            let BlockResponse {
                                file_hash,
                                block_hash,
                                block_data,
                            } = response;
                            let save_path = get_block_dir(&self.file_dir, file_hash);
                            let res = match tfs::create_dir_all(&save_path).await {
                                Ok(_) => {
                                    let file_path: PathBuf =
                                        [save_path, PathBuf::from(block_hash)].iter().collect();
                                    match tfs::write(&file_path, block_data).await {
                                        Ok(_) => Ok(None),
                                        Err(e) => {
                                            let err_msg = format!(
                                                "Could not write the data to {:?}: {}",
                                                file_path, e
                                            );
                                            error!(err_msg);
                                            Err(format_err!(err_msg))
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("{}", e);
                                    Err(format_err!(e))
                                }
                            };
                            sender_send_match(
                                sender,
                                res,
                                format!("message response {}", request_id),
                            );
                        } else {
                            sender_send_match(
                                sender,
                                Ok(Some(response)),
                                format!("message response {}", request_id),
                            )
                        }
                    } else {
                        error!(
                            "Could no find the sender associated with {} for the message response",
                            request_id
                        );
                    }
                }
            },
            SwarmEvent::Behaviour(DragoonBehaviourEvent::RequestInfo(Event::Message {
                peer: _,
                message,
            })) => match message {
                Message::Request {
                    request, channel, ..
                } => {
                    debug!("Received a request for block info: {:?}", request);
                    if let Err(e) = self.info_request(request, channel).await {
                        error!("{}", e)
                    }
                }
                Message::Response {
                    request_id,
                    response,
                } => {
                    if let Some(sender) = self.pending_request_block_info.remove(&request_id) {
                        sender_send_match(
                            sender,
                            Ok(response.0),
                            format!("info response {}", request_id),
                        );
                    } else {
                        error!(
                            "Could no find the sender associated with {} for the info response",
                            request_id
                        );
                    }
                }
            },
            SwarmEvent::ConnectionEstablished {
                peer_id, endpoint, ..
            } => match endpoint {
                ConnectedPoint::Dialer { address, .. } => {
                    if let Some(sender) = self.pending_dial.remove(&address.to_string()) {
                        sender_send_match(sender, Ok(()), format!("dial {}", address));
                    } else {
                        error!(
                                "Could no find the sender associated with the multiaddr dial {} for the dial response (this might be due to a double dial attempt to the same node)",
                                address
                            );
                    }
                }
                ConnectedPoint::Listener { .. } => debug!(
                    "The node with peer id {:?} established a connection with us",
                    peer_id
                ),
            },
            e => warn!("[unknown event] {:?}", e),
        }
    }

    fn read_block_from_disk(block_hash: String, block_dir: PathBuf) -> Result<Vec<u8>>
where {
        let ser_block = sfs::read(block_dir.join(block_hash))?;
        Ok(ser_block)
    }

    async fn message_request(
        &mut self,
        request: BlockRequest,
        channel: ResponseChannel<BlockResponse>,
    ) -> Result<()> {
        let BlockRequest {
            file_hash,
            block_hash,
        } = request;
        let block_dir = get_block_dir(&self.file_dir.clone(), file_hash.clone());
        info!(
            "Searching blocks for the file {0} inside {1:?}",
            file_hash.clone(),
            block_dir
        );
        let ser_block = Self::read_block_from_disk(block_hash.clone(), block_dir)?;
        debug!(
            "Read block {0} for file {1}, got: {2:?}",
            block_hash, file_hash, ser_block
        );
        let channel_info = format!("{:?}", &channel);
        self.swarm
            .behaviour_mut()
            .request_block
            .send_response(
                channel,
                BlockResponse {
                    file_hash: file_hash.clone(),
                    block_hash: block_hash.clone(),
                    block_data: ser_block,
                },
            )
            .map_err(|_| CouldNotSendBlockResponse(block_hash, file_hash, channel_info).into())
    }

    async fn info_request(
        &mut self,
        request: PeerBlockInfoRequest,
        channel: ResponseChannel<PeerBlockInfoResponse>,
    ) -> Result<()> {
        let PeerBlockInfoRequest { file_hash } = request;
        let block_hashes = Self::get_block_list(self.file_dir.clone(), file_hash.clone()).await?;
        debug!(
            "A peer requested the blocks for file {}, node has : {:?}",
            file_hash, block_hashes
        );
        let channel_info = format!("{:?}", &channel);
        let peer_block_info = PeerBlockInfo {
            peer_id_base_58: self.swarm.local_peer_id().to_base58(),
            file_hash: file_hash.clone(),
            block_hashes,
            block_sizes: None,
        };
        self.swarm
            .behaviour_mut()
            .request_info
            .send_response(channel, PeerBlockInfoResponse(peer_block_info))
            .map_err(|_| CouldNotSendInfoResponse(file_hash, channel_info).into())
    }

    async fn handle_command<F, G, P>(&mut self, cmd: DragoonCommand)
    where
        F: PrimeField,
        G: CurveGroup<ScalarField = F>,
        P: DenseUVPolynomial<F>,
        for<'a, 'b> &'a P: Div<&'b P, Output = P>,
    {
        debug!("[cmd] {:?}", cmd);
        match cmd {
            DragoonCommand::Listen { multiaddr, sender } => {
                let res = self.listen(multiaddr).await;
                sender_send_match(sender, res, String::from("listen"));
            }
            DragoonCommand::GetListeners { sender } => {
                let listeners = self.swarm.listeners().cloned().collect::<Vec<Multiaddr>>();

                debug!("sending listeners {:?}", listeners);
                sender_send_match(sender, Ok(listeners), String::from("get listeners"));
            }
            DragoonCommand::GetNetworkInfo { sender } => {
                let network_info = self.swarm.network_info();

                debug!("sending network info {:?}", network_info);
                sender_send_match(sender, Ok(network_info), String::from("GetNetworkInfo"));
            }
            DragoonCommand::RemoveListener {
                listener_id,
                sender,
            } => {
                let res = self.remove_listener(listener_id).await;
                sender_send_match(sender, res, String::from("RemoveListener"));
            }
            DragoonCommand::GetConnectedPeers { sender } => {
                info!("Getting list of connected peers");
                let connected_peers = self
                    .swarm
                    .connected_peers()
                    .cloned()
                    .collect::<Vec<PeerId>>();

                debug!("sending connected_peers {:?}", connected_peers);
                sender_send_match(
                    sender,
                    Ok(connected_peers),
                    String::from("GetConnectedPeers"),
                );
            }
            DragoonCommand::GetFile {
                file_hash,
                output_filename,
                sender,
            } => {
                info!("Starting to get the file {}", file_hash);
                let cmd_sender = self.command_sender.clone();
                let powers_path = self.powers_path.clone();
                tokio::spawn(async move {
                    let res = Self::get_file::<F, G, P>(
                        cmd_sender,
                        file_hash.clone(),
                        output_filename,
                        powers_path,
                    )
                    .await;
                    sender_send_match(sender, res, format!("GetFile {}", file_hash));
                });
            }
            DragoonCommand::DialSingle { multiaddr, sender } => {
                if !self.pending_dial.contains_key(&multiaddr) {
                    let res = self.dial(multiaddr.clone()).await;
                    if res.is_err() {
                        sender_send_match(sender, res, String::from("DialSingle (error)"));
                    } else {
                        // need to check again even though we already did, because there was an await inbetween (and thus a potential modification of the hash_map)
                        if let std::collections::hash_map::Entry::Vacant(e) =
                            self.pending_dial.entry(multiaddr.clone())
                        {
                            e.insert(sender);
                        } else {
                            error!("Another dial attempt to {} modified the list of pending dial while waiting for this dial to complete", multiaddr)
                        }
                    }
                } else {
                    warn!("Tried to double dial on multiaddr {}", multiaddr);
                }
            }
            DragoonCommand::DialMultiple {
                list_multiaddr,
                sender,
            } => {
                let (dial_send, mut dial_recv) = mpsc::unbounded_channel();
                for multiaddr in list_multiaddr {
                    let sender = dial_send.clone();
                    let cmd_sender = self.command_sender.clone();
                    if cmd_sender
                        .send(DragoonCommand::DialSingle {
                            multiaddr: multiaddr.clone(),
                            sender: Sender::SenderMPSC(sender),
                        })
                        .is_err()
                    {
                        error!(
                            "Could not send the dial command for the multiaddr {}",
                            multiaddr
                        );
                    }
                }
                tokio::spawn(async move {
                    let mut err_list = vec![];
                    while let Some(res) = dial_recv.recv().await {
                        if res.is_err() {
                            err_list.push(res);
                        }
                    }
                    let final_res = match err_list[..] {
                        [] => Ok(()),
                        _ => Err(format_err!(
                            "Dial to all supplied multiaddr failed, got the following errors: {:?}",
                            err_list
                        )),
                    };
                    // making the error msg first to not have to clone to prevent borrowed value move
                    let err_msg = format!(
                        "Could not send the result of the dial_multiple operation: {:?}",
                        final_res
                    );
                    sender_send_match(sender, final_res, err_msg);
                });
            }
            DragoonCommand::AddPeer { multiaddr, sender } => {
                let res = self.add_peer(multiaddr).await;
                sender_send_match(sender, res, String::from("AddPeer"));
            }
            DragoonCommand::StartProvide { key, sender } => {
                if let Ok(query_id) = self
                    .swarm
                    .behaviour_mut()
                    .kademlia
                    .start_providing(key.clone().into_bytes().into())
                {
                    self.pending_start_providing.insert(query_id, sender);
                } else {
                    error!("Could not provide {}", key);
                    let err = ProviderError(format!("Could not provide {}", key));

                    debug!("sending error {}", err);
                    sender_send_match(sender, Err(format_err!(err)), String::from("StartProvide"));
                }
            }
            DragoonCommand::StopProvide { key, sender } => {
                self.swarm
                    .behaviour_mut()
                    .kademlia
                    .stop_providing(&key.clone().into_bytes().into());
                //? need to remove from pending_start_providing ? how ? we don't have the queryID
                sender_send_match(sender, Ok(()), "StopProvide".to_string())
            }
            DragoonCommand::GetProviders { key, sender } => {
                let mut provider_stream = self.get_providers(key);
                tokio::spawn(async move {
                    // instead of returning the stream directly through the Sender, put it in a Vec format so it's easier to read for the person getting it
                    let mut all_providers = Vec::<PeerId>::default();
                    while let Some(provider) = provider_stream.next().await {
                        all_providers.push(provider);
                    }
                    sender_send_match(sender, Ok(all_providers), String::from("GetProviders"));
                });
            }
            DragoonCommand::Bootstrap { sender } => {
                let res = self.bootstrap().await;
                sender_send_match(sender, res, String::from("Bootstrap"));
            }
            DragoonCommand::GetBlockFrom {
                peer_id,
                file_hash,
                block_hash,
                save_to_disk,
                sender,
            } => {
                let request_id = self.swarm.behaviour_mut().request_block.send_request(
                    &peer_id,
                    BlockRequest {
                        file_hash,
                        block_hash,
                    },
                );
                self.pending_request_block
                    .insert(request_id, (save_to_disk, sender));
            }
            DragoonCommand::GetBlocksInfoFrom {
                peer_id,
                file_hash,
                sender,
            } => self.get_blocks_info_from(peer_id, file_hash, sender),
            DragoonCommand::GetBlockList { file_hash, sender } => {
                let res = Self::get_block_list(self.file_dir.clone(), file_hash).await;
                sender_send_match(sender, res, String::from("GetBlocksInfoFrom"));
            }
            DragoonCommand::DecodeBlocks {
                block_dir,
                block_hashes,
                output_filename,
                sender,
            } => {
                let res = Self::decode_blocks::<F, G>(
                    PathBuf::from(block_dir),
                    &block_hashes,
                    output_filename,
                )
                .await;
                sender_send_match(sender, res, String::from("DecodeBlocks"));
            }
            DragoonCommand::EncodeFile {
                file_path,
                replace_blocks,
                encoding_method,
                encode_mat_k,
                encode_mat_n,
                sender,
            } => {
                let res = Self::encode_file::<F, G, P>(
                    self.file_dir.clone(),
                    file_path,
                    replace_blocks,
                    encoding_method,
                    encode_mat_k,
                    encode_mat_n,
                    self.powers_path.clone(),
                )
                .await;
                sender_send_match(sender, res, String::from("EncodeFile"));
            }
            DragoonCommand::GetBlockDir { file_hash, sender } => {
                let res = Ok(get_block_dir(&self.file_dir.clone(), file_hash));
                sender_send_match(sender, res, String::from("GetBlockDir"));
            }
            DragoonCommand::GetFileDir { file_hash, sender } => {
                let res = Ok(get_file_dir(&self.file_dir.clone(), file_hash));
                sender_send_match(sender, res, String::from("GetFileDir"));
            }
            DragoonCommand::NodeInfo { sender } => {
                let res = Ok((*(self.swarm.local_peer_id()), self.label.clone()));
                sender_send_match(sender, res, String::from("NodeInfo"));
            }
            DragoonCommand::SendBlockTo {
                peer_id,
                file_hash,
                block_hash,
                sender,
            } => {
                // check if we are already trying to send this given block to this peer
                if !self
                    .pending_send_block_to
                    .contains(&(peer_id, block_hash.clone()))
                {
                    self.pending_send_block_to
                        .insert((peer_id, block_hash.clone()));
                    self.send_block_to(peer_id, block_hash, file_hash, sender);
                    //TODO remove the entry from the hash table once we are done, use a command ?
                } else {
                    let send_id = SendId {
                        peer_id,
                        file_hash,
                        block_hash,
                    };
                    let err = Err(SendBlockToAlreadyStarted { send_id });

                    sender_send_match(sender, err, String::from("SendBlockTo (error)"));
                }
            }
            DragoonCommand::SendBlockList {
                strategy_name,
                file_hash,
                block_list,
                sender,
            } => {
                let number_of_blocks_to_send = block_list.len();
                //not my proudest line with a dynamic type cast
                let send_stream: Pin<Box<dyn FusedStream<Item = SendId> + Send>> =
                    match strategy_name {
                        StrategyName::Random => {
                            let known_peers = self.known_peer_id.clone().into_iter();
                            let peer_input_stream = f_stream::iter(known_peers).fuse();
                            let size_of_block_list = block_list.len();
                            let block_input_stream = f_stream::iter(
                                vec![file_hash; size_of_block_list]
                                    .into_iter()
                                    .zip(block_list),
                            )
                            .fuse();
                            let random_distribution =
                                Box::<send_strategy_impl::random::RandomDistribution>::default();
                            Box::pin(random_distribution.get_send_stream(
                                Box::pin(peer_input_stream),
                                Box::pin(block_input_stream),
                            ))
                        }
                        StrategyName::RoundRobin => {
                            let mut known_peers =
                                self.known_peer_id.clone().into_iter().collect::<Vec<_>>();
                            //sort to ensure the ordering for the tests is not random
                            known_peers.sort();
                            let known_peers = known_peers.into_iter();
                            let peer_input_stream = f_stream::iter(known_peers).fuse();
                            let size_of_block_list = block_list.len();
                            let block_input_stream = f_stream::iter(
                                vec![file_hash; size_of_block_list]
                                    .into_iter()
                                    .zip(block_list),
                            )
                            .fuse();
                            let robin_distribution = Box::<
                                send_strategy_impl::round_robin::RobinDistribution,
                            >::default();
                            Box::pin(robin_distribution.get_send_stream(
                                Box::pin(peer_input_stream),
                                Box::pin(block_input_stream),
                            ))
                        }
                    };
                let cmd_sender = self.command_sender.clone();
                tokio::spawn(async move {
                    let res =
                        Self::send_block_list(number_of_blocks_to_send, send_stream, cmd_sender)
                            .await;
                    sender_send_match(sender, res, String::from("SendBlockList"));
                });
            }
            DragoonCommand::RemoveEntryFromSendBlockToSet {
                peer_id,
                block_hash,
                sender,
            } => {
                self.pending_send_block_to.remove(&(peer_id, block_hash));
                sender_send_match(
                    sender,
                    Ok(()),
                    String::from("RemoveEntryFromSendBlockToSet"),
                );
            }
            DragoonCommand::GetAvailableStorage { sender } => {
                let available_storage = self
                    .current_available_storage_for_send
                    .load(Ordering::Relaxed);
                sender_send_match(
                    sender,
                    Ok(available_storage),
                    String::from("GetAvailableStorage"),
                );
            }
            DragoonCommand::ChangeAvailableSendStorage {
                new_storage_size,
                sender,
            } => {
                let already_used_size = self
                    .current_total_size_of_blocks_on_disk
                    .load(Ordering::Relaxed);
                let result_answer = if already_used_size >= new_storage_size {
                    self.current_available_storage_for_send
                        .store(0, Ordering::Relaxed);
                    format!("New storage size is {} but already used size is {}, no more blocks will be accepted via send request", new_storage_size, already_used_size)
                } else {
                    let remaining_size = new_storage_size - already_used_size;
                    self.current_available_storage_for_send
                        .store(remaining_size, Ordering::Relaxed);
                    // we could have a race condition where the already_used_size changed in the meantime
                    // but since there are no await probably no problem there
                    format!("New total storage space is {}, {} is already used so the remaining available size for send blocks is {}", new_storage_size, already_used_size, remaining_size)
                };
                sender_send_match(
                    sender,
                    Ok(result_answer),
                    String::from("ChangeAvailableSendStorage"),
                )
            }
        }
    }

    async fn listen(&mut self, multiaddr: String) -> Result<u64> {
        if let Ok(addr) = multiaddr.parse() {
            match self.swarm.listen_on(addr) {
                Ok(listener_id) => {
                    info!("Listening on {}", multiaddr);

                    let id = regex::Regex::new(r"ListenerId\((\d+)\)")
                        .unwrap()
                        .captures(&format!("{:?}", listener_id))
                        .unwrap()
                        .get(1)
                        .unwrap()
                        .as_str()
                        .parse::<u64>()
                        .unwrap();
                    self.listeners.insert(id, listener_id);

                    Ok(id)
                }
                Err(te) => {
                    let err_msg = match te {
                        TransportError::Other(e) => e.to_string(),
                        TransportError::MultiaddrNotSupported(addr) => {
                            format!("multiaddr {} not supported", addr)
                        }
                    };
                    error!(err_msg);
                    Err(BadListener(err_msg).into())
                }
            }
        } else {
            let err_msg = format!("Could not parse {}", multiaddr);
            error!(err_msg);
            Err(BadListener(err_msg).into())
        }
    }

    async fn remove_listener(&mut self, listener_id: u64) -> Result<bool> {
        if let Some(listener) = self.listeners.get(&listener_id) {
            Ok(self.swarm.remove_listener(*listener))
        } else {
            let err_msg = format!("Listener {} not found", listener_id);
            error!(err_msg);
            Err(BadListener(err_msg).into())
        }
    }

    /// This function will get the file whose hash is `file_hash`
    /// It will first do a Kademlia request to search the peers that have announced providing this file
    /// When it has this list, it will contact those peers so they can give the list blocks of the file they have
    /// This function will start downloading the blocks she gets the information and verify that the blocks are correct (not corrupted)
    /// It will continue like that until it has `k` distinct blocks, at which point it will check if the k first block allow for file reconstruction
    /// - If it can reconstruct the file, it will close the requests for block info and blocks to all the peers it contacted, construct the file, write it to disk and send the path where the file was written to the user
    /// - If it can't reconstruct the file yet, given the block combination it got from block info, it will try to find the combination of blocks that will allow for file reconstruction with a minimal block download (ie using the max number of already downloaded blocks it can)
    /// - If even after all that it still can't find a combination of blocks that works, it will exit with an error
    async fn get_file<F, G, P>(
        cmd_sender: mpsc::UnboundedSender<DragoonCommand>,
        file_hash: String,
        output_filename: String,
        powers_path: PathBuf,
    ) -> Result<PathBuf>
    where
        F: PrimeField,
        G: CurveGroup<ScalarField = F>,
        P: DenseUVPolynomial<F>,
        for<'a, 'b> &'a P: Div<&'b P, Output = P>,
    {
        info!("Get file: getting providers of file {}", file_hash);
        let (get_prov_sender, get_prov_recv) = oneshot::channel();
        if cmd_sender
            .send(DragoonCommand::GetProviders {
                key: file_hash.clone(),
                sender: Sender::SenderOneS(get_prov_sender),
            })
            .is_err()
        {
            let err_msg = format!("Could not send the command to request the list of providers, shutting down the get_file request for {}", file_hash);
            error!(err_msg);
            return Err(format_err!(err_msg));
        };
        //TODO this needs to be handled differently to return the provider stream to go faster
        //TODO change this to be spawned inside a new task to not have to wait for all the providers to be received to start asking info
        let provider_list = get_prov_recv.await??;
        debug!(
            "Got provider list for file {}: {:?}",
            file_hash, provider_list
        );

        // Check where to write the blocks
        let (block_dir_sender, block_dir_recv) = oneshot::channel();
        if cmd_sender
            .send(DragoonCommand::GetBlockDir {
                file_hash: file_hash.clone(),
                sender: Sender::SenderOneS(block_dir_sender),
            })
            .is_err()
        {
            let err_msg = format!("Could not get the location of where to write the blocks for file request {}, shutting down request", file_hash);
            error!(err_msg);
            return Err(format_err!(err_msg));
        };
        let block_dir = block_dir_recv.await??;
        debug!("Will write the blocks in {:?}", block_dir);
        //TODO create the block directory recursively
        tokio::fs::create_dir_all(&block_dir).await?;
        debug!("Finished creating the directory for blocks");

        // Check where to write the file
        //TODO just get the file dir by taking the parent of the block_dir, no need for a command
        let (get_file_dir_sender, get_file_dir_recv) = oneshot::channel();
        if cmd_sender
            .send(DragoonCommand::GetFileDir {
                file_hash: file_hash.clone(),
                sender: Sender::SenderOneS(get_file_dir_sender),
            })
            .is_err()
        {
            let err_msg = format!("We could get the block directory {:?} to write for {} but could not access the file directory which is its parent", block_dir, file_hash);
            error!(err_msg);
            return Err(format_err!(err_msg));
        };
        let file_dir = get_file_dir_recv.await??;
        debug!("Will write the file in {:?}", file_dir);

        let (info_sender, info_receiver) = mpsc::unbounded_channel();

        debug!(
            "Requesting the information about list of blocks for file {} from peers {:?}",
            file_hash, provider_list
        );

        if provider_list.is_empty() {
            return Err(format_err!("The provider list for the file {} is empty; \nTip: did the nodes with blocks of the file use `start-provide` ?", file_hash));
        }

        for peer_id in provider_list {
            let err_msg = format!("Could not send the command to request the list of blocks from peer {} for the get_file request for {}", peer_id, file_hash);
            if cmd_sender
                .send(DragoonCommand::GetBlocksInfoFrom {
                    peer_id,
                    file_hash: file_hash.clone(),
                    sender: Sender::SenderMPSC(info_sender.clone()),
                })
                .is_err()
            {
                error!(err_msg);
            };
        }
        debug!("Finished requesting block info list for file {}", file_hash);
        drop(info_sender);

        //TODO change this to keep in memory other providers of the same block in case the first one fails (a hash map maybe ?)

        let mut block_hashes_on_disk = vec![];

        async fn download_first_k_blocks<F, G, P>(
            mut info_receiver: UnboundedReceiver<Result<PeerBlockInfo>>,
            powers_path: PathBuf,
            block_hashes_on_disk: &mut Vec<String>,
            cmd_sender: UnboundedSender<DragoonCommand>,
            file_hash: String,
            block_dir: PathBuf,
        ) -> Result<()>
        where
            F: PrimeField,
            G: CurveGroup<ScalarField = F>,
            P: DenseUVPolynomial<F>,
            for<'a, 'b> &'a P: Div<&'b P, Output = P>,
        {
            let mut already_request_block = vec![];
            let powers = get_powers(powers_path).await?;
            let mut number_of_blocks_written: u32 = 0;

            let (block_sender, mut block_receiver) = mpsc::unbounded_channel();

            'download_first_k_blocks: loop {
                tokio::select! {
                        biased;
                        Some(response) = info_receiver.recv() => {

                                //TODO handle errors to keep going even if some peer fail
                                let response = response.map_err(|e| -> anyhow::Error {
                                    format_err!("Could not retrieve peer block block info: {}", e)
                                })?;
                                let PeerBlockInfo { peer_id_base_58, file_hash, block_hashes, .. } = response;
                                debug!("Got block list from {} for file {} : {:?}", peer_id_base_58, file_hash, block_hashes);
                                let blocks_to_request: Vec<String> = block_hashes
                                        .into_iter()
                                        .filter(|x| !already_request_block.contains(x)) // do not request the block if it's already requested
                                        .collect();
                                debug!("Requesting the following blocks from {} for file {} : {:?}", peer_id_base_58, file_hash, blocks_to_request);
                                let bytes = bs58::decode(peer_id_base_58).into_vec().unwrap();
                                let peer_id = PeerId::from_bytes(&bytes).unwrap();
                                for block_hash in blocks_to_request {
                                    let err_msg = format!("Could not send the command to get the block {} from peer {} for file {}", block_hash, peer_id, file_hash);
                                    if cmd_sender.send(DragoonCommand::GetBlockFrom {peer_id, file_hash: file_hash.clone(), block_hash: block_hash.clone(), save_to_disk: false, sender: Sender::SenderMPSC(block_sender.clone())}).is_err() {
                                        error!(err_msg);
                                    }
                                    else {
                                        already_request_block.push(block_hash);
                                    }

                                }
                        },
                        Some(response) = block_receiver.recv() => {
                            //TODO change this unwrap
                            let maybe_block_response = response.unwrap();
                            if let Some(block_response) = maybe_block_response {
                                let block: Block<F,G> = match Block::deserialize_with_mode(&block_response.block_data[..], Compress::Yes, Validate::Yes) {
                                    Ok(block) => block,
                                    Err(e) => {error!("Could not deserialize a block in get-file, got error: {}", e);
                                continue 'download_first_k_blocks}
                                };
                                debug!("Got a block for the file {} : {} ", file_hash, block_response.block_hash);
                                let number_of_blocks_to_reconstruct_file = block.shard.k;
                                debug!("Number of blocks to reconstruct file {} : {}", file_hash, number_of_blocks_to_reconstruct_file);
                                if verify::<F,G,P>(&block, &powers)? {
                                    //TODO check if the new block is not linearly dependant with the other blocks already on disk
                                    debug!("Block {} for file {} was verified successfully; Now dumping to disk", block_response.block_hash, file_hash);
                                    let _ = fs::dump(&block, &block_dir, None, Compress::Yes)?;
                                    number_of_blocks_written += 1;
                                    block_hashes_on_disk.push(block_response.block_hash);
                                    if number_of_blocks_written >= number_of_blocks_to_reconstruct_file {
                                        debug!("Received exactly {} blocks, pausing block download and trying to reconstruct the file {}", number_of_blocks_to_reconstruct_file, file_hash);
                                        //TODO properly stop downloads ? drop/close receiver ?
                                        break 'download_first_k_blocks;
                                    }
                                }
                                else {
                                    //TODO ask the block again ? change provider ?
                                    todo!()
                                }
                            }
                            else {
                                error!("No block response was sent when using get file, the node might have saved it to disk")
                            }

                        }

                }
            }
            Ok(())
        }

        let timeout_duration = Duration::from_secs(10);

        match time::timeout(
            timeout_duration,
            download_first_k_blocks::<F, G, P>(
                info_receiver,
                powers_path,
                &mut block_hashes_on_disk,
                cmd_sender,
                file_hash,
                block_dir.clone(),
            ),
        )
        .await
        {
            Ok(res) => {
                match res {
                    Ok(_) => {} //nothing to do
                    Err(e) => {
                        error!("{}", e);
                        return Err(format_err!(
                            "Getting the required amount of blocks failed due to the following: {}",
                            e
                        ));
                    }
                }
            }
            Err(_) => {
                let err_msg = "Getting the required amount of blocks to make the file timed-out, not enough blocks to make the file";
                error!(err_msg);
                return Err(format_err!(err_msg));
            }
        }

        let _ = Self::decode_blocks::<F, G>(
            block_dir.clone(),
            &block_hashes_on_disk,
            output_filename.clone(),
        )
        .await;

        //TODO if it fails, keep requesting block info, try to check which matrix is invertible taking k-1 blocks already on disk and one more that isn't
        //TODO if it fails, do the same with k-2, etc...
        //TODO when a combination of the blocks that works is found, request the missing blocks
        Ok([file_dir, PathBuf::from(output_filename)].iter().collect())
        //Ok(PathBuf::from(format!("{:?}/{}", file_dir, output_filename)))
    }

    async fn dial(&mut self, multiaddr: String) -> Result<()> {
        if let Ok(addr) = multiaddr.parse::<Multiaddr>() {
            match self.swarm.dial(addr) {
                Ok(()) => Ok(()),
                Err(de) => {
                    let err_msg = format!("Could not dial {0}: {1}", multiaddr, de);
                    error!(err_msg);
                    Err(DialError(err_msg).into())
                }
            }
        } else {
            let err_msg = format!("Could not parse {}", multiaddr);
            error!(err_msg);
            Err(BadListener(err_msg).into())
        }
    }

    async fn add_peer(&mut self, multiaddr: String) -> Result<()> {
        if let Ok(addr) = multiaddr.parse::<Multiaddr>() {
            if let Some(Protocol::P2p(hash)) = addr.iter().last() {
                self.swarm.behaviour_mut().kademlia.add_address(&hash, addr);
                Ok(())
            } else {
                let err_msg = format!("could no isolate P2P component in {}", addr);
                error!(err_msg);
                Err(BadListener(err_msg).into())
            }
        } else {
            let err_msg = format!("Could not parse {}", multiaddr);
            error!(err_msg);
            Err(BadListener(err_msg).into())
        }
    }

    /// This returns the Stream instead of sending it back through the Sender so it can be handled later
    fn get_providers(&mut self, key: String) -> BoxStream<'static, PeerId> {
        let query_id = self
            .swarm
            .behaviour_mut()
            .kademlia
            .get_providers(key.into_bytes().into());
        let (m_sender, mut m_receiver) = mpsc::unbounded_channel::<Result<HashSet<PeerId>>>();
        self.pending_get_providers.insert(query_id, m_sender);
        let providers = async_stream::stream! {
            let mut current_providers: HashSet<PeerId> = Default::default();
            while let Some(Ok(hash_set)) = m_receiver.recv().await {
                for prov in hash_set.into_iter() {
                    if current_providers.insert(prov) {
                        yield prov;
                    }
                }
            }
        };

        providers.boxed()
    }

    async fn bootstrap(&mut self) -> Result<()> {
        match self.swarm.behaviour_mut().kademlia.bootstrap() {
            Ok(_) => Ok(()),
            Err(nkp) => {
                error!("Bootstrap: no known peers");
                Err(BootstrapError(nkp.to_string()).into())
            }
        }
    }

    fn get_blocks_info_from(
        &mut self,
        peer_id: PeerId,
        file_hash: String,
        sender: Sender<PeerBlockInfo>,
    ) {
        let request_id = self
            .swarm
            .behaviour_mut()
            .request_info
            .send_request(&peer_id, PeerBlockInfoRequest { file_hash });
        self.pending_request_block_info.insert(request_id, sender);
    }

    async fn get_block_list(file_dir: PathBuf, file_hash: String) -> Result<Vec<String>> {
        let block_path = get_block_dir(&file_dir, file_hash.clone());
        let mut block_names = vec![];
        let mut dir_entry = tfs::read_dir(block_path).await?;
        while let Some(entry) = dir_entry.next_entry().await? {
            block_names.push(entry.file_name().into_string().map_err(
                |os_string| -> anyhow::Error {
                    format_err!(
                        "Could not convert the os string {:?} as a valid String for file {}",
                        os_string,
                        file_hash,
                    )
                },
            )?);
        }
        Ok(block_names)
    }

    async fn decode_blocks<F, G>(
        block_dir: PathBuf,
        block_hashes: &[String],
        output_filename: String,
    ) -> Result<()>
    where
        F: PrimeField,
        G: CurveGroup<ScalarField = F>,
    {
        let blocks =
            fs::read_blocks::<F, G>(block_hashes, &block_dir, Compress::Yes, Validate::Yes)?;
        let shards: Vec<Shard<F>> = blocks.into_iter().map(|b| b.1.shard).collect();
        let vec_bytes = fec::decode::<F>(shards)?;
        if let Some(parent_dir_path) = Path::new(&block_dir).parent() {
            let file_path: PathBuf = [parent_dir_path, Path::new(&output_filename)]
                .iter()
                .collect();
            info!("Trying to create a file at {:?}", file_path);
            let mut file = tokio::fs::File::create(file_path).await?;
            file.write_all(vec_bytes.as_slice()).await?;
        } else {
            error!("Parent of the block directory does not exist");
            let err = NoParentDirectory(format!("{:?}", block_dir));
            return Err(err.into());
        }
        Ok(())
    }

    async fn encode_file<F, G, P>(
        output_file_dir: PathBuf,
        file_path: String,
        replace_blocks: bool,
        encoding_method: EncodingMethod,
        encode_mat_k: usize,
        encode_mat_n: usize,
        powers_path: PathBuf,
    ) -> Result<(String, String)>
    where
        F: PrimeField,
        G: CurveGroup<ScalarField = F>,
        P: DenseUVPolynomial<F>,
        for<'a, 'b> &'a P: Div<&'b P, Output = P>,
    {
        info!("Reading file to convert from {:?}", file_path);
        let bytes = tokio::fs::read(&file_path).await?;
        let file_hash = Sha256::hash(&bytes)
            .iter()
            .map(|x| format!("{:x}", x))
            .collect::<Vec<_>>()
            .join("");
        let encoding_mat = match encoding_method {
            EncodingMethod::Vandermonde => {
                let points: Vec<F> = (0..encode_mat_n)
                    .map(|i| F::from_le_bytes_mod_order(&i.to_le_bytes()))
                    .collect();
                Matrix::vandermonde(&points, encode_mat_k)?
            }
            EncodingMethod::Random => {
                // use of RNG in async: https://stackoverflow.com/a/75227719
                let mut rng = rand::thread_rng();
                Matrix::random(encode_mat_k, encode_mat_n, &mut rng)
            }
        };
        let shards = fec::encode::<F>(&bytes, &encoding_mat)?;
        let powers = get_powers(powers_path).await?;
        let proof = komodo::semi_avid::prove::<F, G, P>(&bytes, &powers, encode_mat_k)?;
        let blocks = komodo::semi_avid::build::<F, G, P>(&shards, &proof);
        let block_dir = get_block_dir(&output_file_dir, file_hash.clone());
        info!(
            "Checking if the block directory already exists or not: {:?}",
            block_dir
        );
        let dir_exists = tokio::fs::try_exists(&block_dir).await?;
        if dir_exists && replace_blocks {
            info!(
                "Replace block option has been chosen, removing the directory at {:?}",
                block_dir
            );
            tokio::fs::remove_dir_all(&block_dir).await?;
        }
        info!("Creating directory at {:?}", block_dir);
        tokio::fs::create_dir_all(&block_dir).await?;
        let formatted_output = fs::dump_blocks(&blocks, &block_dir, Compress::Yes)?;
        Ok((file_hash, formatted_output))
    }

    fn send_block_to(
        &mut self,
        peer_id: PeerId,
        block_hash: String,
        file_hash: String,
        sender: Sender<(bool, SendId), DragoonError>,
    ) {
        let mut control = self.swarm.behaviour().send_block.new_control();
        let own_peer_id = *self.swarm.local_peer_id();
        let file_dir = self.file_dir.clone();
        let cmd_sender = self.command_sender.clone();
        tokio::spawn(async move {
            let stream = match control.open_stream(peer_id, SEND_BLOCK_PROTOCOL).await {
                Ok(stream) => stream,
                Err(e) => {
                    error!("{}", e);
                    return;
                }
            };
            let res = send_block_to::send_block_to(
                stream,
                own_peer_id,
                peer_id,
                block_hash.clone(),
                file_hash,
                file_dir,
            )
            .await
            .map_err(|send_id| SendBlockToError { send_id });
            let (remove_sender, remove_receiver) = oneshot::channel();
            if cmd_sender
                .send(DragoonCommand::RemoveEntryFromSendBlockToSet {
                    peer_id,
                    block_hash: block_hash.clone(),
                    sender: Sender::SenderOneS(remove_sender),
                })
                .is_err()
            {
                error!(
                    "Could not send the request to remove the entry in the table of pending_send_block_to corresponding to ({}, {})", 
                    peer_id, block_hash
                )
            }

            let _ = remove_receiver.await;
            sender_send_match(sender, res, String::from("SendBlockTo"));
        });
    }

    async fn send_block_list(
        number_of_blocks_to_send: usize,
        send_stream: impl FusedStream<Item = SendId>,
        cmd_sender: mpsc::UnboundedSender<DragoonCommand>,
    ) -> Result<Vec<SendId>, DragoonError> {
        let mut final_block_distribution: Vec<SendId> = Default::default();
        let mut rejected_blocks: Vec<(String, String)> = Default::default();
        let mut accepted_peers: HashSet<PeerId> = Default::default();
        let mut rejected_peers: HashSet<PeerId> = Default::default();

        fn send_block_to_loc(
            peer_id: PeerId,
            file_hash: String,
            block_hash: String,
            cmd_sender: mpsc::UnboundedSender<DragoonCommand>,
            res_sender: mpsc::UnboundedSender<Result<(bool, SendId), DragoonError>>,
        ) {
            let err_msg = format!(
                "Could not send the command SendBlockTo to {} for file_hash {} block_hash {}",
                peer_id, file_hash, block_hash
            );
            if cmd_sender
                .send(DragoonCommand::SendBlockTo {
                    peer_id,
                    file_hash,
                    block_hash,
                    sender: Sender::SenderMPSC(res_sender),
                })
                .is_err()
            {
                error!(err_msg)
            }
        }

        async fn optimistic_loop(
            send_stream: impl FusedStream<Item = SendId>,
            cmd_sender: mpsc::UnboundedSender<DragoonCommand>,
            number_of_blocks_to_send: &usize,
            accepted_peers: &mut HashSet<PeerId>,
            rejected_peers: &mut HashSet<PeerId>,
            rejected_blocks: &mut Vec<(String, String)>,
            final_block_distribution: &mut Vec<SendId>,
        ) -> Result<()> {
            let (res_sender, mut res_recv) = mpsc::unbounded_channel();

            pin_mut!(send_stream);
            let mut res_sender_vec: Vec<_> = std::iter::repeat(res_sender)
                .take(*number_of_blocks_to_send)
                .collect();

            loop {
                tokio::select! {
                    biased;
                    Some(send_info) = send_stream.next() => {
                        let SendId{peer_id, file_hash, block_hash} = send_info;
                        let res_sender = match res_sender_vec.pop() {
                            Some(res_sender) => res_sender,
                            None => {
                                let err_msg = format!(
                                    "There were more blocks to be sent than the expected number to send for file {}",
                                    file_hash
                                );
                                error!(err_msg);
                                return Err(format_err!(err_msg));
                            }
                        };
                        send_block_to_loc(
                            peer_id,
                            file_hash,
                            block_hash,
                            cmd_sender.clone(),
                            res_sender
                        );
                    }
                    Some(send_res) = res_recv.recv() => {
                        match send_res {
                            Ok((boolean, send_id)) => match boolean {
                                true => {
                                    // need to check because we can get a return from the peer refusing a block and accepting one later
                                    // due to the order in which the streams are handled
                                    if !rejected_peers.contains(&send_id.peer_id) {
                                        let inserted_peer_set = accepted_peers.insert(send_id.peer_id);
                                        debug!("inserted {} in accepted set : {}", send_id.peer_id, inserted_peer_set);
                                    }
                                    final_block_distribution.push(send_id)
                                },
                                false => {
                                    let removed_accepted_peer_set = accepted_peers.remove(&send_id.peer_id);
                                    debug!("removed {} from accepted set : {}", send_id.peer_id, removed_accepted_peer_set);
                                    let inserted_reject_peer_set = rejected_peers.insert(send_id.peer_id);
                                    debug!("inserted {} in rejected set : {}", send_id.peer_id, inserted_reject_peer_set);
                                    rejected_blocks.push((send_id.file_hash, send_id.block_hash))
                                },
                            },
                            Err(dragoon_error) => match dragoon_error {
                                SendBlockToError{send_id} => rejected_blocks.push((send_id.file_hash, send_id.block_hash)),
                                SendBlockToAlreadyStarted{send_id} => error!(
                                    "Unexpected multiple send to {:?} for file hash {} block hash {}",
                                    send_id.peer_id,
                                    send_id.file_hash,
                                    send_id.block_hash
                                ),
                                e => error!("Unexpected error for SendBlockTo: {}", e),
                            },
                        }
                    },
                    else => {

                        info!("Finished the first loop of the SendBlockList call");
                        res_recv.close();
                        return Ok(())
                    },
                }
            }
        }

        let timeout_duration = Duration::from_secs(10);

        match time::timeout(
            timeout_duration,
            optimistic_loop(
                send_stream,
                cmd_sender.clone(),
                &number_of_blocks_to_send,
                &mut accepted_peers,
                &mut rejected_peers,
                &mut rejected_blocks,
                &mut final_block_distribution,
            ),
        )
        .await
        {
            Ok(result) => match result {
                Ok(_) => {}
                Err(e) => {
                    return Err(DragoonError::SendBlockListFailed {
                        final_block_distribution,
                        context: e.to_string(),
                    })
                }
            },
            Err(_) => warn!("The first loop of send block to timed-out, attempting recuperation"),
        }

        fn handle_rejected_block(
            maybe_peer_id: Option<PeerId>,
            file_hash: String,
            block_hash: String,
            accepted_peers: &mut Vec<PeerId>,
            accepted_peers_index: &mut usize,
            cmd_sender: mpsc::UnboundedSender<DragoonCommand>,
            res_sender: mpsc::UnboundedSender<Result<(bool, SendId), DragoonError>>,
        ) -> Result<()> {
            if let Some(peer_id) = maybe_peer_id {
                // remove the peer that just rejected the block from the list of peers that previously accepted a peer
                if let Some(index) = accepted_peers
                    .iter()
                    .position(|iter_peer_id| *iter_peer_id == peer_id)
                {
                    accepted_peers.remove(index); //using swap_remove would mess up the order and doesn't ensure all peers get an equivalent number of blocks
                    if let Some(res) = accepted_peers_index.checked_sub(1) {
                        *accepted_peers_index = res
                    };
                } else {
                    debug!(
                    "Tried to remove {} from the list of accepted peers but it was not in the list",
                    peer_id
                );
                }
            }

            // take a new peer to send the block
            let remaining_peer_number = accepted_peers.len();
            let peer_id = match accepted_peers.get(*accepted_peers_index) {
                Some(peer_id) => {
                    *accepted_peers_index += 1;
                    if *accepted_peers_index >= remaining_peer_number {
                        *accepted_peers_index = 0
                    };
                    peer_id
                }
                // there are no more known peers that will accept blocks but we have blocks left to send
                None => {
                    if remaining_peer_number == 0 {
                        return Err(anyhow::Error::msg(
                            "No more peers to send but blocks are left",
                        ));
                    } else {
                        return Err(format_err!(
                            "Invalid get index on the list of accepted peers: remaining peer number is {} but the get index was {}",
                            remaining_peer_number,
                            *accepted_peers_index,
                        ));
                    }
                }
            };

            send_block_to_loc(
                *peer_id,
                file_hash,
                block_hash,
                cmd_sender.clone(),
                res_sender.clone(),
            );
            Ok(())
        }

        if rejected_blocks.is_empty() {
            //checking for size because due to the timeout on the first loop it's possible
            //that we end up not pushing anything to rejected blocks without having actually sent everything
            if final_block_distribution.len() == number_of_blocks_to_send {
                return Ok(final_block_distribution);
            } else {
                return Err(DragoonError::SendBlockListFailed{final_block_distribution, context: "The rejected block list is empty but not all blocks have been sent, unknown configuration".to_string()});
            }
        }

        // recreate the sender (as it was consumed previously)
        let (res_sender, mut res_recv) =
            mpsc::unbounded_channel::<Result<(bool, SendId), DragoonError>>();

        let mut accepted_peers_index = 0;
        let mut accepted_peers: Vec<PeerId> = accepted_peers.into_iter().collect();

        //ensure order stays the same for reproducibility purpose
        accepted_peers.sort();

        for (file_hash, block_hash) in rejected_blocks.clone() {
            match handle_rejected_block(
                None,
                file_hash,
                block_hash,
                &mut accepted_peers,
                &mut accepted_peers_index,
                cmd_sender.clone(),
                res_sender.clone(),
            ) {
                Ok(_) => {}
                Err(e) => {
                    return Err(DragoonError::SendBlockListFailed {
                        final_block_distribution,
                        context: e.to_string(),
                    })
                }
            }
        }

        info!("Now entering error handling for blocks that were not sent");
        'recuperation: while let Some(send_res) = res_recv.recv().await {
            match send_res {
                Ok((boolean, send_id)) => {
                    if boolean {
                        final_block_distribution.push(send_id.clone());
                        // remove the block from the list of rejected blocks
                        if let Some(index) =
                            rejected_blocks.iter().position(|(file_hash, block_hash)| {
                                (file_hash, block_hash) == (&send_id.file_hash, &send_id.block_hash)
                            })
                        {
                            rejected_blocks.swap_remove(index);
                        } else {
                            error!(
                                "Tried to remove {:?} from the list of rejected blocks but it was not in the list",
                                (send_id.file_hash, send_id.block_hash)
                            );
                        }
                    } else {
                        // meaning the block got rejected
                        let SendId {
                            peer_id,
                            file_hash,
                            block_hash,
                        } = send_id;
                        match handle_rejected_block(
                            Some(peer_id),
                            file_hash,
                            block_hash,
                            &mut accepted_peers,
                            &mut accepted_peers_index,
                            cmd_sender.clone(),
                            res_sender.clone(),
                        ) {
                            Ok(_) => {}
                            Err(e) => {
                                return Err(DragoonError::SendBlockListFailed {
                                    final_block_distribution,
                                    context: e.to_string(),
                                })
                            }
                        }
                    }
                }
                Err(dragoon_error) => match dragoon_error {
                    SendBlockToError { send_id } => {
                        let SendId {
                            peer_id,
                            file_hash,
                            block_hash,
                        } = send_id;
                        match handle_rejected_block(
                            Some(peer_id),
                            file_hash,
                            block_hash,
                            &mut accepted_peers,
                            &mut accepted_peers_index,
                            cmd_sender.clone(),
                            res_sender.clone(),
                        ) {
                            Ok(_) => {}
                            Err(e) => {
                                return Err(DragoonError::SendBlockListFailed {
                                    final_block_distribution,
                                    context: e.to_string(),
                                })
                            }
                        }
                    }
                    SendBlockToAlreadyStarted { send_id } => error!(
                        "Unexpected multiple send to {:?} for file hash {} block hash {}",
                        send_id.peer_id, send_id.file_hash, send_id.block_hash
                    ),
                    e => error!("Unexpected error for SendBlockTo: {}", e),
                },
            }
            if rejected_blocks.is_empty() {
                res_recv.close();
                break 'recuperation;
            }
        }
        info!("Finished recuperation loop for send block list without further issues");

        Ok(final_block_distribution)
    }
}

pub(crate) fn get_block_dir(file_dir: &PathBuf, file_hash: String) -> PathBuf {
    [get_file_dir(file_dir, file_hash), PathBuf::from("blocks")]
        .iter()
        .collect()
}

pub(crate) fn get_file_dir(file_dir: &PathBuf, file_hash: String) -> PathBuf {
    [file_dir, &PathBuf::from(file_hash)].iter().collect()
}

pub(crate) async fn get_powers<F, G>(powers_path: PathBuf) -> Result<Powers<F, G>>
where
    F: PrimeField,
    G: CurveGroup<ScalarField = F>,
{
    info!("Getting the powers from {:?}", powers_path);
    let serialized = tokio::fs::read(powers_path).await?;
    Ok(Powers::<F, G>::deserialize_with_mode(
        &serialized[..],
        Compress::Yes,
        Validate::Yes,
    )?)
}
