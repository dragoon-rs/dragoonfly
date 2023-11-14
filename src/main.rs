mod dragoon_network;
mod dragoon_protocol;

use libp2p_core::identity::{ed25519, Keypair};

use axum::extract::Path;
use axum::routing::get;
use axum::Router;
use futures::channel::mpsc::Sender;
use futures::channel::{mpsc, oneshot};
use futures::SinkExt;
use std::error::Error;
use futures::future::err;
use tokio::signal;
use tracing::{error, info};

use crate::dragoon_network::{DragoonCommand, DragoonNetwork};

async fn toto(Path(user_id): Path<String>, mut cmd_sender: Sender<DragoonCommand>) {
    info!("user id {}", user_id);
    let (sender, receiver) = oneshot::channel();

    if cmd_sender.send(DragoonCommand::DragoonTest {
            file_name: "coucou".to_string(),
            sender,
        })
        .await.is_err() {
        error!("Cannot send Command DragoonTest");
    }
    if receiver.await.is_err() {
        error!("Cannot receive a return from Command DragoonTest");
    }
}

#[tokio::main]
pub async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt::try_init().expect("cannot init logger");

    let kp = get_keypair(1);
    let id = kp.public().to_peer_id();
    info!("Peer id: {}", id);

    let (cmd_sender, cmd_receiver) = mpsc::channel(0);

    let app = Router::new().route("/toto/:id", get(move |path| toto(path, cmd_sender.clone())));

    let http_server =
        axum::Server::bind(&"127.0.0.1:3000".parse().unwrap()).serve(app.into_make_service());
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
    let secret_key =
        ed25519::SecretKey::from_bytes(&mut bytes).expect("Cannot convert bytes to SecretKey.");
    Keypair::Ed25519(secret_key.into())
}
