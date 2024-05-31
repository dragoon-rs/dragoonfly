use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PeerBlockInfo {
    pub(crate) peer_id_base_58: String,
    pub(crate) file_hash: String,
    pub(crate) block_hashes: Vec<String>,
}
