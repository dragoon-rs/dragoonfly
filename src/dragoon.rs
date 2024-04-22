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
use std::error::Error;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use std::{fmt, io};

#[derive(Debug)]
pub(crate) enum InEvent {
    Send(Vec<u8>),
}

#[derive(Debug)]
pub enum DragoonEvent {
    Sent { peer: PeerId },
    Received { shard: Shard },
}

#[derive(Debug)]
pub(crate) enum Failure {
    Timeout,
    Unsupported,
    Other {
        error: Box<dyn std::error::Error + Send + 'static>,
    },
}

impl Failure {
    fn other(e: impl std::error::Error + Send + 'static) -> Self {
        Self::Other { error: Box::new(e) }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::Timeout => f.write_str("Protocol timeout"),
            Failure::Other { error } => write!(f, "Protocol error: {error}"),
            Failure::Unsupported => write!(f, "Protocol not supported"),
        }
    }
}

impl Error for Failure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Failure::Timeout => None,
            Failure::Other { error } => Some(&**error),
            Failure::Unsupported => None,
        }
    }
}

type DragoonSendFuture = BoxFuture<'static, Result<(Stream, Duration), Failure>>;
type DragoonRecvFuture = BoxFuture<'static, Result<Shard, io::Error>>;

pub(crate) struct Handler {
    remote_peer_id: PeerId,
    events: VecDeque<ConnectionHandlerEvent<ReadyUpgrade<StreamProtocol>, (), DragoonEvent>>,
    network_send_task: Vec<DragoonSendFuture>,
    network_recv_task: Vec<DragoonRecvFuture>,
    data_to_send: VecDeque<Vec<u8>>,
}

impl Handler {
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
        if let Some(data) = self.data_to_send.pop_front() {
            self.network_send_task
                .push(Box::pin(dragoon_send(output, data, Duration::new(10, 0))));
        }
    }
}

impl ConnectionHandler for Handler {
    type FromBehaviour = InEvent;
    type ToBehaviour = DragoonEvent;
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
            self.network_send_task.remove(id);
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
            self.network_recv_task.remove(id);
        }

        if let Some(s) = shard_received {
            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(
                DragoonEvent::Received { shard: s },
            ));
        }
        return Poll::Pending;
    }

    fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
        match event {
            InEvent::Send(data) => {
                println!("send data for {}", self.remote_peer_id);
                self.data_to_send.push_back(data);
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

pub(crate) struct Behaviour {
    connected_peers: HashSet<PeerId>,
    listen_addresses: ListenAddresses,
    connections: HashMap<ConnectionId, PeerId>,
    events: VecDeque<ToSwarm<DragoonEvent, InEvent>>,
}

impl Behaviour {
    pub(crate) fn new(protocol_version: String) -> Self {
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

    pub(crate) fn send_data_to_peer(&mut self, data: String, peer: PeerId) -> bool {
        if self.connected_peers.contains(&peer) {
            self.events.push_back(ToSwarm::NotifyHandler {
                peer_id: peer,
                handler: NotifyHandler::Any,
                event: InEvent::Send(data.into_bytes()),
            });
            return true;
        } else {
            return false;
        }
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = Handler;
    type ToSwarm = DragoonEvent;

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
        peer: PeerId,
        connection_id: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        println!("Behaviour::on_connection_handler_event, push Event {event:?}");
        match event {
            DragoonEvent::Sent { peer } => {
                self.events.push_front(ToSwarm::GenerateEvent {
                    0: DragoonEvent::Sent { peer },
                });
            }
            DragoonEvent::Received { shard } => {
                self.events.push_front(ToSwarm::GenerateEvent {
                    0: DragoonEvent::Received { shard },
                });
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

async fn dragoon_send(
    stream: Stream,
    data: Vec<u8>,
    timeout: Duration,
) -> Result<(Stream, Duration), Failure> {
    println!("dragoon_send {stream:?}");

    let req = dragoon_send_data(stream, data);
    futures::pin_mut!(req);

    match future::select(req, Delay::new(timeout)).await {
        Either::Left((Ok((stream, rtt)), _)) => Ok((stream, rtt)),
        Either::Left((Err(e), _)) => Err(Failure::other(e)),
        Either::Right(((), _)) => Err(Failure::Timeout),
    }
}

pub(crate) async fn dragoon_send_data<S>(mut stream: S, data: Vec<u8>) -> io::Result<(S, Duration)>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    stream.write_all(data.as_slice()).await?;
    stream.flush().await?;
    let mut recv_payload = [0u8; 4];
    stream.read_exact(&mut recv_payload).await?;
    println!("response payload: {recv_payload:?}");
    let started = Instant::now();
    if recv_payload == [42, 42, 42, 42] {
        Ok((stream, started.elapsed()))
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Dragoon payload mismatch",
        ))
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

#[derive(Debug)]
pub struct Shard {
    pub hash: Vec<u8>,
    pub shard: Vec<u8>,
    pub commit: Vec<u8>,
}

pub(crate) async fn dragoon_receive_data<S>(mut stream: S) -> io::Result<Shard>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    println!("start receive data");
    let mut payload = [0u8; 64];
    stream.read(&mut payload).await?;
    let str = String::from_utf8(payload.to_vec()).unwrap();
    println!("received payload: {:?}", str);
    let response = [42, 42, 42, 42];
    stream.write_all(&response).await?;
    stream.flush().await?;
    let shard = Shard {
        hash: vec![1, 2, 3, 4],
        shard: vec![5, 6, 7, 8, 9],
        commit: vec![0, 1],
    };
    Ok(shard)
}
