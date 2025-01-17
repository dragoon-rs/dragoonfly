mod app;
mod commands;
mod dragoon_swarm;
mod error;
mod peer_block_info;
mod send_block_to;
mod send_strategy;
mod send_strategy_impl;
mod to_serialize;

use axum::routing::{get, post};
use axum::Router;
use clap::Parser;
use libp2p::identity;
use libp2p::identity::Keypair;
use std::sync::Arc;
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
};
use tokio::signal;
use tokio::sync::mpsc;
use tracing::{error, info};

use anyhow::Result;

use ark_bls12_381::{Fr, G1Projective};
use ark_poly::univariate::DensePolynomial;

use crate::dragoon_swarm::DragoonNetwork;

#[derive(Parser)]
#[command(name = "Dragoonfly")]
#[command(version = "1.0")]
#[command(about = "A Provable Coded P2P System", long_about = None)]
struct Cli {
    #[arg(long, short)]
    powers_path: PathBuf,
    #[arg(long, short, default_value_t = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 3000))]
    ip_port: SocketAddr,
    #[arg(long, short, default_value_t = 0)]
    seed: u8,
    #[arg(long, default_value_t = 20)]
    storage_space: usize,
    #[arg(long, default_value_t = Units::G, help = "Standard power of 10 notation")]
    storage_unit: Units,
    #[arg(long, default_value_t = false)]
    replace_file_dir: bool,
    #[arg(short, long)]
    label: Option<String>,
}

#[derive(Debug, Copy, Clone, PartialEq, clap::ValueEnum)]
#[clap(rename_all = "UPPER")]
enum Units {
    B,
    K,
    M,
    G,
    T,
}

impl std::fmt::Display for Units {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        std::fmt::Debug::fmt(self, f)
    }
}

#[tokio::main]
pub(crate) async fn main() -> Result<()> {
    tracing_subscriber::fmt::try_init().expect("cannot init logger");

    let (cmd_sender, cmd_receiver) = mpsc::unbounded_channel();

    let router = Router::new()
        .route("/listen/{multiaddr}", get(commands::create_cmd_listen))
        .route("/get-listeners", get(commands::create_cmd_get_listeners))
        .route(
            "/get-network-info",
            get(commands::create_cmd_get_network_info),
        )
        .route(
            "/remove-listener",
            post(commands::create_cmd_remove_listener),
        )
        .route(
            "/get-connected-peers",
            get(commands::create_cmd_get_connected_peers),
        )
        .route("/dial-single", post(commands::create_cmd_dial_single))
        .route("/dial-multiple", post(commands::create_cmd_dial_multiple))
        .route("/add-peer", post(commands::create_cmd_add_peer))
        .route("/start-provide", post(commands::create_cmd_start_provide))
        .route("/stop-provide", post(commands::create_cmd_stop_provide))
        .route("/get-providers", post(commands::create_cmd_get_providers))
        .route("/bootstrap", get(commands::create_cmd_bootstrap))
        // .route("/dragoon/peers", get(commands::create_cmd_dragoon_peers))
        // .route(
        //     "/dragoon/send/:peer/:block_hash/:block_path",
        //     get(commands::create_cmd_dragoon_send),
        // )
        .route("/decode-blocks", post(commands::create_cmd_decode_blocks))
        .route("/encode-file", post(commands::create_cmd_encode_file))
        .route(
            "/get-block-from/{peer_id_base_58}/{file_hash}/{block_hash}/{save_to_disk}",
            get(commands::create_cmd_get_block_from),
        )
        .route(
            "/get-file/{file_hash}/{output_filename}",
            get(commands::create_cmd_get_file),
        )
        .route(
            "/get-block-list/{file_hash}",
            get(commands::create_cmd_get_block_list),
        )
        .route(
            "/get-blocks-info-from/{peer_id_base_58}/{file_hash}",
            get(commands::create_cmd_get_blocks_info_from),
        )
        .route("/node-info", get(commands::create_cmd_node_info))
        .route("/send-block-to", post(commands::create_cmd_send_block_to))
        .route(
            "/get-available-send-storage",
            get(commands::create_cmd_get_available_storage),
        )
        .route(
            "/send-block-list",
            post(commands::create_cmd_send_block_list),
        )
        .route(
            "/change-available-send-storage",
            post(commands::create_cmd_change_available_send_storage),
        );

    let router = router.with_state(Arc::new(app::AppState::new(cmd_sender.clone())));

    info!("Parsing the command line arguments");
    let cli = Cli::parse();

    let powers_path = cli.powers_path;
    let ip_port: SocketAddr = cli.ip_port;
    let seed = cli.seed;
    let replace_file_dir = cli.replace_file_dir;

    let multiplier = match cli.storage_unit {
        Units::B => 1,
        Units::K => 10usize.pow(3),
        Units::M => 10usize.pow(6),
        Units::G => 10usize.pow(9),
        Units::T => 10usize.pow(12),
    };
    let total_available_storage_for_send = cli.storage_space * multiplier;
    let listener = tokio::net::TcpListener::bind(ip_port).await?;
    info!("Spawning the http server");
    tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, router.into_make_service()).await {
            error!("server error: {}", error);
        }
    });
    let kp = get_keypair(seed);
    let peer_id = kp.public().to_peer_id();
    info!("IP/port: {}", ip_port);
    info!("Peer ID: {} ({})", peer_id, seed);

    info!("Creating the swarm");
    let swarm = dragoon_swarm::create_swarm(kp).await?;
    let network = DragoonNetwork::new(
        swarm,
        cmd_receiver,
        cmd_sender,
        powers_path,
        total_available_storage_for_send,
        peer_id,
        cli.label,
        replace_file_dir,
    );

    info!("Running the network");
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
