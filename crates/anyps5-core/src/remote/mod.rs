//! Phone as controller: the launcher side of the protocol the AnyPS5 Remote phone app speaks.
//!
//! A phone finds the PC through mDNS, connects a WebSocket (port 47700), is paired once with a code
//! the player confirms, and then drives the launcher library and sends pad state over UDP.
//! [`RemoteServer`] is the whole interface: start it, hand it the library with [`RemoteServer::publish`],
//! and listen through [`RemoteEvents`]. Ported from utkuhalis/AnyPS5-ARM (macOS), with the security
//! rules in `docs/DESIGN.md`. Nothing here starts unless the player switched the feature on.

mod control;
mod devices;
mod discovery;
mod layouts;
mod protocol;
mod relay;
mod security;
pub mod vpad;

use std::{
    collections::BTreeMap,
    io,
    net::Ipv4Addr,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Sender, channel},
    },
    thread,
};

pub use devices::{PairedDevice, PairedDevices};
pub use discovery::{choose_addresses, lan_addresses};
pub use layouts::Layouts;
pub use protocol::{
    HELLO_SIZE, INPUT_SIZE, InputPacket, Message, OUTPUT_SIZE, OutputPacket, PROTOCOL_VERSION,
    RemoteGame, TouchPoint, button, game_hello, is_game_hello,
};
pub use relay::PadSink;
pub use security::is_allowed_peer;
pub use vpad::{VirtualPad, VirtualPadError, VirtualPadStatus};

use control::HubMessage;
pub use control::MAX_DEVICES;
use relay::Relay;

/// The port the phone app tries first (it cannot be typed in; only the address can).
pub const CONTROL_PORT: u16 = 47700;
const BRIDGE_PORT: u16 = 47701;
/// A fixed preferred UDP port makes firewall rules possible; the phone learns it from `welcome`.
const PHONE_UDP_PORT: u16 = 47702;

/// A phone asking to be paired: the player sees the code on both screens and answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairingRequest {
    pub id: u64,
    pub device_name: String,
    pub code: String,
}

/// What the server tells the launcher. Called from the server's threads: only enqueue and wake the
/// UI, never block.
pub trait RemoteEvents: Send + Sync {
    fn pairing(&self, request: PairingRequest);
    /// The pairing card is obsolete (answered, timed out, or the phone left).
    fn pairing_ended(&self, id: u64);
    /// A phone said hello and is not welcomed yet, or (`None`) that stopped.
    fn connecting(&self, name: Option<String>);
    fn connected(&self, name: String);
    fn disconnected(&self);
    fn latency(&self, milliseconds: f64);
    /// A request of the welcomed phone: command, select, launch, close, home, saveLayout, wantArt.
    fn request(&self, message: Message);
    /// A running game started (or stopped) greeting on the bridge: it reads the phone's pad.
    fn pad_ready(&self, ready: bool);
}

#[derive(Clone, Debug)]
pub struct RemoteConfig {
    /// The name the phone shows for this PC.
    pub name: String,
    pub devices_file: PathBuf,
    /// Announce through mDNS (off in tests).
    pub advertise: bool,
    pub control_port: u16,
    /// Pair every phone without asking (tests only).
    pub allow_all: bool,
}

impl RemoteConfig {
    pub fn new(name: String, devices_file: PathBuf) -> RemoteConfig {
        RemoteConfig {
            name,
            devices_file,
            advertise: true,
            control_port: CONTROL_PORT,
            allow_all: false,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RemoteError {
    #[error("the phone server could not start: {0}")]
    Io(#[from] io::Error),
}

/// What the player needs to type into the phone when discovery fails.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteInfo {
    pub addresses: Vec<Ipv4Addr>,
    pub control_port: u16,
    pub udp_port: u16,
    /// False when 47700 was busy: the phone's manual entry will not find this server.
    pub preferred_port: bool,
    /// Why the mDNS announcement is missing, if it is.
    pub discovery_error: Option<String>,
}

pub struct RemoteServer {
    hub: Sender<HubMessage>,
    relay: Arc<Relay>,
    stop: Arc<AtomicBool>,
    devices_file: PathBuf,
    info: RemoteInfo,
    _discovery: Option<discovery::Discovery>,
}

impl RemoteServer {
    pub fn start(
        config: RemoteConfig,
        events: Arc<dyn RemoteEvents>,
    ) -> Result<RemoteServer, RemoteError> {
        let stop = Arc::new(AtomicBool::new(false));
        let ready_events = events.clone();
        let relay = Arc::new(Relay::start(
            PHONE_UDP_PORT,
            BRIDGE_PORT,
            Arc::new(move |ready| ready_events.pad_ready(ready)),
        )?);
        let listener = control::bind_listener(config.control_port)?;
        let control_port = listener.local_addr()?.port();
        let (hub_tx, hub_rx) = channel();
        control::spawn_listener(listener, hub_tx.clone(), stop.clone())?;
        let hub = control::Hub::new(
            PairedDevices::load(&config.devices_file),
            events,
            relay.clone(),
            config.name.clone(),
            config.allow_all,
        );
        thread::Builder::new()
            .name("remote-hub".into())
            .spawn(move || hub.run(hub_rx))?;

        let preferred_port = control_port == config.control_port;
        if !preferred_port {
            tracing::warn!(
                "remote: port {} is busy, using {control_port}; typing the address into the phone will not work",
                config.control_port
            );
        }
        let (discovery, addresses, discovery_error) = if config.advertise {
            match discovery::Discovery::start(&config.name, control_port) {
                Ok(d) => {
                    let addresses = d.addresses.clone();
                    (Some(d), addresses, None)
                }
                Err(error) => {
                    tracing::warn!("remote: not announced on the network: {error}");
                    (None, lan_addresses(), Some(error))
                }
            }
        } else {
            (None, lan_addresses(), None)
        };
        let info = RemoteInfo {
            addresses,
            control_port,
            udp_port: relay.phone_port(),
            preferred_port,
            discovery_error,
        };
        Ok(RemoteServer {
            hub: hub_tx,
            relay,
            stop,
            devices_file: config.devices_file,
            info,
            _discovery: discovery,
        })
    }

    pub fn info(&self) -> &RemoteInfo {
        &self.info
    }

    pub fn control_port(&self) -> u16 {
        self.info.control_port
    }

    /// The answer of the player to a pairing card.
    pub fn respond(&self, request: u64, allow: bool) {
        let _ = self.hub.send(HubMessage::Respond { request, allow });
    }

    /// Sends to the welcomed phone (a `library` is also kept for the next welcome).
    pub fn publish(&self, message: Message) {
        let _ = self.hub.send(HubMessage::Publish(message));
    }

    /// Forgets a paired phone; if it is connected it is disconnected.
    pub fn forget(&self, id: &str) {
        let _ = self.hub.send(HubMessage::Forget(id.to_string()));
    }

    /// The phones that were allowed, read from the file.
    pub fn paired(&self) -> Vec<PairedDevice> {
        PairedDevices::load(&self.devices_file).list().to_vec()
    }

    /// The environment that tells a game where the bridge is (`ANYPS5_REMOTE_PAD`). Today no
    /// engine build reads it, so the launcher does not set it; it is kept for when one does.
    pub fn bridge_environment(&self) -> BTreeMap<String, String> {
        BTreeMap::from([(
            "ANYPS5_REMOTE_PAD".to_string(),
            format!("127.0.0.1:{}", self.relay.bridge_port()),
        )])
    }

    /// Where the phone's pad state also goes (the virtual controller), or `None`.
    pub fn set_pad_sink(&self, sink: Option<Arc<dyn PadSink>>) {
        self.relay.set_sink(sink);
    }

    /// Feedback from an OS-level virtual controller (rumble, light bar) to the phone.
    pub fn send_feedback(&self, packet: &OutputPacket) {
        self.relay.send_to_phone(packet);
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        self.relay.stop();
        let _ = self.hub.send(HubMessage::Stop);
    }
}

impl Drop for RemoteServer {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The name of this PC as the phone should show it.
pub fn pc_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .filter(|name| !name.trim().is_empty())
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "AnyPS5 PC".to_string())
}

#[cfg(test)]
mod tests;
