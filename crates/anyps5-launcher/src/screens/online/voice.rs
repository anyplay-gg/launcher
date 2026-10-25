//! Party voice in the online panel: the toggle, the microphone, the mode, the talk key, the
//! devices and each member's volume. Rows like any other (docs/DESIGN.md 17, "Party voice"):
//! state is said in words, an icon only repeats it, and nothing here moves.

use anyps5_core::online::api::AccountId;
use anyps5_core::online::{
    DeviceKind, Hardware, MAX_PERCENT, Mic, Online, PERCENT_STEP, VoiceMode, VoiceProvider,
    VoiceStatus,
};
use egui::Key;

use super::{OnlineAction, OnlineData, OnlinePanel, Page};
use crate::screens::controls::key_name;
use crate::screens::online_call_strings::CallText as CT;
use crate::screens::online_voice_strings::VoiceText as V;
use crate::screens::rows::{Change, Row, source_label};
use crate::ui::i18n::{Arg, Language};
use crate::ui::icons::Icon;
use crate::ui::input::InputEvent;

/// True for a key the player may hold to talk. Keys the launcher itself reads (arrows, Enter,
/// Space, Esc, Backspace, Tab, O, F11) would act on the screen while held; modifiers are not
/// tracked as keys, and keys the controls panel cannot name have no label.
pub fn is_talk_key(key: Key) -> bool {
    !matches!(
        key,
        Key::ArrowLeft
            | Key::ArrowRight
            | Key::ArrowUp
            | Key::ArrowDown
            | Key::Enter
            | Key::Space
            | Key::Escape
            | Key::Backspace
            | Key::Tab
            | Key::O
            | Key::F11
            | Key::Comma
    ) && key_name(key).is_some_and(|name| !is_modifier(name))
}

fn is_modifier(name: &str) -> bool {
    name.contains("Shift")
        || name.contains("Ctrl")
        || name.contains("Alt")
        || name.contains("GUI")
        || matches!(name, "CapsLock" | "Numlock" | "ScrollLock" | "Application")
}

/// The key as a key cap shows it.
pub fn key_label(name: &str, language: Language) -> String {
    match Key::from_name(name).and_then(key_name) {
        Some(sdl) => source_label(&format!("KEY:{sdl}"), language),
        None => name.to_owned(),
    }
}

/// The talk key as an egui key; the default when the stored name is not one.
pub fn talk_key(online: &Online) -> Key {
    Key::from_name(online.voice.prefs().key())
        .filter(|key| is_talk_key(*key))
        .or_else(|| Key::from_name(anyps5_core::online::DEFAULT_KEY))
        .unwrap_or(Key::V)
}

fn status_text(status: &VoiceStatus, language: Language) -> &'static str {
    match status {
        VoiceStatus::Off | VoiceStatus::Failed(_) => V::StatusOff,
        VoiceStatus::NoParty => V::StatusNoParty,
        VoiceStatus::Connecting => V::StatusConnecting,
        VoiceStatus::On => V::StatusOn,
        VoiceStatus::Discord => return CT::StatusDiscord.text(language),
    }
    .text(language)
}

/// What the player's own microphone is doing, in words.
fn mic_text(mic: Mic, key: &str, language: Language) -> String {
    match mic {
        Mic::Off => V::MicOff.text(language).to_owned(),
        Mic::Muted => V::MicMuted.format(language, &[Arg::Str(key)]),
        Mic::Open => V::MicOpen.text(language).to_owned(),
        Mic::Listening => V::MicListening.text(language).to_owned(),
        Mic::Talking => V::MicTalking.text(language).to_owned(),
    }
}

/// The detail line of the Home row: where voice stands.
pub(super) fn home_detail(data: &OnlineData<'_>) -> String {
    let online = data.online;
    status_text(
        &online.voice.status(online.social.party.is_some()),
        data.language,
    )
    .to_owned()
}

impl OnlinePanel {
    /// The toggle with its state, and while voice is on the microphone, the mode and their notes.
    /// `full` leaves out the microphone's state, which the voice page does not repeat.
    pub(super) fn voice_rows(&self, data: &OnlineData<'_>, rows: &mut Vec<Row>, full: bool) {
        let language = data.language;
        let online = data.online;
        let voice = &online.voice;
        let in_party = online.social.party.is_some();
        let status = voice.status(in_party);
        let t = |text: V| text.text(language);
        rows.push(
            Row::choice(
                "voiceProvider",
                CT::VoiceOn.text(language),
                vec![
                    CT::ProviderAnyPlay.text(language).to_owned(),
                    CT::ProviderDiscord.text(language).to_owned(),
                ],
                usize::from(voice.provider() == VoiceProvider::Discord),
            )
            .icon(Icon::DISCORD_LOGO),
        );
        if voice.provider() == VoiceProvider::Discord {
            self.discord_rows(data, rows);
            return;
        }
        rows.push(
            Row::toggle("voiceOn", t(V::PartyVoice), voice.enabled())
                .icon(Icon::MICROPHONE)
                .detail(status_text(&status, language)),
        );
        if let VoiceStatus::Failed(problem) = &status {
            let note = if problem.denied {
                V::FailedDenied
            } else {
                V::FailedGeneric
            };
            rows.push(Row::info("voiceFailed", t(note), None));
        }
        if !voice.enabled() {
            return;
        }
        let key = key_label(talk_key(online).name(), language);
        let mic = voice.mic(in_party);
        if !full {
            let icon = match mic {
                Mic::Off | Mic::Muted => Icon::MICROPHONE_SLASH,
                _ => Icon::MICROPHONE,
            };
            rows.push(
                Row::info(
                    "micState",
                    t(V::Microphone),
                    Some(mic_text(mic, &key, language)),
                )
                .icon(icon),
            );
        }
        rows.push(
            Row::choice(
                "voiceMode",
                t(V::Mode),
                vec![t(V::PushToTalk).to_owned(), t(V::VoiceActivity).to_owned()],
                match voice.mode() {
                    VoiceMode::PushToTalk => 0,
                    VoiceMode::VoiceActivity => 1,
                },
            )
            .icon(Icon::SLIDERS_HORIZONTAL),
        );
        let note = match voice.mode() {
            _ if voice.call_overrides_mode() => V::CallModeNote,
            VoiceMode::PushToTalk if data.game_running => V::PttInGameNote,
            VoiceMode::PushToTalk => V::PttNote,
            VoiceMode::VoiceActivity => V::ActivityNote,
        };
        rows.push(Row::info("voiceNote", t(note), None));
    }

    /// With the voice on Discord the party page says so instead of showing the controls: no
    /// microphone, no mode, no key. The party's link opens Discord; its leader can set it.
    fn discord_rows(&self, data: &OnlineData<'_>, rows: &mut Vec<Row>) {
        let language = data.language;
        let online = data.online;
        let t = |text: CT| text.text(language);
        let party = online.social.party.as_ref();
        let link = party.and_then(|p| p.discord.as_deref());
        rows.push(Row::info("voiceDiscord", t(CT::VoiceOnDiscord), None).icon(Icon::DISCORD_LOGO));
        if let Some(link) = link {
            rows.push(Row::command("openDiscord", t(CT::OpenInDiscord)).icon(Icon::DISCORD_LOGO));
            rows.push(Row::info(
                "discordLinkNow",
                t(CT::DiscordLink),
                Some(link.to_owned()),
            ));
        }
        rows.push(Row::info("voiceNote", t(CT::ProviderDiscordNote), None));
        let leads = party
            .zip(online.account.as_ref())
            .is_some_and(|(p, me)| p.leader == me.id);
        if leads && self.page == Page::Party {
            rows.push(
                Row::text(
                    "discordLink",
                    t(CT::DiscordLinkField),
                    self.link_text.clone(),
                )
                .icon(Icon::LINK)
                .wide()
                .group_start(),
            );
            let valid = anyps5_core::online::api::is_discord_link(self.link_text.trim());
            let mut save = Row::command("saveLink", t(CT::SetLink)).icon(Icon::CHECK);
            if !self.link_text.trim().is_empty() && !valid {
                save = save.detail(t(CT::LinkInvalid));
            }
            rows.push(save);
            if link.is_some() {
                rows.push(Row::command("removeLink", t(CT::RemoveLink)).icon(Icon::X));
            }
            rows.push(Row::info("linkNote", t(CT::LinkNote), None));
        }
    }

    /// The voice page: the same toggle and mode, then the key and the devices.
    pub(super) fn voice_page(&self, data: &OnlineData<'_>, rows: &mut Vec<Row>) {
        let language = data.language;
        let online = data.online;
        let voice = &online.voice;
        let t = |text: V| text.text(language);
        self.voice_rows(data, rows, true);
        if voice.provider() == VoiceProvider::Discord {
            return;
        }
        if voice.mode() == VoiceMode::PushToTalk {
            let key = format!("KEY:{}", key_name(talk_key(online)).unwrap_or("V"));
            let mut keys = Row::keys("talkKey", t(V::TalkKey), vec![key], self.capturing)
                .icon(Icon::KEYBOARD)
                .group_start();
            if self.capturing {
                keys = keys.detail(t(V::TalkKeyNote));
            }
            rows.push(keys);
        }
        let default = t(V::SystemDefault);
        let selected = |name: Option<&str>| name.map_or_else(|| default.to_owned(), str::to_owned);
        rows.push(
            Row::action("voiceInput", t(V::Microphone))
                .icon(Icon::MICROPHONE)
                .detail(selected(voice.prefs().input.as_deref()))
                .group_start(),
        );
        rows.push(
            Row::action("voiceOutput", t(V::Speakers))
                .icon(Icon::HEADPHONES)
                .detail(selected(voice.prefs().output.as_deref())),
        );
        rows.push(Row::command("refreshDevices", t(V::RefreshDevices)).icon(Icon::ARROW_CLOCKWISE));
        let note = match voice.hardware() {
            Hardware::Unknown => Some(V::DevicesUnknown),
            Hardware::NoSound => Some(V::NoSound),
            Hardware::NoMicrophone => Some(V::NoMicrophone),
            Hardware::Ready => None,
        };
        if let Some(note) = note {
            rows.push(Row::info("devicesNote", t(note), None).group_start());
        }
    }

    /// The names a device page lists: what the service found, then the chosen one when it is not
    /// among them.
    pub(super) fn device_names(online: &Online, kind: DeviceKind) -> Vec<String> {
        let Some(devices) = &online.voice.devices else {
            return Vec::new();
        };
        let (found, chosen) = match kind {
            DeviceKind::Input => (&devices.input, online.voice.prefs().input.as_ref()),
            DeviceKind::Output => (&devices.output, online.voice.prefs().output.as_ref()),
        };
        let mut names = found.clone();
        if let Some(chosen) = chosen
            && !names.contains(chosen)
        {
            names.push(chosen.clone());
        }
        names
    }

    pub(super) fn device_page(&self, data: &OnlineData<'_>, rows: &mut Vec<Row>, kind: DeviceKind) {
        let language = data.language;
        let online = data.online;
        let t = |text: V| text.text(language);
        let chosen = match kind {
            DeviceKind::Input => online.voice.prefs().input.as_deref(),
            DeviceKind::Output => online.voice.prefs().output.as_deref(),
        };
        let icon = match kind {
            DeviceKind::Input => Icon::MICROPHONE,
            DeviceKind::Output => Icon::HEADPHONES,
        };
        let mut default = Row::command("deviceDefault", t(V::SystemDefault)).icon(icon);
        if chosen.is_none() {
            default = default.detail(t(V::InUse));
        }
        rows.push(default);
        let found = online.voice.devices.as_ref().map(|d| match kind {
            DeviceKind::Input => &d.input,
            DeviceKind::Output => &d.output,
        });
        for name in Self::device_names(online, kind) {
            let here = found.is_some_and(|f| f.contains(&name));
            let mut row = Row::command("deviceName", name.clone()).icon(icon);
            if chosen == Some(name.as_str()) {
                row = row.detail(if here { t(V::InUse) } else { t(V::DeviceGone) });
            }
            rows.push(row);
        }
        rows.push(Row::info("devicesNote", t(V::DevicesNote), None).group_start());
    }

    /// The part of the member page everyone has: how loud the member is heard, and a mute.
    pub(super) fn member_voice_rows(
        &self,
        id: AccountId,
        data: &OnlineData<'_>,
        rows: &mut Vec<Row>,
    ) {
        let language = data.language;
        let (percent, muted) = data.online.voice.prefs().peer(id);
        let handle = data
            .online
            .social
            .party
            .as_ref()
            .and_then(|p| p.members.iter().find(|m| m.account.id == id))
            .map(|m| m.account.handle.clone())
            .unwrap_or_default();
        rows.push(
            Row::stepper(
                "peerVolume",
                V::Volume.text(language),
                i32::from(percent),
                (0, i32::from(MAX_PERCENT)),
                i32::from(PERCENT_STEP),
                V::VolumeValue.format(language, &[Arg::Int(i64::from(percent))]),
            )
            .icon(Icon::SPEAKER_HIGH),
        );
        rows.push(
            Row::toggle("peerMute", V::Mute.text(language), muted)
                .icon(Icon::SPEAKER_SLASH)
                .detail(V::MuteDetail.format(language, &[Arg::Str(&handle)])),
        );
    }

    /// What a value row of the voice rows changed. True when `change` was one of theirs.
    pub(super) fn voice_change(
        &mut self,
        change: &Change,
        actions: &mut Vec<OnlineAction>,
    ) -> bool {
        match change {
            Change::Toggle { row: "voiceOn", on } => actions.push(OnlineAction::Voice(*on)),
            Change::Choice {
                row: "voiceProvider",
                index,
            } => actions.push(OnlineAction::VoiceProvider(if *index == 0 {
                VoiceProvider::AnyPlay
            } else {
                VoiceProvider::Discord
            })),
            Change::Choice {
                row: "voiceMode",
                index,
            } => actions.push(OnlineAction::VoiceMode(if *index == 0 {
                VoiceMode::PushToTalk
            } else {
                VoiceMode::VoiceActivity
            })),
            Change::Capture { row: "talkKey" } => self.capturing = true,
            Change::Stepper {
                row: "peerVolume",
                value,
            } => {
                if let Page::PartyMember(id) = &self.page {
                    let percent = u16::try_from((*value).clamp(0, i32::from(MAX_PERCENT)))
                        .unwrap_or(MAX_PERCENT);
                    actions.push(OnlineAction::PeerVolume(*id, percent));
                }
            }
            Change::Toggle {
                row: "peerMute",
                on,
            } => {
                if let Page::PartyMember(id) = &self.page {
                    actions.push(OnlineAction::PeerMute(*id, *on));
                }
            }
            _ => return false,
        }
        true
    }

    /// What the action rows of the voice pages do. True when `row` was one of them.
    pub(super) fn voice_run(
        &mut self,
        row: &str,
        data: &OnlineData<'_>,
        actions: &mut Vec<OnlineAction>,
    ) -> bool {
        match (row, self.page.clone()) {
            ("voice", _) => {
                actions.push(OnlineAction::RefreshVoiceDevices);
                self.go(Page::Voice, data);
            }
            ("voiceSettings", _) => {
                actions.push(OnlineAction::RefreshVoiceDevices);
                self.go(Page::Voice, data);
            }
            ("openDiscord", _) => {
                let link = data
                    .online
                    .social
                    .party
                    .as_ref()
                    .and_then(|p| p.discord.clone());
                if let Some(link) = link {
                    actions.push(OnlineAction::OpenDiscord(link));
                }
            }
            ("saveLink", Page::Party) => {
                let link = self.link_text.trim().to_owned();
                if anyps5_core::online::api::is_discord_link(&link) {
                    actions.push(OnlineAction::PartyLink(link));
                    self.link_text.clear();
                }
            }
            ("removeLink", Page::Party) => actions.push(OnlineAction::PartyLink(String::new())),
            ("voiceInput", _) => self.go(Page::VoiceDevices(DeviceKind::Input), data),
            ("voiceOutput", _) => self.go(Page::VoiceDevices(DeviceKind::Output), data),
            ("refreshDevices", _) => actions.push(OnlineAction::RefreshVoiceDevices),
            ("deviceDefault", Page::VoiceDevices(kind)) => {
                actions.push(OnlineAction::VoiceDevice(kind, None));
                self.go(Page::Voice, data);
            }
            ("deviceName", Page::VoiceDevices(kind)) => {
                let index = self.nth(data, "deviceName");
                if let Some(name) = Self::device_names(data.online, kind).get(index) {
                    actions.push(OnlineAction::VoiceDevice(kind, Some(name.clone())));
                    self.go(Page::Voice, data);
                }
            }
            _ => return false,
        }
        true
    }

    /// Starts waiting for the talk key (the `online-voice-key` gallery page).
    pub fn begin_capture(&mut self) {
        self.capturing = true;
    }

    /// While the talk key row waits: a key binds it, Esc and a pad's back keep the old one.
    pub(super) fn capture(&mut self, events: &[InputEvent], actions: &mut Vec<OnlineAction>) {
        for event in events {
            match event {
                InputEvent::CapturedKey {
                    key, physical_key, ..
                } => {
                    let key = physical_key.unwrap_or(*key);
                    if key == Key::Escape {
                        self.capturing = false;
                    } else if is_talk_key(key) {
                        actions.push(OnlineAction::VoiceKey(key.name().to_owned()));
                        self.capturing = false;
                    }
                    // A key that cannot be held to talk is ignored; the row keeps waiting.
                }
                InputEvent::Command(crate::ui::input::Command::Back) => self.capturing = false,
                _ => {}
            }
        }
    }

    /// The page the voice pages lead back to.
    pub(super) fn voice_back(&self) -> Option<Page> {
        match self.page {
            Page::Voice => Some(match &self.previous {
                Some(Page::Party) => Page::Party,
                _ => Page::Home,
            }),
            Page::VoiceDevices(_) => Some(Page::Voice),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_talk_key_is_a_key_that_means_nothing_to_the_launcher() {
        for key in [Key::V, Key::F5, Key::Num3, Key::Backtick, Key::Q] {
            assert!(is_talk_key(key), "{key:?}");
        }
        for key in [
            Key::Space,
            Key::Enter,
            Key::Escape,
            Key::ArrowUp,
            Key::O,
            Key::Tab,
            Key::F11,
            Key::Backspace,
        ] {
            assert!(!is_talk_key(key), "{key:?}");
        }
    }

    #[test]
    fn a_stored_key_name_comes_back_as_the_same_key() {
        for key in [
            Key::V,
            Key::F5,
            Key::Num3,
            Key::Backtick,
            Key::Semicolon,
            Key::Q,
        ] {
            assert_eq!(Key::from_name(key.name()), Some(key), "{key:?}");
        }
        assert_eq!(key_label("V", Language::English), "V");
        assert_eq!(key_label("F5", Language::English), "F5");
        assert_eq!(key_label("nonsense", Language::English), "nonsense");
    }
}
