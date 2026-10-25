//! Gallery pages of the panels: every panel and state rendered over a made-up home screen so it can
//! be screenshotted and compared with the original (`docs/DESIGN.md` screenshot table).
//!
//! ```text
//! anyplay-launcher --gallery=settings|settings1..6|game-settings|confirm|controls|controls-capture|
//!                           controls-end|advanced|trophies|trophies-hidden|trophies-revealed|banner|start|unlocking|log|report|library|
//!                           about-update|behavior-links|settings-codec|settings-codec-off|behavior-clips-off|behavior-clips|behavior-clips-key|toolkit|toolkit-update|toolkit-newer|toolkit-pinned|toolkit-local|toolkit-engine|toolkit-engine-update|
//!                           toolkit-unused|game-toolkit|
//!                           recommended|recommended-warning|whats-new|whats-new-loading|
//!                           saves|saves-empty|restore|
//!                           online|online-browser|online-friends|online-friends-busy|
//!                           online-friends-empty|online-requests|online-friend|
//!                           online-account|online-account-appearing|online-picker|
//!                           online-party|online-party-picker|online-messages|online-conversation|
//!                           online-message|online-request|
//!                           online-party-voice|online-party-member|online-voice|online-voice-key|
//!                           online-voice-devices|online-voice-denied|
//!                           online-screen-ready|online-screen-hosting|online-screen-offer|
//!                           online-screen-cannot|online-screen-watching|online-screen-stalled|
//!                           online-screen-home|
//!                           online-muted|online-restriction|online-paused|online-banned|
//!                           profile-own|profile-own-focus|profile-own-end|profile-friend|profile-friend-hidden|
//!                           profile-request|profile-new|profile-new-friend|profile-private|
//!                           profile-loading|profile-error|profile-offline|
//!                           profile-own-appearing|profile-friend-compare|
//!                           compare|compare-secret|compare-revealed|compare-numbers|
//!                           compare-loading|compare-offline|compare-error|compare-hidden|
//!                           compare-empty|
//!                           feed|feed-reaction|feed-empty|feed-loading|feed-error
//!                           incoming-call|incoming-call-discord|in-call|in-call-open|in-call-discord|calling|
//!                           missed-call|party-discord
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
use anyps5_core::toolkit::InstalledRelease;
use anyps5_core::trophies::{Grade, Trophy, TrophySet};
use egui::{Color32, ColorImage, Context, Rect, TextureHandle, TextureOptions, Ui, pos2, vec2};

use crate::app::preview::{HomeStand, Scene};
use crate::screens::advanced::{AdvancedData, AdvancedPanel};
use crate::screens::banner::{self, BannerLook, BannerTrophy};
use crate::screens::controls::{ControlsData, ControlsPanel};
use crate::screens::game_settings::{
    GameSettingsData, GameSettingsPanel, RecommendedRow, UninstallPanel,
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
use crate::screens::toolkit_pin::PinChoices;
use crate::screens::trophies::{IconCache, TrophyData, TrophyListPanel};
use crate::screens::whatsnew::{NotesView, SAMPLE_NOTES, WhatsNewData, WhatsNewPanel};
use crate::ui::Shell;
use crate::ui::anim::AnimExt;
use crate::ui::chrome::{TileImage, TileOverlay};
use crate::ui::i18n::Key;
use crate::ui::icons::Icon;
use crate::ui::input::InputEvent;
use crate::ui::theme::white;
use anyps5_core::gamefiles::{GameInput, parse_input_ini};

/// The page to show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenPage {
    /// Settings on a category (0 = Appearance, in its rows; the others at the category level).
    Settings(usize),
    GameSettings,
    /// The game sheet on "Toolkit for This Game", the game staying on its own release.
    GameToolkit,
    Confirm,
    Controls,
    ControlsCapture,
    /// The last rows: import, export, reset.
    ControlsEnd,
    Advanced,
    Trophies,
    /// A secret trophy focused, with its Show pill.
    TrophiesHidden,
    /// The same trophy after Show.
    TrophiesRevealed,
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
    /// The About page with the launcher's own update in one state.
    AboutUpdate(UpdateLook),
    /// The notes of a launcher update, in the "What's New" panel.
    WhatsNewLauncher,
    /// Behavior with the AnyPlay links registered to this launcher.
    BehaviourLinks,
    /// Advanced on Linux with share screen on: the OpenH264 row turned on (and downloaded), or off.
    SettingsCodec(bool),
    /// Behavior with Clips off, on at 30 seconds, and waiting for the clip key.
    BehaviourClipsOff,
    BehaviourClips,
    BehaviourClipsKey,
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
    /// The Phone as Controller page in one of its states.
    Phone(PhoneLook),
    /// The profile screen over invented players.
    Profile(ProfileLook),
    /// The trophy comparison with an invented friend, in one of its states.
    Compare(CompareLook),
    /// The Play Together card over the home screen of a game that can be played together.
    Together(crate::screens::gallery_together::TogetherLook),
}

/// The trophy comparison's states (docs/DESIGN.md 17, "Trophy compare").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompareLook {
    /// Both players' trophies in a game the player has.
    Ready,
    /// The focus on a secret trophy, with its Show pill.
    Secret,
    /// The same secret after Show.
    Revealed,
    /// A game that is not in the player's library: the rows are numbers.
    Numbers,
    Loading,
    Offline,
    Error,
    /// The friend keeps their trophies from the player.
    Hidden,
    /// Neither has a trophy.
    Empty,
}

/// The profile screen's states (docs/DESIGN.md 17, "Profile screen").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileLook {
    /// The player's own profile, everything there.
    Own,
    /// The same with the focus on a game row.
    OwnFocus,
    /// The focus on the last row: the page has scrolled and the header has faded.
    OwnEnd,
    /// A friend with everything shared.
    Friend,
    /// A friend who hides trophies and playtime: only what they share is there.
    FriendHidden,
    /// Someone who asked to be friends.
    Request,
    /// The player's own profile while they appear offline.
    OwnAppearing,
    /// A friend, the focus on a game row that opens the comparison.
    FriendCompare,
    /// The player's own profile of a new account.
    New,
    /// A friend who has done nothing yet.
    NewFriend,
    /// A stranger who shares nothing.
    Private,
    Loading,
    Error,
    Offline,
}

impl ProfileLook {
    /// The account the look shows.
    pub fn account(self) -> u64 {
        match self {
            ProfileLook::Own
            | ProfileLook::OwnFocus
            | ProfileLook::OwnEnd
            | ProfileLook::OwnAppearing
            | ProfileLook::New => 1,
            ProfileLook::Friend
            | ProfileLook::FriendCompare
            | ProfileLook::Loading
            | ProfileLook::Error
            | ProfileLook::Offline => 2,
            ProfileLook::FriendHidden => 3,
            ProfileLook::NewFriend => 4,
            ProfileLook::Request => 6,
            ProfileLook::Private => 12,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhoneLook {
    Off,
    Waiting,
    /// A phone asks to be paired.
    Pairing,
    /// A phone plays: two are paired, the pad is ready.
    Connected,
    /// Windows without ViGEmBus.
    DriverMissing,
    /// Linux that may not open /dev/uinput.
    NoAccess,
    /// The server could not start.
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnlineLook {
    Start,
    /// The browser sign-in waits.
    Browser,
    Friends,
    /// The account page while the player appears offline.
    AccountAppearing,
    /// Five friend requests wait (three are answered in place), friends play, one is online.
    FriendsBusy,
    /// Signed in with nobody yet.
    FriendsEmpty,
    /// The cloud is out of reach: the list says so and offers Try Again.
    FriendsOffline,
    /// The cloud has not answered yet.
    FriendsConnecting,
    /// A friend's row has the focus: the quick actions come up.
    FriendsFocus,
    /// The focus stands on the rail.
    FriendsRail,
    /// The Party section while the player is in no party.
    PartyNone,
    /// Every friend request on a page of its own.
    Requests,
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
    /// Share screen, party page: a game runs and the player may share it.
    ScreenReady,
    /// The player shares: Lumen watches directly, Kit through a relay and loses some picture.
    ScreenHosting,
    /// Lumen shares a game; the player may watch.
    ScreenOffer,
    /// Lumen shares, but this build cannot show a stream.
    ScreenCannot,
    /// The player watches Lumen's stream.
    ScreenWatching,
    /// The player watches, and no picture has come for a while.
    ScreenStalled,
    /// Home while a stream is on offer.
    ScreenHome,
    /// Home while a moderator's mute runs.
    Muted,
    /// The page of that mute: what, why, until when, how to appeal.
    Restriction,
    /// A conversation while the mute runs: the field is replaced.
    Paused,
    /// The Start page after a ban.
    Banned,
    /// The feed with items of every kind, the focus on a card.
    Feed,
    /// The feed with the focus on a reaction.
    FeedReaction,
    FeedEmpty,
    FeedLoading,
    FeedError,
    /// A friend calls: the ringing card.
    IncomingCall,
    /// The same while the player's voice is on Discord.
    IncomingCallDiscord,
    /// On a call with two friends, one speaking, the microphone muted.
    InCall,
    /// On a call while the voice is on Discord, the party has a link.
    InCallDiscord,
    /// On a call with the microphone open and the player on the default Push to Talk: the call
    /// talks with Voice Activity and the strip says so.
    InCallOpen,
    /// Waiting for a friend to answer.
    Calling,
    /// The friends list with friends playing together and one without a session.
    TogetherFriends,
    /// A friend's page with a joinable Play Together session.
    TogetherFriend,
    /// The party page with its Play Together row.
    TogetherParty,
    /// A friend who plays the game with no session: why there is nothing to join.
    TogetherNoSession,
    /// The friends list with a missed call.
    MissedCall,
    /// The party page while the voice is on Discord, for the leader.
    PartyDiscord,
}

/// What a launcher release's notes look like (the shape of the GitHub release body).
const LAUNCHER_SAMPLE_NOTES: &str = "## AnyPlay Launcher 0.1.4\n\n### Your Profile\n\n- Your profile button now opens a full-screen profile: your games, hours and trophies.\n- **Edit Profile** opens your account on the web.\n\n### Look and Feel\n\n- Focus is a ring around buttons, rows and cards instead of a white fill.\n- Every control reacts when the mouse is over it.\n\n### Calls\n\n- A call uses Voice Activity while it lasts, so you no longer hold a key to talk.\n";

/// Where the launcher's own update stands on the About page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateLook {
    Checking,
    UpToDate,
    Available,
    Downloading,
    Ready,
    Holding,
    ReadOnly,
    Offline,
    Refused,
}

impl UpdateLook {
    fn state(self) -> LauncherUpdate {
        use crate::screens::settings::UpdateFailure;
        let version = "0.1.4".to_owned();
        match self {
            UpdateLook::Checking => LauncherUpdate::Checking,
            UpdateLook::UpToDate => LauncherUpdate::UpToDate,
            UpdateLook::Available => LauncherUpdate::Available(version),
            UpdateLook::Downloading => LauncherUpdate::Downloading {
                version,
                permille: 420,
            },
            UpdateLook::Ready => LauncherUpdate::Ready(version),
            UpdateLook::Holding => LauncherUpdate::Holding(version),
            UpdateLook::ReadOnly => LauncherUpdate::ReadOnly(version),
            UpdateLook::Offline => LauncherUpdate::Failed(UpdateFailure::Offline),
            UpdateLook::Refused => LauncherUpdate::Failed(UpdateFailure::Refused),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolkitLook {
    Stable,
    Update,
    /// A toolkit newer than the launcher was tested with: its note hangs under the header.
    Newer,
    Pinned,
    Local,
    /// The AnyPlay engine channel with a build installed: its build day hangs under the header.
    Engine,
    /// The engine channel with a newer nightly build waiting.
    EngineUpdate,
    /// Two installed releases nothing uses: the page offers to remove them.
    Unused,
    /// The system's glibc is older than the libraries ask for (Linux): the note hangs under the header.
    Glibc,
}

/// Invented phones for the Phone as Controller page.
fn sample_phone(look: PhoneLook) -> crate::app::phone::PhoneState {
    use anyps5_core::remote::{PairedDevice, PairingRequest, RemoteInfo, VirtualPadStatus};
    let device = |id: &str, name: &str, added: &str| PairedDevice {
        added: added.into(),
        id: id.into(),
        key: String::new(),
        name: name.into(),
    };
    let mut state = crate::app::phone::PhoneState {
        windows: look != PhoneLook::NoAccess,
        info: Some(RemoteInfo {
            addresses: vec!["192.168.178.20".parse().expect("an address")],
            control_port: 47700,
            udp_port: 47702,
            preferred_port: true,
            discovery_error: None,
        }),
        pad: Some(VirtualPadStatus::Ready),
        ..Default::default()
    };
    match look {
        PhoneLook::Off => state.info = None,
        PhoneLook::Waiting => {}
        PhoneLook::Pairing => {
            state.connecting = Some("Pixel 8".into());
            state.pairing = Some(PairingRequest {
                id: 1,
                device_name: "Pixel 8".into(),
                code: "4821".into(),
            });
        }
        PhoneLook::Connected => {
            state.connected = Some("Pixel 8".into());
            state.latency_ms = Some(4.0);
            state.paired = vec![
                device("a1", "Pixel 8", "2026-10-24T18:14:03Z"),
                device("b2", "Galaxy S23", "2026-10-25T09:02:41Z"),
            ];
        }
        PhoneLook::DriverMissing => {
            state.paired = vec![device("a1", "Pixel 8", "2026-10-24T18:14:03Z")];
            state.pad = Some(VirtualPadStatus::DriverMissing);
        }
        PhoneLook::NoAccess => {
            state.paired = vec![device("a1", "Pixel 8", "2026-10-24T18:14:03Z")];
            state.pad = Some(VirtualPadStatus::NoAccess("permission denied".into()));
        }
        PhoneLook::Failed => {
            state.info = None;
            state.failed = true;
        }
    }
    state
}

/// The page named on the command line.
pub fn parse(name: &str) -> Option<ScreenPage> {
    Some(match name {
        "settings" => ScreenPage::Settings(0),
        "game-settings" => ScreenPage::GameSettings,
        "game-toolkit" => ScreenPage::GameToolkit,
        "confirm" => ScreenPage::Confirm,
        "controls" => ScreenPage::Controls,
        "controls-capture" => ScreenPage::ControlsCapture,
        "controls-end" => ScreenPage::ControlsEnd,
        "advanced" => ScreenPage::Advanced,
        "trophies" => ScreenPage::Trophies,
        "trophies-hidden" => ScreenPage::TrophiesHidden,
        "trophies-revealed" => ScreenPage::TrophiesRevealed,
        "banner" => ScreenPage::Banner,
        "start" => ScreenPage::Start,
        "unlocking" => ScreenPage::Unlocking,
        "log" => ScreenPage::Log,
        "report" => ScreenPage::Report,
        "library" => ScreenPage::Library,
        "about-update" => ScreenPage::AboutUpdate(UpdateLook::Ready),
        "about-update-checking" => ScreenPage::AboutUpdate(UpdateLook::Checking),
        "about-update-current" => ScreenPage::AboutUpdate(UpdateLook::UpToDate),
        "about-update-available" => ScreenPage::AboutUpdate(UpdateLook::Available),
        "about-update-downloading" => ScreenPage::AboutUpdate(UpdateLook::Downloading),
        "about-update-holding" => ScreenPage::AboutUpdate(UpdateLook::Holding),
        "about-update-readonly" => ScreenPage::AboutUpdate(UpdateLook::ReadOnly),
        "about-update-offline" => ScreenPage::AboutUpdate(UpdateLook::Offline),
        "about-update-refused" => ScreenPage::AboutUpdate(UpdateLook::Refused),
        "whats-new-launcher" => ScreenPage::WhatsNewLauncher,
        "behavior-links" => ScreenPage::BehaviourLinks,
        "settings-codec" => ScreenPage::SettingsCodec(true),
        "settings-codec-off" => ScreenPage::SettingsCodec(false),
        "behavior-clips-off" => ScreenPage::BehaviourClipsOff,
        "behavior-clips" => ScreenPage::BehaviourClips,
        "behavior-clips-key" => ScreenPage::BehaviourClipsKey,
        "phone-off" => ScreenPage::Phone(PhoneLook::Off),
        "phone-waiting" => ScreenPage::Phone(PhoneLook::Waiting),
        "phone-pairing" => ScreenPage::Phone(PhoneLook::Pairing),
        "phone-connected" => ScreenPage::Phone(PhoneLook::Connected),
        "phone-driver" => ScreenPage::Phone(PhoneLook::DriverMissing),
        "phone-access" => ScreenPage::Phone(PhoneLook::NoAccess),
        "phone-failed" => ScreenPage::Phone(PhoneLook::Failed),
        "toolkit" => ScreenPage::Toolkit(ToolkitLook::Stable),
        "toolkit-update" => ScreenPage::Toolkit(ToolkitLook::Update),
        "toolkit-newer" => ScreenPage::Toolkit(ToolkitLook::Newer),
        "toolkit-pinned" => ScreenPage::Toolkit(ToolkitLook::Pinned),
        "toolkit-local" => ScreenPage::Toolkit(ToolkitLook::Local),
        "toolkit-engine" => ScreenPage::Toolkit(ToolkitLook::Engine),
        "toolkit-engine-update" => ScreenPage::Toolkit(ToolkitLook::EngineUpdate),
        "toolkit-unused" => ScreenPage::Toolkit(ToolkitLook::Unused),
        "toolkit-glibc" => ScreenPage::Toolkit(ToolkitLook::Glibc),
        "recommended" => ScreenPage::Recommended,
        "recommended-warning" => ScreenPage::RecommendedWarning,
        "whats-new" => ScreenPage::WhatsNew,
        "whats-new-loading" => ScreenPage::WhatsNewLoading,
        "saves" => ScreenPage::Saves,
        "saves-empty" => ScreenPage::SavesEmpty,
        "restore" => ScreenPage::Restore,
        "friends-signin" => ScreenPage::Online(OnlineLook::Start),
        "friends-browser" => ScreenPage::Online(OnlineLook::Browser),
        "friends" => ScreenPage::Online(OnlineLook::Friends),
        "friends-busy" => ScreenPage::Online(OnlineLook::FriendsBusy),
        "friends-empty" => ScreenPage::Online(OnlineLook::FriendsEmpty),
        "friends-offline" => ScreenPage::Online(OnlineLook::FriendsOffline),
        "friends-connecting" => ScreenPage::Online(OnlineLook::FriendsConnecting),
        "friends-focus" => ScreenPage::Online(OnlineLook::FriendsFocus),
        "friends-rail" => ScreenPage::Online(OnlineLook::FriendsRail),
        "friends-party-none" => ScreenPage::Online(OnlineLook::PartyNone),
        "friends-requests" => ScreenPage::Online(OnlineLook::Requests),
        "friends-friend" => ScreenPage::Online(OnlineLook::Friend),
        "friends-settings" => ScreenPage::Online(OnlineLook::Account),
        "friends-settings-appearing" => ScreenPage::Online(OnlineLook::AccountAppearing),
        "friends-picker" => ScreenPage::Online(OnlineLook::Picker),
        "friends-party" => ScreenPage::Online(OnlineLook::Party),
        "friends-party-picker" => ScreenPage::Online(OnlineLook::PartyPicker),
        "friends-messages" => ScreenPage::Online(OnlineLook::Messages),
        "friends-message" => ScreenPage::Online(OnlineLook::Message),
        "friends-conversation" => ScreenPage::Online(OnlineLook::Conversation),
        "friends-request" => ScreenPage::Online(OnlineLook::Request),
        "friends-party-voice" => ScreenPage::Online(OnlineLook::PartyVoice),
        "friends-party-member" => ScreenPage::Online(OnlineLook::PartyMember),
        "friends-voice" => ScreenPage::Online(OnlineLook::Voice),
        "friends-voice-key" => ScreenPage::Online(OnlineLook::VoiceKey),
        "friends-voice-devices" => ScreenPage::Online(OnlineLook::VoiceDevices),
        "friends-voice-denied" => ScreenPage::Online(OnlineLook::VoiceDenied),
        "friends-screen-ready" => ScreenPage::Online(OnlineLook::ScreenReady),
        "friends-screen-hosting" => ScreenPage::Online(OnlineLook::ScreenHosting),
        "friends-screen-offer" => ScreenPage::Online(OnlineLook::ScreenOffer),
        "friends-screen-cannot" => ScreenPage::Online(OnlineLook::ScreenCannot),
        "friends-screen-watching" => ScreenPage::Online(OnlineLook::ScreenWatching),
        "friends-screen-stalled" => ScreenPage::Online(OnlineLook::ScreenStalled),
        "friends-screen-home" => ScreenPage::Online(OnlineLook::ScreenHome),
        "friends-muted" => ScreenPage::Online(OnlineLook::Muted),
        "friends-restriction" => ScreenPage::Online(OnlineLook::Restriction),
        "friends-paused" => ScreenPage::Online(OnlineLook::Paused),
        "friends-banned" => ScreenPage::Online(OnlineLook::Banned),
        "profile-own" => ScreenPage::Profile(ProfileLook::Own),
        "profile-own-appearing" => ScreenPage::Profile(ProfileLook::OwnAppearing),
        "profile-friend-compare" => ScreenPage::Profile(ProfileLook::FriendCompare),
        "compare" => ScreenPage::Compare(CompareLook::Ready),
        "compare-secret" => ScreenPage::Compare(CompareLook::Secret),
        "compare-revealed" => ScreenPage::Compare(CompareLook::Revealed),
        "compare-numbers" => ScreenPage::Compare(CompareLook::Numbers),
        "compare-loading" => ScreenPage::Compare(CompareLook::Loading),
        "compare-offline" => ScreenPage::Compare(CompareLook::Offline),
        "compare-error" => ScreenPage::Compare(CompareLook::Error),
        "compare-hidden" => ScreenPage::Compare(CompareLook::Hidden),
        "compare-empty" => ScreenPage::Compare(CompareLook::Empty),
        "profile-own-focus" => ScreenPage::Profile(ProfileLook::OwnFocus),
        "profile-own-end" => ScreenPage::Profile(ProfileLook::OwnEnd),
        "profile-friend" => ScreenPage::Profile(ProfileLook::Friend),
        "profile-friend-hidden" => ScreenPage::Profile(ProfileLook::FriendHidden),
        "profile-request" => ScreenPage::Profile(ProfileLook::Request),
        "profile-new" => ScreenPage::Profile(ProfileLook::New),
        "profile-new-friend" => ScreenPage::Profile(ProfileLook::NewFriend),
        "profile-private" => ScreenPage::Profile(ProfileLook::Private),
        "profile-loading" => ScreenPage::Profile(ProfileLook::Loading),
        "profile-error" => ScreenPage::Profile(ProfileLook::Error),
        "profile-offline" => ScreenPage::Profile(ProfileLook::Offline),
        "friends-activity" => ScreenPage::Online(OnlineLook::Feed),
        "friends-activity-reaction" => ScreenPage::Online(OnlineLook::FeedReaction),
        "friends-activity-empty" => ScreenPage::Online(OnlineLook::FeedEmpty),
        "friends-activity-loading" => ScreenPage::Online(OnlineLook::FeedLoading),
        "friends-activity-error" => ScreenPage::Online(OnlineLook::FeedError),
        "incoming-call" => ScreenPage::Online(OnlineLook::IncomingCall),
        "incoming-call-discord" => ScreenPage::Online(OnlineLook::IncomingCallDiscord),
        "in-call" => ScreenPage::Online(OnlineLook::InCall),
        "in-call-discord" => ScreenPage::Online(OnlineLook::InCallDiscord),
        "in-call-open" => ScreenPage::Online(OnlineLook::InCallOpen),
        "calling" => ScreenPage::Online(OnlineLook::Calling),
        "missed-call" => ScreenPage::Online(OnlineLook::MissedCall),
        "party-discord" => ScreenPage::Online(OnlineLook::PartyDiscord),
        "together-friends" => ScreenPage::Online(OnlineLook::TogetherFriends),
        "together-friend" => ScreenPage::Online(OnlineLook::TogetherFriend),
        "together-party" => ScreenPage::Online(OnlineLook::TogetherParty),
        "together-friend-nosession" => ScreenPage::Online(OnlineLook::TogetherNoSession),
        other if crate::screens::gallery_together::parse(other).is_some() => {
            ScreenPage::Together(crate::screens::gallery_together::parse(other)?)
        }
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
fn online_data<'a>(
    online: &'a anyps5_core::online::Online,
    play_together: Option<&'a str>,
    language: crate::ui::i18n::Language,
    accent: Color32,
) -> crate::screens::online::OnlineData<'a> {
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
        play_together,
    }
}

/// Invented friends: one plays a game you can join, one plays, one is online, one is not; a
/// request waits, one was sent, and a friend invited you.
fn sample_online(look: OnlineLook) -> anyps5_core::online::Online {
    use anyps5_core::online::api::{
        Account, AccountId, Activity, Count, Friend, FriendLists, Invitation, Presence, Privacy,
        Request, SessionRef,
    };
    use anyps5_core::online::control::{Appeal, Restriction};
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
                kind: Default::default(),
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
        OnlineLook::Banned => {
            state.restrictions.ban(Restriction {
                rule: Some("cheating".into()),
                reason: Some(
                    "Used a program that changes the game for everyone in the session.".into(),
                ),
                action: Some(12),
                appeal: Some(Appeal {
                    status: "available".into(),
                    deadline: SAMPLE_NOW as u64 + 150 * 86_400,
                }),
                ..Restriction::new(Restriction::BANNED, Some(SAMPLE_NOW as u64 + 30 * 86_400))
            });
            return state;
        }
        _ => {}
    }
    state.account = Some(SignedIn {
        id: AccountId(1),
        handle: "Mira".into(),
        email: "mira@example.org".into(),
        appear_offline: false,
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
        kind: Default::default(),
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
    if look == OnlineLook::AccountAppearing
        && let Some(account) = state.account.as_mut()
    {
        account.appear_offline = true;
    }
    crate::screens::gallery_feed::sample(&mut state, look);
    if matches!(look, OnlineLook::FriendsBusy | OnlineLook::Requests) {
        state.friends.incoming = [
            (6, "Vale"),
            (9, "Wren"),
            (10, "Ember"),
            (11, "Sol"),
            (12, "Tarn"),
        ]
        .into_iter()
        .map(|(id, handle)| Request {
            account: account(id, handle),
            since: 0,
        })
        .collect();
    }
    if look == OnlineLook::FriendsEmpty {
        state.friends = FriendLists::default();
        state.invitations.clear();
    } else {
        sample_social(&mut state);
    }
    match look {
        OnlineLook::FriendsOffline => state.cloud = Some(Cloud::Offline),
        OnlineLook::FriendsConnecting => state.cloud = None,
        OnlineLook::PartyNone => {
            state.social.apply_party(None);
        }
        _ => {}
    }
    sample_voice(&mut state, look);
    sample_screen(&mut state, look);
    sample_calls(&mut state, look);
    if matches!(
        look,
        OnlineLook::TogetherFriends
            | OnlineLook::TogetherFriend
            | OnlineLook::TogetherParty
            | OnlineLook::TogetherNoSession
    ) {
        crate::screens::gallery_together::sample(&mut state);
        state.session = None;
    }
    if matches!(
        look,
        OnlineLook::Muted | OnlineLook::Restriction | OnlineLook::Paused
    ) {
        state.restrictions.list(vec![Restriction {
            rule: Some("spam".into()),
            reason: Some("The same link went to many people who did not know you.".into()),
            action: Some(41),
            appeal: Some(Appeal {
                status: "available".into(),
                deadline: SAMPLE_NOW as u64 + 150 * 86_400,
            }),
            ..Restriction::new(Restriction::MUTED, Some(SAMPLE_NOW as u64 + 3 * 86_400))
        }]);
    }
    if look == OnlineLook::Picker {
        state.dialog = Some(Dialog {
            id: 1,
            title: NAMES[1].into(),
            max: 3,
        });
    }
    state
}

/// Calls over the invented state: a ringing call, a call in progress, a missed call.
fn sample_calls(state: &mut anyps5_core::online::Online, look: OnlineLook) {
    use anyps5_core::online::api::{Account, AccountId, MissedCall, PartyInvitation};
    use anyps5_core::online::{VoicePrefs, VoiceProvider};
    let account = |id: u64, handle: &str| Account {
        id: AccountId(id),
        handle: handle.into(),
    };
    let discord = matches!(
        look,
        OnlineLook::IncomingCallDiscord | OnlineLook::InCallDiscord | OnlineLook::PartyDiscord
    );
    if discord {
        state.voice.load(VoicePrefs {
            provider: VoiceProvider::Discord,
            ..VoicePrefs::default()
        });
    }
    match look {
        OnlineLook::IncomingCall | OnlineLook::IncomingCallDiscord => {
            // The ringing call comes from a friend who is not in a party with the player.
            state.social.apply_party(None);
            state.social.party_invitations.clear();
            state.social.add_party_invitation(PartyInvitation {
                id: "call1".into(),
                from: account(2, "Lumen"),
                party: "p-call".into(),
                members: 1,
                expires: u64::MAX / 2,
                call: true,
            });
        }
        OnlineLook::InCall | OnlineLook::InCallDiscord | OnlineLook::InCallOpen => {
            state.calls.active = true;
            if look == OnlineLook::InCall {
                state.voice.muted = true;
            }
            if let Some(party) = state.social.party.as_ref().cloned() {
                let mut party = party;
                party.revision += 1;
                if discord {
                    party.discord = Some("https://discord.com/channels/1/2".into());
                }
                state.social.apply_party(Some(party));
            }
        }
        OnlineLook::Calling => {
            state.calls.ringing = Some(account(2, "Lumen"));
            if let Some(mut party) = state.social.party.as_ref().cloned() {
                party.members.truncate(1);
                party.revision += 1;
                state.social.apply_party(Some(party));
            }
        }
        OnlineLook::MissedCall => {
            // A quiet list, so the friends are in view.
            state.invitations.clear();
            state.friends.incoming.clear();
            state.friends.outgoing.clear();
            state.social.party_invitations.clear();
            state.social.apply_party(None);
            state.session = None;
            state.calls.missed.push(MissedCall {
                from: account(4, "Kit"),
                at: SAMPLE_NOW as u64 - 300,
            });
        }
        OnlineLook::PartyDiscord => {
            if let Some(mut party) = state.social.party.as_ref().cloned() {
                party.revision += 1;
                party.discord = Some("https://discord.gg/nSHbHBRdS".into());
                state.social.apply_party(Some(party));
            }
        }
        _ => {}
    }
    // The looks set the call by hand; the voice mode follows it as it does for a real one.
    state.sync_voice_mode();
}

/// Party voice over the invented party: on for the looks that show it, with Lumen speaking.
fn sample_voice(state: &mut anyps5_core::online::Online, look: OnlineLook) {
    use anyps5_core::online::api::AccountId;
    use anyps5_core::online::control::{VoiceLevel, VoicePeer, VoiceState};
    use anyps5_core::online::{Devices, PeerPrefs, VoicePrefs};
    if !matches!(
        look,
        OnlineLook::PartyVoice
            | OnlineLook::InCall
            | OnlineLook::InCallDiscord
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

/// Share screen over the invented party: Lumen and Kit are the other two members.
fn sample_screen(state: &mut anyps5_core::online::Online, look: OnlineLook) {
    use anyps5_core::online::api::AccountId;
    use anyps5_core::online::control::{
        Link, ScreenHosting, ScreenOffer, ScreenPhase, ScreenStats, ScreenViewer, ScreenWatching,
    };
    let stats = ScreenStats {
        width: 1920,
        height: 1080,
        fps: 60,
        kbps: 8000,
    };
    let offer = vec![ScreenOffer {
        account: AccountId(2),
        title: NAMES[1].into(),
        link: Some(Link::Direct),
    }];
    let watching = |phase| ScreenWatching {
        account: AccountId(2),
        title: NAMES[1].into(),
        phase,
        link: Some(Link::Direct),
        rtt_ms: Some(24),
        stats,
        shown_fps: 60,
    };
    let (can_share, can_watch, hosting, offers, watching) = match look {
        OnlineLook::ScreenReady => (true, true, None, vec![], None),
        OnlineLook::ScreenHosting => (
            true,
            true,
            Some(ScreenHosting {
                phase: ScreenPhase::Live,
                title: NAMES[1].into(),
                codec: None,
                stats,
                viewers: vec![
                    ScreenViewer {
                        account: AccountId(2),
                        watching: true,
                        link: Some(Link::Direct),
                        rtt_ms: Some(24),
                        loss_percent: 0,
                    },
                    ScreenViewer {
                        account: AccountId(4),
                        watching: true,
                        link: Some(Link::Relay),
                        rtt_ms: Some(80),
                        loss_percent: 8,
                    },
                ],
            }),
            vec![],
            None,
        ),
        OnlineLook::ScreenOffer | OnlineLook::ScreenHome => (false, true, None, offer, None),
        OnlineLook::ScreenCannot => (false, false, None, offer, None),
        OnlineLook::ScreenWatching => (false, true, None, offer, Some(watching(ScreenPhase::Live))),
        OnlineLook::ScreenStalled => (
            false,
            true,
            None,
            offer,
            Some(watching(ScreenPhase::Stalled)),
        ),
        _ => return,
    };
    state.apply(anyps5_core::online::Line::Output(
        anyps5_core::online::Output::Event(anyps5_core::online::Event::Screen {
            can_share,
            can_watch,
            hosting,
            offers,
            watching,
        }),
    ));
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
        discord: None,
    }));
    state.social.add_party_invitation(PartyInvitation {
        id: "pi1".into(),
        from: account(5, "Rook"),
        party: "p9".into(),
        members: 2,
        expires: 0,
        call: false,
    });
    state.session = Some(Session {
        kind: Default::default(),
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
            unread: 0,
            muted: false,
            pinned: false,
            rev: 0,
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
        delivered: 4,
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
            reply: None,
            edited: None,
            deleted: false,
            reactions: Vec::new(),
            rev: 0,
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
            rev: 0,
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
            rev: 0,
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

/// The invented set's secret trophy ("The Glass Wing").
const SECRET_TROPHY: usize = 6;

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
    avatars: Vec<TextureHandle>,
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
    friends: crate::screens::friends::FriendsScreen,
    online: anyps5_core::online::Online,
    profile: crate::screens::profile::ProfileScreen,
    compare: crate::screens::compare::CompareScreen,
    recommended: RecommendedPanel,
    whats_new: WhatsNewPanel,
    notes: Published,
    /// The notes of a launcher update.
    launcher_notes: Published,
    update: UpdateState,
    toolkit_version: String,
    phone: crate::app::phone::PhoneState,
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
    /// The real home screen the panels are drawn over.
    home: Option<HomeStand>,
    opened: bool,
}

impl ScreenGallery {
    pub fn new(
        page: ScreenPage,
        language: crate::ui::i18n::Language,
        theme: crate::ui::theme::Theme,
    ) -> Self {
        let mut prefs = Preferences::first_run();
        prefs.theme = crate::screens::convert::theme_to_core(theme);
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
                ToolkitLook::Engine | ToolkitLook::EngineUpdate => {
                    prefs.toolkit_engine = true;
                    toolkit_version = "anyplay-30d0fe9".into();
                    if look == ToolkitLook::EngineUpdate {
                        update = UpdateState::Available("anyplay-a1b2c3d".into());
                    }
                }
                ToolkitLook::Unused | ToolkitLook::Glibc => {}
            }
        }
        let phone = match page {
            ScreenPage::Phone(look) => {
                prefs.phone_remote = look != PhoneLook::Off;
                sample_phone(look)
            }
            _ => Default::default(),
        };
        let mut game_settings = GameSettings::default();
        if page == ScreenPage::GameToolkit {
            game_settings.toolkit = Some("v0.1.2".into());
        }
        Self {
            recommended: RecommendedPanel::default(),
            whats_new: WhatsNewPanel::default(),
            notes: Published {
                tag: "0.1.3".into(),
                date: "2026-10-20".into(),
                notes: ReleaseNotes::parse(SAMPLE_NOTES),
            },
            launcher_notes: Published {
                tag: "0.1.4".into(),
                date: String::new(),
                notes: ReleaseNotes::parse_launcher(LAUNCHER_SAMPLE_NOTES),
            },
            update,
            toolkit_version,
            phone,
            page,
            covers: Vec::new(),
            avatars: Vec::new(),
            hero: None,
            prefs,
            game_settings,
            games: (0..NAMES.len()).map(fake_game).collect(),
            set: fake_trophies(),
            settings: SettingsPanel::default(),
            game_panel: GameSettingsPanel::default(),
            uninstall: UninstallPanel::default(),
            saves: SavesPanel::default(),
            restore: RestorePanel::default(),
            friends: crate::screens::friends::FriendsScreen::default(),
            profile: crate::screens::profile::ProfileScreen::default(),
            compare: crate::screens::compare::CompareScreen::default(),
            online: match page {
                ScreenPage::Online(look) => sample_online(look),
                ScreenPage::Profile(look) => {
                    let mut state = sample_online(OnlineLook::Friends);
                    crate::screens::gallery_feed::sample_profile(&mut state, look);
                    state
                }
                ScreenPage::Compare(look) => {
                    let mut state = sample_online(OnlineLook::Friends);
                    crate::screens::gallery_feed::sample_compare(&mut state, look);
                    state
                }
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
            home: None,
            opened: false,
        }
    }

    /// The name for the party's Play Together row: only the look that shows it.
    fn together_name(&self) -> Option<&'static str> {
        (self.page == ScreenPage::Online(OnlineLook::TogetherParty)).then_some(NAMES[1])
    }

    /// The ringing card and the call strip of the call looks.
    fn call_surfaces(
        &self,
        ctx: &Context,
        look: OnlineLook,
        accent: Color32,
        language: crate::ui::i18n::Language,
    ) {
        use crate::screens::calls;
        if let Some(card) = calls::card_data(&self.online, 0) {
            calls::incoming_card(ctx, accent, &card, language, 1.0, true);
        }
        if matches!(
            look,
            OnlineLook::InCall
                | OnlineLook::InCallDiscord
                | OnlineLook::InCallOpen
                | OnlineLook::Calling
        ) && let Some(strip) = calls::strip_data(&self.online)
        {
            calls::call_strip(ctx, &strip, language, 1.0, true);
        }
    }

    /// The comparison with the invented friend Lumen in the first game.
    fn compare_screen(&mut self, shell: &mut Shell, ctx: &Context, look: CompareLook) {
        use crate::screens::compare::{Inputs, view};
        let mut local = crate::screens::gallery_feed::local(&self.covers, &self.avatars, &NAMES);
        // A game the player does not have: no name, no picture, no trophy file.
        let owned = !matches!(look, CompareLook::Numbers | CompareLook::Empty);
        if !owned {
            local.games.remove("GAME00001");
        }
        let revealed = self.compare.revealed().clone();
        let language = shell.language;
        let inputs = Inputs {
            online: &self.online,
            local: &local,
            set: owned.then_some(&self.set),
            account: anyps5_core::online::api::AccountId(2),
            title_id: "GAME00001",
            show_hidden: false,
            revealed: &revealed,
            language,
            utc_offset: Some(0),
        };
        let model = view(&inputs);
        let accent = shell.accent_color();
        let look = crate::screens::compare::Look { language, accent };
        self.compare.show(shell, ctx, &[], &model, inputs.set, look);
    }

    /// The profile screen over the invented state: the library is the gallery's nine games.
    fn profile_screen(&mut self, shell: &mut Shell, ctx: &Context, look: ProfileLook) {
        use crate::screens::profile::{Inputs, OwnGame, OwnLibrary, OwnTrophy};
        use crate::ui::theme::Grade as G;
        let local = crate::screens::gallery_feed::local(&self.covers, &self.avatars, &NAMES);
        // Three of the invented games are the ones the invented friends play.
        let title = |i: usize| match i {
            1 => "GAME00001".to_owned(),
            4 => "GAME00002".to_owned(),
            2 => "GAME00003".to_owned(),
            _ => format!("GAME{:05}", 10 + i),
        };
        let new = look == ProfileLook::New;
        let hours = [0u64, 42, 7, 0, 20, 3, 0, 1, 0];
        let ago = [
            0i64,
            2 * 3_600,
            26 * 3_600,
            0,
            15 * 86_400,
            3 * 86_400,
            0,
            40 * 86_400,
            0,
        ];
        let trophies = [
            None,
            Some((31, 48)),
            Some((9, 40)),
            None,
            Some((12, 41)),
            Some((1, 22)),
            None,
            None,
            None,
        ];
        let trophy = |game: usize, name: &str, grade, ago: i64| OwnTrophy {
            title_id: title(game),
            game: NAMES[game].into(),
            name: name.into(),
            grade,
            at: SAMPLE_NOW - ago,
        };
        let library = OwnLibrary {
            games: if new {
                Vec::new()
            } else {
                (0..NAMES.len())
                    .map(|i| OwnGame {
                        title_id: title(i),
                        name: NAMES[i].to_owned(),
                        art: Some(self.tile(i)),
                        seconds: hours[i] * 3_600 + if hours[i] > 0 { 600 } else { 0 },
                        last_played: (ago[i] > 0).then_some(SAMPLE_NOW - ago[i]),
                        trophies: trophies[i],
                    })
                    .collect()
            },
            trophies: if new {
                Vec::new()
            } else {
                vec![
                    trophy(1, "First Light", G::Gold, 40 * 60),
                    trophy(1, "Harbor Master", G::Silver, 3 * 3_600),
                    trophy(2, "Quiet Passage", G::Bronze, 30 * 3_600),
                    trophy(4, "Lantern Keeper", G::Platinum, 4 * 86_400),
                    trophy(4, "Anchored", G::Bronze, 5 * 86_400),
                ]
            },
            grades: if new { [0; 4] } else { [1, 4, 9, 30] },
        };
        let inputs = Inputs {
            online: &self.online,
            local: &local,
            library: Some(&library),
            account: anyps5_core::online::api::AccountId(look.account()),
            language: shell.language,
            now: SAMPLE_NOW,
            utc_offset: Some(0),
            unread: 0,
        };
        let accent = shell.accent_color();
        self.profile.show(shell, ctx, &[], &inputs, accent);
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
            self.avatars = crate::screens::gallery_feed::avatar_textures(ctx);
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
            let page = self.page;
            let unlocking = matches!(page, ScreenPage::Unlocking);
            let scene = match page {
                ScreenPage::Unlocking => Scene::HomeUnlocking,
                ScreenPage::Online(_) | ScreenPage::Profile(_) => Scene::HomeFriends,
                ScreenPage::Together(_) => Scene::HomeTogether,
                _ => Scene::Home,
            };
            // The real home, as the launcher paints it; its focus stays hidden under a panel.
            self.home
                .get_or_insert_with(|| HomeStand::new(&ctx, self.prefs.clone(), scene, !unlocking))
                .paint(ui, shell);
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
                        tint: crate::ui::theme::color::text_active(),
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
        let unused: Vec<InstalledRelease> = if self.page == ScreenPage::Toolkit(ToolkitLook::Unused)
        {
            [("0.1.2", 412), ("0.1.1", 398)]
                .map(|(tag, mib)| InstalledRelease {
                    tag: tag.into(),
                    bytes: mib * 1024 * 1024,
                })
                .to_vec()
        } else {
            Vec::new()
        };
        // 0.1.2 is installed, 0.1.1 is not: the choice shows both, and the catalogue advises 0.1.2.
        let pin = PinChoices {
            installed: vec!["0.1.3".into(), "0.1.2".into()],
            recent: recent.clone(),
            recommended: Some("v0.1.2".into()),
        };
        let env = SettingsEnv {
            unused: &unused,
            games_found: self.games.len(),
            app_version: "0.1.0",
            toolkit_version: Some(&self.toolkit_version),
            engine_built: self.prefs.toolkit_engine.then_some("2026-10-25"),
            glibc: matches!(self.page, ScreenPage::Toolkit(ToolkitLook::Glibc)).then_some(
                anyps5_core::glibc::Shortfall {
                    needed: anyps5_core::glibc::Version::new(2, 38),
                    system: anyps5_core::glibc::Version::new(2, 35),
                },
            ),
            update: self.update.clone(),
            recent: &recent,
            launcher: match self.page {
                ScreenPage::AboutUpdate(look) => look.state(),
                _ => LauncherUpdate::None,
            },
            launcher_notes: matches!(
                self.page,
                ScreenPage::AboutUpdate(UpdateLook::Ready | UpdateLook::Holding)
            ),
            default_screenshots: std::path::Path::new("C:\\Users\\mira\\Pictures\\AnyPS5"),
            links: if matches!(self.page, ScreenPage::BehaviourLinks) {
                anyps5_core::linkreg::Handler::ThisExe
            } else {
                anyps5_core::linkreg::Handler::Absent
            },
            cjk_fonts: anyps5_core::fonts::CjkState::Missing,
            phone: &self.phone,
            openh264: matches!(self.page, ScreenPage::SettingsCodec(_)).then(|| {
                crate::screens::settings::Openh264Env {
                    state: anyps5_core::openh264::CodecState::Ready,
                    worker: Some(("ready".into(), None)),
                }
            }),
            clips: crate::screens::settings::ClipsEnv {
                count: if self.prefs.clips_seconds > 0 { 3 } else { 0 },
                capturing: matches!(self.page, ScreenPage::BehaviourClipsKey),
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
                pin: &pin,
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
            let data = online_data(&self.online, self.together_name(), language, accent);
            if self.friends.wants_local() {
                self.friends.set_local(crate::screens::gallery_feed::local(
                    &self.covers,
                    &self.avatars,
                    &NAMES,
                ));
            }
            self.friends.show(shell, &ctx, &events, &data);
        }
        if let ScreenPage::Online(look) = self.page {
            self.call_surfaces(&ctx, look, accent, language);
        }
        if let ScreenPage::Together(look) = self.page {
            crate::screens::gallery_together::paint(&ctx, accent, look, language);
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
            let launcher = matches!(self.page, ScreenPage::WhatsNewLauncher);
            let view = if matches!(self.page, ScreenPage::WhatsNewLoading) {
                NotesView::Loading
            } else if launcher {
                NotesView::Ready(&self.launcher_notes)
            } else {
                NotesView::Ready(&self.notes)
            };
            let data = WhatsNewData {
                tag: if launcher { "0.1.4" } else { "0.1.3" },
                view,
                language,
                launcher,
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
                show_hidden: self.prefs.show_hidden_trophies,
            },
        );
        if let ScreenPage::Profile(look) = self.page {
            self.profile_screen(shell, &ctx, look);
        }
        if let ScreenPage::Compare(look) = self.page {
            self.compare_screen(shell, &ctx, look);
        }
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
            ScreenPage::GameToolkit => self.game_panel.open_row(10),
            ScreenPage::Confirm => self.uninstall.open(),
            ScreenPage::Saves | ScreenPage::SavesEmpty => self.saves.open(),
            ScreenPage::Restore => self.restore.open(),
            ScreenPage::Online(look) => {
                use crate::screens::online::Page;
                let language = shell.language;
                let data = online_data(
                    &self.online,
                    self.together_name(),
                    language,
                    shell.accent_color(),
                );
                // The call looks show the ringing card or the strip over the bare home screen.
                if matches!(
                    look,
                    OnlineLook::IncomingCall
                        | OnlineLook::IncomingCallDiscord
                        | OnlineLook::InCall
                        | OnlineLook::InCallDiscord
                        | OnlineLook::InCallOpen
                        | OnlineLook::Calling
                ) {
                    return;
                }
                self.friends.open(&data);
                match look {
                    OnlineLook::Requests => self.friends.show_page(Page::Requests, &data),
                    OnlineLook::FriendsFocus => {
                        self.friends.focus_row("friendRow", 1, &data);
                    }
                    OnlineLook::FriendsRail => self.friends.focus_rail(),
                    OnlineLook::PartyNone => self.friends.show_page(Page::Party, &data),
                    OnlineLook::MissedCall => self.friends.show_page(Page::Home, &data),
                    OnlineLook::PartyDiscord => self.friends.show_page(Page::Party, &data),
                    OnlineLook::Friend => self
                        .friends
                        .show_page(Page::Friend(anyps5_core::online::api::AccountId(2)), &data),
                    OnlineLook::Account | OnlineLook::AccountAppearing => {
                        self.friends.show_page(Page::Account, &data);
                    }
                    OnlineLook::Party | OnlineLook::TogetherParty => {
                        self.friends.show_page(Page::Party, &data);
                    }
                    OnlineLook::TogetherFriend => self
                        .friends
                        .show_page(Page::Friend(anyps5_core::online::api::AccountId(2)), &data),
                    OnlineLook::TogetherNoSession => self
                        .friends
                        .show_page(Page::Friend(anyps5_core::online::api::AccountId(3)), &data),
                    OnlineLook::TogetherFriends => self.friends.show_page(Page::Home, &data),
                    OnlineLook::PartyPicker => {
                        self.friends.show_page(Page::PartyPicker, &data);
                    }
                    OnlineLook::PartyVoice
                    | OnlineLook::ScreenReady
                    | OnlineLook::ScreenHosting
                    | OnlineLook::ScreenOffer
                    | OnlineLook::ScreenCannot
                    | OnlineLook::ScreenWatching
                    | OnlineLook::ScreenStalled => self.friends.show_page(Page::Party, &data),
                    OnlineLook::PartyMember => self.friends.show_page(
                        Page::PartyMember(anyps5_core::online::api::AccountId(2)),
                        &data,
                    ),
                    OnlineLook::Voice | OnlineLook::VoiceDenied => {
                        self.friends.show_page(Page::Voice, &data);
                    }
                    OnlineLook::VoiceKey => {
                        self.friends.show_page(Page::Voice, &data);
                        self.friends.begin_capture();
                    }
                    OnlineLook::VoiceDevices => self.friends.show_page(
                        Page::VoiceDevices(anyps5_core::online::DeviceKind::Input),
                        &data,
                    ),
                    OnlineLook::Messages => self.friends.show_page(Page::Messages, &data),
                    OnlineLook::Message => self
                        .friends
                        .show_page(Page::Message("c-lumen".into(), 6), &data),
                    OnlineLook::Conversation => self
                        .friends
                        .show_page(Page::Conversation("c-lumen".into()), &data),
                    OnlineLook::Request => self
                        .friends
                        .show_page(Page::MessageRequest("c-nyx".into()), &data),
                    OnlineLook::Muted => self.friends.show_page(Page::Home, &data),
                    OnlineLook::Restriction => self
                        .friends
                        .show_page(Page::Restriction("muted".into()), &data),
                    OnlineLook::Paused => self
                        .friends
                        .show_page(Page::Conversation("c-lumen".into()), &data),
                    OnlineLook::Banned => self.friends.show_page(Page::Start, &data),
                    OnlineLook::Feed
                    | OnlineLook::FeedReaction
                    | OnlineLook::FeedEmpty
                    | OnlineLook::FeedLoading
                    | OnlineLook::FeedError => {
                        self.friends.show_page(Page::Feed, &data);
                        match look {
                            OnlineLook::Feed => self.friends.set_feed_focus(1, None),
                            OnlineLook::FeedReaction => self.friends.set_feed_focus(1, Some(1)),
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            ScreenPage::Profile(look) => {
                self.profile
                    .open(anyps5_core::online::api::AccountId(look.account()));
                if look == ProfileLook::OwnEnd {
                    // Past the last row: the focus clamps to it.
                    self.profile.set_focus(99);
                }
                if look == ProfileLook::OwnFocus {
                    // The third game row, after the two buttons.
                    self.profile.set_focus(3);
                }
                if look == ProfileLook::FriendCompare {
                    // The first game row that opens the comparison, after the one button.
                    self.profile.set_focus(1);
                }
            }
            ScreenPage::Compare(look) => {
                self.compare.open();
                match look {
                    // The hidden trophy of the invented set is the seventh row.
                    CompareLook::Secret => self.compare.set_focus(SECRET_TROPHY),
                    CompareLook::Revealed => {
                        self.compare.set_focus(SECRET_TROPHY);
                        self.compare.reveal(SECRET_TROPHY as u32);
                    }
                    CompareLook::Ready => self.compare.set_focus(2),
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
            ScreenPage::TrophiesHidden => self.trophies.open_at(SECRET_TROPHY),
            ScreenPage::TrophiesRevealed => {
                self.trophies.open_at(SECRET_TROPHY);
                self.trophies.reveal(SECRET_TROPHY);
            }
            ScreenPage::Library => self.library.open(1),
            ScreenPage::ControlsEnd => self.controls.open_row(30),
            ScreenPage::Toolkit(look) => {
                self.settings.open_at(5, true);
                // The update page lands on the install row.
                if look == ToolkitLook::Update {
                    self.settings.focus_row(3);
                }
                if look == ToolkitLook::Unused {
                    self.settings.focus_row(7);
                }
            }
            ScreenPage::AboutUpdate(look) => {
                self.settings.open_at(7, true);
                // The row that acts, when there is one; else the first row that can be focused.
                self.settings.focus_row(match look {
                    UpdateLook::Ready
                    | UpdateLook::Holding
                    | UpdateLook::ReadOnly
                    | UpdateLook::Offline
                    | UpdateLook::Refused => 4,
                    UpdateLook::UpToDate => 3,
                    _ => 0,
                });
            }
            ScreenPage::WhatsNewLauncher => {
                self.whats_new
                    .open_launcher(&self.launcher_notes.notes, shell.language);
            }
            ScreenPage::BehaviourLinks => self.settings.open_at(3, true),
            ScreenPage::SettingsCodec(on) => {
                self.prefs.online_openh264 = on;
                self.settings.open_at(6, true);
            }
            ScreenPage::BehaviourClipsOff => {
                self.prefs.clips_seconds = 0;
                self.settings.open_at(3, true);
                self.settings.focus_row(8);
            }
            ScreenPage::BehaviourClips | ScreenPage::BehaviourClipsKey => {
                self.prefs.clips_seconds = 30;
                self.settings.open_at(3, true);
                self.settings
                    .focus_row(if self.page == ScreenPage::BehaviourClipsKey {
                        9
                    } else {
                        8
                    });
            }
            ScreenPage::Phone(look) => {
                self.settings.open_at(4, true);
                // The question lands on its first answer.
                if look == PhoneLook::Pairing {
                    self.settings.focus_row(4);
                }
            }
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
            ScreenPage::Banner | ScreenPage::Unlocking | ScreenPage::Together(_) => {}
        }
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
