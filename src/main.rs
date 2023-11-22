mod app;
mod commands;
mod dragoon_network;
mod error;

use axum::routing::get;
use axum::Router;
use futures::channel::mpsc;
use libp2p::identity;
use libp2p::identity::Keypair;
use std::error::Error;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::signal;
use tracing::info;

use crate::dragoon_network::DragoonNetwork;

#[tokio::main]
pub(crate) async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt::try_init().expect("cannot init logger");

    let (cmd_sender, cmd_receiver) = mpsc::channel(0);

    let app = Router::new()
        .route("/listen/:addr", get(commands::listen))
        .route("/get-listeners", get(commands::get_listeners))
        .route("/get-peer-id", get(commands::get_peer_id))
        .route("/get-network-info", get(commands::get_network_info))
        .route("/remove-listener/:id", get(commands::remove_listener))
        .route("/get-connected-peers", get(commands::get_connected_peers))
        .route("/dial/:addr", get(commands::dial))
        .route("/add-peer/:addr", get(commands::add_peer))
        .route("/start-provide/:key", get(commands::start_provide))
        .route("/get-providers/:key", get(commands::get_providers))
        .route("/bootstrap", get(commands::bootstrap))
        .route("/get/:key", get(commands::get))
        .with_state(Arc::new(app::AppState::new(cmd_sender)));

    let ip_port: SocketAddr = if let Some(ip_port) = std::env::args().nth(1) {
        ip_port
    } else {
        "127.0.0.1:3000".to_string()
    }
    .parse()
    .unwrap();

    let id = if let Some(id) = std::env::args().nth(2) {
        id.parse::<u8>().unwrap()
    } else {
        0
    };

    let http_server = axum::Server::bind(&ip_port).serve(app.into_make_service());
    tokio::spawn(http_server);

    let kp = get_keypair(id);
    info!("Peer id: {} {}", kp.public().to_peer_id(), id);

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
    identity::Keypair::ed25519_from_bytes(bytes).unwrap()
}
