//! Party voice as the launcher knows it (network `spec/control-protocol.md`, `spec/voice.md`,
//! ADR 0014 and 0017). The net service opens the microphone and the speakers, mixes the members
//! and reports who speaks; the launcher only chooses and shows. This module holds
//!
//! - what the player chose ([`VoicePrefs`], kept in the preferences): voice on or off, the mode,
//!   the push-to-talk key, the devices and each member's volume and mute;
//! - what the service reported ([`Voice`]): the state, the levels, the devices;
//! - the commands that carry a choice out, as methods of [`Online`].
//!
//! A new net service starts with voice off and knows nothing of the choices, so every one of
//! them goes out again after its `hello` is answered ([`Online::voice_hello`]).

use anyplay_control::{Command, DeviceKind, VoiceLevel, VoiceMode, VoicePeer, VoiceState};
use serde::{Deserialize, Serialize};

use super::api::AccountId;
use super::{Notice, Online, Purpose};

/// How loud a member is heard, in percent of what they send: the net service takes 0 to 200.
pub const MAX_PERCENT: u16 = 200;
pub const DEFAULT_PERCENT: u16 = 100;
/// The step of the volume row.
pub const PERCENT_STEP: u16 = 10;

/// The key held to talk until the player picks another.
pub const DEFAULT_KEY: &str = "V";

/// Settings for this many members are kept; more would be the preferences file growing without
/// bound for nobody's benefit.
const MAX_PEERS: usize = 256;

/// The volume and mute the player chose for one member.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerPrefs {
    pub account: AccountId,
    pub percent: u16,
    pub muted: bool,
}

/// Where the player talks with their party: AnyPlay's own voice, or Discord, in which case
/// AnyPlay never opens the microphone for a party or a call.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceProvider {
    #[default]
    AnyPlay,
    Discord,
}

/// What the player chose for party voice. Lasts across launches and parties.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoicePrefs {
    pub enabled: bool,
    /// AnyPlay (the default) or Discord.
    #[serde(default)]
    pub provider: VoiceProvider,
    #[serde(default)]
    pub mode: VoiceMode,
    /// The microphone by name; `None` is the system's default.
    #[serde(default)]
    pub input: Option<String>,
    /// The speakers by name; `None` is the system's default.
    #[serde(default)]
    pub output: Option<String>,
    /// The push-to-talk key by name (an egui key name); `None` is [`DEFAULT_KEY`].
    #[serde(default)]
    pub key: Option<String>,
    /// Members whose volume or mute is not the default.
    #[serde(default)]
    pub peers: Vec<PeerPrefs>,
}

impl VoicePrefs {
    pub fn key(&self) -> &str {
        self.key.as_deref().unwrap_or(DEFAULT_KEY)
    }

    /// The volume in percent and the mute of one member.
    pub fn peer(&self, account: AccountId) -> (u16, bool) {
        self.peers
            .iter()
            .find(|p| p.account == account)
            .map_or((DEFAULT_PERCENT, false), |p| (p.percent, p.muted))
    }

    fn set_peer(&mut self, account: AccountId, change: impl FnOnce(&mut u16, &mut bool)) {
        let (mut percent, mut muted) = self.peer(account);
        change(&mut percent, &mut muted);
        percent = percent.min(MAX_PERCENT);
        self.peers.retain(|p| p.account != account);
        if percent != DEFAULT_PERCENT || muted {
            if self.peers.len() >= MAX_PEERS {
                self.peers.remove(0);
            }
            self.peers.push(PeerPrefs {
                account,
                percent,
                muted,
            });
        }
    }
}

/// The sound devices the net service found, and which are chosen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Devices {
    pub input: Vec<String>,
    pub output: Vec<String>,
    pub selected_input: Option<String>,
    pub selected_output: Option<String>,
}

impl From<anyplay_control::VoiceDevices> for Devices {
    fn from(devices: anyplay_control::VoiceDevices) -> Self {
        Self {
            input: devices.input,
            output: devices.output,
            selected_input: devices.selected_input,
            selected_output: devices.selected_output,
        }
    }
}

/// Whether this PC and this net service can do voice at all, as far as the device lists say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hardware {
    /// Not asked yet, or the service has not answered.
    Unknown,
    Ready,
    /// There are speakers but no microphone.
    NoMicrophone,
    /// Neither: a PC without sound, or a net service built without sound devices.
    NoSound,
}

/// Why the net service switched voice off by itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    /// What the service said, for the log and for a note under the toggle.
    pub message: String,
    /// The reason reads like the system refused access to the microphone.
    pub denied: bool,
}

impl Problem {
    fn new(message: &str) -> Self {
        let lower = message.to_lowercase();
        Self {
            denied: ["denied", "permission", "not allowed", "privacy", "access"]
                .iter()
                .any(|word| lower.contains(word)),
            message: message.to_owned(),
        }
    }
}

/// Where voice stands, for the toggle's detail line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Off,
    /// The player talks on Discord: AnyPlay opens no microphone.
    Discord,
    /// Switched on, but the player is in no party: voice starts with the next one.
    NoParty,
    /// The service could not open the devices and switched voice off.
    Failed(Problem),
    Connecting,
    On,
}

/// The state of the player's own microphone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mic {
    /// Voice is not running: nothing is open.
    Off,
    /// Push to talk, key up: nobody hears the player.
    Muted,
    /// Push to talk, key down, nothing said.
    Open,
    /// Voice activity, nothing said.
    Listening,
    /// Speech goes out.
    Talking,
}

/// What the launcher keeps of party voice.
#[derive(Clone, Debug, Default)]
pub struct Voice {
    prefs: VoicePrefs,
    /// The preferences changed since [`Voice::take_changed`].
    changed: bool,
    pub state: VoiceState,
    pub me: VoiceLevel,
    pub peers: Vec<VoicePeer>,
    pub devices: Option<Devices>,
    pub problem: Option<Problem>,
    /// The player muted their own microphone.
    pub muted: bool,
    /// The player hears nobody.
    pub deafened: bool,
    /// The last push-to-talk state sent to the service.
    talking: bool,
}

impl Voice {
    pub fn prefs(&self) -> &VoicePrefs {
        &self.prefs
    }

    /// Takes the choices from the preferences (at start).
    pub fn load(&mut self, prefs: VoicePrefs) {
        self.prefs = prefs;
        self.changed = false;
    }

    /// True once after the preferences changed, so the caller saves them.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// Forgets what the service reported, keeping what the player chose.
    pub(super) fn reset(&mut self) {
        *self = Voice {
            prefs: std::mem::take(&mut self.prefs),
            changed: self.changed,
            ..Voice::default()
        };
    }

    pub fn enabled(&self) -> bool {
        self.prefs.enabled
    }

    pub fn provider(&self) -> VoiceProvider {
        self.prefs.provider
    }

    /// Voice runs on AnyPlay's side: the player switched it on and does not use Discord.
    pub fn runs_here(&self) -> bool {
        self.prefs.enabled && self.prefs.provider == VoiceProvider::AnyPlay
    }

    pub fn mode(&self) -> VoiceMode {
        self.prefs.mode
    }

    pub fn status(&self, in_party: bool) -> Status {
        if self.prefs.provider == VoiceProvider::Discord {
            return Status::Discord;
        }
        if !self.prefs.enabled {
            return match &self.problem {
                Some(problem) => Status::Failed(problem.clone()),
                None => Status::Off,
            };
        }
        if !in_party {
            return Status::NoParty;
        }
        match self.state {
            VoiceState::On => Status::On,
            VoiceState::Off | VoiceState::Connecting => Status::Connecting,
        }
    }

    pub fn mic(&self, in_party: bool) -> Mic {
        if !matches!(self.status(in_party), Status::On | Status::Connecting) {
            return Mic::Off;
        }
        match (self.prefs.mode, self.me.speaking) {
            (_, true) => Mic::Talking,
            (VoiceMode::PushToTalk, false) if self.talking => Mic::Open,
            (VoiceMode::PushToTalk, false) => Mic::Muted,
            (VoiceMode::VoiceActivity, false) => Mic::Listening,
        }
    }

    /// The level of a member who is in the voice, if the service reports one.
    pub fn peer(&self, account: AccountId) -> Option<&VoicePeer> {
        self.peers.iter().find(|p| p.account == account)
    }

    /// True when the member is heard right now: reported speaking and not muted by the player.
    pub fn is_speaking(&self, account: AccountId) -> bool {
        !self.prefs.peer(account).1 && self.peer(account).is_some_and(|p| p.level.speaking)
    }

    pub fn hardware(&self) -> Hardware {
        match &self.devices {
            None => Hardware::Unknown,
            Some(d) if d.input.is_empty() && d.output.is_empty() => Hardware::NoSound,
            Some(d) if d.input.is_empty() => Hardware::NoMicrophone,
            Some(_) => Hardware::Ready,
        }
    }
}

impl Online {
    pub(super) fn voice_event(&mut self, state: VoiceState, me: VoiceLevel, peers: Vec<VoicePeer>) {
        self.voice.state = state;
        self.voice.me = me;
        self.voice.peers = peers;
    }

    /// The service could not run voice and switched it off.
    pub(super) fn voice_failed(&mut self, message: &str) -> Notice {
        let problem = Problem::new(message);
        self.voice.problem = Some(problem.clone());
        self.voice.talking = false;
        if self.voice.prefs.enabled {
            self.voice.prefs.enabled = false;
            self.voice.changed = true;
        }
        Notice::VoiceFailed(problem)
    }

    /// Sends every choice to a service that just started.
    pub(super) fn voice_hello(&mut self) {
        let prefs = self.voice.prefs.clone();
        if prefs.mode != VoiceMode::default() {
            self.send(Purpose::Voice, Command::VoiceMode { mode: prefs.mode });
        }
        for (kind, name) in [
            (DeviceKind::Input, prefs.input.clone()),
            (DeviceKind::Output, prefs.output.clone()),
        ] {
            if name.is_some() {
                self.send(
                    Purpose::SetVoiceDevice(kind),
                    Command::SetVoiceDevice { kind, name },
                );
            }
        }
        for peer in &prefs.peers {
            if peer.percent != DEFAULT_PERCENT {
                self.send(
                    Purpose::Voice,
                    Command::PeerVolume {
                        account: peer.account,
                        percent: peer.percent,
                    },
                );
            }
            if peer.muted {
                self.send(
                    Purpose::Voice,
                    Command::PeerMute {
                        account: peer.account,
                        muted: true,
                    },
                );
            }
        }
        if prefs.enabled && prefs.provider == VoiceProvider::AnyPlay {
            self.send(Purpose::Voice, Command::Voice { on: true });
        }
        self.request_voice_devices();
    }

    /// Asks which microphones and speakers there are.
    pub fn request_voice_devices(&mut self) {
        if !self.is_waiting(|p| *p == Purpose::VoiceDevices) {
            self.send(Purpose::VoiceDevices, Command::VoiceDevices);
        }
    }

    /// Switches party voice on or off. It stays as chosen across parties and launches.
    pub fn set_voice(&mut self, on: bool) {
        if self.voice.prefs.enabled != on {
            self.voice.prefs.enabled = on;
            self.voice.changed = true;
        }
        if on {
            self.voice.problem = None;
        } else {
            self.voice.talking = false;
        }
        if on && self.voice.prefs.provider == VoiceProvider::Discord {
            // On Discord the microphone stays closed whatever the toggle says.
            return;
        }
        self.send(Purpose::Voice, Command::Voice { on });
    }

    /// Chooses where the player talks. Discord switches AnyPlay's voice off; AnyPlay switches it
    /// on again if the player had it on.
    pub fn set_voice_provider(&mut self, provider: VoiceProvider) {
        if self.voice.prefs.provider == provider {
            return;
        }
        self.voice.prefs.provider = provider;
        self.voice.changed = true;
        self.voice.talking = false;
        let on = provider == VoiceProvider::AnyPlay && self.voice.prefs.enabled;
        self.send(Purpose::Voice, Command::Voice { on });
    }

    pub fn set_voice_mode(&mut self, mode: VoiceMode) {
        if self.voice.prefs.mode == mode {
            return;
        }
        self.voice.prefs.mode = mode;
        self.voice.changed = true;
        if mode == VoiceMode::VoiceActivity {
            // A key that is down means nothing in this mode: the service ignores it, and the next
            // switch back starts with the key up.
            self.release_talk();
        }
        self.send(Purpose::Voice, Command::VoiceMode { mode });
    }

    /// The push-to-talk key, or the default again.
    pub fn set_voice_key(&mut self, key: Option<String>) {
        if self.voice.prefs.key != key {
            self.voice.prefs.key = key;
            self.voice.changed = true;
        }
    }

    /// Chooses a microphone or speakers by name; `None` is the system's default.
    pub fn set_voice_device(&mut self, kind: DeviceKind, name: Option<String>) {
        let (slot, shown) = match kind {
            DeviceKind::Input => (
                &mut self.voice.prefs.input,
                self.voice.devices.as_mut().map(|d| &mut d.selected_input),
            ),
            DeviceKind::Output => (
                &mut self.voice.prefs.output,
                self.voice.devices.as_mut().map(|d| &mut d.selected_output),
            ),
        };
        if *slot != name {
            *slot = name.clone();
            self.voice.changed = true;
        }
        if let Some(shown) = shown {
            *shown = name.clone();
        }
        self.send(
            Purpose::SetVoiceDevice(kind),
            Command::SetVoiceDevice { kind, name },
        );
    }

    /// How loud one member is heard, in percent (0 to 200).
    pub fn set_peer_volume(&mut self, account: AccountId, percent: u16) {
        let percent = percent.min(MAX_PERCENT);
        if self.voice.prefs.peer(account).0 == percent {
            return;
        }
        self.voice.prefs.set_peer(account, |p, _| *p = percent);
        self.voice.changed = true;
        self.send(Purpose::Voice, Command::PeerVolume { account, percent });
    }

    pub fn set_peer_mute(&mut self, account: AccountId, muted: bool) {
        if self.voice.prefs.peer(account).1 == muted {
            return;
        }
        self.voice.prefs.set_peer(account, |_, m| *m = muted);
        self.voice.changed = true;
        self.send(Purpose::Voice, Command::PeerMute { account, muted });
    }

    /// The push-to-talk key is down or up. Only changes go out; a key held while voice is off,
    /// or in voice-activity mode, sends nothing.
    pub fn set_talking(&mut self, held: bool) {
        let wanted =
            held && self.voice.prefs.enabled && self.voice.prefs.mode == VoiceMode::PushToTalk;
        if wanted != self.voice.talking {
            self.voice.talking = wanted;
            self.send(Purpose::PushToTalk, Command::PushToTalk { talking: wanted });
        }
    }

    /// True while the service was told the key is down.
    pub fn is_talking(&self) -> bool {
        self.voice.talking
    }

    fn release_talk(&mut self) {
        if self.voice.talking {
            self.voice.talking = false;
            self.send(Purpose::PushToTalk, Command::PushToTalk { talking: false });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::{Cloud, Event, Line, Output, SignedIn, control};

    fn event(event: Event) -> Line {
        Line::Output(Output::Event(event))
    }

    fn lines(online: &mut Online) -> Vec<String> {
        online.take_outbox()
    }

    fn sent(online: &mut Online, needle: &str) -> bool {
        lines(online).iter().any(|l| l.contains(needle))
    }

    fn voice_event(online: &mut Online, state: VoiceState, speaking: bool, peers: Vec<VoicePeer>) {
        online.apply(event(Event::Voice {
            state,
            enabled: true,
            mode: VoiceMode::PushToTalk,
            muted: false,
            deafened: false,
            me: VoiceLevel {
                level: 40,
                speaking,
            },
            peers,
        }));
    }

    fn peer(id: u64, speaking: bool) -> VoicePeer {
        VoicePeer {
            account: AccountId(id),
            level: VoiceLevel {
                level: if speaking { 60 } else { 0 },
                speaking,
            },
            link: None,
        }
    }

    #[test]
    fn the_toggle_is_kept_and_sent() {
        let mut online = Online::default();
        assert_eq!(online.voice.status(false), Status::Off);
        online.set_voice(true);
        assert!(online.voice.take_changed());
        assert!(!online.voice.take_changed());
        let sent = lines(&mut online);
        assert_eq!(sent.len(), 1);
        assert!(sent[0].contains("\"do\":\"voice\"") && sent[0].contains("\"on\":true"));
        assert_eq!(online.voice.status(false), Status::NoParty);
        assert_eq!(online.voice.status(true), Status::Connecting);
        voice_event(&mut online, VoiceState::On, false, vec![]);
        assert_eq!(online.voice.status(true), Status::On);
    }

    #[test]
    fn the_event_does_not_overrule_a_choice_the_service_has_not_heard_yet() {
        let mut online = Online::default();
        online.set_voice(true);
        online.apply(event(Event::Voice {
            state: VoiceState::Off,
            enabled: false,
            mode: VoiceMode::PushToTalk,
            muted: false,
            deafened: false,
            me: VoiceLevel::default(),
            peers: vec![],
        }));
        assert!(online.voice.enabled());
    }

    #[test]
    fn a_device_failure_switches_voice_off_and_says_why() {
        let mut online = Online::default();
        online.set_voice(true);
        let notices = online.apply(event(Event::Notice {
            code: "voice_device".into(),
            message: "Voice could not start: access to the microphone was denied".into(),
        }));
        assert!(matches!(&notices[..], [Notice::VoiceFailed(p)] if p.denied));
        assert!(!online.voice.enabled());
        assert!(online.voice.take_changed());
        let Status::Failed(problem) = online.voice.status(true) else {
            panic!("not failed");
        };
        assert!(problem.message.contains("denied"));
        // Switching on again clears it.
        online.set_voice(true);
        assert_eq!(online.voice.status(true), Status::Connecting);
        // A reason that does not read like a refusal is not called one.
        assert!(!Problem::new("no input device found").denied);
    }

    #[test]
    fn the_microphone_follows_the_mode_the_key_and_the_gate() {
        let mut online = Online::default();
        assert_eq!(online.voice.mic(true), Mic::Off);
        online.set_voice(true);
        voice_event(&mut online, VoiceState::On, false, vec![]);
        assert_eq!(online.voice.mic(true), Mic::Muted);
        assert_eq!(online.voice.mic(false), Mic::Off);
        online.set_talking(true);
        assert_eq!(online.voice.mic(true), Mic::Open);
        voice_event(&mut online, VoiceState::On, true, vec![]);
        assert_eq!(online.voice.mic(true), Mic::Talking);
        online.set_voice_mode(VoiceMode::VoiceActivity);
        voice_event(&mut online, VoiceState::On, false, vec![]);
        assert_eq!(online.voice.mic(true), Mic::Listening);
    }

    #[test]
    fn push_to_talk_sends_changes_only_and_only_when_it_means_something() {
        let mut online = Online::default();
        // Voice off: a held key says nothing.
        online.set_talking(true);
        assert!(lines(&mut online).is_empty());
        online.set_voice(true);
        lines(&mut online);
        online.set_talking(true);
        online.set_talking(true);
        let down = lines(&mut online);
        assert_eq!(down.len(), 1);
        assert!(down[0].contains("\"talking\":true"));
        online.set_talking(false);
        assert!(sent(&mut online, "\"talking\":false"));
        // Voice activity ignores the key and lets go of one held.
        online.set_talking(true);
        lines(&mut online);
        online.set_voice_mode(VoiceMode::VoiceActivity);
        let out = lines(&mut online);
        assert!(out.iter().any(|l| l.contains("\"talking\":false")));
        assert!(out.iter().any(|l| l.contains("voice_activity")));
        online.set_talking(true);
        assert!(lines(&mut online).is_empty());
        // Switching voice off lets go of the key too.
        online.set_voice_mode(VoiceMode::PushToTalk);
        lines(&mut online);
        online.set_talking(true);
        lines(&mut online);
        online.set_voice(false);
        assert!(!online.is_talking());
    }

    #[test]
    fn member_volume_and_mute_are_kept_only_when_they_differ_from_the_default() {
        let mut online = Online::default();
        let lumen = AccountId(2);
        online.set_peer_volume(lumen, 150);
        let out = lines(&mut online);
        assert!(out[0].contains("peer_volume") && out[0].contains("150"));
        online.set_peer_mute(lumen, true);
        assert_eq!(online.voice.prefs().peer(lumen), (150, true));
        online.set_peer_volume(lumen, 100);
        online.set_peer_mute(lumen, false);
        assert!(online.voice.prefs().peers.is_empty());
        // Beyond the range is cut, the same value again sends nothing.
        online.set_peer_volume(lumen, 900);
        assert_eq!(online.voice.prefs().peer(lumen).0, MAX_PERCENT);
        lines(&mut online);
        online.set_peer_volume(lumen, 900);
        assert!(lines(&mut online).is_empty());
    }

    #[test]
    fn a_muted_member_is_not_shown_speaking() {
        let mut online = Online::default();
        voice_event(
            &mut online,
            VoiceState::On,
            false,
            vec![peer(2, true), peer(3, true), peer(4, false)],
        );
        online.set_peer_mute(AccountId(3), true);
        assert!(online.voice.is_speaking(AccountId(2)));
        assert!(!online.voice.is_speaking(AccountId(3)));
        assert!(!online.voice.is_speaking(AccountId(4)));
        assert!(!online.voice.is_speaking(AccountId(9)));
    }

    #[test]
    fn a_new_service_gets_every_choice_after_its_hello() {
        let mut online = Online::default();
        online.voice.load(VoicePrefs {
            enabled: true,
            mode: VoiceMode::VoiceActivity,
            input: Some("Headset".into()),
            output: None,
            key: Some("F".into()),
            peers: vec![PeerPrefs {
                account: AccountId(2),
                percent: 80,
                muted: true,
            }],
            provider: VoiceProvider::default(),
        });
        assert!(!online.voice.take_changed());
        let id = online.send(
            Purpose::Hello,
            Command::Hello {
                launcher: "x".into(),
                cloud: "https://c.example.org".into(),
            },
        );
        lines(&mut online);
        online.apply(Line::Output(Output::Answer(control::Answer::ok(
            id,
            control::Hello {
                service: "anyplay-net".into(),
                protocol: 0,
            },
        ))));
        let out = lines(&mut online).join("");
        for needle in [
            "voice_activity",
            "\"set_voice_device\"",
            "Headset",
            "peer_volume",
            "peer_mute",
            "\"do\":\"voice\"",
            "voice_devices",
        ] {
            assert!(out.contains(needle), "{needle}: {out}");
        }
        // The output is the default: it is not sent.
        assert!(!out.contains("\"kind\":\"output\""));
    }

    #[test]
    fn the_device_lists_say_what_the_pc_can_do() {
        let mut online = Online::default();
        assert_eq!(online.voice.hardware(), Hardware::Unknown);
        let answer = |input: &[&str], output: &[&str]| control::VoiceDevices {
            input: input.iter().map(|s| (*s).to_owned()).collect(),
            output: output.iter().map(|s| (*s).to_owned()).collect(),
            ..Default::default()
        };
        for (devices, hardware) in [
            (answer(&[], &[]), Hardware::NoSound),
            (answer(&[], &["Speakers"]), Hardware::NoMicrophone),
            (answer(&["Mic"], &["Speakers"]), Hardware::Ready),
        ] {
            let id = online.send(Purpose::VoiceDevices, Command::VoiceDevices);
            online.apply(Line::Output(Output::Answer(control::Answer::ok(
                id, devices,
            ))));
            assert_eq!(online.voice.hardware(), hardware);
        }
    }

    #[test]
    fn choosing_a_device_updates_the_list_and_the_preferences() {
        let mut online = Online::default();
        let id = online.send(Purpose::VoiceDevices, Command::VoiceDevices);
        online.apply(Line::Output(Output::Answer(control::Answer::ok(
            id,
            control::VoiceDevices {
                input: vec!["Mic".into(), "Headset".into()],
                output: vec!["Speakers".into()],
                ..Default::default()
            },
        ))));
        lines(&mut online);
        online.set_voice_device(DeviceKind::Input, Some("Headset".into()));
        assert_eq!(
            online
                .voice
                .devices
                .as_ref()
                .unwrap()
                .selected_input
                .as_deref(),
            Some("Headset")
        );
        assert_eq!(online.voice.prefs().input.as_deref(), Some("Headset"));
        assert!(sent(&mut online, "\"name\":\"Headset\""));
        online.set_voice_device(DeviceKind::Input, None);
        assert_eq!(online.voice.prefs().input, None);
    }

    #[test]
    fn stopping_keeps_the_choices_and_forgets_the_levels() {
        let mut online = Online::default();
        online.set_voice(true);
        online.set_talking(true);
        voice_event(&mut online, VoiceState::On, true, vec![peer(2, true)]);
        online.stopped(crate::online::Status::Off);
        assert!(online.voice.enabled());
        assert!(online.voice.peers.is_empty());
        assert!(!online.is_talking());
        assert_eq!(online.voice.state, VoiceState::Off);
    }

    #[test]
    fn signing_out_keeps_the_choices() {
        let mut online = Online::default();
        online.set_voice(true);
        online.apply(event(Event::State {
            cloud: Cloud::Online,
            account: Some(SignedIn {
                id: AccountId(1),
                handle: "Alec".into(),
                email: "a@example.org".into(),
            }),
            pending: None,
        }));
        voice_event(&mut online, VoiceState::On, false, vec![peer(2, true)]);
        online.apply(event(Event::State {
            cloud: Cloud::Online,
            account: None,
            pending: None,
        }));
        assert!(online.voice.enabled());
        assert!(online.voice.peers.is_empty());
    }

    #[test]
    fn the_choices_survive_the_preferences_file() {
        let prefs = VoicePrefs {
            enabled: true,
            mode: VoiceMode::VoiceActivity,
            input: Some("Mic".into()),
            output: None,
            key: Some("F".into()),
            peers: vec![PeerPrefs {
                account: AccountId(7),
                percent: 120,
                muted: false,
            }],
            provider: VoiceProvider::default(),
        };
        let text = serde_json::to_string(&prefs).unwrap();
        assert_eq!(serde_json::from_str::<VoicePrefs>(&text).unwrap(), prefs);
        // An older file has none of it.
        assert_eq!(
            serde_json::from_str::<VoicePrefs>("{\"enabled\":false}").unwrap(),
            VoicePrefs::default()
        );
        assert_eq!(VoicePrefs::default().key(), DEFAULT_KEY);
    }
}
