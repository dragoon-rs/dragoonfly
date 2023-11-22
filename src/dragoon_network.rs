use futures::channel::{mpsc, oneshot};
use futures::prelude::*;

use libp2p::core::transport::ListenerId;
use libp2p::identity::Keypair;
use libp2p::{
    core::Multiaddr,
    kad,
    identify,
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
use tracing::{error, info};

use crate::commands::DragoonCommand;
use crate::error::DragoonError::{BadListener, DialError};

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
            identify:            identify::Behaviour::new(identify::Config::new(
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
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60*60)))
        .build();

    swarm
        .behaviour_mut()
        .kademlia
        .set_mode(Some(kad::Mode::Server));

    Ok(swarm)
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
    listeners: HashMap<u64, ListenerId>,
    pending_start_providing: HashMap<kad::QueryId, oneshot::Sender<()>>,
    pending_get_providers: HashMap<kad::QueryId, oneshot::Sender<HashSet<PeerId>>>,
}

impl DragoonNetwork {
    pub fn new(
        swarm: Swarm<DragoonBehaviour>,
        command_receiver: mpsc::Receiver<DragoonCommand>,
    ) -> Self {
        Self {
            swarm,
            command_receiver,
            listeners: HashMap::new(),
            pending_start_providing: Default::default(),
            pending_get_providers: Default::default(),
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
        match event {
            SwarmEvent::Behaviour(DragoonBehaviourEvent::Kademlia(
                kad::Event::OutboundQueryProgressed {
                    id,
                    result: kad::QueryResult::StartProviding(Ok(result_ok)),
                    ..
                },
            )) => {
                let sender: oneshot::Sender<()> = self
                    .pending_start_providing
                    .remove(&id)
                    .expect("Completed query to be previously pending.");
                info!("started providing {:?}", result_ok);
                let _ = sender.send(());
            }
            SwarmEvent::Behaviour(DragoonBehaviourEvent::Kademlia(
                kad::Event::OutboundQueryProgressed {
                    id,
                    result: kad::QueryResult::GetProviders(get_providers_result),
                    ..
                },
            )) => {
                if let Ok(res) = get_providers_result {
                    match res {
                        kad::GetProvidersOk::FoundProviders { providers, .. } => {
                            info!("Found providers {providers:?}");
                            if let Some(sender) = self.pending_get_providers.remove(&id) {
                                sender.send(providers).expect("Receiver not to be dropped");
                            }
                        }
                        kad::GetProvidersOk::FinishedWithNoAdditionalRecord { closest_peers } => {
                            info!("Finished get providers {closest_peers:?}");
                        }
                    }
                } else {
                    info!("GetProviders returned an error");
                    if let Some(sender) = self.pending_get_providers.remove(&id) {
                        sender
                            .send(HashSet::default())
                            .expect("Receiver not to be dropped");
                        self.swarm
                            .behaviour_mut()
                            .kademlia
                            .query_mut(&id)
                            .unwrap()
                            .finish();
                    }
                }
            }
            SwarmEvent::Behaviour(DragoonBehaviourEvent::Identify(identify::Event::Sent { peer_id, .. })) => {
                info!("Sent identify info to {peer_id:?}")
            }
            // Prints out the info received via the identify event
            SwarmEvent::Behaviour(DragoonBehaviourEvent::Identify(identify::Event::Received { peer_id, info })) => {
                info!("Received {info:?}");
                self.swarm.behaviour_mut().kademlia.add_address(
                    &peer_id,
                    info.listen_addrs.get(0).unwrap().clone()
                );
                info!("peer added");

            }
            // SwarmEvent::ConnectionEstablished { peer_id, endpoint, num_established, concurrent_dial_errors, established_in } => {
            //     match endpoint {
            //         ConnectedPoint::Listener { local_addr, send_back_addr } => {
            //             self.swarm
            //                 .behaviour_mut()
            //                 .kademlia.bootstrap()
            //                 .add_address(peer_id, )
            //         }
            //         ConnectedPoint::Dialer { address, role_override } => {
            //             info!("connectionEstablished to {adrress:?}");
            //         }
            //     }
            // }
            e => info!("{e:?}"),
        }
    }

    async fn handle_command(&mut self, cmd: DragoonCommand) {
        match cmd {
            DragoonCommand::Listen { multiaddr, sender } => {
                if let Ok(addr) = multiaddr.parse() {
                    match self.swarm.listen_on(addr) {
                        Ok(listener_id) => {
                            info!("listening on {}", multiaddr);

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

                            if sender.send(Ok(id)).is_err() {
                                error!("could not send listener ID");
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

                            if sender.send(Err(Box::new(BadListener(err_msg)))).is_err() {
                                error!("Cannot send result");
                            }
                        }
                    }
                } else {
                    error!("cannot parse addr {}", multiaddr);
                    let err = BadListener(format!("could not parse {}", multiaddr));
                    if sender.send(Err(Box::new(err))).is_err() {
                        error!("Cannot send result");
                    }
                }
            }
            DragoonCommand::GetListeners { sender } => {
                info!("getting listeners");
                let listeners = self
                    .swarm
                    .listeners()
                    .into_iter()
                    .cloned()
                    .collect::<Vec<Multiaddr>>();

                if sender.send(Ok(listeners)).is_err() {
                    error!("could not send list of listeners");
                }
            }
            DragoonCommand::GetPeerId { sender } => {
                info!("getting peer ID");
                let peer_id = *self.swarm.local_peer_id();

                if sender.send(Ok(peer_id)).is_err() {
                    error!("could not send peer ID");
                }
            }
            DragoonCommand::GetNetworkInfo { sender } => {
                info!("getting network info");
                let network_info = self.swarm.network_info();

                if sender.send(Ok(network_info)).is_err() {
                    error!("could not send network info");
                }
            }
            DragoonCommand::RemoveListener {
                listener_id,
                sender,
            } => {
                info!("removing listener");

                if let Some(listener) = self.listeners.get(&listener_id) {
                    let res = self.swarm.remove_listener(*listener);

                    if sender.send(Ok(res)).is_err() {
                        error!("could not send remove listener");
                    }
                } else {
                    error!("could not find listener");
                    let err = BadListener(format!("listener {} not found", listener_id));
                    if sender.send(Err(Box::new(err))).is_err() {
                        error!("Cannot send result");
                    }
                }
            }
            DragoonCommand::GetConnectedPeers { sender } => {
                info!("getting list of connected peers");
                let connected_peers = self
                    .swarm
                    .connected_peers()
                    .into_iter()
                    .cloned()
                    .collect::<Vec<PeerId>>();

                if sender.send(Ok(connected_peers)).is_err() {
                    error!("could not send list of connected peers");
                }
            }
            DragoonCommand::Dial { multiaddr, sender } => {
                if let Ok(addr) = multiaddr.parse::<Multiaddr>() {
                    info!("dialing {}", addr);
                    match self.swarm.dial(addr) {
                        Ok(()) => {
                            if sender.send(Ok(())).is_err() {
                                error!("could not send result");
                            }
                        }
                        Err(de) => {
                            error!("error: {}", de);

                            let err = DialError(de.to_string());
                            if sender.send(Err(Box::new(err))).is_err() {
                                error!("Cannot send result");
                            }
                        }
                    }
                } else {
                    error!("cannot parse addr {}", multiaddr);
                    let err = BadListener(format!("could not parse {}", multiaddr));
                    if sender.send(Err(Box::new(err))).is_err() {
                        error!("Cannot send result");
                    }
                }
            }
            DragoonCommand::AddPeer { multiaddr, sender } => {
                if let Ok(addr) = multiaddr.parse::<Multiaddr>() {
                    info!("adding peer {} from {}", addr, multiaddr);
                    if let Some(Protocol::P2p(hash)) = addr.iter().last() {
                        self.swarm.behaviour_mut().kademlia.add_address(&hash, addr);
                        if sender.send(Ok(())).is_err() {
                            error!("could not send result");
                        }
                    }
                } else {
                    error!("cannot parse addr {}", multiaddr);
                    let err = BadListener(format!("could not parse {}", multiaddr));
                    if sender.send(Err(Box::new(err))).is_err() {
                        error!("Cannot send result");
                    }
                }
            }
            DragoonCommand::StartProvide { key, sender } => {
                if let Ok(query_id) = self
                    .swarm
                    .behaviour_mut()
                    .kademlia
                    .start_providing(key.into_bytes().into())
                {
                    self.pending_start_providing.insert(query_id, sender);
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
                self.swarm.behaviour_mut().kademlia.bootstrap().unwrap();

                if sender.send(()).is_err() {
                    error!("could not send result");
                }
            }
        }
    }
}
