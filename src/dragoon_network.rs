use anyhow::Result;
use futures::channel::{mpsc, oneshot};
use futures::prelude::*;
use tokio::io::AsyncWriteExt;

use libp2p::core::transport::ListenerId;
use libp2p::identity::Keypair;
use libp2p::kad::{QueryId, QueryResult};
use libp2p::request_response::{Event, Message, OutboundRequestId};
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
use std::error::Error;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;
use tracing::{debug, error, info, warn};

use crate::commands::{DragoonCommand, EncodingMethod};
use crate::dragoon::Behaviour;
use crate::error::DragoonError::{
    BadListener, BootstrapError, DialError, NoParentDirectory, PeerNotFound, ProviderError,
};

use komodo::{
    self,
    fec::{self, Shard},
    fs,
    linalg::Matrix,
    zk::Powers,
};

use rand;

use ark_ec::CurveGroup;
use ark_ff::PrimeField;
use ark_poly::DenseUVPolynomial;
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize, Compress, Validate};
use ark_std::ops::Div;

use crate::dragoon::DragoonEvent;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRequest(String);
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FileResponse(Option<Vec<u8>>);

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
    request_response: request_response::cbor::Behaviour<FileRequest, FileResponse>,
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
    pending_start_providing:
        HashMap<kad::QueryId, oneshot::Sender<Result<(), Box<dyn Error + Send>>>>,
    pending_get_providers:
        HashMap<kad::QueryId, oneshot::Sender<Result<HashSet<PeerId>, Box<dyn Error + Send>>>>,
    pending_request_file:
        HashMap<OutboundRequestId, oneshot::Sender<Result<Vec<u8>, Box<dyn Error + Send>>>>,
    pending_put_record: HashMap<kad::QueryId, oneshot::Sender<Result<(), Box<dyn Error + Send>>>>,
    pending_get_record:
        HashMap<kad::QueryId, oneshot::Sender<Result<Vec<u8>, Box<dyn Error + Send>>>>,
}

impl<F, G> DragoonNetwork<F, G>
where
    F: PrimeField,
    G: CurveGroup<ScalarField = F>,
{
    pub fn new(
        swarm: Swarm<DragoonBehaviour<F, G>>,
        command_receiver: mpsc::Receiver<DragoonCommand>,
    ) -> Self {
        Self {
            swarm,
            command_receiver,
            listeners: HashMap::new(),
            pending_start_providing: Default::default(),
            pending_get_providers: Default::default(),
            pending_request_file: Default::default(),
            pending_put_record: Default::default(),
            pending_get_record: Default::default(),
        }
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
                            if let Some(sender) = self.pending_get_providers.remove(&id) {
                                debug!("Sending providers: {:?}", providers);
                                if sender.send(Ok(providers)).is_err() {
                                    error!("Cannot send result");
                                }
                            } else {
                                error!("could not find {} in the providers", id);
                            }
                        }
                        kad::GetProvidersOk::FinishedWithNoAdditionalRecord { closest_peers } => {
                            info!("Finished get providers {closest_peers:?}");
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
            kad::QueryResult::GetRecord(Ok(get_record_ok)) => {
                let value = match get_record_ok {
                    kad::GetRecordOk::FoundRecord(record) => {
                        info!("value found");
                        record.record.value
                    }
                    kad::GetRecordOk::FinishedWithNoAdditionalRecord { .. } => {
                        vec![]
                    }
                };

                if let Some(sender) = self.pending_get_record.remove(&id) {
                    debug!("Writing value to disk: {:?}", value);
                    if sender.send(Ok(value)).is_err() {
                        //TODO change this to write the value to disk
                        error!("Cannot send result");
                    }
                } else {
                    error!("could not find {} in the get records", id);
                }
            }
            kad::QueryResult::GetRecord(Err(err)) => {
                error!("Failed to get record: {err:?}");
                if let Some(sender) = self.pending_get_record.remove(&id) {
                    debug!("Sending empty value");
                    if sender.send(Ok(vec![])).is_err() {
                        error!("Cannot send result");
                    }
                } else {
                    error!("could not find {} in the get records", id);
                }
            }
            kad::QueryResult::PutRecord(Ok(kad::PutRecordOk { .. })) => {
                if let Some(sender) = self.pending_put_record.remove(&id) {
                    debug!("Sending empty response");
                    if sender.send(Ok(())).is_err() {
                        error!("Cannot send result");
                    }
                } else {
                    error!("could not find {} in the put records", id);
                }
            }
            kad::QueryResult::PutRecord(Err(err)) => {
                error!("Failed to put record: {err:?}");
            }
            e => warn!("[unknown event] {:?}", e),
        }
    }

    // async fn query_get_record_dump(value: Vec<u8>, dump_dir: String, filename: String) -> Result<()> {
    //     let dump_path = Path::new(dump_dir).join(&Path::new(filename));
    //     info!("Dumping the file to: {:?}", dump_path);
    //     let mut file = File::create(&dump_path)?;
    //     file.write_all(&value)?;
    //     Ok(())
    // }

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
                if let Some(addr) = info.listen_addrs.get(0) {
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
                    if request.0 == "toto" {
                        self.swarm
                            .behaviour_mut()
                            .request_response
                            .send_response(channel, FileResponse(Some(vec![1, 2, 3, 4])));
                        return;
                    }
                    if request.0 == "tata" {
                        self.swarm
                            .behaviour_mut()
                            .request_response
                            .send_response(channel, FileResponse(Some(vec![4, 3, 2, 1])));
                        return;
                    }
                    self.swarm
                        .behaviour_mut()
                        .request_response
                        .send_response(channel, FileResponse(None));
                }
                Message::Response {
                    request_id,
                    response,
                } => {
                    if self.pending_request_file.contains_key(&request_id) {
                        info!("response: {:?}", response.0);
                        if let Some(sender) = self.pending_request_file.remove(&request_id) {
                            if let Some(res) = response.0 {
                                if sender.send(Ok(res)).is_err() {
                                    error!("Could not send result");
                                }
                            } else {
                                let err = ProviderError(format!("data not found"));

                                debug!("sending error {}", err);
                                if sender.send(Err(Box::new(err))).is_err() {
                                    error!("Could not send result");
                                }
                            }
                        } else {
                            error!("could not find {} in the request files", request_id);
                        }
                    }
                }
            },
            e => warn!("[unknown event] {:?}", e),
        }
    }

    async fn handle_command<P>(&mut self, cmd: DragoonCommand)
    where
        P: DenseUVPolynomial<F>,
        for<'a, 'b> &'a P: Div<&'b P, Output = P>,
    {
        debug!("[cmd] {:?}", cmd);
        match cmd {
            DragoonCommand::Listen { multiaddr, sender } => {
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

                            debug!("sending id {}", id);
                            if sender.send(Ok(id)).is_err() {
                                error!("Could not send listener ID");
                            }
                        }
                        Err(te) => {
                            let err_msg = match te {
                                TransportError::Other(e) => e.to_string(),
                                TransportError::MultiaddrNotSupported(addr) => {
                                    format!("multiaddr {} not supported", addr)
                                }
                            };

                            error!("{}", err_msg);

                            debug!("sending error {}", err_msg);
                            if sender.send(Err(Box::new(BadListener(err_msg)))).is_err() {
                                error!("Could not send result");
                            }
                        }
                    }
                } else {
                    error!("Could not parse addr {}", multiaddr);
                    let err = BadListener(format!("Could not parse {}", multiaddr));

                    debug!("sending error {}", err);
                    if sender.send(Err(Box::new(err))).is_err() {
                        error!("Could not send result");
                    }
                }
            }
            DragoonCommand::GetListeners { sender } => {
                let listeners = self
                    .swarm
                    .listeners()
                    .into_iter()
                    .cloned()
                    .collect::<Vec<Multiaddr>>();

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
                if let Some(listener) = self.listeners.get(&listener_id) {
                    let res = self.swarm.remove_listener(*listener);

                    debug!("sending result {}", res);
                    if sender.send(Ok(res)).is_err() {
                        error!("Could not send remove listener");
                    }
                } else {
                    error!("Listener {} not found", listener_id);
                    let err = BadListener(format!("Listener {} not found", listener_id));

                    debug!("sending error {}", err);
                    if sender.send(Err(Box::new(err))).is_err() {
                        error!("Could not send result");
                    }
                }
            }
            DragoonCommand::GetConnectedPeers { sender } => {
                info!("Getting list of connected peers");
                let connected_peers = self
                    .swarm
                    .connected_peers()
                    .into_iter()
                    .cloned()
                    .collect::<Vec<PeerId>>();

                debug!("sending connected_peers {:?}", connected_peers);
                if sender.send(Ok(connected_peers)).is_err() {
                    error!("Could not send list of connected peers");
                }
            }
            DragoonCommand::Dial { multiaddr, sender } => {
                if let Ok(addr) = multiaddr.parse::<Multiaddr>() {
                    match self.swarm.dial(addr) {
                        Ok(()) => {
                            debug!("sending empty response");
                            if sender.send(Ok(())).is_err() {
                                error!("Could not send result");
                            }
                        }
                        Err(de) => {
                            error!("error: {}", de);
                            let err = DialError(de.to_string());

                            debug!("sending error {}", err);
                            if sender.send(Err(Box::new(err))).is_err() {
                                error!("Could not send result");
                            }
                        }
                    }
                } else {
                    error!("Could not parse addr {}", multiaddr);
                    let err = BadListener(format!("Could not parse {}", multiaddr));

                    debug!("sending error {}", err);
                    if sender.send(Err(Box::new(err))).is_err() {
                        error!("Could not send result");
                    }
                }
            }
            DragoonCommand::AddPeer { multiaddr, sender } => {
                if let Ok(addr) = multiaddr.parse::<Multiaddr>() {
                    if let Some(Protocol::P2p(hash)) = addr.iter().last() {
                        self.swarm.behaviour_mut().kademlia.add_address(&hash, addr);

                        debug!("sending empty response");
                        if sender.send(Ok(())).is_err() {
                            error!("Could not send result");
                        }
                    } else {
                        error!("could no isolate P2P component in {}", addr);
                        let err =
                            BadListener(format!("could no isolate P2P component in {}", addr));

                        debug!("sending error {}", err);
                        if sender.send(Err(Box::new(err))).is_err() {
                            error!("Could not send result");
                        }
                    }
                } else {
                    error!("Cannot parse addr {}", multiaddr);
                    let err = BadListener(format!("Could not parse {}", multiaddr));

                    debug!("sending error {}", err);
                    if sender.send(Err(Box::new(err))).is_err() {
                        error!("Could not send result");
                    }
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
                match self.swarm.behaviour_mut().kademlia.bootstrap() {
                    Ok(_) => {
                        if sender.send(Ok(())).is_err() {
                            error!("Could not send result");
                        }
                    }
                    Err(nkp) => {
                        error!("error: {}", nkp);
                        let err = BootstrapError(nkp.to_string());

                        debug!("sending error {}", err);
                        if sender.send(Err(Box::new(err))).is_err() {
                            error!("Could not send result");
                        }
                    }
                }
            }
            DragoonCommand::PutRecord {
                block_hash,
                block_dir,
                sender,
            } => match Self::put_record(self, block_hash, block_dir).await {
                Ok(id) => {
                    self.pending_put_record.insert(id, sender);
                }
                Err(err) => {
                    if sender.send(Err(err).map_err(|e| e.into())).is_err() {
                        error!("Could not send the result of the put_record operation")
                    }
                }
            },
            DragoonCommand::GetRecord { key, sender } => {
                let id = self
                    .swarm
                    .behaviour_mut()
                    .kademlia
                    .get_record(key.into_bytes().into());
                self.pending_get_record.insert(id, sender);
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
            } =>
            // TODO go search the block on the disk
            {
                match PeerId::from_str(peerid.as_str()) {
                    Ok(peer) => {
                        if self
                            .swarm
                            .behaviour_mut()
                            .dragoon
                            .send_data_to_peer(block_hash, block_path, peer)
                        {
                            if sender.send(Ok(())).is_err() {
                                error!("could not send result");
                            }
                        } else {
                            let err = PeerNotFound;
                            if sender.send(Err(Box::new(err))).is_err() {
                                error!("Cannot send result");
                            }
                        }
                    }
                    Err(err) => {
                        if sender.send(Err(Box::new(err))).is_err() {
                            error!("Cannot send result");
                        }
                    }
                }
            }
            DragoonCommand::DragoonGet {
                peerid,
                key,
                sender,
            } => match PeerId::from_str(peerid.as_str()) {
                Ok(peer) => {
                    let request_id = self
                        .swarm
                        .behaviour_mut()
                        .request_response
                        .send_request(&peer, FileRequest(key));
                    self.pending_request_file.insert(request_id, sender);
                }
                Err(err) => {
                    if sender.send(Err(Box::new(err))).is_err() {
                        error!("Cannot send result");
                    }
                }
            },
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
                        Self::encode_file(
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

    async fn put_record(&mut self, block_hash: String, block_dir: String) -> Result<QueryId> {
        let block = fs::read_blocks::<F, G>(
            &[block_hash.clone()],
            Path::new(&block_dir),
            Compress::Yes,
            Validate::Yes,
        )?[0]
            .clone()
            .1;
        let mut buf = vec![0; block.serialized_size(Compress::Yes)];
        block.serialize_with_mode(&mut buf[..], Compress::Yes)?;
        let record = kad::Record {
            key: block_hash.into_bytes().into(),
            value: buf,
            publisher: None,
            expires: None,
        };
        info!("Putting record {:?} in a kad record", record);
        let id = self
            .swarm
            .behaviour_mut()
            .kademlia
            .put_record(record, kad::Quorum::One)?;

        Ok(id)
    }

    async fn decode_blocks(
        block_dir: String,
        block_hashes: Vec<String>,
        output_filename: String,
    ) -> Result<()> {
        let blocks = fs::read_blocks::<F, G>(
            &block_hashes,
            &Path::new(&block_dir),
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
        file_path: String,
        replace_blocks: bool,
        encoding_method: EncodingMethod,
        encode_mat_k: usize,
        encode_mat_n: usize,
        powers_path: String,
    ) -> Result<String>
    where
        P: DenseUVPolynomial<F>,
        for<'a, 'b> &'a P: Div<&'b P, Output = P>,
    {
        info!("Reading file to convert from {:?}", file_path);
        let bytes = tokio::fs::read(&file_path).await?;
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
        let file_dir = if let Some(file_dir) = Path::new(&file_path).parent() {
            file_dir
        } else {
            error!("Parent of the block directory does not exist");
            let err = NoParentDirectory(file_path);
            return Err(err.into());
        };
        let block_dir: PathBuf = [file_dir, Path::new("blocks")].iter().collect();
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
        tokio::fs::create_dir(&block_dir).await?;
        let formatted_output = fs::dump_blocks(&blocks, &block_dir, Compress::Yes)?;
        Ok(formatted_output)
    }
}
