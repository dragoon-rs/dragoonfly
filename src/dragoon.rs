use futures::future::{BoxFuture, Either};
use futures::{future, ready, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use futures_timer::Delay;
use libp2p::core::upgrade::ReadyUpgrade;
use libp2p::core::Endpoint;
use libp2p::swarm::behaviour::ConnectionEstablished;
use libp2p::swarm::handler::{ConnectionEvent, FullyNegotiatedInbound, FullyNegotiatedOutbound};
use libp2p::swarm::{
    dial_opts, ConnectionClosed, ConnectionDenied, ConnectionHandler, ConnectionHandlerEvent,
    ConnectionId, DialError, DialFailure, FromSwarm, ListenAddresses, NetworkBehaviour,
    NotifyHandler, SubstreamProtocol, THandler, THandlerInEvent, THandlerOutEvent, ToSwarm,
};
use libp2p::{Multiaddr, PeerId, Stream, StreamProtocol};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use ark_ec::CurveGroup;
use ark_ff::PrimeField;
use ark_serialize::{CanonicalSerialize, Compress, Validate};
use komodo::{fs, Block};

use anyhow::Result;

use std::path::PathBuf;
use tracing::info;

use crate::error::DragoonError::Timeout;

#[derive(Debug)]
pub(crate) enum InEvent {
    Send((String, String)), // the hash of the block
}

#[derive(Debug)]
pub enum DragoonEvent<F, G>
where
    F: PrimeField,
    G: CurveGroup<ScalarField = F>,
{
    Sent { peer: PeerId },
    Received { block: Block<F, G> },
}

type DragoonSendFuture = BoxFuture<'static, Result<(Stream, Duration)>>;
type DragoonRecvFuture<F, G> = BoxFuture<'static, Result<Block<F, G>, io::Error>>;

pub(crate) struct Handler<F, G>
where
    F: PrimeField,
    G: CurveGroup<ScalarField = F>,
{
    remote_peer_id: PeerId,
    events: VecDeque<ConnectionHandlerEvent<ReadyUpgrade<StreamProtocol>, (), DragoonEvent<F, G>>>,
    network_send_task: Vec<DragoonSendFuture>,
    network_recv_task: Vec<DragoonRecvFuture<F, G>>,
    data_to_send: VecDeque<(String, String)>, // the hash of the data
}

impl<F, G> Handler<F, G>
where
    F: PrimeField,
    G: CurveGroup<ScalarField = F>,
{
    pub(crate) fn new(remote_peer_id: PeerId) -> Self {
        Self {
            remote_peer_id,
            events: VecDeque::new(),
            network_send_task: Vec::new(),
            network_recv_task: Vec::new(),
            data_to_send: VecDeque::new(),
        }
    }

    fn on_fully_negotiated_inbound(
        &mut self,
        FullyNegotiatedInbound {
            protocol: output, ..
        }: FullyNegotiatedInbound<
            <Self as ConnectionHandler>::InboundProtocol,
            <Self as ConnectionHandler>::InboundOpenInfo,
        >,
    ) {
        self.network_recv_task
            .push(Box::pin(dragoon_receive_data(output)));
    }

    fn on_fully_negotiated_outbound(
        &mut self,
        FullyNegotiatedOutbound {
            protocol: output, ..
        }: FullyNegotiatedOutbound<
            <Self as ConnectionHandler>::OutboundProtocol,
            <Self as ConnectionHandler>::OutboundOpenInfo,
        >,
    ) {
        if let Some((block_hash, block_dir)) = self.data_to_send.pop_front() {
            self.network_send_task.push(Box::pin(dragoon_send::<F, G>(
                output,
                block_hash,
                block_dir,
                Duration::new(10, 0),
            )));
        }
    }
}

impl<F, G> ConnectionHandler for Handler<F, G>
where
    F: PrimeField,
    G: CurveGroup<ScalarField = F>,
{
    type FromBehaviour = InEvent;
    type ToBehaviour = DragoonEvent<F, G>;
    type InboundProtocol = ReadyUpgrade<StreamProtocol>;
    type OutboundProtocol = ReadyUpgrade<StreamProtocol>;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(ReadyUpgrade::new(StreamProtocol::new("/dragoon/1.0.0")), ())
    }

    #[tracing::instrument(level = "trace", name = "ConnectionHandler::poll", skip(self, cx))]
    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::ToBehaviour>,
    > {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(event);
        }
        let mut to_remove = Vec::new();
        let mut shard_sent = None;
        for (id, fut) in &mut self.network_send_task.iter_mut().enumerate() {
            match ready!(fut.as_mut().poll(cx)) {
                Ok(o) => {
                    println!("send task finished {o:?}");
                    to_remove.push(id);
                    shard_sent = Some(self.remote_peer_id);
                    break;
                }
                Err(e) => {
                    println!("send task error : {e}")
                }
            }
        }
        for &id in to_remove.iter().rev() {
            // ! drop the pinned future from memory as we don't need it anymore
            std::mem::drop(self.network_send_task.remove(id));
        }
        if let Some(peer) = shard_sent {
            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                DragoonEvent::Sent { peer },
            ));
        }

        to_remove.clear();
        let mut shard_received = None;
        for (id, fut) in &mut self.network_recv_task.iter_mut().enumerate() {
            match ready!(fut.as_mut().poll(cx)) {
                Ok(o) => {
                    println!("recvtask finished {o:?}");
                    to_remove.push(id);
                    shard_received = Some(o);
                    break;
                }
                Err(e) => {
                    println!("recvtask error {e}");
                }
            }
        }
        for &id in to_remove.iter().rev() {
            // ! drop the pinned future from memory as we don't need it anymore
            std::mem::drop(self.network_recv_task.remove(id));
        }

        if let Some(b) = shard_received {
            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                DragoonEvent::Received { block: b },
            ));
        }
        return Poll::Pending;
    }

    fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
        match event {
            InEvent::Send((data, data_path)) => {
                info!("send data for {}", self.remote_peer_id);
                self.data_to_send.push_back((data, data_path));
                self.events
                    .push_back(ConnectionHandlerEvent::OutboundSubstreamRequest {
                        protocol: SubstreamProtocol::new(
                            ReadyUpgrade::new(StreamProtocol::new("/dragoon/1.0.0")),
                            (),
                        ),
                    });
            }
        }
    }

    fn on_connection_event(
        &mut self,
        event: ConnectionEvent<
            Self::InboundProtocol,
            Self::OutboundProtocol,
            Self::InboundOpenInfo,
            Self::OutboundOpenInfo,
        >,
    ) {
        println!("Handler::on_connection_event: {event:?}");
        match event {
            ConnectionEvent::FullyNegotiatedInbound(protocol, ..) => {
                self.on_fully_negotiated_inbound(protocol)
            }
            ConnectionEvent::FullyNegotiatedOutbound(protocol, ..) => {
                self.on_fully_negotiated_outbound(protocol)
            }
            ConnectionEvent::LocalProtocolsChange(_) => {
                // self.events
                //     .push_back(ConnectionHandlerEvent::OutboundSubstreamRequest {
                //         protocol: SubstreamProtocol::new(
                //             ReadyUpgrade::new(StreamProtocol::new("/dragoon/1.0.0")),
                //             (),
                //         ),
                //     });
            }
            ConnectionEvent::DialUpgradeError(_) => {}
            ConnectionEvent::ListenUpgradeError(_) => {}
            ConnectionEvent::RemoteProtocolsChange(_) => {}
            _ => {}
        }
    }
}

pub(crate) struct Behaviour<F, G>
where
    F: PrimeField,
    G: CurveGroup<ScalarField = F>,
{
    connected_peers: HashSet<PeerId>,
    listen_addresses: ListenAddresses,
    connections: HashMap<ConnectionId, PeerId>,
    events: VecDeque<ToSwarm<DragoonEvent<F, G>, InEvent>>,
}

impl<F, G> Behaviour<F, G>
where
    F: PrimeField,
    G: CurveGroup<ScalarField = F>,
{
    pub(crate) fn new() -> Self {
        Self {
            connected_peers: HashSet::new(),
            listen_addresses: Default::default(),
            connections: HashMap::new(),
            events: Default::default(),
        }
    }

    fn on_connection_established(
        &mut self,
        ConnectionEstablished {
            peer_id,
            other_established,
            ..
        }: ConnectionEstablished,
    ) {
        // Peer's first connection.
        if other_established == 0 {
            tracing::info!("add peer {peer_id}");
            self.connected_peers.insert(peer_id);
        }
    }

    fn on_connection_closed(
        &mut self,
        ConnectionClosed {
            peer_id,
            remaining_established,
            connection_id,
            ..
        }: ConnectionClosed,
    ) {
        self.connections.remove(&connection_id);

        if remaining_established == 0 {
            //self.connection_updated(peer_id, None, NodeStatus::Disconnected);
            self.connected_peers.remove(&peer_id);
        }
    }

    fn on_dial_failure(&mut self, DialFailure { peer_id, error, .. }: DialFailure) {
        if peer_id.is_none() {
            return;
        }

        match error {
            DialError::LocalPeerId { .. }
            | DialError::WrongPeerId { .. }
            | DialError::Aborted
            | DialError::Denied { .. }
            | DialError::Transport(_)
            | DialError::NoAddresses => {
                if let DialError::Transport(addresses) = error {
                    for (_, _) in addresses {
                        //self.address_failed(peer_id, addr)
                    }
                }
            }
            DialError::DialPeerConditionFalse(
                dial_opts::PeerCondition::Disconnected
                | dial_opts::PeerCondition::NotDialing
                | dial_opts::PeerCondition::DisconnectedAndNotDialing,
            ) => {
                // We might (still) be connected, or about to be connected, thus do not report the
                // failure to the queries.
            }
            DialError::DialPeerConditionFalse(dial_opts::PeerCondition::Always) => {
                unreachable!("DialPeerCondition::Always can not trigger DialPeerConditionFalse.");
            }
        }
    }

    pub(crate) fn get_connected_peer(&self) -> HashSet<PeerId> {
        self.connected_peers.clone()
    }

    pub(crate) fn send_data_to_peer(
        &mut self,
        block_hash: String,
        block_path: String,
        peer: PeerId,
    ) -> bool {
        if self.connected_peers.contains(&peer) {
            self.events.push_back(ToSwarm::NotifyHandler {
                peer_id: peer,
                handler: NotifyHandler::Any,
                event: InEvent::Send((block_hash, block_path)),
            });
            true
        } else {
            false
        }
    }
}

impl<F, G> NetworkBehaviour for Behaviour<F, G>
where
    F: PrimeField,
    G: CurveGroup<ScalarField = F>,
{
    // ? should the type be generic
    type ConnectionHandler = Handler<F, G>;
    type ToSwarm = DragoonEvent<F, G>;

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        peer: PeerId,
        _local_addr: &Multiaddr,
        _remote_addr: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(Handler::new(peer))
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        peer: PeerId,
        _addr: &Multiaddr,
        _role_override: Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(Handler::new(peer))
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        println!("Behaviour::on_swarm_event: {event:?}");
        self.listen_addresses.on_swarm_event(&event);

        match event {
            FromSwarm::ConnectionEstablished(connection_established) => {
                self.on_connection_established(connection_established)
            }
            FromSwarm::ConnectionClosed(connection_closed) => {
                self.on_connection_closed(connection_closed)
            }
            FromSwarm::DialFailure(dial_failure) => self.on_dial_failure(dial_failure),
            //FromSwarm::AddressChange(address_change) => self.on_address_change(address_change),
            _ => {}
        }
    }

    fn on_connection_handler_event(
        &mut self,
        _peer: PeerId,
        _connection_id: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        println!("Behaviour::on_connection_handler_event, push Event {event:?}");
        match event {
            DragoonEvent::Sent { peer } => {
                self.events
                    .push_front(ToSwarm::GenerateEvent(DragoonEvent::Sent { peer }));
            }
            DragoonEvent::Received { block } => {
                self.events
                    .push_front(ToSwarm::GenerateEvent(DragoonEvent::Received { block }));
            }
        }
    }

    #[tracing::instrument(level = "trace", name = "NetworkBehaviour::poll", skip(self))]
    fn poll(&mut self, _: &mut Context<'_>) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        if let Some(event) = self.events.pop_front() {
            println!("NetworkBehaviour::poll event : {event:?}");
            return Poll::Ready(event);
        }

        Poll::Pending
    }
}

async fn dragoon_send<F, G>(
    stream: Stream,
    block_hash: String,
    block_dir: String,
    timeout: Duration,
) -> Result<(Stream, Duration)>
where
    F: PrimeField,
    G: CurveGroup<ScalarField = F>,
{
    println!("dragoon_send {stream:?}");

    let req = dragoon_send_data::<_, F, G>(stream, block_hash, block_dir);
    futures::pin_mut!(req);

    match future::select(req, Delay::new(timeout)).await {
        Either::Left((Ok((stream, rtt)), _)) => Ok((stream, rtt)),
        Either::Left((Err(e), _)) => Err(e),
        Either::Right(((), _)) => Err(Timeout.into()),
    }
}

pub(crate) fn read_block_from_disk<F, G>(block_hash: String, block_dir: PathBuf) -> Result<Vec<u8>>
where
    F: PrimeField,
    G: CurveGroup<ScalarField = F>,
{
    let block =
        match fs::read_blocks::<F, G>(&[block_hash], &block_dir, Compress::Yes, Validate::Yes) {
            Ok(vec) => vec[0].clone().1,
            Err(e) => return Err(e), // ? would it be better to return the error
        };
    let mut buf = vec![0; block.serialized_size(Compress::Yes)];
    block.serialize_with_mode(&mut buf[..], Compress::Yes)?;
    Ok(buf)
}

pub(crate) async fn dragoon_send_data<S, F, G>(
    mut stream: S,
    block_hash: String,
    block_dir: String,
) -> Result<(S, Duration)>
where
    S: AsyncRead + AsyncWrite + Unpin,
    F: PrimeField,
    G: CurveGroup<ScalarField = F>,
{
    let buf = read_block_from_disk::<F, G>(block_hash, PathBuf::from(block_dir))?;
    stream.write_all(&buf[..]).await?;
    stream.flush().await?;
    let mut recv_payload = [0u8; 4];
    stream.read_exact(&mut recv_payload).await?;
    println!("response payload: {recv_payload:?}");
    let started = Instant::now();
    if recv_payload == [42, 42, 42, 42] {
        Ok((stream, started.elapsed()))
    } else {
        Err(anyhow::Error::from(io::Error::new(
            io::ErrorKind::InvalidData,
            "Dragoon payload mismatch",
        )))
    }
}

// async fn dragoon_recv(mut stream: Stream) -> Result<(Stream), Failure> {
//     println!("dragoon_recv");
//     let mut payload = [0u8; 4];
//     stream.read_exact(&mut payload).await?;
//     stream.write_all(&payload).await?;
//     stream.flush().await?;
//     Ok(stream)
// }

pub(crate) async fn dragoon_receive_data<S, F, G>(mut stream: S) -> io::Result<Block<F, G>>
where
    S: AsyncRead + AsyncWrite + Unpin,
    F: PrimeField,
    G: CurveGroup<ScalarField = F>,
{
    println!("start receive data");
    const PAYLOAD_SIZE: usize = 64;
    let mut payload = [0u8; PAYLOAD_SIZE];
    stream.read_exact(&mut payload).await?;
    let str = String::from_utf8(payload.to_vec()).unwrap();
    println!("received payload: {:?}", str);
    let response = [42, 42, 42, 42];
    stream.write_all(&response).await?;
    stream.flush().await?;

    let block = Block::default();
    Ok(block)
}
