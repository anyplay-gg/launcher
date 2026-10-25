//! LudusCtrl: a controller connected to an Android phone plays on this PC through the LudusCtrl
//! app (protocol and app by Josip Parlov, @parl0v). Written from the app's protocol document,
//! DSLINK v2 (`docs/PROTOCOL.md` in github.com/parl0v/ludusctrl); docs/ludusctrl-api.md is the
//! interface the launcher binds to.
//!
//! One UDP port carries pairing, input and acks; a beacon tells phones on the network where it is.
//! A phone pairs once (ECDH on P-256, commit then reveal, a six-digit code the player compares on
//! the launcher's pairing question) and signs every input packet with the key that came of it.
//! Its pad goes to the launcher's virtual controller through the one [`PadSlot`], which a browser
//! client will share later. Nothing here starts unless the player switched it on, and nothing
//! touches the internet.

mod crypto;
mod link;
mod pairing;
mod slot;
mod store;
mod wire;

use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

pub use slot::{Holding, Owner, PadSlot, Refusal};
pub use store::{LudusPhone, LudusStore};
pub use wire::PadLayout;

use super::{
    PairingRequest, RemoteError,
    discovery::lan_addresses,
    relay::{bind_phone_socket, recoverable},
    security::{Subnet, is_allowed_peer, is_on_link, local_subnets},
    vpad::PadState,
};
use link::Link;
use pairing::{Answer, Pairing};
use wire::{INPUT_MAGIC, PAIR_REQ_MAGIC};

/// The UDP port of pairing, input and acks. A busy port falls back to a free one; the beacon
/// advertises whichever is bound.
pub const LUDUS_PORT: u16 = 45989;
/// Where the beacon goes.
pub const LUDUS_BEACON_PORT: u16 = 45990;

const BEACON_EVERY: Duration = Duration::from_secs(2);
/// The adapters are looked at again this often (beacon sockets, the on-link rule).
const ADAPTERS_EVERY: Duration = Duration::from_secs(15);
/// The receive loop wakes at least this often for the watchdog and the pairing timers.
const TICK: Duration = Duration::from_millis(250);

/// Where the virtual pad state goes. Called on the server's thread: keep it quick.
pub trait PadTarget: Send + Sync {
    /// One complete XInput state; false when the controller could not take it.
    fn write(&self, state: &PadState) -> bool;
    /// Unplug: nothing more comes for a while.
    fn release(&self);
}

/// What the server tells the launcher. Called from the server's thread: only enqueue and wake the
/// UI, never block.
pub trait LudusEvents: Send + Sync {
    /// A phone revealed its nonce: ask the player. The code reads "260 329".
    fn pairing(&self, request: PairingRequest);
    /// The question is obsolete: answered, a minute without an answer, or the server stopped.
    fn pairing_ended(&self, id: u64);
    fn status(&self, status: LudusStatus);
}

/// Who holds the pad.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LudusVia {
    /// A phone of the LudusCtrl app, by the name it paired with.
    App { name: String },
    /// A browser client (step 2 of the feature; nothing claims this yet).
    Browser { peer: IpAddr },
}

/// What the settings show of LudusCtrl.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LudusStatus {
    /// Nobody has fed the pad within the watchdog (1.5 s); the beacon is on.
    Waiting,
    /// A phone or a browser holds the pad.
    Linked {
        via: LudusVia,
        /// What the controller on the phone is (the layout byte of the last packet).
        layout: PadLayout,
        /// A controller is connected to the phone; without one the pad is held at neutral.
        controller: bool,
        /// A pad target exists and its last write worked (the ack's pad-live bit).
        pad_live: bool,
    },
}

/// Where the beacon goes and from which addresses.
#[derive(Clone, Debug)]
pub struct BeaconTarget {
    pub to: SocketAddrV4,
    /// The local addresses to send from, one socket each; `None` is every LAN adapter (no
    /// loopback, no virtual adapter), looked at again every 15 s.
    pub from: Option<Vec<Ipv4Addr>>,
}

impl Default for BeaconTarget {
    fn default() -> Self {
        BeaconTarget {
            to: SocketAddrV4::new(Ipv4Addr::BROADCAST, LUDUS_BEACON_PORT),
            from: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LudusConfig {
    pub store_file: PathBuf,
    pub port: u16,
    /// `None`: no beacon (tests).
    pub beacon: Option<BeaconTarget>,
    /// Pair every phone without asking (tests only).
    pub allow_all: bool,
    /// Listen on this address only; `None` is every interface. Tests bind loopback, so a test
    /// run never asks the firewall for a port.
    pub bind: Option<IpAddr>,
}

impl LudusConfig {
    pub fn new(store_file: PathBuf) -> LudusConfig {
        LudusConfig {
            store_file,
            port: LUDUS_PORT,
            beacon: Some(BeaconTarget::default()),
            allow_all: false,
            bind: None,
        }
    }
}

/// Private, not carrier-grade NAT (100.64/10, also Tailscale), and on a network this PC is
/// plugged into.
fn allowed_source(ip: IpAddr, subnets: &[Subnet]) -> bool {
    let ip = ip.to_canonical();
    let cgnat = matches!(ip, IpAddr::V4(v4) if v4.octets()[0] == 100 && (64..=127).contains(&v4.octets()[1]));
    is_allowed_peer(ip) && !cgnat && is_on_link(ip, subnets)
}

struct Core {
    store: LudusStore,
    pairing: Pairing,
    link: Link,
    status: LudusStatus,
    subnets: Vec<Subnet>,
    subnets_at: Instant,
}

struct Shared {
    socket: UdpSocket,
    core: Mutex<Core>,
    slot: Arc<PadSlot>,
    events: Arc<dyn LudusEvents>,
    stop: AtomicBool,
    linked: AtomicBool,
    allow_all: bool,
}

pub struct LudusServer {
    shared: Arc<Shared>,
    port: u16,
}

impl LudusServer {
    pub fn start(
        config: LudusConfig,
        events: Arc<dyn LudusEvents>,
    ) -> Result<LudusServer, RemoteError> {
        let socket = bind_phone_socket(config.bind, config.port)?;
        socket.set_read_timeout(Some(TICK))?;
        let port = socket.local_addr()?.port();
        if port != config.port {
            tracing::warn!("ludusctrl: port {} is busy, using {port}", config.port);
        }
        let store = LudusStore::load(&config.store_file);
        let pc_id = store.pc_id();
        let shared = Arc::new(Shared {
            socket,
            core: Mutex::new(Core {
                pairing: Pairing::new(pc_id),
                store,
                link: Link::default(),
                status: LudusStatus::Waiting,
                subnets: local_subnets(),
                subnets_at: Instant::now(),
            }),
            slot: Arc::new(PadSlot::default()),
            events,
            stop: AtomicBool::new(false),
            linked: AtomicBool::new(false),
            allow_all: config.allow_all,
        });
        let receiver = shared.clone();
        thread::Builder::new()
            .name("ludusctrl".into())
            .spawn(move || receiver.receive())?;
        if let Some(target) = config.beacon {
            let beacon = shared.clone();
            thread::Builder::new()
                .name("ludusctrl-beacon".into())
                .spawn(move || beacon.beacon(&target, port, pc_id))?;
        }
        Ok(LudusServer { shared, port })
    }

    /// The UDP port really bound.
    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn status(&self) -> LudusStatus {
        self.shared.core().status.clone()
    }

    /// The player's answer to pairing question `request`.
    pub fn respond(&self, request: u64, allow: bool) {
        self.shared.respond(request, allow);
    }

    pub fn paired(&self) -> Vec<LudusPhone> {
        self.shared.core().store.list()
    }

    /// Forgets a phone (its hex id). If it holds the pad, the pad goes to neutral.
    pub fn forget(&self, id: &str) {
        let mut core = self.shared.core();
        if let Err(error) = core.store.remove(id) {
            tracing::warn!("ludusctrl: could not save the phone list: {error}");
        }
        if let Some(phone) = store::id_from_hex(id) {
            core.link.forget(&phone);
            self.shared.slot.release(Owner::App(phone), Instant::now());
        }
        self.shared.publish_status(&mut core);
    }

    /// Where the pad goes; `None` is nowhere (the acks say the pad is not live). The one
    /// virtual pad of LudusCtrl: the app and a browser client both feed it through [`PadSlot`].
    pub fn set_pad(&self, pad: Option<Arc<dyn PadTarget>>) {
        self.shared.slot.set_target(pad);
    }

    /// The pad slot, for a browser bridge: it feeds the pad through the slot and never holds a
    /// pad of its own.
    pub fn pad_slot(&self) -> Arc<PadSlot> {
        self.shared.slot.clone()
    }

    pub fn stop(&self) {
        if self.shared.stop.swap(true, Ordering::Relaxed) {
            return;
        }
        self.shared.slot.stop();
        let mut core = self.shared.core();
        if let Some(open) = core.pairing.cancel(Instant::now()) {
            self.shared.events.pairing_ended(open);
        }
    }
}

impl Drop for LudusServer {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Shared {
    fn core(&self) -> std::sync::MutexGuard<'_, Core> {
        self.core.lock().expect("ludusctrl state")
    }

    fn publish_status(&self, core: &mut Core) {
        let holding = self.slot.holding();
        // A browser holding the pad does not pause the beacon; a phone of the app does.
        let app = holding.is_some_and(|h| matches!(h.owner, Owner::App(_)));
        self.linked.store(app, Ordering::Relaxed);
        let status = match holding {
            None => LudusStatus::Waiting,
            Some(holding) => LudusStatus::Linked {
                via: match holding.owner {
                    Owner::App(id) => LudusVia::App {
                        name: core
                            .store
                            .phones()
                            .iter()
                            .find(|p| p.id == id)
                            .map(|p| p.name.clone())
                            .unwrap_or_default(),
                    },
                    Owner::Browser(_) => LudusVia::Browser {
                        peer: holding.source.ip().to_canonical(),
                    },
                },
                layout: holding.layout,
                controller: holding.controller,
                pad_live: holding.pad_live,
            },
        };
        if status != core.status {
            core.status = status.clone();
            self.events.status(status);
        }
    }

    fn respond(&self, request: u64, allow: bool) {
        let mut core = self.core();
        let core = &mut *core;
        let answer = core
            .pairing
            .answer(request, allow, &mut core.store, Instant::now());
        if let Some(answer) = answer {
            if answer == Answer::Full {
                tracing::warn!("ludusctrl: eight phones are paired, the new one was refused");
            }
            self.events.pairing_ended(request);
        }
    }

    fn receive(&self) {
        let mut buffer = [0u8; 512];
        while !self.stop.load(Ordering::Relaxed) {
            let received = self.socket.recv_from(&mut buffer);
            let now = Instant::now();
            let mut ask = None;
            {
                let mut core = self.core();
                let core = &mut *core;
                if now.saturating_duration_since(core.subnets_at) >= ADAPTERS_EVERY {
                    core.subnets = local_subnets();
                    core.subnets_at = now;
                }
                match received {
                    Ok((size, source))
                        if size > 0 && allowed_source(source.ip(), &core.subnets) =>
                    {
                        let datagram = &buffer[..size.min(buffer.len())];
                        let reply = match datagram[0] {
                            PAIR_REQ_MAGIC => {
                                let handled = core.pairing.handle(datagram, source.ip(), now);
                                ask = handled.ask;
                                handled.reply.map(|r| r.to_vec())
                            }
                            INPUT_MAGIC => {
                                core.link
                                    .input(datagram, source, now, &mut core.store, &self.slot)
                            }
                            _ => None,
                        };
                        if let Some(reply) = reply {
                            let _ = self.socket.send_to(&reply, source);
                        }
                    }
                    Ok(_) => {}
                    Err(error) if recoverable(&error) => {}
                    Err(_) => thread::sleep(Duration::from_millis(10)),
                }
                if let Some(ended) = core.pairing.expire(now) {
                    self.events.pairing_ended(ended);
                }
                self.slot.tick(now);
                self.publish_status(core);
            }
            if let Some(request) = ask {
                let id = request.id;
                self.events.pairing(request);
                if self.allow_all {
                    self.respond(id, true);
                }
            }
        }
    }

    /// Announces the input port from every adapter, every 2 s, paused while a phone of the app is
    /// linked.
    /// Binding each socket to its address gets the broadcast out of every adapter, not only the
    /// one the routing table picks.
    fn beacon(&self, target: &BeaconTarget, port: u16, pc_id: wire::Id) {
        let packet = wire::beacon(port, &pc_id);
        let mut sockets: Vec<UdpSocket> = Vec::new();
        let mut built: Option<Instant> = None;
        let mut sent: Option<Instant> = None;
        let mut was_linked = false;
        while !self.stop.load(Ordering::Relaxed) {
            let now = Instant::now();
            if built.is_none_or(|at| now.saturating_duration_since(at) >= ADAPTERS_EVERY) {
                let from = target.from.clone().unwrap_or_else(lan_addresses);
                sockets = from
                    .into_iter()
                    .filter_map(|ip| {
                        let socket = UdpSocket::bind(SocketAddrV4::new(ip, 0)).ok()?;
                        socket.set_broadcast(true).ok()?;
                        Some(socket)
                    })
                    .collect();
                built = Some(now);
            }
            let linked = self.linked.load(Ordering::Relaxed);
            // Straight away when the link drops, not after the interval.
            let due = was_linked
                || sent.is_none_or(|at| now.saturating_duration_since(at) >= BEACON_EVERY);
            if !linked && due {
                for socket in &sockets {
                    let _ = socket.send_to(&packet, SocketAddr::V4(target.to));
                }
                sent = Some(now);
            }
            was_linked = linked;
            thread::sleep(Duration::from_millis(50));
        }
    }
}

#[cfg(test)]
mod tests;
