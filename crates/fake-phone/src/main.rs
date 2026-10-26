//! Test helper: a phone without a phone (and a game without a game) for the launcher's phone server.
//!
//! ```text
//! fake-phone phone --addr 127.0.0.1:47700 --key-file k.txt --script "wait500,select:ID,wait300,launch:ID,wait500,press:cross,wait1500,home"
//! fake-phone phone --discover ...            find the PC through mDNS instead of --addr
//! fake-phone game --bridge 127.0.0.1:47701   greet like an engine would and answer pad packets
//! fake-phone browser --url "https://127.0.0.1:45991/#t=TOKEN" --press cross --seconds 3
//!                                            LudusCtrl's browser client (see browser.rs)
//! ```
//! Script words: `waitMS`, `select:ID`, `launch:ID`, `close`, `home`, `command:NAME`, `art:ID`,
//! `layout:ID:TEXT`, `flood:N` (N quick `left` commands), `press:cross+r1`, `release`, `stick:LX+LY+RX+RY`, `touch:X+Y`, `expect:library`,
//! `expect:running=ID`, `expect:selected=ID`, `expect:feedback`, `expect:closed`, `expect:rejected`.

mod browser;

use std::{
    collections::BTreeMap,
    env, fs,
    net::{SocketAddr, ToSocketAddrs, UdpSocket},
    process::ExitCode,
    time::{Duration, Instant},
};

use anyps5_core::remote::{
    InputPacket, Message, OutputPacket, TouchPoint, button, game_hello, is_game_hello,
};
use tungstenite::{Message as Ws, stream::MaybeTlsStream};

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let mode = args.next().unwrap_or_default();
    let options = parse_options(args.collect());
    let outcome = match mode.as_str() {
        "phone" => phone(&options),
        "game" => game(&options),
        "browser" => browser::browser(&options),
        _ => Err("usage: fake-phone phone|game|browser [--flag value]...".into()),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            println!("FAIL {error}");
            ExitCode::FAILURE
        }
    }
}

fn parse_options(args: Vec<String>) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let mut at = 0;
    while at < args.len() {
        if let Some(name) = args[at].strip_prefix("--") {
            let value = args.get(at + 1).filter(|v| !v.starts_with("--"));
            map.insert(name.to_string(), value.cloned().unwrap_or_default());
            at += if value.is_some() { 2 } else { 1 };
        } else {
            at += 1;
        }
    }
    map
}

fn discover() -> Result<SocketAddr, String> {
    let daemon = mdns_sd::ServiceDaemon::new().map_err(|e| e.to_string())?;
    let browse = daemon
        .browse("_anyps5-remote._tcp.local.")
        .map_err(|e| e.to_string())?;
    let until = Instant::now() + Duration::from_secs(8);
    while Instant::now() < until {
        if let Ok(mdns_sd::ServiceEvent::ServiceResolved(info)) =
            browse.recv_timeout(Duration::from_millis(200))
        {
            let v4 = info.get_addresses_v4();
            let address = v4.iter().next().ok_or("resolved without an address")?;
            println!(
                "discovered {} at {}:{} (all: {:?})",
                info.get_fullname(),
                address,
                info.get_port(),
                info.get_addresses()
            );
            return Ok(SocketAddr::new((*address).into(), info.get_port()));
        }
    }
    Err("no launcher found through mDNS".into())
}

fn button_named(name: &str) -> u32 {
    match name {
        "cross" => button::CROSS,
        "circle" => button::CIRCLE,
        "square" => button::SQUARE,
        "triangle" => button::TRIANGLE,
        "l1" => button::L1,
        "r1" => button::R1,
        "l2" => button::L2,
        "r2" => button::R2,
        "l3" => button::L3,
        "r3" => button::R3,
        "options" => button::OPTIONS,
        "up" => button::UP,
        "down" => button::DOWN,
        "left" => button::LEFT,
        "right" => button::RIGHT,
        "touchpad" => button::TOUCHPAD,
        _ => 0,
    }
}

fn phone(options: &BTreeMap<String, String>) -> Result<(), String> {
    let address: SocketAddr = if options.contains_key("discover") {
        discover()?
    } else {
        options
            .get("addr")
            .map(String::as_str)
            .unwrap_or("127.0.0.1:47700")
            .to_socket_addrs()
            .map_err(|e| e.to_string())?
            .next()
            .ok_or("bad address")?
    };
    let key_file = options.get("key-file");
    let stored = key_file.and_then(|f| fs::read_to_string(f).ok());
    let device_id = options
        .get("device-id")
        .cloned()
        .unwrap_or_else(|| "fa4e0000fa4e0000fa4e0000fa4e0000".into());
    let hz: u64 = options
        .get("stream-hz")
        .and_then(|v| v.parse().ok())
        .unwrap_or(125);
    let script: Vec<String> = options
        .get("script")
        .map(|s| s.split(',').map(str::to_string).collect())
        .unwrap_or_default();

    let (mut ws, _) =
        tungstenite::connect(format!("ws://{address}/")).map_err(|e| e.to_string())?;
    if let MaybeTlsStream::Plain(stream) = ws.get_mut() {
        stream
            .set_read_timeout(Some(Duration::from_millis(2)))
            .map_err(|e| e.to_string())?;
    }
    let send = |ws: &mut tungstenite::WebSocket<MaybeTlsStream<std::net::TcpStream>>,
                m: Message| {
        println!("-> {}", m.to_json());
        ws.send(Ws::text(m.to_json())).map_err(|e| e.to_string())
    };
    send(
        &mut ws,
        Message::Hello {
            protocol: 1,
            device_id,
            device_name: options
                .get("name")
                .cloned()
                .unwrap_or_else(|| "Fake Phone".into()),
            key: stored
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
        },
    )?;

    // Against a PC on this machine the pad socket stays on loopback (no firewall prompt).
    let local = if address.ip().is_loopback() {
        "127.0.0.1:0"
    } else {
        "0.0.0.0:0"
    };
    let udp = UdpSocket::bind(local).map_err(|e| e.to_string())?;
    udp.set_nonblocking(true).map_err(|e| e.to_string())?;
    let mut session: Option<(u64, SocketAddr)> = None;
    let mut library: Option<Message> = None;
    let mut buttons = 0u32;
    let mut sticks = [128u8; 4];
    let mut touch = TouchPoint::default();
    let mut sequence = 0u32;
    let clock = Instant::now();
    let mut next_pad = Instant::now();
    let mut step = 0usize;
    let mut wait_until = Instant::now();
    let mut feedback = 0u32;
    let mut closed = false;
    let mut rejected = false;
    let mut waited_for_welcome = Instant::now();
    let finish_by = Instant::now() + Duration::from_secs(120);
    let mut buffer = [0u8; 512];

    loop {
        if Instant::now() > finish_by {
            return Err("timed out".into());
        }
        match ws.read() {
            Ok(Ws::Text(text)) => {
                println!("<- {}", short(text.as_str()));
                match Message::parse(text.as_str()) {
                    Some(Message::Welcome {
                        session: s,
                        udp_port,
                        key,
                        ..
                    }) => {
                        if let Some(file) = key_file {
                            fs::write(file, &key).map_err(|e| e.to_string())?;
                        }
                        let target = SocketAddr::new(address.ip(), udp_port);
                        session = Some((s.parse().map_err(|_| "bad session")?, target));
                        waited_for_welcome = Instant::now();
                        println!("welcomed, pad to {target}");
                    }
                    Some(Message::Library { .. }) => {
                        library = Some(Message::parse(text.as_str()).unwrap())
                    }
                    Some(Message::Rejected { .. }) => rejected = true,
                    _ => {}
                }
            }
            Ok(Ws::Close(_)) => closed = true,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => closed = true,
        }
        while let Ok((n, _)) = udp.recv_from(&mut buffer) {
            if let Some(out) = OutputPacket::parse(&buffer[..n]) {
                feedback += 1;
                if feedback <= 3 || out.large_motor > 0 {
                    println!(
                        "<- pad output seq={} wait={}us rumble={}/{} light={:?}",
                        out.echo_sequence,
                        out.wait_micros,
                        out.large_motor,
                        out.small_motor,
                        out.light
                    );
                }
            }
        }
        if let Some((id, target)) = session
            && hz > 0
            && Instant::now() >= next_pad
        {
            next_pad = Instant::now() + Duration::from_micros(1_000_000 / hz);
            sequence += 1;
            let mut packet = InputPacket::neutral(id);
            packet.sequence = sequence;
            packet.sent_micros = clock.elapsed().as_micros() as u64;
            packet.buttons = buttons;
            packet.sticks = sticks;
            packet.touch[0] = touch;
            if buttons & button::L2 != 0 {
                packet.l2 = 255;
            }
            if buttons & button::R2 != 0 {
                packet.r2 = 255;
            }
            let _ = udp.send_to(&packet.encode(), target);
        }
        if closed && !script.is_empty() && step < script.len() && !rejected {
            return Err("connection closed before the script ended".into());
        }
        if rejected {
            if script.get(step).map(String::as_str) == Some("expect:rejected") {
                println!("OK rejected");
                return Ok(());
            }
            return Err("rejected".into());
        }
        if session.is_none() && waited_for_welcome.elapsed() > Duration::from_secs(60) {
            return Err("never welcomed (pairing card waiting?)".into());
        }
        if session.is_none() || Instant::now() < wait_until {
            continue;
        }
        let Some(word) = script.get(step) else {
            println!("OK script done, {feedback} feedback packets");
            return Ok(());
        };
        step += 1;
        println!("# {word}");
        let (head, rest) = word.split_once(':').unwrap_or((word.as_str(), ""));
        match head {
            w if w.starts_with("wait") => {
                let ms: u64 = w[4..].parse().unwrap_or(0);
                wait_until = Instant::now() + Duration::from_millis(ms);
            }
            "select" => send(&mut ws, Message::Select { id: rest.into() })?,
            "launch" => send(&mut ws, Message::Launch { id: rest.into() })?,
            "flood" => {
                for _ in 0..rest.parse::<u32>().unwrap_or(100) {
                    send(
                        &mut ws,
                        Message::Command {
                            name: "left".into(),
                        },
                    )?;
                }
            }
            "close" => send(&mut ws, Message::Close)?,
            "home" => send(&mut ws, Message::Home)?,
            "create" => send(&mut ws, Message::Create)?,
            "command" => send(&mut ws, Message::Command { name: rest.into() })?,
            "art" => send(&mut ws, Message::WantArt { id: rest.into() })?,
            "layout" => {
                let (id, text) = rest.split_once(':').unwrap_or((rest, ""));
                send(
                    &mut ws,
                    Message::SaveLayout {
                        id: id.into(),
                        layout: Some(text.into()),
                    },
                )?
            }
            "press" => buttons = rest.split('+').map(button_named).fold(0, |a, b| a | b),
            "release" => {
                buttons = 0;
                sticks = [128; 4];
                touch = TouchPoint::default();
            }
            "stick" => {
                let v: Vec<u8> = rest.split('+').filter_map(|n| n.parse().ok()).collect();
                if v.len() == 4 {
                    sticks = [v[0], v[1], v[2], v[3]];
                }
            }
            "touch" => {
                let v: Vec<u16> = rest.split('+').filter_map(|n| n.parse().ok()).collect();
                if v.len() == 2 {
                    touch = TouchPoint {
                        active: true,
                        id: 1,
                        x: v[0],
                        y: v[1],
                    };
                }
            }
            "expect" => {
                let (what, value) = rest.split_once('=').unwrap_or((rest, ""));
                match what {
                    "feedback" if feedback > 0 => println!("OK feedback"),
                    "feedback" => return Err("no pad output came back".into()),
                    "closed" if closed => println!("OK closed"),
                    "closed" => return Err("connection still open".into()),
                    _ => {
                        let Some(Message::Library {
                            games,
                            selected,
                            running,
                            ..
                        }) = &library
                        else {
                            return Err(format!("expected {what}, but no library arrived"));
                        };
                        match what {
                            "library" => println!("OK library with {} games", games.len()),
                            "running" if running.as_deref() == Some(value) => {
                                println!("OK running {value}")
                            }
                            "selected" if selected.as_deref() == Some(value) => {
                                println!("OK selected {value}")
                            }
                            _ => {
                                return Err(format!(
                                    "expectation failed: {rest} (running {running:?}, selected {selected:?})"
                                ));
                            }
                        }
                    }
                }
            }
            _ => return Err(format!("unknown script word {word}")),
        }
    }
}

fn short(text: &str) -> String {
    if text.len() > 200 {
        format!("{}... ({} bytes)", &text[..200], text.len())
    } else {
        text.to_string()
    }
}

fn game(options: &BTreeMap<String, String>) -> Result<(), String> {
    let bridge: SocketAddr = options
        .get("bridge")
        .map(String::as_str)
        .unwrap_or("127.0.0.1:47701")
        .parse()
        .map_err(|_| "bad bridge address")?;
    let seconds: u64 = options
        .get("secs")
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    let socket = UdpSocket::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    socket
        .set_read_timeout(Some(Duration::from_millis(50)))
        .map_err(|e| e.to_string())?;
    let until = Instant::now() + Duration::from_secs(seconds);
    let mut last_hello = Instant::now() - Duration::from_secs(2);
    let mut buffer = [0u8; 512];
    let (mut received, mut last_buttons) = (0u32, u32::MAX);
    while Instant::now() < until {
        if last_hello.elapsed() >= Duration::from_secs(1) {
            let _ = socket.send_to(&game_hello(), bridge);
            debug_assert!(is_game_hello(&game_hello()));
            last_hello = Instant::now();
        }
        if let Ok((n, _)) = socket.recv_from(&mut buffer)
            && let Some(input) = InputPacket::parse(&buffer[..n])
        {
            received += 1;
            if input.buttons != last_buttons {
                last_buttons = input.buttons;
                println!(
                    "game: buttons={:#x} sticks={:?} l2={} r2={} touch={:?}",
                    input.buttons, input.sticks, input.l2, input.r2, input.touch[0]
                );
            }
            let answer = OutputPacket {
                echo_sequence: input.sequence,
                echo_sent_micros: input.sent_micros,
                large_motor: if input.buttons & button::CROSS != 0 {
                    200
                } else {
                    0
                },
                light_valid: true,
                light: [30, 30, 255],
                ..OutputPacket::default()
            };
            let _ = socket.send_to(&answer.encode(), bridge);
        }
    }
    println!("game done, {received} pad packets");
    if received == 0 {
        return Err("no pad packet reached the game".into());
    }
    Ok(())
}
