use anyhow::Result;
use futures::stream::FusedStream;
use futures::StreamExt;
use libp2p::PeerId;
use std::pin::Pin;

pub(crate) trait SendStrategy {
    type PeerInput;
    type BlockInput;

    fn choose_next_peer_block(
        &mut self,
        peer_input: Option<Self::PeerInput>,
        block_input: Self::BlockInput,
    ) -> Result<SendId>;

    fn get_send_stream<U, V>(
        mut self: Box<Self>,
        mut peer_input_stream: Pin<Box<U>>,
        mut block_input_stream: Pin<Box<V>>,
    ) -> impl FusedStream<Item = SendId> + Send
    where
        U: FusedStream<Item = Self::PeerInput> + Send,
        V: FusedStream<Item = Self::BlockInput> + Send,
        Self: 'static + Send,
        Self::PeerInput: Send,
        Self::BlockInput: Send,
    {
        async_stream::stream! {
            while let Some(block_input) = block_input_stream.next().await {
                let maybe_peer = peer_input_stream.next().await;
                if let Ok(res) = self.choose_next_peer_block(maybe_peer, block_input) {
                    yield res;
                }
                else {
                    break;
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SendId {
    pub(crate) peer_id: PeerId,
    pub(crate) file_hash: String,
    pub(crate) block_hash: String,
}
