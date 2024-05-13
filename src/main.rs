mod app;
mod commands;
mod dragoon;
mod dragoon_network;
mod error;
mod to_serialize;

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

use ark_bls12_381::{Fr, G1Projective};
use ark_poly::univariate::DensePolynomial;

use crate::dragoon_network::DragoonNetwork;

#[tokio::main]
pub(crate) async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt::try_init().expect("cannot init logger");

    let (cmd_sender, cmd_receiver) = mpsc::channel(0);

    let router = Router::new()
        .route("/listen/:addr", get(commands::create_cmd_listen))
        .route("/get-listeners", get(commands::create_cmd_get_listeners))
        .route("/get-peer-id", get(commands::create_cmd_get_peer_id))
        .route(
            "/get-network-info",
            get(commands::create_cmd_get_network_info),
        )
        .route(
            "/remove-listener/:id",
            get(commands::create_cmd_remove_listener),
        )
        .route(
            "/get-connected-peers",
            get(commands::create_cmd_get_connected_peers),
        )
        .route("/dial/:addr", get(commands::create_cmd_dial))
        .route("/add-peer/:addr", get(commands::create_cmd_add_peer))
        .route(
            "/start-provide/:key",
            get(commands::create_cmd_start_provide),
        )
        .route(
            "/get-providers/:key",
            get(commands::create_cmd_get_providers),
        )
        .route("/bootstrap", get(commands::create_cmd_bootstrap))
        .route(
            "/put-record/:block_hash/:block_dir",
            get(commands::create_cmd_put_record),
        )
        .route("/get-record/:key", get(commands::create_cmd_get_record))
        .route("/dragoon/peers", get(commands::create_cmd_dragoon_peers))
        .route(
            "/dragoon/send/:peer/:block_hash/:block_path",
            get(commands::create_cmd_dragoon_send),
        )
        .route(
            "/dragoon/get/:peer/:key",
            get(commands::create_cmd_dragoon_get),
        )
        .route(
            "/decode-blocks/:block-dir/:block_hashes/:output_filename",
            get(commands::create_cmd_decode_blocks),
        )
        .route(
            "/encode-file/:file_path/:replace-blocks/:encoding-method/:encode_mat_k/:encode_mat_n/:powers_path",
            get(commands::create_cmd_encode_file),
        );

    let router = router.with_state(Arc::new(app::AppState::new(cmd_sender)));

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

    let http_server = axum::Server::bind(&ip_port).serve(router.into_make_service());
    tokio::spawn(http_server);

    let kp = get_keypair(id);
    info!("IP/port: {}", ip_port);
    info!("Peer ID: {} ({})", kp.public().to_peer_id(), id);

    let swarm = dragoon_network::create_swarm::<Fr, G1Projective>(kp).await?;
    let network = DragoonNetwork::new(swarm, cmd_receiver);
    tokio::spawn(network.run::<DensePolynomial<Fr>>());

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
