use futures::channel::{mpsc, oneshot};
use futures::prelude::*;

use libp2p::core::transport::ListenerId;
use libp2p::identity::Keypair;
use libp2p::request_response::{OutboundRequestId, ResponseChannel};
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
use std::time::Duration;
use tracing::{debug, error, info, warn};

use crate::commands::DragoonCommand;
use crate::error::DragoonError::{BadListener, BootstrapError, DialError, ProviderError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRequest(String);
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FileResponse(Vec<u8>);

pub(crate) async fn create_swarm(
    id_keys: Keypair,
) -> Result<Swarm<DragoonBehaviour>, Box<dyn Error>> {
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
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60 * 60)))
        .build();

    swarm
        .behaviour_mut()
        .kademlia
        .set_mode(Some(kad::Mode::Server));

    Ok(swarm)
}

#[derive(Debug)]
pub(crate) enum Event {
    InboundRequest {
        request: String,
        channel: ResponseChannel<FileResponse>,
    },
}

#[derive(NetworkBehaviour)]
pub(crate) struct DragoonBehaviour {
    request_response: request_response::cbor::Behaviour<FileRequest, FileResponse>,
    identify: identify::Behaviour,
    kademlia: kad::Behaviour<kad::store::MemoryStore>,
}

pub(crate) struct DragoonNetwork {
    swarm: Swarm<DragoonBehaviour>,
    command_receiver: mpsc::Receiver<DragoonCommand>,
    event_sender: mpsc::Sender<Event>,
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

impl DragoonNetwork {
    pub fn new(
        swarm: Swarm<DragoonBehaviour>,
        command_receiver: mpsc::Receiver<DragoonCommand>,
        event_sender: mpsc::Sender<Event>,
    ) -> Self {
        Self {
            swarm,
            command_receiver,
            event_sender,
            listeners: HashMap::new(),
            pending_start_providing: Default::default(),
            pending_get_providers: Default::default(),
            pending_request_file: Default::default(),
            pending_put_record: Default::default(),
            pending_get_record: Default::default(),
        }
    }

    pub async fn run(mut self) {
        info!("Starting Dragoon Network");
        loop {
            futures::select! {
                e = self.swarm.next() => self.handle_event(e.expect("Swarm stream to be infinite.")).await,
                cmd = self.command_receiver.next() =>  match cmd {
                    Some(c) => self.handle_command(c).await,
                    None => return,
                }
            }
        }
    }

    async fn handle_event(&mut self, event: SwarmEvent<DragoonBehaviourEvent>) {
        debug!("[event] {:?}", event);
        match event {
            SwarmEvent::Behaviour(DragoonBehaviourEvent::Kademlia(
                kad::Event::OutboundQueryProgressed { id, result, .. },
            )) => match result {
                kad::QueryResult::StartProviding(Ok(result_ok)) => {
                    if let Some(sender) = self.pending_start_providing.remove(&id) {
                        info!("Started providing {:?}", result_ok);
                        debug!("Sending empty response");
                        if sender.send(Ok(())).is_err() {
                            error!("Could not send result");
                        }
                    } else {
                        error!("Could not find id = {} in the start providers", id);
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
                            kad::GetProvidersOk::FinishedWithNoAdditionalRecord {
                                closest_peers,
                            } => {
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
                                let err = ProviderError(format!(
                                    "could not find {} in the query ids",
                                    id
                                ));
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
                        debug!("Sending value {:?}", value);
                        if sender.send(Ok(value)).is_err() {
                            error!("Cannot send result");
                        }
                    } else {
                        error!("could not find {} in the get records", id);
                    }
                }
                kad::QueryResult::GetRecord(Err(err)) => {
                    error!("Failed to get record: {err:?}");
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
                _ => {}
            },
            SwarmEvent::Behaviour(DragoonBehaviourEvent::Identify(identify::Event::Sent {
                peer_id,
                ..
            })) => info!("Sent identify info to {}", peer_id),
            SwarmEvent::Behaviour(DragoonBehaviourEvent::Identify(identify::Event::Received {
                peer_id,
                info,
            })) => {
                info!("Received identify info '{:?}' from {}", info, peer_id);
                self.swarm
                    .behaviour_mut()
                    .kademlia
                    .add_address(&peer_id, info.listen_addrs.get(0).unwrap().clone());
                info!("Added peer {}", peer_id);
            }
            SwarmEvent::Behaviour(DragoonBehaviourEvent::RequestResponse(
                request_response::Event::Message { message, .. },
            )) => match message {
                request_response::Message::Request {
                    request, channel, ..
                } => {
                    debug!("Sending inbound request '{}'", request.0);
                    if let Err(se) = self
                        .event_sender
                        .send(Event::InboundRequest {
                            request: request.0,
                            channel,
                        })
                        .await
                    {
                        error!("could not send inbound request: {}", se);
                    }
                }
                request_response::Message::Response {
                    request_id,
                    response,
                } => {
                    debug!("Preparing response from request {}", request_id);
                    if let Some(sender) = self.pending_request_file.remove(&request_id) {
                        debug!("Sending response {:?}", response);
                        if sender.send(Ok(response.0)).is_err() {
                            error!("Could not send result");
                        }
                    } else {
                        error!("could not find {} in the request files", request_id);
                    }
                }
            },
            SwarmEvent::Behaviour(DragoonBehaviourEvent::RequestResponse(
                request_response::Event::OutboundFailure {
                    request_id, error, ..
                },
            )) => {
                debug!("Request {} failed with {}", request_id, error);
                if let Some(sender) = self.pending_request_file.remove(&request_id) {
                    debug!("Sending error {}", error);
                    if sender.send(Err(Box::new(error))).is_err() {
                        error!("Could not send result");
                    }
                } else {
                    error!("could not find {} in the request files", request_id);
                }
            }
            e => warn!("[unknown event] {:?}", e),
        }
    }

    async fn handle_command(&mut self, cmd: DragoonCommand) {
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
            DragoonCommand::Get { key, peer, sender } => {
                let request_id = self
                    .swarm
                    .behaviour_mut()
                    .request_response
                    .send_request(&peer, FileRequest(key));
                self.pending_request_file.insert(request_id, sender);
            }
            DragoonCommand::AddFile { file, channel } => {
                if self
                    .swarm
                    .behaviour_mut()
                    .request_response
                    .send_response(channel, FileResponse(file))
                    .is_err()
                {
                    error!("Could not send response");
                }
            }
            DragoonCommand::PutRecord { key, value, sender } => {
                let record = kad::Record {
                    key: key.into_bytes().into(),
                    value,
                    publisher: None,
                    expires: None,
                };
                if let Ok(id) = self
                    .swarm
                    .behaviour_mut()
                    .kademlia
                    .put_record(record, kad::Quorum::One)
                {
                    self.pending_put_record.insert(id, sender);
                } else {
                    error!("Could not put record ");
                }
            }
            DragoonCommand::GetRecord { key, sender } => {
                let id = self
                    .swarm
                    .behaviour_mut()
                    .kademlia
                    .get_record(key.into_bytes().into());
                self.pending_get_record.insert(id, sender);
            }
        }
    }
}
