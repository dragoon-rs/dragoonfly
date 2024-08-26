use std::{collections::HashSet, path::PathBuf};

use axum::response::{IntoResponse, Json, Response};
use libp2p::{swarm::NetworkInfo, Multiaddr, PeerId};
use serde::ser::Serialize;

use crate::send_strategy::SendId;
use crate::{
    commands::SerNetworkInfo, dragoon_swarm::BlockResponse, peer_block_info::PeerBlockInfo,
};

// can't implement Serialize for Json as those are a external Trait and Struct, so we need a wrapper
pub(crate) struct JsonWrapper<T>(pub Json<T>);

pub(crate) trait ConvertSer {
    fn convert_ser(&self) -> impl Serialize;
}

/// Used to implement for all types that are already Serialize and only need to return self (kind of a default implementatioon basically).
/// We do not directly implement for all Serialize, because it is a foreign trait and could lead to double implementation conflicts later on if PeerID decided to implement Serialize for example
///
/// Macro rule taken from: <https://stackoverflow.com/a/50223259>
macro_rules! impl_Convert {
    (for $($t:ty),+) => {
        $(impl ConvertSer for $t {
            fn convert_ser(&self) -> impl Serialize {
                self
            }
        })*
    }
}

// impl convert for all the types that are already Serialize and thus just return themselves
impl_Convert!(for u64, String, bool, &str, Vec<Multiaddr>, Vec<u8>, PeerBlockInfo, BlockResponse, PathBuf, usize);

impl ConvertSer for PeerId {
    fn convert_ser(&self) -> impl Serialize {
        self.to_base58()
    }
}

impl ConvertSer for NetworkInfo {
    fn convert_ser(&self) -> impl Serialize {
        SerNetworkInfo::new(self)
    }
}

impl ConvertSer for () {
    fn convert_ser(&self) -> impl Serialize {
        "".convert_ser()
        // default return is just an empty string when we don't have anything
    }
}

impl<T> ConvertSer for Vec<T>
where
    T: ConvertSer,
{
    fn convert_ser(&self) -> impl Serialize {
        self.iter()
            .map(|convertable| convertable.convert_ser())
            .collect::<Vec<_>>()
    }
}

impl<T> ConvertSer for HashSet<T>
where
    T: ConvertSer,
{
    fn convert_ser(&self) -> impl Serialize {
        self.iter()
            .map(|convertable| convertable.convert_ser())
            .collect::<Vec<_>>()
    }
}

// I tried to find a way to impl for T: ConvertSer but due to opaque return types in the match statement it doesn't seem to be possible
// And none of the trait are object safe either so that's not a solution
impl ConvertSer for Option<PeerId> {
    fn convert_ser(&self) -> impl Serialize {
        match self {
            Some(peer_id) => peer_id.to_base58(),
            None => String::from("None"),
        }
    }
}

impl ConvertSer for Option<BlockResponse> {
    fn convert_ser(&self) -> impl Serialize {
        match self {
            Some(block_response) => block_response.clone(),
            None => BlockResponse {
                file_hash: "None".to_string(),
                block_hash: "None".to_string(),
                block_data: vec![],
            },
        }
    }
}

impl ConvertSer for SendId {
    fn convert_ser(&self) -> impl Serialize {
        let SendId {
            peer_id,
            file_hash,
            block_hash,
        } = self;
        (peer_id.to_base58(), file_hash, block_hash)
    }
}

impl<U, V> ConvertSer for (U, V)
where
    U: ConvertSer,
    V: ConvertSer,
{
    fn convert_ser(&self) -> impl Serialize {
        let (u, v) = self;
        (u.convert_ser(), v.convert_ser())
    }
}

impl<T> IntoResponse for JsonWrapper<T>
where
    T: Serialize,
{
    fn into_response(self) -> Response {
        // Json already has impl<T> IntoResponse for Json<T> where T: Serialize
        // so we just need to extract the Json from the wrapper and use the into_response
        self.0.into_response()
    }
}
