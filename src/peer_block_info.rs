use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct PeerBlockInfo {
    pub(crate) peer_id_base_58: String,
    pub(crate) file_hash: String,
    pub(crate) block_hashes: Vec<String>,
    pub(crate) block_sizes: Option<Vec<usize>>,
}
