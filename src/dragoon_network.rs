use anyhow::{self, Result};
use futures::channel::mpsc;
use futures::prelude::*;
use tokio::io::AsyncWriteExt;

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
use std::collections::HashMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;
use tracing::{debug, error, info, warn};

use crate::commands::{DragoonCommand, EncodingMethod, Sender};
use crate::dragoon::{self, Behaviour};
use crate::error::DragoonError::{
    BadListener, BootstrapError, CouldNotSendBlockResponse, DialError, NoParentDirectory,
    PeerNotFound, ProviderError,
};

use komodo::{
    self,
    fec::{self, Shard},
    fs,
    linalg::Matrix,
    zk::Powers,
};

use resolve_path::PathResolveExt;
use rs_merkle::{algorithms::Sha256, Hasher};

use ark_ec::CurveGroup;
use ark_ff::PrimeField;
use ark_poly::DenseUVPolynomial;
use ark_serialize::{CanonicalDeserialize, Compress, Validate};
use ark_std::ops::Div;

use crate::dragoon::DragoonEvent;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BlockRequest {
    file_hash: String,
    block_hash: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BlockResponse(Vec<u8>);

pub(crate) async fn create_swarm<F, G>(
    id_keys: Keypair,
) -> Result<Swarm<DragoonBehaviour<F, G>>, Box<dyn Error>>
where
    F: PrimeField,
    G: CurveGroup<ScalarField = F>,
{
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
            request_response: request_response::cbor::Behaviour::new(
                [(
                    StreamProtocol::new("/file-exchange/1"),
                    ProtocolSupport::Full,
                )],
                request_response::Config::default(),
            ),
            dragoon: Behaviour::new(),
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
pub(crate) struct DragoonBehaviour<F, G>
where
    F: PrimeField,
    G: CurveGroup<ScalarField = F>,
{
    request_response: request_response::cbor::Behaviour<BlockRequest, BlockResponse>,
    identify: identify::Behaviour,
    kademlia: kad::Behaviour<kad::store::MemoryStore>,
    dragoon: Behaviour<F, G>,
}

pub(crate) struct DragoonNetwork<F, G>
where
    F: PrimeField,
    G: CurveGroup<ScalarField = F>,
{
    swarm: Swarm<DragoonBehaviour<F, G>>,
    command_receiver: mpsc::Receiver<DragoonCommand>,
    listeners: HashMap<u64, ListenerId>,
    file_dir: PathBuf,
    pending_start_providing: HashMap<kad::QueryId, Sender<()>>,
    pending_get_providers: HashMap<kad::QueryId, Sender<Vec<PeerId>>>,
    pending_request_file: HashMap<OutboundRequestId, Sender<Vec<u8>>>,
}

impl<F, G> DragoonNetwork<F, G>
where
    F: PrimeField,
    G: CurveGroup<ScalarField = F>,
{
    pub fn new(
        swarm: Swarm<DragoonBehaviour<F, G>>,
        command_receiver: mpsc::Receiver<DragoonCommand>,
        peer_id: PeerId,
        replace: bool,
    ) -> Self {
        Self {
            swarm,
            command_receiver,
            listeners: HashMap::new(),
            file_dir: Self::create_block_dir(peer_id, replace).unwrap(),
            pending_start_providing: Default::default(),
            pending_get_providers: Default::default(),
            pending_request_file: Default::default(),
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

    pub async fn run<P>(mut self)
    where
        P: DenseUVPolynomial<F>,
        for<'a, 'b> &'a P: Div<&'b P, Output = P>,
    {
        info!("Starting Dragoon Network");
        loop {
            futures::select! {
                e = self.swarm.next() => self.handle_event(e.expect("Swarm stream to be infinite.")).await,
                cmd = self.command_receiver.next() =>  match cmd {
                    Some(c) => self.handle_command::<P>(c).await,
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
                    if sender.send(Ok(())).is_err() {
                        error!("Could not send result");
                    }
                } else {
                    warn!("Could not find id = {} in the start providers", id);
                }
            }
            kad::QueryResult::GetProviders(get_providers_result) => {
                if let Ok(res) = get_providers_result {
                    match res {
                        kad::GetProvidersOk::FoundProviders { providers, .. } => {
                            info!("Found providers {:?}", providers);
                        }
                        kad::GetProvidersOk::FinishedWithNoAdditionalRecord { closest_peers } => {
                            info!("Finished get providers {closest_peers:?}");
                            if let Some(sender) = self.pending_get_providers.remove(&id) {
                                debug!("Sending all found providers: {:?}", closest_peers);
                                if sender.send(Ok(closest_peers)).is_err() {
                                    error!("Cannot send result");
                                }
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
                            if sender.send(Ok(Vec::default())).is_err() {
                                error!("Cannot send result");
                            }
                        } else {
                            error!("could not find {} in the query ids", id);
                            let err =
                                ProviderError(format!("could not find {} in the query ids", id));
                            debug!("Sending error");
                            if sender.send(Err(Box::new(err))).is_err() {
                                error!("Cannot send result");
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

    async fn handle_event(&mut self, event: SwarmEvent<DragoonBehaviourEvent<F, G>>) {
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
            )) => self.handle_query_result(result, id).await,
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
            SwarmEvent::Behaviour(DragoonBehaviourEvent::Dragoon(event)) => match event {
                DragoonEvent::Sent { peer } => {
                    info!("Sent a shard to peer {peer}");
                }
                DragoonEvent::Received { block } => {
                    info!("Received a shard : {block:?}");
                    self.swarm
                        .behaviour_mut()
                        .kademlia
                        .start_providing(block.shard.hash.into())
                        .unwrap();
                }
            },
            SwarmEvent::Behaviour(DragoonBehaviourEvent::RequestResponse(Event::Message {
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
                    if let Some(sender) = self.pending_request_file.remove(&request_id) {
                        if sender.send(Ok(response.0)).is_err() {
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
            e => warn!("[unknown event] {:?}", e),
        }
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
        let block_dir: PathBuf = [
            self.file_dir.clone(),
            PathBuf::from(file_hash.clone()),
            PathBuf::from("blocks"),
        ]
        .iter()
        .collect();
        info!(
            "Searching blocks for the file {0} inside {1:?}",
            file_hash.clone(),
            block_dir
        );
        let ser_block = dragoon::read_block_from_disk::<F, G>(block_hash.clone(), block_dir)?;
        debug!(
            "Read block {0} for file {1}, got: {2:?}",
            block_hash, file_hash, ser_block
        );
        let channel_info = format!("{:?}", &channel);
        self.swarm
            .behaviour_mut()
            .request_response
            .send_response(channel, BlockResponse(ser_block))
            .map_err(|_| CouldNotSendBlockResponse(block_hash, file_hash, channel_info).into())
    }

    async fn handle_command<P>(&mut self, cmd: DragoonCommand)
    where
        P: DenseUVPolynomial<F>,
        for<'a, 'b> &'a P: Div<&'b P, Output = P>,
    {
        debug!("[cmd] {:?}", cmd);
        match cmd {
            DragoonCommand::Listen { multiaddr, sender } => {
                if sender
                    .send(self.listen(multiaddr).await.map_err(|err| err.into()))
                    .is_err()
                {
                    error!("Could not send the result of the listen operation")
                }
            }
            DragoonCommand::GetListeners { sender } => {
                let listeners = self.swarm.listeners().cloned().collect::<Vec<Multiaddr>>();

                debug!("sending listeners {:?}", listeners);
                if sender.send(Ok(listeners)).is_err() {
                    error!("Could not send list of listeners");
                }
            }
            DragoonCommand::GetPeerId { sender } => {
                let peer_id = *self.swarm.local_peer_id();

                debug!("sending peer_id {}", peer_id);
                if sender.send(Ok(peer_id)).is_err() {
                    error!("Could not send peer ID");
                }
            }
            DragoonCommand::GetNetworkInfo { sender } => {
                let network_info = self.swarm.network_info();

                debug!("sending network info {:?}", network_info);
                if sender.send(Ok(network_info)).is_err() {
                    error!("Could not send network info");
                }
            }
            DragoonCommand::RemoveListener {
                listener_id,
                sender,
            } => {
                if sender
                    .send(
                        self.remove_listener(listener_id)
                            .await
                            .map_err(|err| err.into()),
                    )
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
                if sender.send(Ok(connected_peers)).is_err() {
                    error!("Could not send list of connected peers");
                }
            }
            DragoonCommand::Dial { multiaddr, sender } => {
                if sender
                    .send(self.dial(multiaddr).await.map_err(|err| err.into()))
                    .is_err()
                {
                    error!("Could not send the result of the dial operation")
                }
            }
            DragoonCommand::AddPeer { multiaddr, sender } => {
                if sender
                    .send(self.add_peer(multiaddr).await.map_err(|err| err.into()))
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
                    if sender.send(Err(Box::new(err))).is_err() {
                        error!("Could not send result");
                    }
                }
            }
            DragoonCommand::GetProviders { key, sender } => {
                let query_id = self
                    .swarm
                    .behaviour_mut()
                    .kademlia
                    .get_providers(key.into_bytes().into());
                self.pending_get_providers.insert(query_id, sender);
            }
            DragoonCommand::Bootstrap { sender } => {
                if sender
                    .send(self.bootstrap().await.map_err(|err| err.into()))
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
                let request_id = self.swarm.behaviour_mut().request_response.send_request(
                    &peer_id,
                    BlockRequest {
                        file_hash,
                        block_hash,
                    },
                );
                self.pending_request_file.insert(request_id, sender);
            }
            DragoonCommand::DragoonPeers { sender } => {
                if sender
                    .send(Ok(self.swarm.behaviour_mut().dragoon.get_connected_peer()))
                    .is_err()
                {
                    error!("could not send result");
                }
            }
            DragoonCommand::DragoonSend {
                block_hash,
                block_path,
                peerid,
                sender,
            } => {
                if sender
                    .send(
                        self.dragoon_send(block_hash, block_path, peerid)
                            .await
                            .map_err(|err| err.into()),
                    )
                    .is_err()
                {
                    error!("Could not send the result of the dragoon_send operation")
                }
            }
            DragoonCommand::DecodeBlocks {
                block_dir,
                block_hashes,
                output_filename,
                sender,
            } => {
                if sender
                    .send(
                        Self::decode_blocks(block_dir, block_hashes, output_filename)
                            .await
                            .map_err(|err| err.into()),
                    )
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
                if sender
                    .send(
                        self.encode_file(
                            file_path,
                            replace_blocks,
                            encoding_method,
                            encode_mat_k,
                            encode_mat_n,
                            powers_path,
                        )
                        .await
                        .map_err(|err| err.into()),
                    )
                    .is_err()
                {
                    error!("Could not send the result of the encode_file operation")
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

    async fn bootstrap(&mut self) -> Result<()> {
        match self.swarm.behaviour_mut().kademlia.bootstrap() {
            Ok(_) => Ok(()),
            Err(nkp) => {
                error!("Bootstrap: no known peers");
                Err(BootstrapError(nkp.to_string()).into())
            }
        }
    }

    async fn dragoon_send(
        &mut self,
        block_hash: String,
        block_path: String,
        peerid: String,
    ) -> Result<()> {
        // TODO go search the block on the disk
        let peer = PeerId::from_str(&peerid)?;

        if self
            .swarm
            .behaviour_mut()
            .dragoon
            .send_data_to_peer(block_hash, block_path, peer)
        {
            Ok(())
        } else {
            error!("Dragoon send: peer {} not found", peer);
            Err(PeerNotFound.into())
        }
    }

    async fn decode_blocks(
        block_dir: String,
        block_hashes: Vec<String>,
        output_filename: String,
    ) -> Result<()> {
        let blocks = fs::read_blocks::<F, G>(
            &block_hashes,
            Path::new(&block_dir),
            Compress::Yes,
            Validate::Yes,
        )?;
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
            let err = NoParentDirectory(block_dir);
            return Err(err.into());
        }
        Ok(())
    }

    async fn encode_file<P>(
        &mut self,
        file_path: String,
        replace_blocks: bool,
        encoding_method: EncodingMethod,
        encode_mat_k: usize,
        encode_mat_n: usize,
        powers_path: String,
    ) -> Result<(String, String)>
    where
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
        info!("Getting the powers from {:?}", powers_path);
        let serialized = tokio::fs::read(powers_path).await?;
        let powers =
            Powers::<F, G>::deserialize_with_mode(&serialized[..], Compress::Yes, Validate::Yes)?;
        let proof = komodo::prove::<F, G, P>(&bytes, &powers, encode_mat_k)?;
        let blocks = komodo::build::<F, G, P>(&shards, &proof);
        let file_dir: PathBuf = [self.file_dir.clone(), PathBuf::from(file_hash.clone())]
            .iter()
            .collect();
        let block_dir: PathBuf = [&file_dir, Path::new("blocks")].iter().collect();
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
}
