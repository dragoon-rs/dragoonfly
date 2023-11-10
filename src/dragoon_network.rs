use crate::dragoon_protocol::{DragoonCodec, DragoonProtocol, FileRequest, FileResponse};
use libp2p::futures::channel::{mpsc, oneshot};
use libp2p::futures::StreamExt;
use libp2p::request_response;
use libp2p_core::identity::Keypair;
use libp2p_kad::store::MemoryStore;
use libp2p_kad::{Kademlia, KademliaEvent};
use libp2p_request_response::ProtocolSupport;
use libp2p_swarm::{NetworkBehaviour, Swarm};
use std::error::Error;
use std::iter;
use tracing::info;

pub async fn create_swarm(id_keys: Keypair) -> Result<Swarm<DragoonBehaviour>, Box<dyn Error>> {
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
pub struct DragoonBehaviour {
    request_response: request_response::Behaviour<DragoonCodec>,
    kademlia: Kademlia<MemoryStore>,
}

#[derive(Debug)]
pub enum DragoonEvent {
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

#[derive(Debug)]
pub enum DragoonCommand {
    DragoonTest {
        file_name: String,
        sender: oneshot::Sender<Result<(), Box<dyn Error + Send>>>,
    },
}

pub struct DragoonNetwork {
    swarm: Swarm<DragoonBehaviour>,
    command_receiver: mpsc::Receiver<DragoonCommand>,
}

impl DragoonNetwork {
    pub fn new(
        swarm: Swarm<DragoonBehaviour>,
        command_receiver: mpsc::Receiver<DragoonCommand>,
    ) -> Self {
        Self {
            swarm,
            command_receiver,
        }
    }

    pub async fn run(mut self) {
        info!("Starting Dragoon Network");
        self.swarm
            .listen_on("/ip4/127.0.0.1/tcp/31000".parse().unwrap())
            .expect("Listening not to fail.");
        loop {
            futures::select! {
                e = self.swarm.next() => println!("{:?}",e),
                cmd = self.command_receiver.next() =>  match cmd {
                    Some(c) => self.handle_command(c).await,
                    None => return,
                }
            }
        }
    }

    async fn handle_command(&mut self, cmd: DragoonCommand) {
        match cmd {
            DragoonCommand::DragoonTest { file_name, sender } => {
                info!(file_name);
                sender.send(Ok(())).expect("TODO: panic message");
            }
        }
    }
}
