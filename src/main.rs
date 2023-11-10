mod dragoon_protocol;

use libp2p::kad::{Kademlia, KademliaEvent};
use libp2p::kad::store::MemoryStore;
use libp2p::request_response;
use libp2p_identity::{ed25519, Keypair};
use libp2p_swarm::{NetworkBehaviour, Swarm};
use crate::dragoon_protocol::{DragoonCodec, FileRequest, FileResponse};

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "DragoonEvent")]
struct DragoonBehaviour {
    request_response: request_response::Behaviour<DragoonCodec>,
    kademlia: Kademlia<MemoryStore>,
}

#[derive(Debug)]
enum DragoonEvent {
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

fn main() {
    println!("Hello, world!");
    let kp = get_keypair(1);
    let id = kp.public().to_peer_id();
    println!("{}", id);
}

fn get_keypair(seed: u8) -> Keypair {
    let mut bytes = [0u8; 32];
    bytes[0] = seed;
    Keypair::ed25519_from_bytes(bytes).unwrap()
}