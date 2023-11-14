mod dragoon_network;
mod dragoon_protocol;

use libp2p_core::identity::{ed25519, Keypair};

use axum::extract::Path;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::get;
use axum::Router;
use futures::channel::mpsc::Sender;
use futures::channel::{mpsc, oneshot};
use futures::SinkExt;
use std::error::Error;
use tokio::signal;
use tracing::{error, info};

use crate::dragoon_network::{DragoonCommand, DragoonNetwork};

const IP_PORT: &str = "127.0.0.1:3000";

async fn listen(Path(multiaddr): Path<String>, mut cmd_sender: Sender<DragoonCommand>) {
    let (sender, receiver) = oneshot::channel();

    if let Err(e) = cmd_sender
        .send(DragoonCommand::Listen { multiaddr, sender })
        .await
    {
        error!("Cannot send command Listen: {:?}", e);
    }
    if let Err(e) = receiver.await {
        error!("Cannot receive a return from command Listen: {:?}", e);
    }
}

async fn get_listeners(mut cmd_sender: Sender<DragoonCommand>) -> Response {
    let (sender, receiver) = oneshot::channel();

    if let Err(e) = cmd_sender
        .send(DragoonCommand::GetListener { sender })
        .await
    {
        error!("Cannot send command GetListener: {:?}", e);
    }

    match receiver.await {
        Err(e) => {
            error!("Cannot receive a return from command GetListener: {:?}", e);
            Json("").into_response()
        }
        Ok(res) => match res {
            Err(e) => {
                error!("GetListener returned an error: {:?}", e);
                Json("").into_response()
            }
            Ok(listeners) => Json(listeners).into_response(),
        },
    }
}

async fn get_peer_id(mut cmd_sender: Sender<DragoonCommand>) -> Response {
    let (sender, receiver) = oneshot::channel();

    if let Err(e) = cmd_sender.send(DragoonCommand::GetPeerId { sender }).await {
        error!("Cannot send command GetPeerId: {:?}", e);
    }

    match receiver.await {
        Err(e) => {
            error!("Cannot receive a return from command GetPeerId: {:?}", e);
            Json("").into_response()
        }
        Ok(res) => match res {
            Err(e) => {
                error!("GetPeerId returned an error: {:?}", e);
                Json("").into_response()
            }
            Ok(peer_id) => Json(peer_id.to_base58()).into_response(),
        },
    }
}

#[tokio::main]
pub async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt::try_init().expect("cannot init logger");

    let (cmd_sender, cmd_receiver) = mpsc::channel(0);

    let cmd_sender_2 = cmd_sender.clone();
    let cmd_sender_3 = cmd_sender.clone();
    let app = Router::new()
        .route("/listen/:addr", get(move |addr| listen(addr, cmd_sender)))
        .route("/get-listeners", get(move || get_listeners(cmd_sender_2)))
        .route("/get-peer-id", get(move || get_peer_id(cmd_sender_3)));

    let http_server =
        axum::Server::bind(&IP_PORT.parse().unwrap()).serve(app.into_make_service());
    tokio::spawn(http_server);

    let kp = get_keypair(1);
    info!("Peer id: {}", kp.public().to_peer_id());

    let swarm = dragoon_network::create_swarm(kp).await?;
    let network = DragoonNetwork::new(swarm, cmd_receiver);
    tokio::spawn(network.run());

    let shutdown = signal::ctrl_c();
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
