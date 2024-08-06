use anyhow::{format_err, Result};
use libp2p::PeerId;
use rand::seq::SliceRandom;

use tracing::error;

use crate::send_strategy::{SendId, SendStrategy};

#[derive(Default)]
pub(crate) struct RandomDistribution {
    already_seen_peers: Vec<PeerId>,
}

impl SendStrategy for RandomDistribution {
    type PeerInput = PeerId;
    type BlockInput = (String, String);

    fn choose_next_peer_block(
        &mut self,
        peer_input: Option<Self::PeerInput>,
        block_input: Self::BlockInput,
    ) -> Result<SendId> {
        let (file_hash, block_hash) = block_input;
        if let Some(peer_id) = peer_input {
            self.already_seen_peers.push(peer_id);
            Ok(SendId {
                peer_id,
                file_hash,
                block_hash,
            })
        } else if let Some(peer_id) = self.already_seen_peers.choose(&mut rand::thread_rng()) {
            Ok(SendId {
                peer_id: *peer_id,
                file_hash,
                block_hash,
            })
        } else {
            let err_msg =
                String::from("The stream of peers to choose who to send blocks to was empty");
            error!(err_msg);
            Err(format_err!(err_msg))
        }
    }
}
