mod dragoon_network;
mod dragoon_protocol;

use libp2p_core::identity::{ed25519, Keypair};

use libp2p_swarm::{NetworkBehaviour, Swarm};

use tokio::signal;
use tracing::{info};
use crate::dragoon_network::DragoonNetwork;
use std::error::Error;
#[tokio::main]
pub async fn main() -> Result<(), Box<dyn Error>>{
    tracing_subscriber::fmt::try_init().expect("cannot init logger");

    let kp = get_keypair(1);
    let id = kp.public().to_peer_id();
    info!("Peer id: {}", id);

    let swarm = dragoon_network::create_swarm(kp).await?;
    let network = DragoonNetwork::new(swarm);
    let shutdown = signal::ctrl_c();
    tokio::spawn(network.run());

    tokio::select! {
        _ = shutdown => {
            info!("shutdown Dragoon node");
        }
    }
    Ok(())
}

fn get_keypair(seed: u8) -> Keypair {
    let mut bytes = [0u8; 32];
    bytes[0] = seed;
    let secret_key = ed25519::SecretKey::from_bytes(&mut bytes).expect(
        "Cannot convert bytes to SecretKey.",
    );
    Keypair::Ed25519(secret_key.into())
}
