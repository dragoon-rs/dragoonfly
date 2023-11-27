use std::collections::VecDeque;
use std::task::{Context, Poll};
use libp2p::identity::PublicKey;
use libp2p::{Multiaddr, PeerId, StreamProtocol};
use libp2p::core::Endpoint;
use libp2p::core::upgrade::ReadyUpgrade;
use libp2p::swarm::{ConnectionDenied, ConnectionHandler, ConnectionHandlerEvent, ConnectionId, FromSwarm, NetworkBehaviour, SubstreamProtocol, THandler, THandlerInEvent, THandlerOutEvent, ToSwarm};
use libp2p::swarm::handler::ConnectionEvent;

#[derive(Debug)]
pub struct Event {
    pub peer: PeerId,
}

pub struct Handler {
    remote_peer_id: PeerId,
    events: VecDeque<ConnectionHandlerEvent<ReadyUpgrade<StreamProtocol>, (), Event>>
}

impl Handler {
    pub fn new(remote_peer_id: PeerId) -> Self {
        Self {
            remote_peer_id,
            events: VecDeque::new()
        }
    }
}

impl ConnectionHandler for Handler {
    type FromBehaviour = ();
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

    #[tracing::instrument(level = "trace", name = "ConnectionHandler::poll", skip(self, cx))]
    fn poll(
        &mut self,
        cx: &mut Context<'_>
    ) -> Poll<ConnectionHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::ToBehaviour>> {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(event);
        }
        return Poll::Pending;
    }

    fn on_behaviour_event(&mut self, _event: Self::FromBehaviour) {
        todo!()
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

            }
            ConnectionEvent::FullyNegotiatedOutbound(protocol,..) => {

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
    pub_key: PublicKey,
    events: VecDeque<Event>,
}

impl Behaviour {
    pub fn new(protocol_version: String, local_public_key: PublicKey) -> Self {
        Self {
            protocol_version,
            pub_key: local_public_key,
            events: Default::default(),
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
    }

    fn on_connection_handler_event(&mut self, peer: PeerId, _connection_id: ConnectionId, _event: THandlerOutEvent<Self>) {
        println!("Behaviour::on_connection_handler_event, push Event");
        self.events.push_front(Event {
            peer
        })
    }

    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(ToSwarm::GenerateEvent(event));
        }

        Poll::Pending
    }
}

pub(crate) async fn send_dragoon_data()