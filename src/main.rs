mod app;
mod commands;
mod dragoon_network;
mod error;
mod peer_block_info;
mod to_serialize;

use axum::routing::get;
use axum::Router;
use libp2p::identity;
use libp2p::identity::Keypair;
use std::error::Error;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::signal;
use tokio::sync::mpsc;
use tracing::info;

use ark_bls12_381::{Fr, G1Projective};
use ark_poly::univariate::DensePolynomial;

use crate::dragoon_network::DragoonNetwork;

#[tokio::main]
pub(crate) async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt::try_init().expect("cannot init logger");

    let (cmd_sender, cmd_receiver) = mpsc::unbounded_channel();

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
        // .route("/dragoon/peers", get(commands::create_cmd_dragoon_peers))
        // .route(
        //     "/dragoon/send/:peer/:block_hash/:block_path",
        //     get(commands::create_cmd_dragoon_send),
        // )
        .route(
            "/decode-blocks/:block-dir/:block_hashes/:output_filename",
            get(commands::create_cmd_decode_blocks),
        )
        .route(
            "/encode-file/:file_path/:replace-blocks/:encoding-method/:encode_mat_k/:encode_mat_n/:powers_path",
            get(commands::create_cmd_encode_file),
        )
        .route("/get-block-from/:peer_id_base_58/:file_hash/:block_hash", get(commands::create_cmd_get_block_from))
        .route("/get-file/:file_hash/:output_filename/:powers_path", get(commands::create_cmd_get_file))
        .route("/get-block-list/:file_hash", get(commands::create_cmd_get_block_list))
        .route("/get-blocks-info-from/:peer_id_base_58/:file_hash", get(commands::create_cmd_get_blocks_info_from))
        .route("/node-info", get(commands::create_cmd_node_info));

    let router = router.with_state(Arc::new(app::AppState::new(cmd_sender.clone())));

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
    let peer_id = kp.public().to_peer_id();
    info!("IP/port: {}", ip_port);
    info!("Peer ID: {} ({})", peer_id, id);

    let swarm = dragoon_network::create_swarm(kp).await?;
    let network = DragoonNetwork::new(swarm, cmd_receiver, cmd_sender, peer_id, true);
    tokio::spawn(network.run::<Fr, G1Projective, DensePolynomial<Fr>>());

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
