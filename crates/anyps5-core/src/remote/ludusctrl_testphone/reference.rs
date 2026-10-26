//! A small receiver written straight from the spec, so the harness's own tests can run
//! [`run_conformance`](super::conformance::run_conformance) before (and apart from) the
//! launcher's receiver. Loopback only, one pairing slot, no browser path. It is the harness's
//! fixture, not the receiver under test.

use std::{
    collections::HashMap,
    io,
    net::{Ipv4Addr, SocketAddr, UdpSocket},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use super::{conformance::PcUnderTest, *};

/// Deliberate bugs, to show that the harness catches them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Quirk {
    #[default]
    None,
    /// Accepts any seq within a session, as if the replay check were missing.
    AcceptStale,
    /// Derives the code from the wrong salt (without the nonce).
    CodeWithoutNonce,
    /// Never releases the pad on the watchdog.
    NoWatchdog,
    /// Takes the KID's phone without checking the MAC.
    NoMacCheck,
}

#[derive(Default)]
struct Shared {
    prompts: Vec<u32>,
    allow: bool,
    pad: PadState,
}

pub struct ReferencePc {
    addr: SocketAddr,
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    beacon: Option<UdpSocket>,
}

impl ReferencePc {
    /// Starts on a free loopback port. With `beacon`, beacons go to that socket's port on
    /// 127.0.0.1 and the socket is handed to the harness.
    pub fn start(beacon: Option<UdpSocket>, auto_allow: bool, quirk: Quirk) -> io::Result<Self> {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))?;
        socket.set_read_timeout(Some(Duration::from_millis(20)))?;
        let addr = socket.local_addr()?;
        let beacon_to = match &beacon {
            Some(s) => Some(SocketAddr::from((
                Ipv4Addr::LOCALHOST,
                s.local_addr()?.port(),
            ))),
            None => None,
        };
        let shared = Arc::new(Mutex::new(Shared::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let mut receiver = Receiver {
            socket,
            shared: shared.clone(),
            beacon_to,
            auto_allow,
            quirk,
            pc_id: random(),
            pair_by_ip: HashMap::new(),
            current: None,
            finished: HashMap::new(),
            phones: Vec::new(),
            seqs: HashMap::new(),
            holder: None,
            linked: false,
            last_packet: Instant::now(),
            received: 0,
            last_ack: HashMap::new(),
            unsigned_ack: HashMap::new(),
            last_beacon: None,
            beacon_paused: false,
        };
        let thread = {
            let stop = stop.clone();
            thread::spawn(move || {
                let mut buf = [0; 2048];
                while !stop.load(Ordering::Relaxed) {
                    if let Ok((n, from)) = receiver.socket.recv_from(&mut buf) {
                        receiver.handle(&buf[..n], from);
                    }
                    receiver.tick();
                }
            })
        };
        Ok(Self {
            addr,
            shared,
            stop,
            thread: Some(thread),
            beacon,
        })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }
}

impl Drop for ReferencePc {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl PcUnderTest for ReferencePc {
    fn input_addr(&self) -> SocketAddr {
        self.addr
    }
    fn beacon_socket(&mut self) -> Option<UdpSocket> {
        self.beacon.take()
    }
    fn prompt_codes(&mut self) -> Vec<u32> {
        self.shared.lock().unwrap().prompts.clone()
    }
    fn allow(&mut self) {
        self.shared.lock().unwrap().allow = true;
    }
    fn pad(&mut self) -> Option<PadState> {
        Some(self.shared.lock().unwrap().pad)
    }
}

struct Attempt {
    key: (Id, Point),
    phone_pub: Point,
    commit: [u8; 16],
    pc: EphemeralKey,
    pc_pub: Point,
    created: Instant,
    prompt: Option<(Derived, Instant)>,
}

/// A finished attempt's answer: state, PC key, K when accepted, and when it finished.
type Finished = (u8, Point, Option<Key>, Instant);

struct Phone {
    id: Id,
    key: Key,
    kid: Kid,
    highest_session: u32,
}

struct Receiver {
    socket: UdpSocket,
    shared: Arc<Mutex<Shared>>,
    beacon_to: Option<SocketAddr>,
    auto_allow: bool,
    quirk: Quirk,
    pc_id: Id,
    pair_by_ip: HashMap<std::net::IpAddr, Instant>,
    current: Option<Attempt>,
    finished: HashMap<(Id, Point), Finished>,
    phones: Vec<Phone>,
    seqs: HashMap<Id, (u32, u32)>,
    holder: Option<Id>,
    linked: bool,
    last_packet: Instant,
    received: u32,
    last_ack: HashMap<Id, Instant>,
    unsigned_ack: HashMap<(u8, SocketAddr), Instant>,
    last_beacon: Option<Instant>,
    beacon_paused: bool,
}

const SECOND: Duration = Duration::from_secs(1);

impl Receiver {
    fn send(&self, data: &[u8], to: SocketAddr) {
        let _ = self.socket.send_to(data, to);
    }

    fn handle(&mut self, data: &[u8], from: SocketAddr) {
        if !from.ip().is_loopback() || data.is_empty() {
            return;
        }
        if data[0] == PAIR_REQ_MAGIC {
            if let Some(reply) = self.pair(data, from) {
                self.send(&reply.encode(), from);
            }
            return;
        }
        if data.len() < 2 || data[0] != INPUT_MAGIC {
            return;
        }
        if data[1] != PROTOCOL_VERSION {
            if data.len() >= UNSIGNED_ACK_SIZE
                && self.unsigned_due(ack_flag::VERSION_MISMATCH, from)
            {
                let seq = u32::from_le_bytes(data[2..6].try_into().unwrap());
                let ack = Ack::encode_unsigned(seq, 0, ack_flag::VERSION_MISMATCH);
                self.send(&ack, from);
            }
            return;
        }
        if data.len() != INPUT_SIZE {
            return;
        }
        let seq = u32::from_le_bytes(data[2..6].try_into().unwrap());
        let session = u32::from_le_bytes(data[20..24].try_into().unwrap());
        let kid: Kid = data[24..28].try_into().unwrap();
        let candidates: Vec<usize> = (0..self.phones.len())
            .filter(|&i| self.phones[i].kid == kid)
            .collect();
        if candidates.is_empty() {
            if self.unsigned_due(ack_flag::NOT_PAIRED, from) {
                self.send(&Ack::encode_unsigned(seq, 0, ack_flag::NOT_PAIRED), from);
            }
            return;
        }
        let Some(index) = candidates.into_iter().find(|&i| {
            self.quirk == Quirk::NoMacCheck
                || mac_ok(
                    &self.phones[i].key,
                    &data[..INPUT_SIGNED],
                    &data[INPUT_SIGNED..],
                )
        }) else {
            return;
        };

        let phone = &mut self.phones[index];
        let id = phone.id;
        let fresh = if session > phone.highest_session {
            phone.highest_session = session;
            true
        } else if session < phone.highest_session {
            return;
        } else {
            match self.seqs.get(&id) {
                Some(&(s, newest)) if s == session => {
                    let ahead = seq.wrapping_sub(newest);
                    if !(0 < ahead && ahead < 0x8000_0000) && self.quirk != Quirk::AcceptStale {
                        return;
                    }
                }
                _ => return,
            }
            false
        };
        self.seqs.insert(id, (session, seq));

        let now = Instant::now();
        if self.holder != Some(id) {
            let held = self.linked && now - self.last_packet < Duration::from_millis(300);
            if !fresh && held {
                self.ack(index, seq, false, from);
                return;
            }
            self.holder = Some(id);
            self.last_ack.remove(&id);
        }
        self.linked = true;
        self.last_packet = now;
        self.received = self.received.wrapping_add(1);

        let pad = PadState {
            buttons: u16::from_le_bytes([data[6], data[7]]),
            left_x: i16::from_le_bytes([data[8], data[9]]),
            left_y: i16::from_le_bytes([data[10], data[11]]),
            right_x: i16::from_le_bytes([data[12], data[13]]),
            right_y: i16::from_le_bytes([data[14], data[15]]),
            left_trigger: data[16],
            right_trigger: data[17],
        };
        self.shared.lock().unwrap().pad = if data[18] & 0x01 != 0 {
            pad.masked()
        } else {
            PadState::NEUTRAL
        };
        self.ack(index, seq, true, from);
    }

    fn unsigned_due(&mut self, kind: u8, from: SocketAddr) -> bool {
        let now = Instant::now();
        if self
            .unsigned_ack
            .get(&(kind, from))
            .is_some_and(|&t| now - t < SECOND)
        {
            return false;
        }
        if self.unsigned_ack.len() > 64 {
            self.unsigned_ack.clear();
        }
        self.unsigned_ack.insert((kind, from), now);
        true
    }

    fn ack(&mut self, index: usize, seq: u32, live: bool, to: SocketAddr) {
        let phone = &self.phones[index];
        let now = Instant::now();
        if self
            .last_ack
            .get(&phone.id)
            .is_some_and(|&t| now - t < ACK_INTERVAL)
        {
            return;
        }
        self.last_ack.insert(phone.id, now);
        let flags = if live { ack_flag::PAD_LIVE } else { 0 };
        let ack = Ack::encode_signed(seq, self.received, flags, &phone.key);
        self.send(&ack, to);
    }

    // --- pairing ---------------------------------------------------------------------------

    fn response(&self, state: u8, pc_pub: Point, key: Option<&Key>) -> PairResp {
        PairResp::new(self.pc_id, pc_pub, state, key)
    }

    fn pair(&mut self, data: &[u8], from: SocketAddr) -> Option<PairResp> {
        if data.len() != PAIR_REQ_SIZE || data[1] != PROTOCOL_VERSION {
            return None;
        }
        let now = Instant::now();
        if self
            .pair_by_ip
            .get(&from.ip())
            .is_some_and(|&t| now - t < SECOND)
        {
            return None;
        }
        self.pair_by_ip.insert(from.ip(), now);
        let phone_id: Id = data[2..10].try_into().unwrap();
        let phone_pub: Point = data[10..75].try_into().unwrap();
        let (name_len, phase) = (data[75], data[108]);
        let field: [u8; 16] = data[109..125].try_into().unwrap();
        if name_len as usize > NAME_MAX || phase > 1 || phone_pub[0] != 0x04 {
            return None;
        }
        self.expire();
        let key = (phone_id, phone_pub);

        if let Some(&(state, pc_pub, link, _)) = self.finished.get(&key) {
            return Some(self.response(state, pc_pub, link.as_ref()));
        }
        if let Some(cur) = &mut self.current
            && cur.key == key
        {
            if cur.prompt.is_none() && phase == 1 && commitment(&field) == cur.commit {
                let peer = parse_point(&cur.phone_pub)?;
                let z = cur.pc.agree(&peer);
                let mut derived = derive(&z, &cur.phone_pub, &cur.pc_pub, &field);
                if self.quirk == Quirk::CodeWithoutNonce {
                    derived.code = derive(&z, &cur.phone_pub, &cur.pc_pub, &[0; 16]).code;
                }
                cur.prompt = Some((derived, now + Duration::from_secs(60)));
                self.shared.lock().unwrap().prompts.push(derived.code);
                if self.auto_allow {
                    self.finish(pair_state::ACCEPTED);
                    let (state, pc_pub, link, _) = self.finished[&key];
                    return Some(self.response(state, pc_pub, link.as_ref()));
                }
            }
            let pc_pub = self.current.as_ref().unwrap().pc_pub;
            return Some(self.response(pair_state::WAITING, pc_pub, None));
        }
        if self.current.is_some() {
            return Some(self.response(pair_state::BUSY, [0; 65], None));
        }
        if phase == 1 {
            return None;
        }
        parse_point(&phone_pub)?;
        let pc = EphemeralKey::random();
        let pc_pub = pc.public();
        self.current = Some(Attempt {
            key,
            phone_pub,
            commit: field,
            pc,
            pc_pub,
            created: now,
            prompt: None,
        });
        Some(self.response(pair_state::WAITING, pc_pub, None))
    }

    fn finish(&mut self, state: u8) {
        let Some(cur) = self.current.take() else {
            return;
        };
        let link = cur
            .prompt
            .filter(|_| state == pair_state::ACCEPTED)
            .map(|(d, _)| d);
        if let Some(derived) = link {
            self.phones.retain(|p| p.id != cur.key.0);
            self.seqs.remove(&cur.key.0);
            self.phones.push(Phone {
                id: cur.key.0,
                key: derived.key,
                kid: derived.kid,
                highest_session: 0,
            });
        }
        let key = link.map(|d| d.key);
        self.finished
            .insert(cur.key, (state, cur.pc_pub, key, Instant::now()));
    }

    fn expire(&mut self) {
        let now = Instant::now();
        self.finished
            .retain(|_, v| now - v.3 < Duration::from_secs(90));
        let allow = std::mem::take(&mut self.shared.lock().unwrap().allow);
        let Some(cur) = &self.current else {
            return;
        };
        match cur.prompt {
            Some(_) if allow => self.finish(pair_state::ACCEPTED),
            Some((_, until)) if now >= until => self.finish(pair_state::DENIED),
            None if now - cur.created > Duration::from_secs(10) => {
                self.finish(pair_state::DENIED);
            }
            _ => {}
        }
    }

    // --- timers ----------------------------------------------------------------------------

    fn tick(&mut self) {
        self.expire();
        if self.linked && self.last_packet.elapsed() > WATCHDOG_TIMEOUT {
            self.linked = false;
            self.holder = None;
            if self.quirk != Quirk::NoWatchdog {
                self.shared.lock().unwrap().pad = PadState::NEUTRAL;
            }
        }
        let Some(to) = self.beacon_to else {
            return;
        };
        if self.linked {
            self.beacon_paused = true;
            return;
        }
        let resumed = std::mem::take(&mut self.beacon_paused);
        let due = self
            .last_beacon
            .is_none_or(|t| t.elapsed() >= Duration::from_secs(2));
        if resumed || due {
            self.last_beacon = Some(Instant::now());
            let beacon = Beacon {
                input_port: self.socket.local_addr().map(|a| a.port()).unwrap_or(0),
                pc_id: Some(self.pc_id),
            };
            self.send(&beacon.encode(), to);
        }
    }
}
