//! Ported from utkuhalis/AnyPS5-ARM (DiscordPresence.swift, GPL-2.0-only) and changed since; see README "Credits".
//! "Playing <game>" on the player's Discord profile, ported from AnyPS5-ARM
//! (`DiscordPresence.swift`): the Discord app's local IPC pipe carries frames of a little-endian
//! opcode and length followed by JSON. Without a running Discord nothing happens, and a Discord
//! started later is found on the next game.
//!
//! A game Discord knows (its public list of detectable applications) shows as itself, with
//! AnyPlay under it; any other game shows under the AnyPlay application. A player who plays
//! online gets "on AnyPlay", the size of the party and, when the network issued a join code, a
//! "Join on AnyPlay" button (docs/DESIGN.md 7.4).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Seconds since the epoch, as Discord's timestamps want them.
pub fn unix_seconds(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

use serde_json::{Value, json};

/// AnyPlay's own application in the Discord Developer Portal. (Until this switch the launcher
/// shared the AnyPS5 application, `1562749843724566549`, with the macOS launcher.)
pub const ANYPLAY_APPLICATION_ID: &str = "1563714917998133288";
/// The large image of an activity shown under AnyPlay's application: an asset of that
/// application, named by its key.
const IMAGE_KEY: &str = "anyplay";
const IMAGE_TEXT: &str = "AnyPlay";
/// The buttons of an activity: the first opens the join link, the second tells a friend without
/// the launcher where to get it. Discord shows two at most, each label up to 32 characters and
/// each address up to 512, both over https.
pub const JOIN_LABEL: &str = "Join on AnyPlay";
pub const GET_LABEL: &str = "Get AnyPlay";
pub const GET_URL: &str = "https://github.com/anyplaynet/launcher/releases/latest";
const MAX_LABEL: usize = 32;
const MAX_BUTTON_URL: usize = 512;
const _: () = assert!(JOIN_LABEL.len() <= MAX_LABEL && GET_LABEL.len() <= MAX_LABEL);
/// Discord's public list of the games it detects.
const DETECTABLE_URL: &str = "https://discord.com/api/v9/applications/detectable";
/// The cached list is fetched again once it is a week old.
const MAX_AGE: Duration = Duration::from_secs(7 * 24 * 3600);
/// Opcodes of the IPC protocol.
const OP_HANDSHAKE: u32 = 0;
const OP_FRAME: u32 = 1;

/// One frame: opcode and length (both little-endian `u32`), then the JSON.
pub fn encode(op: u32, json: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(8 + json.len());
    frame.extend_from_slice(&op.to_le_bytes());
    frame.extend_from_slice(&(json.len() as u32).to_le_bytes());
    frame.extend_from_slice(json);
    frame
}

/// The opcode and JSON of a complete frame.
pub fn decode(frame: &[u8]) -> Option<(u32, &[u8])> {
    let op = u32::from_le_bytes(frame.get(0..4)?.try_into().ok()?);
    let length = u32::from_le_bytes(frame.get(4..8)?.try_into().ok()?) as usize;
    (frame.len() == 8 + length).then(|| (op, &frame[8..]))
}

/// The games Discord knows, by normalised name and alias.
#[derive(Clone, Debug, Default)]
pub struct DiscordGames {
    ids: HashMap<String, String>,
}

impl DiscordGames {
    /// From the list as Discord serves it, or as the launcher caches it: objects with an `id`,
    /// a `name` and `aliases`. The first entry wins a name.
    pub fn from_json(json: &[u8]) -> Self {
        let mut ids = HashMap::new();
        let entries: Vec<Value> = serde_json::from_slice(json).unwrap_or_default();
        for entry in &entries {
            let (Some(id), Some(name)) = (entry["id"].as_str(), entry["name"].as_str()) else {
                continue;
            };
            let aliases = entry["aliases"].as_array().into_iter().flatten();
            for key in std::iter::once(name).chain(aliases.filter_map(Value::as_str)) {
                let normal = normalized(key);
                if !normal.is_empty() {
                    ids.entry(normal).or_insert_with(|| id.to_owned());
                }
            }
        }
        Self { ids }
    }

    pub fn application_id(&self, game: &str) -> Option<&str> {
        self.ids.get(&normalized(game)).map(String::as_str)
    }

    /// The cached list, fetched again from Discord when it is missing or a week old. Without a
    /// network the cache, or an empty list, stays.
    pub fn load(cache: &Path) -> Self {
        let cached = std::fs::read(cache).ok();
        let age = std::fs::metadata(cache)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok());
        if let Some(cached) = &cached
            && age.is_some_and(|a| a < MAX_AGE)
        {
            return Self::from_json(cached);
        }
        match fetch() {
            Some(slim) => {
                if let Some(parent) = cache.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                // Written next to the cache and renamed, so a reader never sees half a file.
                let partial = cache.with_extension("partial");
                let written =
                    std::fs::write(&partial, &slim).and_then(|()| std::fs::rename(&partial, cache));
                if let Err(err) = written {
                    tracing::debug!("cannot cache the Discord game list: {err}");
                }
                Self::from_json(&slim)
            }
            None => cached.map_or_else(Self::default, |c| Self::from_json(&c)),
        }
    }
}

/// Discord's list cut down to id, name and aliases.
fn fetch() -> Option<Vec<u8>> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .new_agent();
    let mut response = agent.get(DETECTABLE_URL).call().ok()?;
    let text = response
        .body_mut()
        .with_config()
        .limit(64 << 20)
        .read_to_string()
        .ok()?;
    let entries: Vec<Value> = serde_json::from_str(&text).ok()?;
    if entries.is_empty() {
        return None;
    }
    let slim: Vec<Value> = entries
        .iter()
        .map(|e| json!({"id": e["id"], "name": e["name"], "aliases": e["aliases"]}))
        .collect();
    serde_json::to_vec(&slim).ok()
}

/// Lowercase letters and digits as words, so "Plants vs. Zombies™: Replanted" finds
/// "Plants vs. Zombies: Replanted".
fn normalized(name: &str) -> String {
    name.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// What the player plays, for the profile.
#[derive(Clone, PartialEq, Eq)]
pub struct Playing {
    pub game: String,
    pub since_unix: i64,
    /// Set while the player plays through AnyPlay's network (signed in, in a session or party).
    pub online: Option<Online>,
}

/// The network side of what is shown.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Online {
    /// Members and room of the player's party, when in one.
    pub party: Option<(usize, u16)>,
    /// The join link (it holds the join code), when the network issued one and the player allows
    /// the button.
    pub join_url: Option<String>,
}

// The link is a secret of a sort and is never printed.
impl std::fmt::Debug for Online {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Online")
            .field("party", &self.party)
            .field("join", &self.join_url.is_some())
            .finish()
    }
}

impl std::fmt::Debug for Playing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Playing")
            .field("game", &self.game)
            .field("online", &self.online)
            .finish_non_exhaustive()
    }
}

/// The second line of an online activity: "on AnyPlay", then the party as "Party 2 of 4".
fn online_state(online: &Online) -> String {
    match online.party {
        Some((members, room)) => format!("on AnyPlay · Party {members} of {room}"),
        None => "on AnyPlay".to_owned(),
    }
}

/// The two buttons for a join link, or none when the link is not one Discord would accept.
fn buttons(join_url: &str) -> Option<Value> {
    let acceptable = join_url.starts_with("https://")
        && join_url.len() <= MAX_BUTTON_URL
        && !join_url
            .chars()
            .any(|c| c.is_control() || c.is_whitespace());
    acceptable.then(|| {
        json!([
            {"label": JOIN_LABEL, "url": join_url},
            {"label": GET_LABEL, "url": GET_URL},
        ])
    })
}

/// The activity for a game, and the application to show it as: the game's own when Discord
/// knows it, AnyPlay's otherwise.
///
/// - Offline, a known game has the launcher as details; an unknown one has its name as details
///   and the launcher as state.
/// - Online, both have the name as details and "on AnyPlay" (with the party) as state. A known
///   game keeps Discord's own header and image; one under AnyPlay's application gets AnyPlay's
///   image.
/// - The buttons come only with a join link. The activity never carries a party or secrets, as
///   Discord would then ignore the buttons; the party size is part of the text.
pub fn activity(playing: &Playing, games: &DiscordGames) -> (Value, String) {
    let mut activity = json!({"timestamps": {"start": playing.since_unix}, "type": 0});
    let known = games.application_id(&playing.game);
    match (&playing.online, known) {
        (None, Some(_)) => activity["details"] = json!("AnyPlay Launcher · Windows"),
        (None, None) => {
            activity["details"] = json!(playing.game);
            activity["state"] = json!("AnyPlay Launcher · Windows");
        }
        (Some(online), _) => {
            activity["details"] = json!(playing.game);
            activity["state"] = json!(online_state(online));
            if let Some(buttons) = online.join_url.as_deref().and_then(buttons) {
                activity["buttons"] = buttons;
            }
        }
    }
    match known {
        Some(id) => (activity, id.to_owned()),
        None => {
            activity["assets"] = json!({"large_image": IMAGE_KEY, "large_text": IMAGE_TEXT});
            (activity, ANYPLAY_APPLICATION_ID.to_owned())
        }
    }
}

enum Message {
    Playing(Playing),
    Clear,
}

/// The presence. Calls only send a message: the pipe, the handshake and the game list live on
/// a thread of their own, so nothing here ever waits for Discord.
pub struct Presence {
    tx: Sender<Message>,
}

impl Presence {
    /// Starts the worker; `cache` holds Discord's game list between launches.
    pub fn spawn(cache: PathBuf) -> Self {
        let (tx, rx) = channel();
        let spawned = std::thread::Builder::new()
            .name("discord".into())
            .spawn(move || Worker::new(cache).run(rx));
        if let Err(err) = spawned {
            tracing::warn!("cannot start the Discord thread: {err}");
        }
        Self { tx }
    }

    /// Shows what the player plays; sent again, it replaces what was shown.
    pub fn playing(&self, playing: Playing) {
        let _ = self.tx.send(Message::Playing(playing));
    }

    pub fn clear(&self) {
        let _ = self.tx.send(Message::Clear);
    }
}

/// A connection to the Discord app, shaken hands as one application.
struct Connection {
    pipe: Box<dyn Pipe>,
    application: String,
}

trait Pipe: Read + Write + Send {}
impl<T: Read + Write + Send> Pipe for T {}

struct Worker {
    cache: PathBuf,
    games: Option<DiscordGames>,
    connection: Option<Connection>,
    nonce: u64,
}

impl Worker {
    fn new(cache: PathBuf) -> Self {
        Self {
            cache,
            games: None,
            connection: None,
            nonce: 0,
        }
    }

    fn run(mut self, rx: Receiver<Message>) {
        while let Ok(message) = rx.recv() {
            match message {
                Message::Playing(playing) => {
                    let games = self
                        .games
                        .get_or_insert_with(|| DiscordGames::load(&self.cache));
                    let (activity, application) = activity(&playing, games);
                    self.set_activity(activity, &application);
                }
                Message::Clear => {
                    if let Some(application) =
                        self.connection.as_ref().map(|c| c.application.clone())
                    {
                        self.set_activity(Value::Null, &application);
                    }
                }
            }
        }
    }

    fn set_activity(&mut self, activity: Value, application: &str) {
        // Discord ties a connection to the application it shook hands as.
        if self
            .connection
            .as_ref()
            .is_some_and(|c| c.application != application)
        {
            self.connection = None;
        }
        self.nonce += 1;
        let command = json!({
            "cmd": "SET_ACTIVITY",
            "args": {"pid": std::process::id(), "activity": activity},
            "nonce": format!("anyps5-{}-{}", std::process::id(), self.nonce),
        });
        let Ok(body) = serde_json::to_vec(&command) else {
            return;
        };
        // A Discord that restarted closed the old pipe: connect once more and send again.
        for _ in 0..2 {
            let Some(connection) = self.connected(application) else {
                return;
            };
            if send(connection.pipe.as_mut(), OP_FRAME, &body)
                && receive(connection.pipe.as_mut()).is_some()
            {
                return;
            }
            self.connection = None;
        }
    }

    /// The open connection, or a new one to the first Discord pipe that answers the handshake.
    fn connected(&mut self, application: &str) -> Option<&mut Connection> {
        if self.connection.is_none() {
            self.connection = (0..10).find_map(|index| {
                let mut pipe = open_pipe(index)?;
                let handshake =
                    serde_json::to_vec(&json!({"v": 1, "client_id": application})).ok()?;
                if send(pipe.as_mut(), OP_HANDSHAKE, &handshake)
                    && receive(pipe.as_mut()).is_some_and(|(op, _)| op == OP_FRAME)
                {
                    Some(Connection {
                        pipe,
                        application: application.to_owned(),
                    })
                } else {
                    None
                }
            });
        }
        self.connection.as_mut()
    }
}

fn send(pipe: &mut dyn Pipe, op: u32, json: &[u8]) -> bool {
    pipe.write_all(&encode(op, json)).is_ok() && pipe.flush().is_ok()
}

fn receive(pipe: &mut dyn Pipe) -> Option<(u32, Vec<u8>)> {
    let mut header = [0u8; 8];
    pipe.read_exact(&mut header).ok()?;
    let length = u32::from_le_bytes(header[4..8].try_into().ok()?) as usize;
    if length >= 1 << 20 {
        return None;
    }
    let mut body = vec![0u8; length];
    pipe.read_exact(&mut body).ok()?;
    let op = u32::from_le_bytes(header[0..4].try_into().ok()?);
    Some((op, body))
}

/// `\\.\pipe\discord-ipc-N` on Windows.
#[cfg(windows)]
fn open_pipe(index: u32) -> Option<Box<dyn Pipe>> {
    let path = format!(r"\\.\pipe\discord-ipc-{index}");
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .ok()?;
    Some(Box::new(file))
}

/// `discord-ipc-N` in the runtime folder, the Flatpak and Snap folders below it, or the temporary
/// folder elsewhere (see [`crate::platform::discord_socket_candidates`]).
#[cfg(not(windows))]
fn open_pipe(index: u32) -> Option<Box<dyn Pipe>> {
    crate::platform::discord_socket_candidates(index, &crate::platform::process_vars)
        .into_iter()
        .find_map(|socket| std::os::unix::net::UnixStream::connect(socket).ok())
        .and_then(|stream| {
            stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
            Some(Box::new(stream) as Box<dyn Pipe>)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let frame = encode(1, br#"{"a":1}"#);
        assert_eq!(&frame[0..4], &[1, 0, 0, 0]);
        assert_eq!(&frame[4..8], &[7, 0, 0, 0]);
        assert_eq!(decode(&frame), Some((1, &br#"{"a":1}"#[..])));
        assert_eq!(decode(&frame[..9]), None);
        assert_eq!(decode(&[1, 2]), None);
    }

    #[test]
    fn games_are_found_by_name_and_alias_whatever_the_punctuation() {
        let games = DiscordGames::from_json(
            br#"[
                {"id": "1", "name": "Plants vs. Zombies: Replanted", "aliases": []},
                {"id": "2", "name": "Ghost of Yotei", "aliases": ["Ghost of Y\u014dtei"]},
                {"id": "3", "name": "Plants vs Zombies Replanted", "aliases": []},
                {"name": "No id"}
            ]"#,
        );
        assert_eq!(
            games.application_id("Plants vs. Zombies\u{2122}: Replanted"),
            Some("1")
        );
        assert_eq!(games.application_id("GHOST OF YOTEI"), Some("2"));
        assert_eq!(games.application_id("Ghost of Y\u{14d}tei"), Some("2"));
        assert_eq!(games.application_id("Unknown"), None);
        assert_eq!(
            DiscordGames::from_json(b"not json").application_id("x"),
            None
        );
    }

    #[test]
    fn known_games_show_as_themselves_others_under_anyplay() {
        let games = DiscordGames::from_json(br#"[{"id": "42", "name": "Stray", "aliases": []}]"#);
        let (known, as_known) = activity(&offline("Stray"), &games);
        assert_eq!(as_known, "42");
        assert_eq!(known["details"], "AnyPlay Launcher · Windows");
        assert!(known.get("state").is_none());
        assert!(
            known.get("assets").is_none(),
            "the game keeps its own image"
        );
        assert_eq!(known["timestamps"]["start"], 100);
        let (other, as_other) = activity(&offline("Hollow Orchard"), &games);
        assert_eq!(as_other, ANYPLAY_APPLICATION_ID);
        assert_eq!(other["details"], "Hollow Orchard");
        assert_eq!(other["state"], "AnyPlay Launcher · Windows");
        assert_eq!(other["type"], 0);
        assert!(other.get("buttons").is_none() && other.get("party").is_none());
    }

    fn offline(game: &str) -> Playing {
        Playing {
            game: game.to_owned(),
            since_unix: 100,
            online: None,
        }
    }

    fn online(game: &str, party: Option<(usize, u16)>, join: Option<&str>) -> Playing {
        Playing {
            online: Some(Online {
                party,
                join_url: join.map(str::to_owned),
            }),
            ..offline(game)
        }
    }

    const JOIN: &str = "https://anyplaynet.com/join/k3x9m2pqra";

    #[test]
    fn anyplay_has_its_own_application_and_image() {
        assert_eq!(ANYPLAY_APPLICATION_ID, "1563714917998133288");
        let (other, application) = activity(&offline("Hollow Orchard"), &DiscordGames::default());
        assert_eq!(application, "1563714917998133288");
        assert_eq!(other["assets"]["large_image"], "anyplay");
        assert_eq!(other["assets"]["large_text"], "AnyPlay");
    }

    #[test]
    fn playing_online_says_so_with_the_party() {
        let games = DiscordGames::from_json(br#"[{"id": "42", "name": "Stray", "aliases": []}]"#);
        let (alone, _) = activity(&online("Hollow Orchard", None, None), &games);
        assert_eq!(alone["details"], "Hollow Orchard");
        assert_eq!(alone["state"], "on AnyPlay");
        assert!(alone.get("buttons").is_none(), "no link, no buttons");
        let (party, application) = activity(&online("Hollow Orchard", Some((2, 4)), None), &games);
        assert_eq!(party["state"], "on AnyPlay · Party 2 of 4");
        assert_eq!(application, ANYPLAY_APPLICATION_ID);
        assert_eq!(party["assets"]["large_image"], "anyplay");
        assert_eq!(party["timestamps"]["start"], 100);
    }

    #[test]
    fn a_known_game_online_keeps_its_header_and_gets_the_same_lines() {
        let games = DiscordGames::from_json(br#"[{"id": "42", "name": "Stray", "aliases": []}]"#);
        let (known, application) = activity(&online("Stray", Some((3, 8)), None), &games);
        assert_eq!(application, "42");
        assert_eq!(known["details"], "Stray");
        assert_eq!(known["state"], "on AnyPlay · Party 3 of 8");
        assert!(known.get("assets").is_none());
    }

    #[test]
    fn the_buttons_join_and_get() {
        let (activity, _) = activity(
            &online("Hollow Orchard", Some((1, 4)), Some(JOIN)),
            &DiscordGames::default(),
        );
        assert_eq!(
            activity["buttons"],
            json!([
                {"label": "Join on AnyPlay", "url": JOIN},
                {"label": "Get AnyPlay", "url": GET_URL},
            ])
        );
        // Discord ignores buttons next to a party or secrets, so the activity has neither.
        assert!(activity.get("party").is_none() && activity.get("secrets").is_none());
    }

    #[test]
    fn buttons_keep_to_discords_limits() {
        assert!(JOIN_LABEL.chars().count() <= MAX_LABEL);
        assert!(GET_LABEL.chars().count() <= MAX_LABEL);
        assert!(GET_URL.starts_with("https://") && GET_URL.len() <= MAX_BUTTON_URL);
        let long = format!("https://anyplaynet.com/join/{}", "a".repeat(MAX_BUTTON_URL));
        for refused in [
            "http://anyplaynet.com/join/k3x9m2pqra",
            "ftp://anyplaynet.com/join/k3x9m2pqra",
            "javascript:alert(1)",
            "https://anyplaynet.com/join/k3x9m2pqra extra",
            "https://anyplaynet.com/join/k3x9m2pqra\n",
            "",
            long.as_str(),
        ] {
            assert!(buttons(refused).is_none(), "{refused:?}");
            let (activity, _) = activity(
                &online("Hollow Orchard", None, Some(refused)),
                &DiscordGames::default(),
            );
            assert!(activity.get("buttons").is_none(), "{refused:?}");
        }
        let at_the_limit = format!(
            "https://a.b/{}",
            "x".repeat(MAX_BUTTON_URL - "https://a.b/".len())
        );
        assert_eq!(at_the_limit.len(), MAX_BUTTON_URL);
        assert!(buttons(&at_the_limit).is_some());
        let two = buttons(JOIN).unwrap();
        assert_eq!(two.as_array().map(Vec::len), Some(2), "two buttons at most");
    }

    #[test]
    fn debug_output_never_holds_the_link() {
        let text = format!("{:?}", online("Hollow Orchard", Some((1, 4)), Some(JOIN)));
        assert!(
            !text.contains("k3x9m2pqra") && !text.contains("anyplaynet"),
            "{text}"
        );
    }

    /// A Discord stand-in: records what is written and answers every frame once.
    struct FakeDiscord {
        written: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
        replies: std::io::Cursor<Vec<u8>>,
    }

    impl Read for FakeDiscord {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.replies.read(buf)
        }
    }

    impl Write for FakeDiscord {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.written.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn set_activity_sends_one_command_frame_and_reads_the_answer() {
        let written = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let answer = encode(OP_FRAME, br#"{"cmd":"SET_ACTIVITY"}"#);
        let mut worker = Worker::new(PathBuf::new());
        worker.connection = Some(Connection {
            pipe: Box::new(FakeDiscord {
                written: written.clone(),
                replies: std::io::Cursor::new(answer),
            }),
            application: ANYPLAY_APPLICATION_ID.to_owned(),
        });
        let (activity, application) =
            activity(&offline("Hollow Orchard"), &DiscordGames::default());
        worker.set_activity(activity, &application);
        let bytes = written.lock().unwrap().clone();
        let (op, json) = decode(&bytes).expect("one complete frame");
        assert_eq!(op, OP_FRAME);
        let command: Value = serde_json::from_slice(json).unwrap();
        assert_eq!(command["cmd"], "SET_ACTIVITY");
        assert_eq!(command["args"]["pid"], std::process::id());
        assert_eq!(command["args"]["activity"]["details"], "Hollow Orchard");
        assert!(worker.connection.is_some(), "the connection stays open");
    }

    #[test]
    fn the_set_activity_frame_carries_the_buttons() {
        let written = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut worker = Worker::new(PathBuf::new());
        worker.connection = Some(Connection {
            pipe: Box::new(FakeDiscord {
                written: written.clone(),
                replies: std::io::Cursor::new(encode(OP_FRAME, br#"{"cmd":"SET_ACTIVITY"}"#)),
            }),
            application: ANYPLAY_APPLICATION_ID.to_owned(),
        });
        let playing = online("Hollow Orchard", Some((2, 4)), Some(JOIN));
        let (activity, application) = activity(&playing, &DiscordGames::default());
        assert_eq!(application, ANYPLAY_APPLICATION_ID);
        worker.set_activity(activity, &application);
        let bytes = written.lock().unwrap().clone();
        let (_, json) = decode(&bytes).expect("one complete frame");
        let command: Value = serde_json::from_slice(json).unwrap();
        let sent = &command["args"]["activity"];
        assert_eq!(sent["state"], "on AnyPlay · Party 2 of 4");
        assert_eq!(sent["buttons"][0]["label"], "Join on AnyPlay");
        assert_eq!(sent["buttons"][0]["url"], JOIN);
        assert_eq!(sent["buttons"][1]["label"], "Get AnyPlay");
        assert_eq!(sent["assets"]["large_image"], "anyplay");
    }

    /// Against a running Discord: only the handshake, which shows nothing on the profile.
    /// `cargo test -p anyps5-core -- --ignored discord_handshake`
    #[test]
    #[ignore = "needs a running Discord"]
    fn discord_handshake() {
        let mut worker = Worker::new(PathBuf::new());
        assert!(
            worker.connected(ANYPLAY_APPLICATION_ID).is_some(),
            "no Discord pipe answered the handshake"
        );
    }

    #[test]
    fn a_fresh_cache_is_used_without_the_network() {
        let dir = std::env::temp_dir().join(format!("anyps5-discord-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cache = dir.join("discord-games.json");
        std::fs::write(
            &cache,
            br#"[{"id": "7", "name": "Astro Bot", "aliases": []}]"#,
        )
        .unwrap();
        assert_eq!(
            DiscordGames::load(&cache).application_id("Astro Bot"),
            Some("7")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
