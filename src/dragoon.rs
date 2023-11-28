use std::collections::{HashMap, HashSet, VecDeque};
use std::{fmt, io};
use std::error::Error;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, future};
use futures::future::{BoxFuture, Either};
use libp2p::identity::PublicKey;
use libp2p::{Multiaddr, PeerId, Stream, StreamProtocol};
use libp2p::core::Endpoint;
use libp2p::core::upgrade::ReadyUpgrade;
use libp2p::swarm::{ConnectionClosed, ConnectionDenied, ConnectionHandler, ConnectionHandlerEvent, ConnectionId, dial_opts, DialError, DialFailure, FromSwarm, ListenAddresses, NetworkBehaviour, NotifyHandler, SubstreamProtocol, THandler, THandlerInEvent, THandlerOutEvent, ToSwarm};
use libp2p::swarm::behaviour::ConnectionEstablished;
use libp2p::swarm::handler::{ConnectionEvent, FullyNegotiatedInbound, FullyNegotiatedOutbound};
use futures_timer::Delay;

#[derive(Debug)]
pub enum InEvent {
    Send(Vec<u8>)
}

#[derive(Debug)]
pub enum Event {
    Sent {
        peer: PeerId,
    }
}


#[derive(Debug)]
pub enum Failure {
    /// The ping timed out, i.e. no response was received within the
    /// configured ping timeout.
    Timeout,
    /// The peer does not support the ping protocol.
    Unsupported,
    /// The ping failed for reasons other than a timeout.
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
            Failure::Timeout => f.write_str("Ping timeout"),
            Failure::Other { error } => write!(f, "Ping error: {error}"),
            Failure::Unsupported => write!(f, "Ping protocol not supported"),
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

pub struct Handler {
    remote_peer_id: PeerId,
    events: VecDeque<ConnectionHandlerEvent<ReadyUpgrade<StreamProtocol>, (), Event>>,

}

impl Handler {
    pub fn new(remote_peer_id: PeerId) -> Self {
        Self {
            remote_peer_id,
            events: VecDeque::new()
        }
    }

    fn on_fully_negotiated_inbound(
        &mut self,
        FullyNegotiatedInbound {
            protocol: output, ..
        }:FullyNegotiatedInbound<
            <Self as ConnectionHandler>::InboundProtocol,
            <Self as ConnectionHandler>::InboundOpenInfo,
        >,
    ) {
       // TODO:  dragoon_send(output, &[1, 2, 3, 4], Duration::new(10,0)).await
       //     .expect("TODO: panic message");
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

    }
}


impl ConnectionHandler for Handler {
    type FromBehaviour = InEvent;
    type ToBehaviour = Event;
    type InboundProtocol = ReadyUpgrade<StreamProtocol>;
    type OutboundProtocol = ReadyUpgrade<StreamProtocol>;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(
            ReadyUpgrade::new(StreamProtocol::new("/dragoon/1.0.0")), ()
        )
    }

    #[tracing::instrument(level = "trace", name = "ConnectionHandler::poll", skip(self, _cx))]
    fn poll(
        &mut self,
        _cx: &mut Context<'_>
    ) -> Poll<ConnectionHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::ToBehaviour>> {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(event);
        }
        return Poll::Pending;
    }

    fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
        match event {
            InEvent::Send(data) => {
                println!("send data for {}", self.remote_peer_id);
                self.events.push_back(ConnectionHandlerEvent::OutboundSubstreamRequest {
                    protocol: SubstreamProtocol::new(
                        ReadyUpgrade::new(StreamProtocol::new("/dragoon/1.0.0")),
                        ()
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
            Self::OutboundOpenInfo
        >) {
        println!("Handler::on_connection_event: {event:?}");
        match event {
            ConnectionEvent::FullyNegotiatedInbound(protocol, ..) => {
                self.on_fully_negotiated_inbound(protocol)
            }
            ConnectionEvent::FullyNegotiatedOutbound(protocol,..) => {
                self.on_fully_negotiated_outbound(protocol)
            }
            ConnectionEvent::LocalProtocolsChange(_) => {
                self.events
                    .push_back(ConnectionHandlerEvent::OutboundSubstreamRequest {
                        protocol: SubstreamProtocol::new(
                            ReadyUpgrade::new(StreamProtocol::new("/dragoon/1.0.0")),
                            (),
                        ),
                    });
            }
            ConnectionEvent::DialUpgradeError(_) => {}
            ConnectionEvent::ListenUpgradeError(_) => {}
            ConnectionEvent::RemoteProtocolsChange(_) => {}
            _ => {}
        }
    }
}

pub struct Behaviour {
    protocol_version: String,
    connected_peers: HashSet<PeerId>,
    listen_addresses: ListenAddresses,
    connections: HashMap<ConnectionId, PeerId>,
    handlers: HashMap<PeerId,Handler>,
    events: VecDeque<ToSwarm<Event, InEvent>>,
}

impl Behaviour {
    pub fn new(protocol_version: String, local_public_key: PublicKey) -> Self {
        Self {
            protocol_version,
            connected_peers: HashSet::new(),
            listen_addresses: Default::default(),
            connections: HashMap::new(),
            handlers: HashMap::new(),
            events: Default::default(),
        }
    }

    fn on_connection_established(
        &mut self,
        ConnectionEstablished {
            peer_id,
            failed_addresses,
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
        let Some(peer_id) = peer_id else { return };

        match error {
            DialError::LocalPeerId { .. }
            | DialError::WrongPeerId { .. }
            | DialError::Aborted
            | DialError::Denied { .. }
            | DialError::Transport(_)
            | DialError::NoAddresses => {
                if let DialError::Transport(addresses) = error {
                    for (addr, _) in addresses {
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

    pub fn get_connected_peer(&self) -> HashSet<PeerId> {
        self.connected_peers.clone()
    }

    pub fn send_data_to_peer(&mut self, peer: PeerId) {
        if self.connected_peers.contains(&peer) {
            self.events.push_back(ToSwarm::NotifyHandler {
                peer_id: peer,
                handler: NotifyHandler::Any,
                event: InEvent::Send(vec![1,2,3,4]),
            });
        }
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = Handler;
    type ToSwarm = Event;

    fn handle_established_inbound_connection(&mut self, _connection_id: ConnectionId, peer: PeerId, local_addr: &Multiaddr, remote_addr: &Multiaddr) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(Handler::new(peer))
    }

    fn handle_established_outbound_connection(&mut self, _connection_id: ConnectionId, peer: PeerId, addr: &Multiaddr, role_override: Endpoint) -> Result<THandler<Self>, ConnectionDenied> {
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

    fn on_connection_handler_event(&mut self, peer: PeerId, _connection_id: ConnectionId, _event: THandlerOutEvent<Self>) {
        println!("Behaviour::on_connection_handler_event, push Event");
    }

    #[tracing::instrument(level = "trace", name = "NetworkBehaviour::poll", skip(self))]
    fn poll(&mut self, _: &mut Context<'_>) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(event);
        }

        Poll::Pending
    }
}

async fn dragoon_send(stream: Stream, data: &[u8], timeout: Duration) -> Result<(Stream, Duration), Failure> {
    let req = dragoon_send_data(stream, data);
    futures::pin_mut!(req);

    match future::select(req, Delay::new(timeout)).await {
        Either::Left((Ok((stream, rtt)), _)) => Ok((stream, rtt)),
        Either::Left((Err(e), _)) => Err(Failure::other(e)),
        Either::Right(((), _)) => Err(Failure::Timeout),
    }
}

pub(crate) async fn dragoon_send_data<S>(mut stream: S, data: &[u8]) -> io::Result<(S, Duration)>
    where
        S: AsyncRead + AsyncWrite + Unpin,
{
    stream.write_all(data).await?;
    stream.flush().await?;
    let mut recv_payload = [0u8; 4];
    stream.read_exact(&mut recv_payload).await?;
    let started = Instant::now();
    if recv_payload == [42,42,42,42] {
        Ok((stream, started.elapsed()))
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Dragoon payload mismatch",
        ))
    }
}

pub(crate) async fn dragoon_receive_data<S>(mut stream: S) -> io::Result<S>
    where
        S: AsyncRead + AsyncWrite + Unpin,
{
    let mut payload = [0u8; 4];
    stream.read_exact(&mut payload).await?;
    let mut response = [42,42,42,42];
    stream.write_all(&response).await?;
    stream.flush().await?;
    Ok(stream)
}