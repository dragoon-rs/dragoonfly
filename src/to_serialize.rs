use std::collections::HashSet;

use axum::response::{IntoResponse, Json, Response};
use libp2p::{swarm::NetworkInfo, Multiaddr, PeerId};
use serde::ser::Serialize;

use crate::commands::SerNetworkInfo;

// can't implement Serialize for Json as those are a external Trait and Struct, so we need a wrapper
pub(crate) struct JsonWrapper<T>(pub Json<T>);

pub(crate) trait ConvertSer {
    fn convert_ser(&self) -> impl Serialize;
}

/// Used to implement for all types that are already Serialize and only need to return self (kind of a default implementatioon basically)
/// We do not directly implement for all Serialize, because it is a foreign trait and could lead to double implementation conflicts later on if PeerID decided to implement Serialize for example
/// Macro rule taken from: https://stackoverflow.com/a/50223259
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
impl_Convert!(for u64, String, bool, &str, Vec<Multiaddr>, Vec<u8>, (String, String));

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
