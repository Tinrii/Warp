use crate::events::{process_message_event, MessagingEvents};
use crate::registry::PeerOption;
use crate::{agent_name, Config, GroupRegistry, PeerRegistry};
use anyhow::anyhow;
use libp2p::{
    self, autonat,
    dcutr::behaviour::{Behaviour as DcutrBehaviour, Event as DcutrEvent},
    gossipsub::{
        Gossipsub, GossipsubConfigBuilder, GossipsubEvent, IdentTopic as Topic,
        MessageAuthenticity, ValidationMode,
    },
    identify::{Identify, IdentifyConfig, IdentifyEvent, IdentifyInfo},
    identity::Keypair,
    kad::{store::MemoryStore, Kademlia, KademliaConfig, KademliaEvent, QueryResult},
    mdns::{Mdns, MdnsConfig, MdnsEvent},
    ping::{self, Ping, PingEvent},
    relay::v2::{
        client::{self, Client as RelayClient, Event as RelayClientEvent},
        relay::{Event as RelayServerEvent, Relay as RelayServer},
    },
    swarm::{behaviour::toggle::Toggle, NetworkBehaviour, Swarm, SwarmEvent},
    tokio_development_transport, Multiaddr, NetworkBehaviour, PeerId, Transport,
};
use log::{error, info};
use std::time::Duration;
use tokio::sync::mpsc::Sender;
use warp::{
    error::Error,
    multipass::MultiPass,
    raygun::Message,
    sync::{Arc, Mutex},
};

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "BehaviourEvent", event_process = false)]
pub struct RayGunBehavior {
    pub gossipsub: Gossipsub,
    pub mdns: Toggle<Mdns>,
    pub ping: Ping,
    pub dcutr: Toggle<DcutrBehaviour>,
    pub relay_server: Toggle<RelayServer>,
    pub relay_client: Toggle<RelayClient>,
    pub kademlia: Kademlia<MemoryStore>,
    pub identity: Identify,
    pub autonat: autonat::Behaviour,
    #[behaviour(ignore)]
    pub inner: Arc<Mutex<Vec<Message>>>,
    #[behaviour(ignore)]
    pub account: Arc<Mutex<Box<dyn MultiPass>>>,
    #[behaviour(ignore)]
    pub peer_registry: PeerRegistry,
    #[behaviour(ignore)]
    pub group_registry: GroupRegistry,
}

pub enum BehaviourEvent {
    Gossipsub(GossipsubEvent),
    Mdns(MdnsEvent),
    Ping(PingEvent),
    Dcutr(DcutrEvent),
    RelayServer(RelayServerEvent),
    RelayClient(RelayClientEvent),
    Kad(KademliaEvent),
    Identify(IdentifyEvent),
    Autonat(autonat::Event),
}

impl From<GossipsubEvent> for BehaviourEvent {
    fn from(event: GossipsubEvent) -> Self {
        BehaviourEvent::Gossipsub(event)
    }
}

impl From<MdnsEvent> for BehaviourEvent {
    fn from(event: MdnsEvent) -> Self {
        BehaviourEvent::Mdns(event)
    }
}

impl From<PingEvent> for BehaviourEvent {
    fn from(event: PingEvent) -> Self {
        BehaviourEvent::Ping(event)
    }
}

impl From<DcutrEvent> for BehaviourEvent {
    fn from(event: DcutrEvent) -> Self {
        BehaviourEvent::Dcutr(event)
    }
}

impl From<RelayServerEvent> for BehaviourEvent {
    fn from(event: RelayServerEvent) -> Self {
        BehaviourEvent::RelayServer(event)
    }
}

impl From<RelayClientEvent> for BehaviourEvent {
    fn from(event: RelayClientEvent) -> Self {
        BehaviourEvent::RelayClient(event)
    }
}

impl From<KademliaEvent> for BehaviourEvent {
    fn from(event: KademliaEvent) -> Self {
        BehaviourEvent::Kad(event)
    }
}

impl From<IdentifyEvent> for BehaviourEvent {
    fn from(event: IdentifyEvent) -> Self {
        BehaviourEvent::Identify(event)
    }
}

impl From<autonat::Event> for BehaviourEvent {
    fn from(event: autonat::Event) -> Self {
        BehaviourEvent::Autonat(event)
    }
}

pub async fn swarm_loop<E>(
    swarm: &mut Swarm<RayGunBehavior>,
    event: SwarmEvent<BehaviourEvent, E>,
) {
    match event {
        SwarmEvent::Behaviour(BehaviourEvent::RelayServer(event)) => {
            info!("{:?}", event);
        }
        SwarmEvent::Behaviour(BehaviourEvent::RelayClient(
            RelayClientEvent::ReservationReqAccepted { .. },
        )) => {
            //TODO: Store and esstablish information regarding reservation
            info!("Relay accepted our reservation request.");
        }
        SwarmEvent::Behaviour(BehaviourEvent::RelayClient(event)) => {
            info!("{:?}", event);
        }
        SwarmEvent::Behaviour(BehaviourEvent::Gossipsub(event)) => match event {
            GossipsubEvent::Message { message, .. } => {
                if let Ok(events) = serde_json::from_slice::<MessagingEvents>(&message.data) {
                    if let Err(e) = process_message_event(swarm.behaviour().inner.clone(), &events)
                    {
                        error!("Error processing message event: {}", e);
                    }
                }
            }

            //TODO: Perform a check to see if topic is a registered group before insertion of peer
            GossipsubEvent::Subscribed { peer_id, topic } => {
                let mut group_registry = swarm.behaviour_mut().group_registry.clone();
                if !group_registry.exist(topic.to_string()) {
                    if let Err(e) = group_registry.register_group(topic.to_string()) {
                        error!("Error registering group: {}", e);
                    }
                }
                if !group_registry.exist(topic.to_string()) {
                    if let Err(e) = group_registry.insert_peer(topic.to_string(), peer_id) {
                        error!("Error inserting peer to group: {}", e);
                    }
                }
            }
            GossipsubEvent::Unsubscribed { peer_id, topic } => {
                let mut group_registry = swarm.behaviour_mut().group_registry.clone();
                if let Err(e) = group_registry.remove_peer(topic.to_string(), peer_id) {
                    error!("Error moving peer from group: {}", e);
                }
            }
            GossipsubEvent::GossipsubNotSupported { .. } => {}
        },
        SwarmEvent::Behaviour(BehaviourEvent::Mdns(event)) => match event {
            MdnsEvent::Discovered(list) => {
                for (peer, _addr) in list {
                    swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer);
                }
            }
            MdnsEvent::Expired(list) => {
                for (peer, _addr) in list {
                    if let Some(mdns) = swarm.behaviour().mdns.as_ref() {
                        if !mdns.has_node(&peer) {
                            swarm.behaviour_mut().gossipsub.remove_explicit_peer(&peer);
                        }
                    }
                }
            }
        },
        SwarmEvent::Behaviour(BehaviourEvent::Ping(_)) => {}
        SwarmEvent::Behaviour(BehaviourEvent::Kad(event)) => match event {
            KademliaEvent::OutboundQueryCompleted { result, .. } => match result {
                QueryResult::Bootstrap(_) => {}
                QueryResult::GetClosestPeers(Ok(ok)) => {
                    for peer in ok.peers {
                        let addrs = swarm.behaviour_mut().kademlia.addresses_of_peer(&peer);
                        for addr in addrs {
                            swarm.behaviour_mut().kademlia.add_address(&peer, addr);
                        }
                    }
                }
                _ => {}
            },
            KademliaEvent::RoutingUpdated {
                peer: _,
                addresses: _,
                ..
            } => {}
            _ => {}
        },
        SwarmEvent::Behaviour(BehaviourEvent::Identify(event)) => {
            if let IdentifyEvent::Received {
                peer_id,
                info:
                    IdentifyInfo {
                        listen_addrs,
                        protocols,
                        agent_version,
                        public_key,
                        ..
                    },
            } = event
            {
                if agent_version.eq(&agent_name()) {
                    let mut registry = swarm.behaviour_mut().peer_registry.clone();
                    //TODO: Test to make sure a deadlock doesnt occur due to internal mutex
                    let mut exist = false;
                    if !registry.exist(PeerOption::PublicKey(public_key.clone())) {
                        exist = true;
                    }
                    if exist {
                        registry.add_public_key(public_key);
                    }
                }
                if protocols
                    .iter()
                    .any(|p| p.as_bytes() == libp2p::kad::protocol::DEFAULT_PROTO_NAME)
                {
                    for addr in listen_addrs {
                        swarm.behaviour_mut().kademlia.add_address(&peer_id, addr);
                    }
                }
            }
        }
        SwarmEvent::Behaviour(BehaviourEvent::Autonat(_)) => {}
        SwarmEvent::Behaviour(BehaviourEvent::Dcutr(_)) => {}
        SwarmEvent::ConnectionEstablished { .. } => {}
        SwarmEvent::ConnectionClosed { .. } => {}
        SwarmEvent::IncomingConnection { .. } => {}
        SwarmEvent::IncomingConnectionError { .. } => {}
        SwarmEvent::OutgoingConnectionError { .. } => {}
        SwarmEvent::BannedPeer { .. } => {}
        SwarmEvent::NewListenAddr { address, .. } => {
            info!("Listening on {}", address);
        }
        SwarmEvent::ExpiredListenAddr { .. } => {}
        SwarmEvent::ListenerClosed { .. } => {}
        SwarmEvent::ListenerError { .. } => {}
        SwarmEvent::Dialing(peer) => {
            info!("Dialing {}", peer);
        }
    }
}

pub enum SwarmCommands {
    DialPeer(PeerId),
    DialAddr(Multiaddr),
    BanPeer(PeerId),
    UnbanPeer(PeerId),
    DisconnectPeer(PeerId),
    SubscribeToTopic(Topic),
    UnsubscribeFromTopic(Topic),
    PublishToTopic(Topic, Vec<u8>),
    FindPeer(PeerId),
}

pub fn swarm_command(
    swarm: &mut Swarm<RayGunBehavior>,
    commands: Option<SwarmCommands>,
) -> anyhow::Result<()> {
    match commands {
        Some(SwarmCommands::DialPeer(peer)) => swarm.dial(peer)?,
        Some(SwarmCommands::DialAddr(addr)) => swarm.dial(addr)?,
        Some(SwarmCommands::BanPeer(peer)) => swarm.ban_peer_id(peer),
        Some(SwarmCommands::UnbanPeer(peer)) => swarm.unban_peer_id(peer),
        Some(SwarmCommands::DisconnectPeer(peer)) => {
            swarm.disconnect_peer_id(peer).map_err(|_| Error::Other)?;
        }
        Some(SwarmCommands::SubscribeToTopic(topic)) => {
            swarm.behaviour_mut().gossipsub.subscribe(&topic)?;
        }
        Some(SwarmCommands::UnsubscribeFromTopic(topic)) => {
            swarm.behaviour_mut().gossipsub.unsubscribe(&topic)?;
        }
        Some(SwarmCommands::PublishToTopic(topic, data)) => {
            swarm.behaviour_mut().gossipsub.publish(topic, data)?;
        }
        Some(SwarmCommands::FindPeer(peer)) => {
            swarm.behaviour_mut().kademlia.get_closest_peers(peer);
        }
        _ => {} //TODO: Invalid command?
    }
    Ok(())
}

pub async fn swarm_events(
    swarm: &mut Swarm<RayGunBehavior>,
    event: Option<MessagingEvents>,
    tx: Sender<Result<(), Error>>,
) -> anyhow::Result<()> {
    if let Some(event) = event {
        let topic = match &event {
            MessagingEvents::NewMessage(message) => message.conversation_id(),
            MessagingEvents::EditMessage(id, _, _) => *id,
            MessagingEvents::DeleteMessage(id, _) => *id,
            MessagingEvents::PinMessage(id, _, _, _) => *id,
            MessagingEvents::DeleteConversation(id) => *id,
            MessagingEvents::ReactMessage(id, _, _, _, _) => *id,
            MessagingEvents::Ping(id, _) => *id,
        };

        //TODO: Encrypt the bytes of data with a shared key between two (or more?) peers
        match serde_json::to_vec(&event) {
            Ok(bytes) => {
                if let Err(e) = swarm_command(
                    swarm,
                    Some(SwarmCommands::SubscribeToTopic(Topic::new(
                        topic.to_string(),
                    ))),
                ) {
                    if let Err(e) = tx.send(Err(Error::Any(e))).await {
                        error!("{}", e);
                    }
                }
                if let Err(e) = swarm_command(
                    swarm,
                    Some(SwarmCommands::PublishToTopic(
                        Topic::new(topic.to_string()),
                        bytes,
                    )),
                ) {
                    if let Err(e) = tx.send(Err(Error::Any(e))).await {
                        error!("{}", e);
                    }
                }

                if let Err(e) = tx.send(Ok(())).await {
                    error!("{}", e);
                }
            }
            Err(e) => {
                if let Err(e) = tx.send(Err(Error::from(e))).await {
                    error!("{}", e);
                }
            }
        }
    }
    Ok(())
}

pub async fn create_behaviour(
    keypair: Keypair,
    conversation: Arc<Mutex<Vec<Message>>>,
    account: Arc<Mutex<Box<dyn MultiPass>>>,
    peer_registry: PeerRegistry,
    group_registry: GroupRegistry,
    config: &Config,
) -> anyhow::Result<Swarm<RayGunBehavior>> {
    let config = config.clone();
    let pubkey = keypair.public();

    let peer = PeerId::from(keypair.public());

    let gossipsub = {
        let gossipsub_config = GossipsubConfigBuilder::default()
            .validation_mode(ValidationMode::Strict)
            .build()
            .map_err(|e| anyhow!(e))?;

        Gossipsub::new(
            MessageAuthenticity::Signed(keypair.clone()),
            gossipsub_config,
        )
        .map_err(|e| anyhow!(e))?
    };

    let mdns = match config.behaviour.mdns.enable {
        true => {
            let mut mdns_config = MdnsConfig::default();
            mdns_config.enable_ipv6 = config.behaviour.mdns.enable_ipv6;
            Mdns::new(mdns_config).await.ok()
        }
        false => None,
    }
    .into();

    let mut kad_config = KademliaConfig::default();
    kad_config
        .set_query_timeout(Duration::from_secs(5 * 60))
        .set_connection_idle_timeout(Duration::from_secs(5 * 60))
        .set_provider_publication_interval(Some(Duration::from_secs(60)));

    let relay_server = match config.behaviour.relay_server.enable {
        true => Some(RelayServer::new(peer, Default::default())),
        false => None,
    }
    .into();

    let (relay_transport, relay_client): (
        Option<client::transport::ClientTransport>,
        Toggle<RelayClient>,
    ) = match config.behaviour.relay_client.enable {
        true => {
            let (transport, client) = RelayClient::new_transport_and_behaviour(peer);
            (Some(transport), Some(client).into())
        }
        false => (None, None.into()),
    };

    let dcutr = match config.behaviour.dcutr.enable {
        true => Some(DcutrBehaviour::new()),
        false => None,
    }
    .into();

    let ping = Ping::new(ping::Config::new().with_keep_alive(true));
    let kademlia = Kademlia::with_config(peer, MemoryStore::new(peer), kad_config);
    let identity = Identify::new(
        IdentifyConfig::new("/ipfs/0.1.0".into(), pubkey).with_agent_version(agent_name()),
    );
    let autonat = autonat::Behaviour::new(peer, Default::default());
    let inner = conversation;

    let relay_client_enabled = relay_client.is_enabled();

    let behaviour = RayGunBehavior {
        gossipsub,
        mdns,
        ping,
        kademlia,
        inner,
        dcutr,
        account,
        relay_server,
        relay_client,
        identity,
        autonat,
        peer_registry,
        group_registry,
    };

    let transport = transport(keypair, relay_transport)?;

    let swarm = libp2p::swarm::SwarmBuilder::new(transport, behaviour, peer)
        .executor(Box::new(|fut| {
            tokio::spawn(fut);
        }))
        .build();

    if relay_client_enabled {}

    Ok(swarm)
}

pub fn transport(
    keypair: Keypair,
    relay_transport: Option<client::transport::ClientTransport>,
) -> std::io::Result<libp2p::core::transport::Boxed<(PeerId, libp2p::core::muxing::StreamMuxerBox)>>
{
    match relay_transport {
        None => tokio_development_transport(keypair),
        Some(relay_transport) => {
            let dns_tcp = libp2p::dns::TokioDnsConfig::system(
                libp2p::tcp::TokioTcpConfig::new().nodelay(true),
            )?;
            let ws_dns_tcp = libp2p::websocket::WsConfig::new(libp2p::dns::TokioDnsConfig::system(
                libp2p::tcp::TokioTcpConfig::new().nodelay(true),
            )?);

            let transport = relay_transport.or_transport(dns_tcp.or_transport(ws_dns_tcp));

            let noise_keys = libp2p::noise::Keypair::<libp2p::noise::X25519Spec>::new()
                .into_authentic(&keypair)
                .expect("Signing libp2p-noise static DH keypair failed.");

            Ok(transport
                .upgrade(libp2p::core::upgrade::Version::V1)
                .authenticate(libp2p::noise::NoiseConfig::xx(noise_keys).into_authenticated())
                .multiplex(libp2p::core::upgrade::SelectUpgrade::new(
                    libp2p::yamux::YamuxConfig::default(),
                    libp2p::mplex::MplexConfig::default(),
                ))
                .timeout(std::time::Duration::from_secs(20))
                .boxed())
        }
    }
}