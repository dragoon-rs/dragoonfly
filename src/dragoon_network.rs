use anyhow::{self, format_err, Result};
use futures::prelude::*;
use futures::stream::BoxStream;
use tokio::fs as tfs;
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, oneshot};

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
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::{debug, error, info, warn};

use crate::commands::{DragoonCommand, EncodingMethod, Sender, SenderMPSC};
use crate::error::DragoonError::{
    BadListener, BootstrapError, CouldNotSendBlockResponse, CouldNotSendInfoResponse, DialError,
    NoParentDirectory, ProviderError,
};
use crate::peer_block_info::PeerBlockInfo;

use komodo::{
    self,
    fec::{self, Shard},
    fs,
    linalg::Matrix,
    verify,
    zk::Powers,
    Block,
};

use resolve_path::PathResolveExt;
use rs_merkle::{algorithms::Sha256, Hasher};

use ark_ec::CurveGroup;
use ark_ff::PrimeField;
use ark_poly::DenseUVPolynomial;
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize, Compress, Validate};
use ark_std::ops::Div;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BlockRequest {
    file_hash: String,
    block_hash: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BlockResponse {
    block_hash: String,
    block_data: Vec<u8>,
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
}

pub(crate) struct DragoonNetwork {
    swarm: Swarm<DragoonBehaviour>,
    command_receiver: mpsc::UnboundedReceiver<DragoonCommand>,
    command_sender: mpsc::UnboundedSender<DragoonCommand>,
    listeners: HashMap<u64, ListenerId>,
    file_dir: PathBuf,
    pending_start_providing: HashMap<kad::QueryId, Sender<()>>,
    pending_get_providers: HashMap<kad::QueryId, SenderMPSC<HashSet<PeerId>>>,
    pending_request_block_info: HashMap<OutboundRequestId, Sender<PeerBlockInfo>>,
    pending_request_block: HashMap<OutboundRequestId, Sender<BlockResponse>>,
    //TODO add a pending_request_file using the hash as a key
    //TODO value should have a Sender<String> to return the result path and a SenderMPSC<FileCmdResult> which delivers
    //TODO the result of the operations that GetFile from DragoonBehaviour asks of other modules (like kad and request_block / request_info)
}

impl DragoonNetwork {
    pub fn new(
        swarm: Swarm<DragoonBehaviour>,
        command_receiver: mpsc::UnboundedReceiver<DragoonCommand>,
        command_sender: mpsc::UnboundedSender<DragoonCommand>,
        peer_id: PeerId,
        replace: bool,
    ) -> Self {
        Self {
            swarm,
            command_receiver,
            command_sender,
            listeners: HashMap::new(),
            file_dir: Self::create_block_dir(peer_id, replace).unwrap(),
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
            let _ = std::fs::remove_dir_all(&base_path); // ignore the error if the directory does not exist
        }
        std::fs::create_dir_all(&base_path)?;
        info!(
            "Created the directory for node {} at {:?}",
            peer_id, base_path
        );
        Ok(base_path)
    }

    pub async fn run<F, G, P>(mut self)
    where
        F: PrimeField,
        G: CurveGroup<ScalarField = F>,
        P: DenseUVPolynomial<F>,
        for<'a, 'b> &'a P: Div<&'b P, Output = P>,
    {
        info!("Starting Dragoon Network");
        loop {
            tokio::select! {
                e = self.swarm.next() => self.handle_event::<F,G>(e.expect("Swarm stream to be infinite.")).await,
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
                    if match sender {
                        Sender::SenderMPSC(sender) => {
                            sender.send(Ok(())).map_err(|_| format_err!(""))
                        }
                        Sender::SenderOneS(sender) => {
                            sender.send(Ok(())).map_err(|_| format_err!(""))
                        }
                    }
                    .is_err()
                    {
                        error!("Could not send the result of the StartProviding query result");
                    }
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

    async fn handle_event<F, G>(&mut self, event: SwarmEvent<DragoonBehaviourEvent>)
    where
        F: PrimeField,
        G: CurveGroup<ScalarField = F>,
    {
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
                    if let Err(e) = self.message_request::<F, G>(request, channel).await {
                        error!("{}", e)
                    }
                }
                Message::Response {
                    request_id,
                    response,
                } => {
                    if let Some(sender) = self.pending_request_block.remove(&request_id) {
                        if match sender {
                            Sender::SenderMPSC(sender) => {
                                sender.send(Ok(response)).map_err(|_| format_err!(""))
                            }
                            Sender::SenderOneS(sender) => {
                                sender.send(Ok(response)).map_err(|_| format_err!(""))
                            }
                        }
                        .is_err()
                        {
                            error!("Couldn't send the result of the message response operation of id {}", request_id);
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
            })) => {
                match message {
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
                            if match sender {
                                Sender::SenderMPSC(sender) => {
                                    sender.send(Ok(response.0)).map_err(|_| format_err!(""))
                                }
                                Sender::SenderOneS(sender) => {
                                    sender.send(Ok(response.0)).map_err(|_| format_err!(""))
                                }
                            }
                            .is_err()
                            {
                                error!("Couldn't send the result of the info response operation of id {}", request_id);
                            }
                        } else {
                            error!(
                                "Could no find the sender associated with {} for the info response",
                                request_id
                            );
                        }
                    }
                }
            }
            e => warn!("[unknown event] {:?}", e),
        }
    }

    fn get_block_dir(&mut self, file_hash: String) -> PathBuf {
        [self.get_file_dir(file_hash), PathBuf::from("blocks")]
            .iter()
            .collect()
    }

    fn get_file_dir(&mut self, file_hash: String) -> PathBuf {
        [self.file_dir.clone(), PathBuf::from(file_hash)]
            .iter()
            .collect()
    }

    fn read_block_from_disk<F, G>(block_hash: String, block_dir: PathBuf) -> Result<Vec<u8>>
    where
        F: PrimeField,
        G: CurveGroup<ScalarField = F>,
    {
        let block = match fs::read_blocks::<F, G>(
            &[block_hash],
            &block_dir,
            Compress::Yes,
            Validate::Yes,
        ) {
            Ok(vec) => vec[0].clone().1,
            Err(e) => return Err(e),
        };
        let mut buf = vec![0; block.serialized_size(Compress::Yes)];
        block.serialize_with_mode(&mut buf[..], Compress::Yes)?;
        Ok(buf)
    }

    async fn message_request<F, G>(
        &mut self,
        request: BlockRequest,
        channel: ResponseChannel<BlockResponse>,
    ) -> Result<()>
    where
        F: PrimeField,
        G: CurveGroup<ScalarField = F>,
    {
        let BlockRequest {
            file_hash,
            block_hash,
        } = request;
        let block_dir = self.get_block_dir(file_hash.clone());
        info!(
            "Searching blocks for the file {0} inside {1:?}",
            file_hash.clone(),
            block_dir
        );
        let ser_block = Self::read_block_from_disk::<F, G>(block_hash.clone(), block_dir)?;
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
        let block_hashes = self.get_block_list(file_hash.clone()).await?;
        debug!(
            "A peer requested the blocks for file {}, node has : {:?}",
            file_hash, block_hashes
        );
        let channel_info = format!("{:?}", &channel);
        let peer_block_info = PeerBlockInfo {
            peer_id_base_58: self.swarm.local_peer_id().to_base58(),
            file_hash: file_hash.clone(),
            block_hashes,
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
                if match sender {
                    Sender::SenderMPSC(sender) => sender.send(res).map_err(|_| format_err!("")),
                    Sender::SenderOneS(sender) => sender.send(res).map_err(|_| format_err!("")),
                }
                .is_err()
                {
                    error!("Could not send the result of the listen operation")
                }
            }
            DragoonCommand::GetListeners { sender } => {
                let listeners = self.swarm.listeners().cloned().collect::<Vec<Multiaddr>>();

                debug!("sending listeners {:?}", listeners);
                if match sender {
                    Sender::SenderMPSC(sender) => {
                        sender.send(Ok(listeners)).map_err(|_| format_err!(""))
                    }
                    Sender::SenderOneS(sender) => {
                        sender.send(Ok(listeners)).map_err(|_| format_err!(""))
                    }
                }
                .is_err()
                {
                    error!("Could not send list of listeners");
                }
            }
            DragoonCommand::GetPeerId { sender } => {
                let peer_id = *self.swarm.local_peer_id();

                debug!("sending peer_id {}", peer_id);
                if match sender {
                    Sender::SenderMPSC(sender) => {
                        sender.send(Ok(peer_id)).map_err(|_| format_err!(""))
                    }
                    Sender::SenderOneS(sender) => {
                        sender.send(Ok(peer_id)).map_err(|_| format_err!(""))
                    }
                }
                .is_err()
                {
                    error!("Could not send peer ID");
                }
            }
            DragoonCommand::GetNetworkInfo { sender } => {
                let network_info = self.swarm.network_info();

                debug!("sending network info {:?}", network_info);
                if match sender {
                    Sender::SenderMPSC(sender) => {
                        sender.send(Ok(network_info)).map_err(|_| format_err!(""))
                    }
                    Sender::SenderOneS(sender) => {
                        sender.send(Ok(network_info)).map_err(|_| format_err!(""))
                    }
                }
                .is_err()
                {
                    error!("Could not send network info");
                }
            }
            DragoonCommand::RemoveListener {
                listener_id,
                sender,
            } => {
                let res = self.remove_listener(listener_id).await;
                if match sender {
                    Sender::SenderMPSC(sender) => sender.send(res).map_err(|_| format_err!("")),
                    Sender::SenderOneS(sender) => sender.send(res).map_err(|_| format_err!("")),
                }
                .is_err()
                {
                    error!("Could not send the result of the remove_listener operation")
                }
            }
            DragoonCommand::GetConnectedPeers { sender } => {
                info!("Getting list of connected peers");
                let connected_peers = self
                    .swarm
                    .connected_peers()
                    .cloned()
                    .collect::<Vec<PeerId>>();

                debug!("sending connected_peers {:?}", connected_peers);
                if match sender {
                    Sender::SenderMPSC(sender) => sender
                        .send(Ok(connected_peers))
                        .map_err(|_| format_err!("")),
                    Sender::SenderOneS(sender) => sender
                        .send(Ok(connected_peers))
                        .map_err(|_| format_err!("")),
                }
                .is_err()
                {
                    error!("Could not send list of connected peers");
                }
            }
            DragoonCommand::GetFile {
                file_hash,
                output_filename,
                powers_path,
                sender,
            } => {
                info!("Starting to get the file {}", file_hash);
                let cmd_sender = self.command_sender.clone();
                tokio::spawn(async move {
                    let res = Self::get_file::<F, G, P>(
                        cmd_sender,
                        file_hash.clone(),
                        output_filename,
                        powers_path,
                    )
                    .await;
                    if match sender {
                        Sender::SenderMPSC(sender) => sender.send(res).map_err(|_| format_err!("")),
                        Sender::SenderOneS(sender) => sender.send(res).map_err(|_| format_err!("")),
                    }
                    .is_err()
                    {
                        error!(
                            "Could not send the result of the get_file {} operation",
                            file_hash
                        )
                    }
                });
            }
            DragoonCommand::Dial { multiaddr, sender } => {
                let res = self.dial(multiaddr).await;
                if match sender {
                    Sender::SenderMPSC(sender) => sender.send(res).map_err(|_| format_err!("")),
                    Sender::SenderOneS(sender) => sender.send(res).map_err(|_| format_err!("")),
                }
                .is_err()
                {
                    error!("Could not send the result of the dial operation")
                }
            }
            DragoonCommand::AddPeer { multiaddr, sender } => {
                let res = self.add_peer(multiaddr).await;
                if match sender {
                    Sender::SenderMPSC(sender) => sender.send(res).map_err(|_| format_err!("")),
                    Sender::SenderOneS(sender) => sender.send(res).map_err(|_| format_err!("")),
                }
                .is_err()
                {
                    error!("Could not send the result of the add_peer operation")
                }
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
                    if match sender {
                        Sender::SenderMPSC(sender) => sender
                            .send(Err(format_err!(err)))
                            .map_err(|_| format_err!("")),
                        Sender::SenderOneS(sender) => sender
                            .send(Err(format_err!(err)))
                            .map_err(|_| format_err!("")),
                    }
                    .is_err()
                    {
                        error!("Could not send result");
                    }
                }
            }
            DragoonCommand::GetProviders { key, sender } => {
                let mut provider_stream = self.get_providers(key);
                tokio::spawn(async move {
                    // instead of returning the stream directly through the Sender, put it in a Vec format so it's easier to read for the person getting it
                    let mut all_providers = Vec::<PeerId>::default();
                    while let Some(provider) = provider_stream.next().await {
                        all_providers.push(provider);
                    }
                    if match sender {
                        Sender::SenderMPSC(sender) => {
                            sender.send(Ok(all_providers)).map_err(|_| format_err!(""))
                        }
                        Sender::SenderOneS(sender) => {
                            sender.send(Ok(all_providers)).map_err(|_| format_err!(""))
                        }
                    }
                    .is_err()
                    {
                        error!("Could not send the result of the GetProviders command");
                    }
                });
            }
            DragoonCommand::Bootstrap { sender } => {
                let res = self.bootstrap().await;
                if match sender {
                    Sender::SenderMPSC(sender) => sender.send(res).map_err(|_| format_err!("")),
                    Sender::SenderOneS(sender) => sender.send(res).map_err(|_| format_err!("")),
                }
                .is_err()
                {
                    error!("Could not send the result of the bootstrap operation")
                }
            }
            DragoonCommand::GetBlockFrom {
                peer_id,
                file_hash,
                block_hash,
                sender,
            } => {
                let request_id = self.swarm.behaviour_mut().request_block.send_request(
                    &peer_id,
                    BlockRequest {
                        file_hash,
                        block_hash,
                    },
                );
                self.pending_request_block.insert(request_id, sender);
            }
            DragoonCommand::GetBlocksInfoFrom {
                peer_id,
                file_hash,
                sender,
            } => self.get_blocks_info_from(peer_id, file_hash, sender),
            DragoonCommand::GetBlockList { file_hash, sender } => {
                let res = self.get_block_list(file_hash).await;

                if match sender {
                    Sender::SenderMPSC(sender) => sender.send(res).map_err(|_| format_err!("")),
                    Sender::SenderOneS(sender) => sender.send(res).map_err(|_| format_err!("")),
                }
                .is_err()
                {
                    error!("Could not send the result of the get_block_list operation")
                }
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
                if match sender {
                    Sender::SenderMPSC(sender) => sender.send(res).map_err(|_| format_err!("")),
                    Sender::SenderOneS(sender) => sender.send(res).map_err(|_| format_err!("")),
                }
                .is_err()
                {
                    error!("Could not send the result of the decode_blocks operation")
                }
            }
            DragoonCommand::EncodeFile {
                file_path,
                replace_blocks,
                encoding_method,
                encode_mat_k,
                encode_mat_n,
                powers_path,
                sender,
            } => {
                let res = self
                    .encode_file::<F, G, P>(
                        file_path,
                        replace_blocks,
                        encoding_method,
                        encode_mat_k,
                        encode_mat_n,
                        powers_path,
                    )
                    .await;
                if match sender {
                    Sender::SenderMPSC(sender) => sender.send(res).map_err(|_| format_err!("")),
                    Sender::SenderOneS(sender) => sender.send(res).map_err(|_| format_err!("")),
                }
                .is_err()
                {
                    error!("Could not send the result of the encode_file operation")
                }
            }
            DragoonCommand::GetBlockDir { file_hash, sender } => {
                let res = Ok(self.get_block_dir(file_hash));
                if match sender {
                    Sender::SenderMPSC(sender) => sender.send(res).map_err(|_| format_err!("")),
                    Sender::SenderOneS(sender) => sender.send(res).map_err(|_| format_err!("")),
                }
                .is_err()
                {
                    error!("Could not send the result of the get_block_dir operation")
                }
            }
            DragoonCommand::GetFileDir { file_hash, sender } => {
                let res = Ok(self.get_file_dir(file_hash));
                if match sender {
                    Sender::SenderMPSC(sender) => sender.send(res).map_err(|_| format_err!("")),
                    Sender::SenderOneS(sender) => sender.send(res).map_err(|_| format_err!("")),
                }
                .is_err()
                {
                    error!("Could not send the result of the get_file_dir operation")
                }
            }
            DragoonCommand::NodeInfo { sender } => {
                let res = Ok(*(self.swarm.local_peer_id()));
                if match sender {
                    Sender::SenderMPSC(sender) => sender.send(res).map_err(|_| format_err!("")),
                    Sender::SenderOneS(sender) => sender.send(res).map_err(|_| format_err!("")),
                }
                .is_err()
                {
                    error!("Could not send the result of the node_info operation")
                }
            }
        }
    }

    // * keeping for later for the logic of block read and serialize
    // async fn put_record(&mut self, block_hash: String, block_dir: String) -> Result<QueryId> {
    //     let block = fs::read_blocks::<F, G>(
    //         &[block_hash.clone()],
    //         Path::new(&block_dir),
    //         Compress::Yes,
    //         Validate::Yes,
    //     )?[0]
    //         .clone()
    //         .1;
    //     let mut buf = vec![0; block.serialized_size(Compress::Yes)];
    //     block.serialize_with_mode(&mut buf[..], Compress::Yes)?;
    //     let record = kad::Record {
    //         key: block_hash.into_bytes().into(),
    //         value: buf,
    //         publisher: None,
    //         expires: None,
    //     };
    //     info!("Putting record {:?} in a kad record", record);
    //     let id = self
    //         .swarm
    //         .behaviour_mut()
    //         .kademlia
    //         .put_record(record, kad::Quorum::One)?;

    //     Ok(id)
    // }

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
        powers_path: String,
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

        let (info_sender, mut info_receiver) = mpsc::unbounded_channel();

        debug!(
            "Requesting the information about list of blocks for file {} from peers {:?}",
            file_hash, provider_list
        );
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

        //TODO change this to keep in memory other providers of the same block in case the first one fails (a hash map maybe ?)
        let mut already_request_block = vec![];
        let mut block_hashes_on_disk = vec![];
        let powers = Self::get_powers(powers_path).await?;
        let mut number_of_blocks_written: u32 = 0;

        let (block_sender, mut block_receiver) = mpsc::unbounded_channel();

        'download_first_k_blocks: loop {
            tokio::select! {
                    Some(response) = info_receiver.recv() => {

                            //TODO handle errors to keep going even if some peer fail
                            let response = response.map_err(|e| -> anyhow::Error {
                                format_err!("Could not retrieve peer block block info: {}", e)
                            })?;
                            let PeerBlockInfo { peer_id_base_58, file_hash, block_hashes } = response;
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
                                if cmd_sender.send(DragoonCommand::GetBlockFrom {peer_id, file_hash: file_hash.clone(), block_hash: block_hash.clone(), sender: Sender::SenderMPSC(block_sender.clone())}).is_err() {
                                    error!(err_msg);
                                }
                                else {
                                    already_request_block.push(block_hash);
                                }

                            }
                    },
                    Some(block_response) = block_receiver.recv() => {
                        //TODO change this unwrap
                        let block_response = block_response.unwrap();
                        let block: Block<F,G> = Block::deserialize_with_mode(&block_response.block_data[..], Compress::Yes, Validate::Yes)?;
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

    async fn get_block_list(&mut self, file_hash: String) -> Result<Vec<String>> {
        let block_path = self.get_block_dir(file_hash.clone());
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
        &mut self,
        file_path: String,
        replace_blocks: bool,
        encoding_method: EncodingMethod,
        encode_mat_k: usize,
        encode_mat_n: usize,
        powers_path: String,
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
        let powers = Self::get_powers(powers_path).await?;
        let proof = komodo::prove::<F, G, P>(&bytes, &powers, encode_mat_k)?;
        let blocks = komodo::build::<F, G, P>(&shards, &proof);
        let block_dir = self.get_block_dir(file_hash.clone());
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

    async fn get_powers<F, G>(powers_path: String) -> Result<Powers<F, G>>
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
}
