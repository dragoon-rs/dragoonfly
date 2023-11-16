use libp2p::futures::channel::mpsc;
use libp2p::futures::StreamExt;
use libp2p::{request_response, TransportError};
use libp2p_core::identity::Keypair;
use libp2p_core::multiaddr::Protocol;
use libp2p_core::transport::ListenerId;
use libp2p_core::{Multiaddr, PeerId};
use libp2p_kad::store::MemoryStore;
use libp2p_kad::{Kademlia, KademliaEvent};
use libp2p_request_response::ProtocolSupport;
use libp2p_swarm::{NetworkBehaviour, Swarm};
use std::collections::HashMap;
use std::error::Error;
use std::iter;
use tracing::{error, info};

use crate::commands::DragoonCommand;
use crate::dragoon_protocol::{DragoonCodec, DragoonProtocol, FileRequest, FileResponse};
use crate::error::DragoonError::{BadListener, DialError};

pub(crate) async fn create_swarm(
    id_keys: Keypair,
) -> Result<Swarm<DragoonBehaviour>, Box<dyn Error>> {
    let peer_id = id_keys.public().to_peer_id();
    let swarm = Swarm::with_threadpool_executor(
        libp2p::development_transport(id_keys).await?,
        DragoonBehaviour {
            kademlia: Kademlia::new(peer_id, MemoryStore::new(peer_id)),
            request_response: request_response::Behaviour::new(
                DragoonCodec(),
                iter::once((DragoonProtocol(), ProtocolSupport::Full)),
                Default::default(),
            ),
        },
        peer_id,
    );
    Ok(swarm)
}

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "DragoonEvent")]
pub(crate) struct DragoonBehaviour {
    request_response: request_response::Behaviour<DragoonCodec>,
    kademlia: Kademlia<MemoryStore>,
}

#[derive(Debug)]
pub(crate) enum DragoonEvent {
    RequestResponse(request_response::Event<FileRequest, FileResponse>),
    Kademlia(KademliaEvent),
}

impl From<request_response::Event<FileRequest, FileResponse>> for DragoonEvent {
    fn from(event: request_response::Event<FileRequest, FileResponse>) -> Self {
        DragoonEvent::RequestResponse(event)
    }
}

impl From<KademliaEvent> for DragoonEvent {
    fn from(event: KademliaEvent) -> Self {
        DragoonEvent::Kademlia(event)
    }
}

pub(crate) struct DragoonNetwork {
    swarm: Swarm<DragoonBehaviour>,
    command_receiver: mpsc::Receiver<DragoonCommand>,
    listeners: HashMap<u64, ListenerId>,
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
        }
    }

    pub async fn run(mut self) {
        info!("Starting Dragoon Network");
        loop {
            futures::select! {
                e = self.swarm.next() => info!("{:?}",e),
                cmd = self.command_receiver.next() =>  match cmd {
                    Some(c) => self.handle_command(c).await,
                    None => return,
                }
            }
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
                    if sender
                        .send(Err(Box::new(BadListener(format!(
                            "could not parse {}",
                            multiaddr
                        )))))
                        .is_err()
                    {
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
                    if sender
                        .send(Err(Box::new(BadListener(format!(
                            "listener {} not found",
                            listener_id
                        )))))
                        .is_err()
                    {
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

                            if sender
                                .send(Err(Box::new(DialError(de.to_string()))))
                                .is_err()
                            {
                                error!("Cannot send result");
                            }
                        }
                    }
                } else {
                    error!("cannot parse addr {}", multiaddr);
                    if sender
                        .send(Err(Box::new(BadListener(format!(
                            "could not parse {}",
                            multiaddr
                        )))))
                        .is_err()
                    {
                        error!("Cannot send result");
                    }
                }
            }
            DragoonCommand::AddPeer { multiaddr, sender } => {
                if let Ok(addr) = multiaddr.parse::<Multiaddr>() {
                    info!("adding peer {} from {}", addr, multiaddr);
                    if let Some(Protocol::P2p(hash)) = addr.iter().last() {
                        let peer_id = PeerId::from_multihash(hash).expect("Valid hash.");
                        self.swarm
                            .behaviour_mut()
                            .kademlia
                            .add_address(&peer_id, addr);
                        if sender.send(Ok(())).is_err() {
                            error!("could not send result");
                        }
                    }
                } else {
                    error!("cannot parse addr {}", multiaddr);
                    //TODO: Send error to sender and check if is_err
                }
            }
        }
    }
}
