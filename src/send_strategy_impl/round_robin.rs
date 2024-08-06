//! Start by returning the peers as they come, then cycle on the known peers once the entire list of peer is known

use anyhow::{format_err, Result};
use libp2p::PeerId;

use tracing::error;

use crate::send_strategy::{SendId, SendStrategy};

#[derive(Default)]
pub(crate) struct RobinDistribution {
    already_seen_peers: Vec<PeerId>,
    round_index: usize,
}

impl SendStrategy for RobinDistribution {
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
        } else if let Some(peer_id) = self.already_seen_peers.get(self.round_index) {
            self.round_index += 1;
            if self.round_index >= self.already_seen_peers.len() {
                self.round_index = 0;
            }
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
