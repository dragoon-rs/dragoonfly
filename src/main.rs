mod dragoon_network;
mod dragoon_protocol;

use libp2p_core::identity::{ed25519, Keypair};

use tokio::signal;
use tracing::{info};
use crate::dragoon_network::{DragoonCommand, DragoonNetwork};
use std::error::Error;
use axum::{Router, ServiceExt};
use axum::extract::Path;
use axum::routing::get;
use futures::channel::{mpsc, oneshot};
use futures::channel::mpsc::Sender;
use futures::SinkExt;


async fn toto(Path(user_id): Path<String>, mut cmd_sender: Sender<DragoonCommand>) {
    println!("user id {}", user_id);
    let (sender, receiver) = oneshot::channel();
    cmd_sender.send(DragoonCommand::DragoonTest {file_name: "coucou".to_string(), sender}).await.expect("Command reveiver not to be dropped");
    receiver.await.expect("Sender not to be dropped");
}

#[tokio::main]
pub async fn main() -> Result<(), Box<dyn Error>>{
    tracing_subscriber::fmt::try_init().expect("cannot init logger");

    let kp = get_keypair(1);
    let id = kp.public().to_peer_id();
    info!("Peer id: {}", id);

    let (cmd_sender, cmd_receiver) = mpsc::channel(0);

    let app = Router::new()
        .route("/toto/:id", get({
            move |path| toto(path, cmd_sender.clone())
        }),
        );

    let http_server = axum::Server::bind(&"127.0.0.1:3000".parse().unwrap())
        .serve(app.into_make_service());
    tokio::spawn(http_server);

    let swarm = dragoon_network::create_swarm(kp).await?;
    let network = DragoonNetwork::new(swarm, cmd_receiver);
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
