//! Gallery pages of the panels: every panel and state rendered over a made-up home screen so it can
//! be screenshotted and compared with the original (`docs/DESIGN.md` screenshot table).
//!
//! ```text
//! anyplay-launcher --gallery=settings|settings1..6|game-settings|confirm|controls|controls-capture|
//!                           controls-end|advanced|trophies|banner|start|unlocking|log|report|library|
//!                           about-update|behavior-links|toolkit|toolkit-update|toolkit-newer|toolkit-pinned|toolkit-local|
//!                           recommended|recommended-warning|whats-new|whats-new-loading|
//!                           saves|saves-empty|restore|
//!                           online|online-browser|online-friends|online-friend|
//!                           online-account|online-picker|
//!                           online-party|online-party-picker|online-messages|online-conversation|
//!                           online-message|online-request|
//!                           online-party-voice|online-party-member|online-voice|online-voice-key|
//!                           online-voice-devices|online-voice-denied
//!                 [--screenshot=out.png] [--size=WxH] [--tr]
//! ```
//!
//! All data is invented (names, trophies, artwork are generated); nothing of a real game is used.
//! Every page shows exactly one focused element and puts glass over artwork, never over the bare
//! theme.

use anyps5_core::community::CommunityLayout;
use anyps5_core::controls::{PadAction, Preset};
use anyps5_core::library::{ExecutableKind, Game};
use anyps5_core::notes::ReleaseNotes;
use anyps5_core::paths::GameId;
use anyps5_core::recommended::{Caution, Change, Kind, Plan, Setting};
use anyps5_core::releases::Published;
use anyps5_core::report::{ReportInput, SystemInfo};
use anyps5_core::settings::{GameSettings, Preferences};
use anyps5_core::trophies::{Grade, Trophy, TrophySet};
use egui::{
    Align2, Color32, ColorImage, Context, Rect, TextureHandle, TextureOptions, Ui, UiBuilder, pos2,
    vec2,
};

use crate::screens::advanced::{AdvancedData, AdvancedPanel};
use crate::screens::banner::{self, BannerLook, BannerTrophy};
use crate::screens::controls::{ControlsData, ControlsPanel};
use crate::screens::game_settings::{
    GameSettingsData, GameSettingsPanel, RecommendedRow, TrophyProgress, UninstallPanel,
};
use crate::screens::library::{LibraryData, LibraryEntry, LibraryGrid, StateText};
use crate::screens::log::{LogData, LogView, ReviewData};
use crate::screens::recommended::{RecommendedData, RecommendedPanel};
use crate::screens::saves::{RestorePanel, SavesData, SavesPanel};
use crate::screens::settings::{
    LauncherUpdate, SettingsEnv, SettingsPanel, UpdateState, apply_preferences,
};
use crate::screens::share_strings::Share;
use crate::screens::start::{StartData, StartScreen};
use crate::screens::trophies::{IconCache, TrophyData, TrophyListPanel};
use crate::screens::unlock::{UnlockProgress, UnlockStep, hero_progress};
use crate::screens::whatsnew::{NotesView, SAMPLE_NOTES, WhatsNewData, WhatsNewPanel};
use crate::ui::Shell;
use crate::ui::anim::AnimExt;
use crate::ui::artwork::paint_artwork;
use crate::ui::chrome::{Fact, TileImage, TileOverlay, TileStyle, facts_line, paint_tile};
use crate::ui::components::ConsoleButton;
use crate::ui::i18n::Key;
use crate::ui::icons::Icon;
use crate::ui::input::InputEvent;
use crate::ui::text;
use crate::ui::theme::{color, size, ty, white};
use anyps5_core::gamefiles::{GameInput, parse_input_ini};

/// The page to show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenPage {
    /// Settings on a category (0 = Appearance, in its rows; the others at the category level).
    Settings(usize),
    GameSettings,
    Confirm,
    Controls,
    ControlsCapture,
    /// The last rows: import, export, reset.
    ControlsEnd,
    Advanced,
    Trophies,
    Banner,
    Start,
    Unlocking,
    Log,
    /// The review of a report to AnyPS5.
    Report,
    Library,
    /// The Toolkit category, in its rows: stable, an update waiting, pinned, a local build.
    Toolkit(ToolkitLook),
    /// The About page with a downloaded launcher update waiting ("Restart Now").
    AboutUpdate,
    /// Behavior with the AnyPlay links registered to this launcher.
    BehaviourLinks,
    Recommended,
    RecommendedWarning,
    WhatsNew,
    WhatsNewLoading,
    /// The backups of the saves of the invented game: three of them, or none.
    Saves,
    SavesEmpty,
    /// The question before one is put back.
    Restore,
    /// The online area on one of its pages, over invented friends.
    Online(OnlineLook),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnlineLook {
    Start,
    /// The browser sign-in waits.
    Browser,
    Friends,
    Friend,
    Account,
    Picker,
    /// The party of three, over its chat and a session the leader could take it into.
    Party,
    /// Picking friends to invite to the party.
    PartyPicker,
    /// The conversations, a message request apart, unread first.
    Messages,
    /// One message in full.
    Message,
    /// A conversation with a friend, with news.
    Conversation,
    /// A stranger's first messages, waiting for an answer.
    Request,
    /// The party with voice on: Lumen speaks, Kit is muted for the player.
    PartyVoice,
    /// One member: how loud they are heard, mute.
    PartyMember,
    /// The voice page.
    Voice,
    /// The voice page while the talk key waits for a key.
    VoiceKey,
    /// The microphones to choose from.
    VoiceDevices,
    /// The net service switched voice off: the system refused the microphone.
    VoiceDenied,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolkitLook {
    Stable,
    Update,
    /// A toolkit newer than the launcher was tested with: its note hangs under the header.
    Newer,
    Pinned,
    Local,
}

/// The page named on the command line.
pub fn parse(name: &str) -> Option<ScreenPage> {
    Some(match name {
        "settings" => ScreenPage::Settings(0),
        "game-settings" => ScreenPage::GameSettings,
        "confirm" => ScreenPage::Confirm,
        "controls" => ScreenPage::Controls,
        "controls-capture" => ScreenPage::ControlsCapture,
        "controls-end" => ScreenPage::ControlsEnd,
        "advanced" => ScreenPage::Advanced,
        "trophies" => ScreenPage::Trophies,
        "banner" => ScreenPage::Banner,
        "start" => ScreenPage::Start,
        "unlocking" => ScreenPage::Unlocking,
        "log" => ScreenPage::Log,
        "report" => ScreenPage::Report,
        "library" => ScreenPage::Library,
        "about-update" => ScreenPage::AboutUpdate,
        "behavior-links" => ScreenPage::BehaviourLinks,
        "toolkit" => ScreenPage::Toolkit(ToolkitLook::Stable),
        "toolkit-update" => ScreenPage::Toolkit(ToolkitLook::Update),
        "toolkit-newer" => ScreenPage::Toolkit(ToolkitLook::Newer),
        "toolkit-pinned" => ScreenPage::Toolkit(ToolkitLook::Pinned),
        "toolkit-local" => ScreenPage::Toolkit(ToolkitLook::Local),
        "recommended" => ScreenPage::Recommended,
        "recommended-warning" => ScreenPage::RecommendedWarning,
        "whats-new" => ScreenPage::WhatsNew,
        "whats-new-loading" => ScreenPage::WhatsNewLoading,
        "saves" => ScreenPage::Saves,
        "saves-empty" => ScreenPage::SavesEmpty,
        "restore" => ScreenPage::Restore,
        "online" => ScreenPage::Online(OnlineLook::Start),
        "online-browser" => ScreenPage::Online(OnlineLook::Browser),
        "online-friends" => ScreenPage::Online(OnlineLook::Friends),
        "online-friend" => ScreenPage::Online(OnlineLook::Friend),
        "online-account" => ScreenPage::Online(OnlineLook::Account),
        "online-picker" => ScreenPage::Online(OnlineLook::Picker),
        "online-party" => ScreenPage::Online(OnlineLook::Party),
        "online-party-picker" => ScreenPage::Online(OnlineLook::PartyPicker),
        "online-messages" => ScreenPage::Online(OnlineLook::Messages),
        "online-message" => ScreenPage::Online(OnlineLook::Message),
        "online-conversation" => ScreenPage::Online(OnlineLook::Conversation),
        "online-request" => ScreenPage::Online(OnlineLook::Request),
        "online-party-voice" => ScreenPage::Online(OnlineLook::PartyVoice),
        "online-party-member" => ScreenPage::Online(OnlineLook::PartyMember),
        "online-voice" => ScreenPage::Online(OnlineLook::Voice),
        "online-voice-key" => ScreenPage::Online(OnlineLook::VoiceKey),
        "online-voice-devices" => ScreenPage::Online(OnlineLook::VoiceDevices),
        "online-voice-denied" => ScreenPage::Online(OnlineLook::VoiceDenied),
        other => {
            let n: usize = other.strip_prefix("settings")?.parse().ok()?;
            (n < crate::screens::settings::Category::ALL.len())
                .then_some(ScreenPage::Settings(n))?
        }
    })
}

/// The moment the invented messages are told from: 12:00 UTC on the 22nd of October.
const SAMPLE_NOW: i64 = 1_792_670_400;

/// What the online panel shows over the invented online state: AnyPlay's own cloud.
fn online_data(
    online: &anyps5_core::online::Online,
    language: crate::ui::i18n::Language,
    accent: Color32,
) -> crate::screens::online::OnlineData<'_> {
    crate::screens::online::OnlineData {
        online,
        cloud: Some("https://cloud.example.org"),
        custom_cloud: false,
        official_cloud: true,
        relay_only: false,
        language,
        accent,
        now: SAMPLE_NOW,
        utc_offset: Some(0),
        game_running: false,
    }
}

/// Invented friends: one plays a game you can join, one plays, one is online, one is not; a
/// request waits, one was sent, and a friend invited you.
fn sample_online(look: OnlineLook) -> anyps5_core::online::Online {
    use anyps5_core::online::api::{
        Account, AccountId, Activity, Count, Friend, FriendLists, Invitation, Presence, Privacy,
        Request, SessionRef,
    };
    use anyps5_core::online::{Cloud, Dialog, Online, Pending, SignedIn, Status};
    use std::collections::BTreeMap;
    let account = |id: u64, handle: &str| Account {
        id: AccountId(id),
        handle: handle.into(),
    };
    let playing = |title: &str, joinable: bool| {
        Some(Activity {
            title_id: "TEST00001".into(),
            title: title.into(),
            version: "01.000".into(),
            session: joinable.then(|| SessionRef {
                id: "s1".into(),
                joinable: true,
                members: 2,
                max_members: 4,
            }),
        })
    };
    let friend = |id, handle, online, activity| Friend {
        account: account(id, handle),
        since: 0,
        presence: Presence {
            online,
            activity,
            revision: 0,
        },
    };
    let mut state = Online::default();
    state.status = Status::Running;
    state.cloud = Some(Cloud::Online);
    match look {
        OnlineLook::Start => return state,
        OnlineLook::Browser => {
            state.pending = Some(Pending::Browser);
            return state;
        }
        _ => {}
    }
    state.account = Some(SignedIn {
        id: AccountId(1),
        handle: "Mira".into(),
        email: "mira@example.org".into(),
    });
    state.friends = FriendLists {
        friends: vec![
            friend(2, "Lumen", true, playing(NAMES[1], true)),
            friend(3, "Juno", true, playing(NAMES[4], false)),
            friend(4, "Kit", true, None),
            friend(5, "Rook", false, None),
        ],
        incoming: vec![Request {
            account: account(6, "Vale"),
            since: 0,
        }],
        outgoing: vec![Request {
            account: account(7, "Ash"),
            since: 0,
        }],
        blocked: vec![account(8, "Spamford")],
    };
    state.invitations = vec![Invitation {
        id: "i1".into(),
        from: account(3, "Juno"),
        session: "s2".into(),
        title_id: "TEST00002".into(),
        title: NAMES[4].into(),
        version: "01.000".into(),
        expires: 0,
    }];
    let count = |platinum, gold, silver, bronze| Count {
        bronze,
        silver,
        gold,
        platinum,
    };
    state.cards = BTreeMap::from([
        (AccountId(2), Some(count(1, 4, 9, 30))),
        (AccountId(3), None),
        (AccountId(4), Some(count(0, 0, 0, 0))),
    ]);
    state.privacy = Some(Privacy::default());
    sample_social(&mut state);
    sample_voice(&mut state, look);
    if look == OnlineLook::Picker {
        state.dialog = Some(Dialog {
            id: 1,
            title: NAMES[1].into(),
            max: 3,
        });
    }
    state
}

/// Party voice over the invented party: on for the looks that show it, with Lumen speaking.
fn sample_voice(state: &mut anyps5_core::online::Online, look: OnlineLook) {
    use anyps5_core::online::api::AccountId;
    use anyps5_core::online::control::{VoiceLevel, VoicePeer, VoiceState};
    use anyps5_core::online::{Devices, PeerPrefs, VoicePrefs};
    if !matches!(
        look,
        OnlineLook::PartyVoice
            | OnlineLook::PartyMember
            | OnlineLook::Voice
            | OnlineLook::VoiceKey
            | OnlineLook::VoiceDevices
            | OnlineLook::VoiceDenied
    ) {
        return;
    }
    state.voice.devices = Some(Devices {
        input: vec![
            "Headset Microphone (USB Audio)".into(),
            "Microphone Array (Built-in)".into(),
        ],
        output: vec![
            "Headset Earphone (USB Audio)".into(),
            "Speakers (Built-in)".into(),
        ],
        selected_input: Some("Headset Microphone (USB Audio)".into()),
        selected_output: None,
    });
    state.voice.load(VoicePrefs {
        enabled: look != OnlineLook::VoiceDenied,
        input: Some("Headset Microphone (USB Audio)".into()),
        peers: vec![
            PeerPrefs {
                account: AccountId(4),
                percent: 100,
                muted: true,
            },
            PeerPrefs {
                account: AccountId(2),
                percent: 130,
                muted: false,
            },
        ],
        ..VoicePrefs::default()
    });
    if look == OnlineLook::VoiceDenied {
        state.voice.problem = Some(anyps5_core::online::Problem {
            message: "access denied".into(),
            denied: true,
        });
        return;
    }
    if let Some(party) = &mut state.social.party {
        for member in &mut party.members {
            member.voice = member.account.id != AccountId(4) || look != OnlineLook::PartyVoice;
        }
    }
    state.voice.state = VoiceState::On;
    state.voice.me = VoiceLevel {
        level: 0,
        speaking: false,
    };
    state.voice.peers = vec![
        VoicePeer {
            account: AccountId(2),
            level: VoiceLevel {
                level: 62,
                speaking: true,
            },
            link: Some(anyps5_core::online::Link::Direct),
        },
        VoicePeer {
            account: AccountId(4),
            level: VoiceLevel {
                level: 30,
                speaking: true,
            },
            link: Some(anyps5_core::online::Link::Relay),
        },
    ];
}

/// An invented party of three, a session, four conversations and a stranger who wrote first.
fn sample_social(state: &mut anyps5_core::online::Online) {
    use anyps5_core::online::api::{
        Account, AccountId, Conversation, ConversationKind, Conversations, Message, Messages,
        Party, PartyInvitation, PartyMember, ReadMark, Visibility,
    };
    use anyps5_core::online::control::Member;
    use anyps5_core::online::{Fetch, Session};
    let account = |id: u64, handle: &str| Account {
        id: AccountId(id),
        handle: handle.into(),
    };
    state.social.apply_party(Some(Party {
        id: "p1".into(),
        leader: AccountId(1),
        members: [(1, "Mira"), (2, "Lumen"), (4, "Kit")]
            .iter()
            .map(|(id, handle)| PartyMember {
                account: account(*id, handle),
                joined: 0,
                endpoint: String::new(),
                relay: None,
                voice: false,
            })
            .collect(),
        max_members: 8,
        conversation: "c-party".into(),
        created: 0,
        revision: 3,
    }));
    state.social.add_party_invitation(PartyInvitation {
        id: "pi1".into(),
        from: account(5, "Rook"),
        party: "p9".into(),
        members: 2,
        expires: 0,
    });
    state.session = Some(Session {
        id: "s1".into(),
        title_id: "TEST00001".into(),
        title: NAMES[1].into(),
        version: "01.000".into(),
        leader: AccountId(1),
        max_members: 4,
        visibility: Visibility::Friends,
        members: vec![Member {
            account: account(1, "Mira"),
            address: "198.18.0.1".into(),
            joined: 0,
            link: None,
        }],
        created: 0,
    });
    let chat = |id: &str, kind, with: &[(u64, &str)], last: u64, read: u64, updated: u64| {
        let mut members = vec![account(1, "Mira")];
        members.extend(with.iter().map(|(i, h)| account(*i, h)));
        Conversation {
            id: id.into(),
            kind,
            name: None,
            members,
            last,
            read,
            marks: Vec::new(),
            updated,
            request: false,
            party: None,
        }
    };
    let now = SAMPLE_NOW as u64;
    let mut lumen = chat(
        "c-lumen",
        ConversationKind::Direct,
        &[(2, "Lumen")],
        7,
        4,
        now - 720,
    );
    lumen.marks = vec![ReadMark {
        account: AccountId(2),
        seq: 4,
    }];
    let mut group = chat(
        "c-raid",
        ConversationKind::Group,
        &[(2, "Lumen"), (3, "Juno"), (4, "Kit")],
        30,
        30,
        now - 40_000,
    );
    group.name = Some("Raid night".into());
    let mut party_chat = chat(
        "c-party",
        ConversationKind::Party,
        &[(2, "Lumen"), (4, "Kit")],
        5,
        5,
        now - 3_000,
    );
    party_chat.party = Some("p1".into());
    let mut stranger = chat(
        "c-nyx",
        ConversationKind::Direct,
        &[(9, "Nyx")],
        2,
        0,
        now - 1_900,
    );
    stranger.request = true;
    state.social.set_conversations(Conversations {
        conversations: vec![
            lumen,
            party_chat,
            chat(
                "c-juno",
                ConversationKind::Direct,
                &[(3, "Juno")],
                12,
                12,
                now - 20_000,
            ),
            group,
        ],
        requests: vec![stranger],
    });
    let said =
        |conversation: &str, seq: u64, who: u64, handle: &str, text: &str, ago: u64| Message {
            conversation: conversation.into(),
            seq,
            sender: account(who, handle),
            text: text.into(),
            created: now - ago,
        };
    state.social.ingest(
        "c-lumen",
        Fetch::Newest,
        Messages {
            messages: vec![
                said("c-lumen", 1, 2, "Lumen", "Are you around tonight?", 59_000),
                said(
                    "c-lumen",
                    2,
                    1,
                    "Mira",
                    "Yes, after dinner. Around 8.",
                    58_900,
                ),
                said(
                    "c-lumen",
                    3,
                    2,
                    "Lumen",
                    "Great. I will set up the lobby.",
                    58_800,
                ),
                said("c-lumen", 4, 1, "Mira", "Bringing Juno too.", 58_700),
                said(
                    "c-lumen",
                    5,
                    2,
                    "Lumen",
                    "Found a new route on the second map.",
                    1_140,
                ),
                said(
                    "c-lumen",
                    6,
                    2,
                    "Lumen",
                    "It cuts the run in half. Want to try it with the others?",
                    1_080,
                ),
                said("c-lumen", 7, 2, "Lumen", "Ready when you are.", 720),
            ],
            last: 7,
        },
    );
    state.social.ingest(
        "c-nyx",
        Fetch::Newest,
        Messages {
            messages: vec![
                said(
                    "c-nyx",
                    1,
                    9,
                    "Nyx",
                    "Hi! I saw you in the lobby yesterday. Want to team up?",
                    1_960,
                ),
                said(
                    "c-nyx",
                    2,
                    9,
                    "Nyx",
                    "My squad needs a fourth player.",
                    1_900,
                ),
            ],
            last: 2,
        },
    );
}

/// Three backups of the invented game: one before an unlock, a manual one, one after playing.
fn sample_backups() -> Vec<anyps5_core::saves::Backup> {
    use anyps5_core::saves::{Backup, Reason};
    let backup = |created: i64, reason: Reason, bytes: u64| Backup {
        path: std::path::PathBuf::new(),
        created,
        reason,
        bytes,
    };
    vec![
        backup(1_792_677_900, Reason::BeforeUnlock, 3_024_877),
        backup(1_792_591_200, Reason::AfterPlaying, 2_980_112),
        backup(1_792_245_600, Reason::Manual, 412_600),
    ]
}

/// What the catalogue could say about the invented game: two changes, or only a warning.
fn sample_plan(warning: bool) -> Plan {
    if warning {
        return Plan {
            changes: Vec::new(),
            cautions: vec![Caution {
                entry: "no-fixed-texture-cache".into(),
                setting: Setting::Env("APS5_TEXTURE_CACHE_MIB".into()),
                value: "3072".into(),
                why: "A fixed texture cache made this game slower on an 8 GB card. Remove APS5_TEXTURE_CACHE_MIB from your system settings.".into(),
            }],
        };
    }
    Plan {
        changes: vec![
            Change {
                entry: "playgo-installed".into(),
                kind: Kind::Required,
                setting: Setting::Env("APS5_PLAYGO_UNLISTED_CHUNKS".into()),
                current: None,
                proposed: "installed".into(),
                why: "This dump has no PlayGo data. Without this the game cannot load its saves."
                    .into(),
            },
            Change {
                entry: "fixed-base".into(),
                kind: Kind::Recommended,
                setting: Setting::Relink("--windows-fixed-base".into()),
                current: None,
                proposed: "0x400000".into(),
                why: "The program expects to be loaded at 0x400000.".into(),
            },
        ],
        cautions: Vec::new(),
    }
}

/// Names of the invented games.
const NAMES: [&str; 9] = [
    "Starlight Harbor",
    "Ember Drift",
    "Paper Moon",
    "Tidebreaker",
    "Neon Orchard",
    "Hollow Reef",
    "Cinder Run",
    "Glasswing",
    "Salt & Lantern",
];

/// A generated cover: diagonal bands and a big disc in a hue of its own.
fn cover_image(seed: usize, w: usize, h: usize) -> ColorImage {
    let hue = (seed as f32 * 0.137 + 0.05).fract();
    let tint = |t: f32, k: f32| {
        let (r, g, b) = hsv(hue + 0.08 * t, 0.55 + 0.25 * k, 0.25 + 0.65 * k);
        [r, g, b, 255]
    };
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            let band = ((fx * 3.0 + fy * 2.0 + seed as f32 * 0.4).sin() * 0.5 + 0.5) * 0.45;
            let d = ((fx - 0.62).powi(2) + (fy - 0.42).powi(2)).sqrt();
            let disc = (1.0 - (d / (0.24 + 0.02 * seed as f32 % 0.1)).clamp(0.0, 1.0)).powf(0.6);
            let k = (0.2 + band + disc * 0.5).clamp(0.0, 1.0);
            let vignette = 1.0 - 0.5 * ((fx - 0.5).powi(2) + (fy - 0.5).powi(2));
            let mut px = tint(fy, k);
            for c in &mut px[..3] {
                *c = (f32::from(*c) * vignette) as u8;
            }
            rgba.extend_from_slice(&px);
        }
    }
    ColorImage::from_rgba_unmultiplied([w, h], &rgba)
}

fn hsv(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let h = h.rem_euclid(1.0) * 6.0;
    let (i, f) = (h.floor() as i32, h.fract());
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    let (r, g, b) = match i {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

/// A small PNG for a trophy icon.
fn trophy_png(seed: usize) -> Vec<u8> {
    let image = cover_image(seed + 3, 96, 96);
    let mut rgba = Vec::new();
    for p in &image.pixels {
        rgba.extend_from_slice(&p.to_array());
    }
    let img = image::RgbaImage::from_raw(96, 96, rgba).expect("size matches");
    let mut bytes = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::Png,
    )
    .expect("png encodes");
    bytes
}

fn fake_game(index: usize) -> Game {
    let title_id = format!("PPSA{:05}", 90000 + index);
    Game {
        id: GameId::from_key(format!("c:/games/{}", NAMES[index].to_lowercase())),
        folder: format!("C:\\Games\\{}", NAMES[index]).into(),
        title_id,
        name: NAMES[index].to_owned(),
        names: Default::default(),
        version: "01.000.010".into(),
        content_id: None,
        icon: None,
        background: None,
        music: None,
        executable: ExecutableKind::Elf,
        size: None,
        unlocked_with: Some("0.1.3-abc".into()),
    }
}

fn fake_trophies() -> TrophySet {
    type Spec = (
        &'static str,
        &'static str,
        Grade,
        bool,
        Option<i64>,
        Option<(u64, u64)>,
    );
    let spec: [Spec; 8] = [
        (
            "Lantern Keeper",
            "Earn every other trophy",
            Grade::Platinum,
            false,
            None,
            None,
        ),
        (
            "First Light",
            "Light your first lantern",
            Grade::Bronze,
            false,
            Some(1_792_271_640_000_000),
            None,
        ),
        (
            "Harbor Master",
            "Dock at every harbor",
            Grade::Silver,
            false,
            None,
            Some((23, 51)),
        ),
        (
            "Salt in the Wind",
            "Sail through a storm",
            Grade::Bronze,
            false,
            Some(1_792_274_520_000_000),
            None,
        ),
        (
            "Moonlit Tide",
            "Finish the night voyage",
            Grade::Gold,
            false,
            None,
            None,
        ),
        (
            "Quiet Passage",
            "Cross the reef unseen",
            Grade::Silver,
            false,
            None,
            Some((3, 6)),
        ),
        (
            "The Glass Wing",
            "Find what was lost",
            Grade::Gold,
            true,
            None,
            None,
        ),
        (
            "Anchored",
            "Claim your first ship",
            Grade::Bronze,
            false,
            Some(1_792_347_180_000_000),
            None,
        ),
    ];
    TrophySet {
        title: "Starlight Harbor".into(),
        icon: Some(trophy_png(0)),
        trophies: spec
            .iter()
            .enumerate()
            .map(
                |(i, (name, detail, grade, hidden, unlocked, progress))| Trophy {
                    service_label: 0,
                    number: i as u32,
                    grade: *grade,
                    hidden: *hidden,
                    name: (*name).to_owned(),
                    detail: (*detail).to_owned(),
                    icon: Some(trophy_png(i + 1)),
                    unlocked: *unlocked,
                    progress: *progress,
                },
            )
            .collect(),
    }
}

/// The panels with their invented data.
pub struct ScreenGallery {
    page: ScreenPage,
    covers: Vec<TextureHandle>,
    hero: Option<TextureHandle>,
    prefs: Preferences,
    game_settings: GameSettings,
    games: Vec<Game>,
    set: TrophySet,
    settings: SettingsPanel,
    game_panel: GameSettingsPanel,
    uninstall: UninstallPanel,
    saves: SavesPanel,
    restore: RestorePanel,
    online_panel: crate::screens::online::OnlinePanel,
    online: anyps5_core::online::Online,
    recommended: RecommendedPanel,
    whats_new: WhatsNewPanel,
    notes: Published,
    update: UpdateState,
    toolkit_version: String,
    controls: ControlsPanel,
    /// A game whose files name its controls, for the controls page.
    game_input: GameInput,
    /// Layouts the community shares for the invented game.
    community: Vec<CommunityLayout>,
    advanced: AdvancedPanel,
    trophies: TrophyListPanel,
    library: LibraryGrid,
    log: LogView,
    start: StartScreen,
    banner_icons: IconCache,
    opened: bool,
}

impl ScreenGallery {
    pub fn new(page: ScreenPage, language: crate::ui::i18n::Language) -> Self {
        let mut prefs = Preferences::first_run();
        // --tr shows the page in Turkish.
        if language == crate::ui::i18n::Language::Turkish {
            prefs.language = anyps5_core::settings::Language::Turkish;
        }
        prefs.onboarded = true;
        prefs.profile_name = "Mira".into();
        prefs.games_folder = Some("D:\\Games".into());
        prefs.toolkit_checked_at = Some("2026-10-22 09:30".into());
        let mut update = UpdateState::UpToDate;
        let mut toolkit_version = "0.1.3-4f2a9c".to_owned();
        if let ScreenPage::Toolkit(look) = page {
            match look {
                ToolkitLook::Stable => {}
                ToolkitLook::Update => update = UpdateState::Available("0.1.4".into()),
                ToolkitLook::Newer => toolkit_version = "0.1.9-c0ffee".into(),
                ToolkitLook::Pinned => {
                    prefs.toolkit_pinned = Some("v0.1.2".into());
                    toolkit_version = "0.1.2-9d1e47".into();
                }
                ToolkitLook::Local => {
                    prefs.toolkit = Some("D:\\anyps5\\build".into());
                    toolkit_version = "local-4f2a9c1d7e03".into();
                }
            }
        }
        Self {
            recommended: RecommendedPanel::default(),
            whats_new: WhatsNewPanel::default(),
            notes: Published {
                tag: "0.1.3".into(),
                date: "2026-10-20".into(),
                notes: ReleaseNotes::parse(SAMPLE_NOTES),
            },
            update,
            toolkit_version,
            page,
            covers: Vec::new(),
            hero: None,
            prefs,
            game_settings: GameSettings::default(),
            games: (0..NAMES.len()).map(fake_game).collect(),
            set: fake_trophies(),
            settings: SettingsPanel::default(),
            game_panel: GameSettingsPanel::default(),
            uninstall: UninstallPanel::default(),
            saves: SavesPanel::default(),
            restore: RestorePanel::default(),
            online_panel: crate::screens::online::OnlinePanel::default(),
            online: match page {
                ScreenPage::Online(look) => sample_online(look),
                _ => anyps5_core::online::Online::default(),
            },
            controls: ControlsPanel::default(),
            game_input: sample_game_input(),
            community: sample_community(),
            advanced: AdvancedPanel::default(),
            trophies: TrophyListPanel::default(),
            library: LibraryGrid::default(),
            log: LogView::default(),
            start: StartScreen::default(),
            banner_icons: IconCache::default(),
            opened: false,
        }
    }

    fn ensure_art(&mut self, ctx: &Context) {
        if self.covers.is_empty() {
            self.covers = (0..NAMES.len())
                .map(|i| {
                    ctx.load_texture(
                        format!("cover-{i}"),
                        cover_image(i, 192, 192),
                        TextureOptions::LINEAR,
                    )
                })
                .collect();
            self.hero =
                Some(ctx.load_texture("hero", cover_image(1, 640, 360), TextureOptions::LINEAR));
        }
    }

    fn tile(&self, i: usize) -> TileImage {
        TileImage {
            texture: self.covers[i].id(),
            aspect: 1.0,
        }
    }

    /// Draws the made-up home screen and then the panel of the page.
    pub fn ui(&mut self, shell: &mut Shell, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.ensure_art(&ctx);
        if !self.opened {
            self.opened = true;
            apply_preferences(shell, &ctx, &self.prefs);
            self.open_page(shell, &ctx);
        }
        let language = shell.language;
        let accent = shell.accent_color();
        let events: Vec<InputEvent> = Vec::new();
        let name = NAMES[1];
        if matches!(self.page, ScreenPage::Banner) {
            self.draw_game_frame(ui);
        } else {
            self.draw_home(ui, shell, matches!(self.page, ScreenPage::Unlocking));
        }
        let entries: Vec<LibraryEntry> = (0..NAMES.len())
            .map(|i| LibraryEntry {
                name: NAMES[i].to_owned(),
                icon: Some(self.tile(i)),
                overlay: if i == 5 {
                    TileOverlay::Running
                } else {
                    TileOverlay::None
                },
                state: if i == 5 {
                    StateText {
                        icon: Icon::PLAY_CIRCLE,
                        text: language.text(Key::Running).into(),
                        tint: accent,
                    }
                } else {
                    StateText {
                        icon: Icon::CHECK_CIRCLE,
                        text: language.text(Key::Ready).into(),
                        tint: white(0.75),
                    }
                },
            })
            .collect();
        let resolutions: Vec<String> = Vec::new();
        let recent: Vec<String> = ["v0.1.3", "v0.1.2", "v0.1.1", "v0.1.0"]
            .map(str::to_owned)
            .to_vec();
        let progress = TrophyProgress::from(&self.set);
        let env = SettingsEnv {
            games_found: self.games.len(),
            app_version: "0.1.0",
            toolkit_version: Some(&self.toolkit_version),
            update: self.update.clone(),
            recent: &recent,
            launcher: if matches!(self.page, ScreenPage::AboutUpdate) {
                LauncherUpdate::Ready("0.1.3".into())
            } else {
                LauncherUpdate::None
            },
            default_screenshots: std::path::Path::new("C:\\Users\\mira\\Pictures\\AnyPS5"),
            links: if matches!(self.page, ScreenPage::BehaviourLinks) {
                anyps5_core::linkreg::Handler::ThisExe
            } else {
                anyps5_core::linkreg::Handler::Absent
            },
        };
        let icon = Some(self.tile(1));
        self.settings
            .show(shell, &ctx, &events, &mut self.prefs, &env);
        {
            let mut data = GameSettingsData {
                title: name,
                settings: &mut self.game_settings,
                resolutions: &resolutions,
                trophies: Some(progress),
                installed: true,
                saves: Some(3),
                recommended: Some(RecommendedRow {
                    changes: 1,
                    needed: true,
                }),
                unlock_note: Some(
                    "Unlocked with v0.1.2. v0.1.3 is installed. This update changes how games are converted."
                        .into(),
                ),
                language,
                accent,
            };
            self.game_panel.show(shell, &ctx, &events, &mut data);
        }
        self.uninstall.show(shell, &ctx, &events, language, accent);
        {
            let backups = if self.page == ScreenPage::SavesEmpty {
                Vec::new()
            } else {
                sample_backups()
            };
            let data = SavesData {
                title: name,
                backups: &backups,
                has_saves: true,
                running: false,
                keep: anyps5_core::saves::KEEP,
                language,
                accent,
            };
            self.saves.show(shell, &ctx, &events, &data);
            let stamp = backups
                .first()
                .map(crate::screens::saves::stamp)
                .unwrap_or_default();
            self.restore
                .show(shell, &ctx, &events, &stamp, language, accent);
        }
        {
            let data = online_data(&self.online, language, accent);
            self.online_panel.show(shell, &ctx, &events, &data);
        }
        {
            let plan = sample_plan(matches!(self.page, ScreenPage::RecommendedWarning));
            let data = RecommendedData {
                title: name,
                plan: &plan,
                language,
                accent,
            };
            self.recommended.show(shell, &ctx, &events, &data);
        }
        {
            let view = if matches!(self.page, ScreenPage::WhatsNewLoading) {
                NotesView::Loading
            } else {
                NotesView::Ready(&self.notes)
            };
            let data = WhatsNewData {
                tag: "0.1.3",
                view,
                language,
            };
            self.whats_new.show(shell, &ctx, &events, &data);
        }
        {
            let mut data = ControlsData {
                title: name,
                title_id: "PPSA90001",
                community: &self.community,
                settings: &mut self.game_settings,
                language,
                accent,
                game_input: Some(&self.game_input),
            };
            self.controls.show(shell, &ctx, &events, &mut data);
        }
        {
            let game = &self.games[1];
            let mut data = AdvancedData {
                game,
                settings: &mut self.game_settings,
                resolutions: &resolutions,
                language,
                accent,
            };
            self.advanced.show(shell, &ctx, &events, &mut data);
        }
        self.trophies.show(
            shell,
            &ctx,
            &events,
            &TrophyData {
                game_key: "gallery",
                game_name: name,
                set: &self.set,
                language,
                accent,
            },
        );
        self.library.show(
            shell,
            &ctx,
            &events,
            &LibraryData {
                entries: &entries,
                language,
                accent,
            },
        );
        let diagnosis =
            anyps5_core::diagnosis::Diagnosis::Crashed("0xC0000005 (ACCESS_VIOLATION)".into());
        let report = matches!(self.page, ScreenPage::Report);
        let review = anyps5_core::issue::prepare(&sample_report());
        let hint = Share::ReportHint.text(language);
        self.log.show(
            shell,
            &ctx,
            &events,
            &LogData {
                game_name: Some(name),
                diagnosis: Some(&diagnosis),
                text: if report { &review.preview } else { SAMPLE_LOG },
                language,
                review: report.then_some(ReviewData { hint }),
            },
        );
        self.start.show(
            shell,
            &ctx,
            &events,
            &StartData {
                game_name: name,
                icon,
                language,
                accent,
                reduced_motion: false,
            },
        );
        if matches!(self.page, ScreenPage::Banner) {
            self.draw_banner(ui, language);
        }
    }

    fn open_page(&mut self, shell: &mut Shell, ctx: &Context) {
        match self.page {
            ScreenPage::Settings(n) => self.settings.open_at(n, n == 0),
            ScreenPage::GameSettings => self.game_panel.open_row(3),
            ScreenPage::Confirm => self.uninstall.open(),
            ScreenPage::Saves | ScreenPage::SavesEmpty => self.saves.open(),
            ScreenPage::Restore => self.restore.open(),
            ScreenPage::Online(look) => {
                use crate::screens::online::Page;
                let language = shell.language;
                let data = online_data(&self.online, language, shell.accent_color());
                self.online_panel.open(&data);
                match look {
                    OnlineLook::Friend => self
                        .online_panel
                        .show_page(Page::Friend(anyps5_core::online::api::AccountId(2)), &data),
                    OnlineLook::Account => self.online_panel.show_page(Page::Account, &data),
                    OnlineLook::Party => self.online_panel.show_page(Page::Party, &data),
                    OnlineLook::PartyPicker => {
                        self.online_panel.show_page(Page::PartyPicker, &data);
                    }
                    OnlineLook::PartyVoice => self.online_panel.show_page(Page::Party, &data),
                    OnlineLook::PartyMember => self.online_panel.show_page(
                        Page::PartyMember(anyps5_core::online::api::AccountId(2)),
                        &data,
                    ),
                    OnlineLook::Voice | OnlineLook::VoiceDenied => {
                        self.online_panel.show_page(Page::Voice, &data);
                    }
                    OnlineLook::VoiceKey => {
                        self.online_panel.show_page(Page::Voice, &data);
                        self.online_panel.begin_capture();
                    }
                    OnlineLook::VoiceDevices => self.online_panel.show_page(
                        Page::VoiceDevices(anyps5_core::online::DeviceKind::Input),
                        &data,
                    ),
                    OnlineLook::Messages => self.online_panel.show_page(Page::Messages, &data),
                    OnlineLook::Message => self
                        .online_panel
                        .show_page(Page::Message("c-lumen".into(), 6), &data),
                    OnlineLook::Conversation => self
                        .online_panel
                        .show_page(Page::Conversation("c-lumen".into()), &data),
                    OnlineLook::Request => self
                        .online_panel
                        .show_page(Page::MessageRequest("c-nyx".into()), &data),
                    _ => {}
                }
            }
            ScreenPage::Controls => self.controls.open(),
            ScreenPage::ControlsCapture => {
                self.controls.open_row(3);
                self.controls.begin_capture(shell, PadAction::Square);
            }
            ScreenPage::Advanced => self.advanced.open_row(4),
            ScreenPage::Trophies => self.trophies.open_at(1),
            ScreenPage::Library => self.library.open(1),
            ScreenPage::ControlsEnd => self.controls.open_row(30),
            ScreenPage::Toolkit(look) => {
                self.settings.open_at(4, true);
                // The update page lands on the install row.
                if look == ToolkitLook::Update {
                    self.settings.focus_row(3);
                }
            }
            ScreenPage::AboutUpdate => {
                self.settings.open_at(6, true);
                self.settings.focus_row(2);
            }
            ScreenPage::BehaviourLinks => self.settings.open_at(3, true),
            ScreenPage::Recommended | ScreenPage::RecommendedWarning => {
                let plan = sample_plan(matches!(self.page, ScreenPage::RecommendedWarning));
                self.recommended.open(&plan, shell.language);
            }
            ScreenPage::WhatsNew => {
                self.whats_new
                    .open_expanded(&self.notes.notes, shell.language, 2);
            }
            ScreenPage::WhatsNewLoading => self.whats_new.open(),
            ScreenPage::Log | ScreenPage::Report => self.log.open(),
            ScreenPage::Start => self.start.open_at(ctx.clock()),
            ScreenPage::Banner | ScreenPage::Unlocking => {}
        }
    }

    /// The home screen under the panels: artwork with scrims, the tile row, the title, buttons and
    /// the facts, at the positions of the original (section 5.3). One tile carries the focus ring
    /// only on the pages where the home is the subject.
    fn draw_home(&self, ui: &mut Ui, shell: &Shell, unlocking: bool) {
        let ctx = ui.ctx().clone();
        let painter = ui.painter().clone();
        let screen = ui.max_rect();
        let language = shell.language;
        let accent = shell.accent_color();
        if let Some(hero) = &self.hero {
            paint_artwork(
                &painter,
                screen,
                hero.id(),
                16.0 / 9.0,
                ctx.clock(),
                true,
                1.0,
            );
        }
        text::paint(
            &painter,
            pos2(64.0, 34.0),
            Align2::LEFT_TOP,
            ty::TOP_BAR_TITLE,
            language.text(Key::Games),
            Color32::WHITE,
        );
        // The tile row: the selected game large, a label column, then small tiles.
        let selected = Rect::from_min_size(pos2(64.0, 112.0), vec2(176.0, 176.0));
        let mut style = TileStyle::new(accent);
        style.image = Some(self.tile(1));
        style.overlay = if unlocking {
            TileOverlay::Installing(0.39)
        } else {
            TileOverlay::None
        };
        paint_tile(&painter, selected, &style);
        text::paint(
            &painter,
            pos2(262.4, 168.0),
            Align2::LEFT_TOP,
            ty::TILE_NAME,
            NAMES[1],
            Color32::WHITE,
        );
        let state = if unlocking {
            language.text(Key::StepRelink)
        } else {
            language.text(Key::Ready)
        };
        painter.text(
            pos2(262.4, 204.0),
            Align2::LEFT_CENTER,
            if unlocking {
                Icon::ARROW_CIRCLE_DOWN.glyph()
            } else {
                Icon::CHECK_CIRCLE.glyph()
            },
            Icon::CHECK_CIRCLE.font(14.0, true),
            if unlocking {
                accent
            } else {
                color::state_neutral()
            },
        );
        text::paint(
            &painter,
            pos2(282.0, 204.0),
            Align2::LEFT_CENTER,
            ty::STATE_LABEL,
            state,
            if unlocking {
                accent
            } else {
                color::state_neutral()
            },
        );
        for (n, x) in [556.0, 690.0, 824.0, 958.0].into_iter().enumerate() {
            let r = Rect::from_min_size(pos2(x, 122.44), vec2(118.0, 118.0));
            let mut s = TileStyle::new(accent);
            s.image = Some(self.tile(n + 2));
            paint_tile(&painter, r, &s);
        }
        // Title, buttons, facts.
        text::paint(
            &painter,
            pos2(72.0, 560.0),
            Align2::LEFT_TOP,
            ty::HERO_TITLE,
            NAMES[1],
            Color32::WHITE,
        );
        let mut buttons = ui.new_child(
            UiBuilder::new().max_rect(Rect::from_min_size(pos2(72.0, 651.0), vec2(900.0, 54.0))),
        );
        buttons.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            ConsoleButton::new(if unlocking {
                language.text(Key::Cancel)
            } else {
                language.text(Key::Play)
            })
            .icon(if unlocking { Icon::X } else { Icon::PLAY })
            .icon_fill(!unlocking)
            .prominent(true)
            .min_width(size::PLAY_MIN_WIDTH)
            .show(ui);
            ConsoleButton::new("3/8")
                .icon(Icon::TROPHY)
                .icon_fill(true)
                .show(ui);
            ConsoleButton::icon_only(Icon::DOTS_THREE, language.text(Key::Options)).show(ui);
            if unlocking {
                ui.add_space(10.0);
                hero_progress(
                    ui,
                    &UnlockProgress::new(UnlockStep::Relink, 0.55),
                    language,
                    accent,
                );
            }
        });
        let mut facts = ui.new_child(
            UiBuilder::new().max_rect(Rect::from_min_size(pos2(72.0, 727.0), vec2(1000.0, 96.0))),
        );
        let status = if unlocking {
            language.text(Key::StepRelink)
        } else {
            language.text(Key::Ready)
        };
        facts_line(
            &mut facts,
            &[
                Fact {
                    icon: Icon::CIRCLE_DASHED,
                    title: language.text(Key::Status),
                    value: status,
                    value_color: None,
                },
                Fact {
                    icon: Icon::HARD_DRIVE,
                    title: language.text(Key::Size),
                    value: "8.39 GB",
                    value_color: None,
                },
                Fact {
                    icon: Icon::HASH,
                    title: language.text(Key::TitleID),
                    value: "PPSA90001",
                    value_color: None,
                },
                Fact {
                    icon: Icon::TAG,
                    title: language.text(Key::Version),
                    value: "01.000.010",
                    value_color: None,
                },
            ],
            language,
        );
    }

    /// A stand-in game window for the banner page.
    fn draw_game_frame(&self, ui: &mut Ui) {
        let screen = ui.max_rect();
        if let Some(hero) = &self.hero {
            ui.painter().image(
                hero.id(),
                screen,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    }

    fn draw_banner(&mut self, ui: &mut Ui, language: crate::ui::i18n::Language) {
        let ctx = ui.ctx().clone();
        let t = &self.set.trophies[1];
        let trophy = BannerTrophy::from_core(t);
        let texture = self
            .banner_icons
            .get(&ctx, "banner/gallery", trophy.icon.as_deref(), true);
        let window = Rect::from_min_size(
            pos2(ui.max_rect().center().x - banner::WINDOW.0 * 0.5, 0.0),
            vec2(banner::WINDOW.0, banner::WINDOW.1),
        );
        banner::paint_card(
            ui,
            window,
            1.0,
            &trophy,
            BannerLook::SETTLED,
            texture,
            language,
        );
    }
}

/// A report for an invented game on an invented PC.
fn sample_report() -> ReportInput {
    ReportInput {
        game_name: "Ember Drift".into(),
        title_id: "PPSA90001".into(),
        version: "01.002.000".into(),
        system: SystemInfo {
            cpu: "Generic CPU".into(),
            threads: 16,
            ram_gb: 32,
            gpus: vec!["Generic GPU 8 GB".into()],
            drivers: vec!["560.94".into()],
            windows: "Windows 11 Home 25H2 (build 26200.1000)".into(),
        },
        launcher_version: env!("CARGO_PKG_VERSION").into(),
        toolkit_version: Some("0.1.3".into()),
        reason: Some("The game crashed (0xC0000005 (ACCESS_VIOLATION)).".into()),
        settings: serde_json::json!({"fullscreen": true, "windowSize": 100, "controls": {"Cross": ["KEY:F"]}}),
        crash: None,
        session: SAMPLE_LOG.into(),
        now: "2026-10-22T09:31:00Z".into(),
    }
}

/// Layouts a community might share for the invented game.
fn sample_community() -> Vec<CommunityLayout> {
    let mouse = Preset::MouseAndKeyboard.changes();
    vec![CommunityLayout {
        name: "Mouse look".into(),
        author: Some("mira".into()),
        description: None,
        changes: mouse,
        tuning: None,
    }]
}

const SAMPLE_LOG: &str = "[anyps5-launcher] start 2026-10-22T09:30:00Z PPSA90001
[engine] loading modules (58)
[engine] module libSceGnmDriver.prx ok
[engine] module libSceAudioOut.prx ok
[engine] vulkan device: Generic GPU, 8 GB
[engine] shader cache: 412 pipelines
[engine] first frame presented
[engine] sceNpTrophy2: 8 trophies registered
[engine] unhandled exception 0xC0000005 at 0x00007FF6A1B2C3D4
The game crashed (0xC0000005 (ACCESS_VIOLATION)).";

/// The bindings an Unreal game's `DefaultInput.ini` might hold.
fn sample_game_input() -> GameInput {
    let ini = r#"
+ActionMappings=(ActionName="Jump",Key=SpaceBar)
+ActionMappings=(ActionName="Jump",Key=Gamepad_FaceButton_Bottom)
+ActionMappings=(ActionName="Dodge",Key=LeftAlt)
+ActionMappings=(ActionName="Dodge",Key=Gamepad_FaceButton_Right)
+ActionMappings=(ActionName="Interact",Key=E)
+ActionMappings=(ActionName="Interact",Key=Gamepad_FaceButton_Left)
+ActionMappings=(ActionName="UsePotion",Key=Q)
+ActionMappings=(ActionName="UsePotion",Key=Gamepad_FaceButton_Top)
+ActionMappings=(ActionName="MeleeAttack",Key=LeftMouseButton)
+ActionMappings=(ActionName="MeleeAttack",Key=Gamepad_RightShoulder)
+ActionMappings=(ActionName="RangedAttack",Key=RightMouseButton)
+ActionMappings=(ActionName="RangedAttack",Key=Gamepad_RightTrigger)
+ActionMappings=(ActionName="Map",Key=M)
+ActionMappings=(ActionName="Map",Key=Gamepad_Special_Left)
+ActionMappings=(ActionName="Pause",Key=Escape)
+ActionMappings=(ActionName="Pause",Key=Gamepad_Special_Right)
+AxisMappings=(AxisName="MoveForward",Scale=1.0,Key=W)
+AxisMappings=(AxisName="MoveForward",Scale=-1.0,Key=S)
+AxisMappings=(AxisName="MoveForward",Scale=1.0,Key=Gamepad_LeftY)
+AxisMappings=(AxisName="MoveRight",Scale=1.0,Key=D)
+AxisMappings=(AxisName="MoveRight",Scale=-1.0,Key=A)
+AxisMappings=(AxisName="MoveRight",Scale=1.0,Key=Gamepad_LeftX)
"#;
    GameInput::Bindings {
        actions: parse_input_ini(ini),
        source: std::path::PathBuf::from("pakchunk0-ps5.pak"),
    }
}
