//! The app settings (docs/DESIGN.md 5.7): categories on the left, their rows on the right. Values
//! live in the core's [`Preferences`]; the panel edits them in place, applies theme, accent,
//! language, motion and sound to the shell right away and reports `changed` so the host saves.
//!
//! Categories: Appearance, Library, Language, Behaviour, Toolkit (Windows addition: update check
//! and CPU conversion), Advanced, About. File and folder choices that only need a native dialog run
//! here; everything that needs the host (rescan, quit, the toolkit download) is an action.

use std::path::PathBuf;

use anyps5_core::compat::{self, Feature, Fit, Since};
use anyps5_core::fonts::CjkState;
use anyps5_core::linkreg::Handler;
use anyps5_core::links::SettingsPage;
use anyps5_core::openh264::CodecState;
use anyps5_core::settings::{Backdrop, CpuConversion, Preferences, ToolkitChannel};
use anyps5_core::toolkit::InstalledRelease;
use egui::{Align2, Color32, Context, Rect, Sense, Ui, UiBuilder, pos2, vec2};

use crate::app::ludus::LudusState;
use crate::app::phone::PhoneState;
use crate::screens::clips_strings::ClipsText;
use crate::screens::convert;
use crate::screens::dialogs;
use crate::screens::discord_strings::DiscordText;
use crate::screens::format::{abbreviate_path, home_dir};
use crate::screens::ingame::InGameText;
use crate::screens::ludus::{self, LudusAction};
use crate::screens::ludus_web;
use crate::screens::phone::{self, PhoneAction};
use crate::screens::phone_strings::PhoneText;
use crate::screens::rows::{self, Change, ListOptions, Row};
use crate::screens::side::{Output, commands};
use crate::screens::steam_strings::SteamText;
use crate::screens::strings::Extra;
use crate::screens::toolkit_pin::{self, REMOVE_ROWS};
use crate::ui::Shell;
use crate::ui::anim::{AnimExt, Motion, lerp};
use crate::ui::chrome::fullscreen_panel;
use crate::ui::components;
use crate::ui::focus::PanelNav;
use crate::ui::glass::GlassStyle;
use crate::ui::i18n::{Arg, Key, Language};
use crate::ui::icons::Icon;
use crate::ui::input::{self, InputEvent};
use crate::ui::sound::Cue;
use crate::ui::text;
use crate::ui::theme::{Accent, Theme, pad, radius, size, ty, white};

/// The categories, in the order of the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Appearance,
    Library,
    Language,
    Behaviour,
    Phone,
    Toolkit,
    Advanced,
    About,
}

impl Category {
    pub const ALL: [Category; 8] = [
        Category::Appearance,
        Category::Library,
        Category::Language,
        Category::Behaviour,
        Category::Phone,
        Category::Toolkit,
        Category::Advanced,
        Category::About,
    ];

    /// The category a link names.
    pub fn from_link(page: SettingsPage) -> Category {
        match page {
            SettingsPage::Appearance => Category::Appearance,
            SettingsPage::Library => Category::Library,
            SettingsPage::Language => Category::Language,
            SettingsPage::Behavior => Category::Behaviour,
            SettingsPage::Phone => Category::Phone,
            SettingsPage::Toolkit => Category::Toolkit,
            SettingsPage::Advanced => Category::Advanced,
            SettingsPage::About => Category::About,
        }
    }

    pub fn title(self, language: Language) -> &'static str {
        match self {
            Category::Appearance => language.text(Key::Appearance),
            Category::Library => language.text(Key::Storage),
            Category::Language => language.text(Key::Language),
            Category::Behaviour => language.text(Key::Behaviour),
            Category::Phone => PhoneText::Title.text(language),
            Category::Toolkit => Extra::Toolkit.text(language),
            Category::Advanced => language.text(Key::Advanced),
            Category::About => language.text(Key::About),
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Category::Appearance => Icon::PAINT_BRUSH,
            Category::Library => Icon::SQUARES_FOUR,
            Category::Language => Icon::GLOBE,
            Category::Behaviour => Icon::SLIDERS_HORIZONTAL,
            Category::Phone => Icon::DEVICE_MOBILE,
            Category::Toolkit => Icon::HARD_DRIVE,
            Category::Advanced => Icon::WRENCH,
            Category::About => Icon::INFO,
        }
    }
}

/// Where the toolkit update check stands.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum UpdateState {
    /// Never checked (or the check is off).
    #[default]
    Unknown,
    Checking,
    /// The toolkit download runs.
    Downloading,
    UpToDate,
    /// A newer release exists: its version.
    Available(String),
    Failed,
}

/// Where the launcher's own update stands (the About page and the notice on the home screen).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum LauncherUpdate {
    /// Nothing to show: not checked yet, or checking is off.
    #[default]
    None,
    /// A check runs.
    Checking,
    /// The last check found nothing newer.
    UpToDate,
    /// A newer version exists; its download begins: its version.
    Available(String),
    /// The download runs: the version and its share in thousandths.
    Downloading { version: String, permille: u16 },
    /// A newer version is downloaded and verified: its version.
    Ready(String),
    /// The player asked to restart while a game runs: the restart waits for the game's end.
    Holding(String),
    /// A newer version exists but this folder cannot be written to: its version.
    ReadOnly(String),
    /// A newer version exists but this launcher cannot install it itself (the Linux `.tar.gz`):
    /// its version. The player takes it from the releases page.
    Manual(String),
    /// The check or the download failed.
    Failed(UpdateFailure),
}

/// Why the last check or download failed, as far as a player needs to know.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateFailure {
    /// GitHub could not be reached.
    Offline,
    /// The release was refused (it could not be verified).
    Refused,
    /// Anything else.
    Other,
}

impl LauncherUpdate {
    /// A verified update waits (the notice, "Restart Now" and the notes belong to these states).
    pub fn staged_version(&self) -> Option<&str> {
        match self {
            LauncherUpdate::Ready(v) | LauncherUpdate::Holding(v) => Some(v),
            _ => None,
        }
    }
}

/// What the Clips rows show that is not a preference.
#[derive(Clone, Copy, Debug, Default)]
pub struct ClipsEnv {
    /// How many clips the screenshots folder holds.
    pub count: usize,
    /// The clip key row waits for a key press.
    pub capturing: bool,
}

/// What the settings show that is not a preference.
pub struct SettingsEnv<'a> {
    pub games_found: usize,
    pub app_version: &'a str,
    /// The installed toolkit's version, `None` when there is none.
    pub toolkit_version: Option<&'a str>,
    /// The day the installed AnyPlay engine was built (`2026-10-25`), when that channel is in use.
    pub engine_built: Option<&'a str>,
    /// The toolkit needs a newer glibc than this system has (Linux).
    pub glibc: Option<anyps5_core::glibc::Shortfall>,
    pub update: UpdateState,
    /// The last releases (`v0.1.3`), for the pinned channel; empty until fetched.
    pub recent: &'a [String],
    /// Installed releases that neither the toolkit in use nor a game nor the pinned channel needs,
    /// newest first, with their size: the page offers to remove them.
    pub unused: &'a [InstalledRelease],
    pub launcher: LauncherUpdate,
    /// The staged update has notes the player can read (the "What's New" row).
    pub launcher_notes: bool,
    /// The folder screenshots go to when the preference is unset.
    pub default_screenshots: &'a std::path::Path,
    /// Who answers `anyps5://` links on this PC.
    pub links: Handler,
    /// Whether the built-in fonts have the Japanese, Chinese and Korean set.
    pub cjk_fonts: CjkState,
    /// What the phone server is doing.
    pub phone: &'a PhoneState,
    /// What the LudusCtrl receiver is doing.
    pub ludus: &'a LudusState,
    pub clips: ClipsEnv,
    pub location: LocationEnv,
    /// Linux with share screen on: Cisco's OpenH264 and what the share worker made of it. `None`
    /// shows no codec row.
    pub openh264: Option<Openh264Env>,
}

/// What the unlocked games location row shows (docs/DESIGN.md 5.7).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LocationEnv {
    /// The last look at the location, from a worker; `None` before the first answer.
    pub report: Option<anyps5_core::location::Report>,
    /// A move runs: copies done, copies in all, thousandths of the bytes.
    pub moving: Option<MoveEnv>,
    /// A move stopped with copies left: where they are, and how many (0: not known).
    pub left: Option<(PathBuf, usize)>,
}

/// How far a move is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoveEnv {
    pub done: usize,
    pub total: usize,
    pub permille: u16,
}

/// The codec row's facts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Openh264Env {
    /// The download.
    pub state: CodecState,
    /// The share worker's `state` and `detail` for the library, from its last answer.
    pub worker: Option<(String, Option<String>)>,
}

/// What the host has to do after a frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingsAction {
    Close,
    /// A new games folder was chosen (already stored in the preferences): rescan and watch it.
    GamesFolderChosen(PathBuf),
    /// A folder was picked for the unlocked games (not stored yet: the host checks it).
    UnlockedFolderPicked(PathBuf),
    /// "Use Default Location".
    UseDefaultLocation,
    /// "Cancel Move".
    CancelMove,
    /// "Continue Moving".
    ContinueMove,
    /// "Leave Games Where They Are".
    LeaveOldCopies,
    Rescan,
    ShowScreenshots,
    /// "Show Clips": the folder the clips are in.
    ShowClips,
    /// "Check now" (or the check was switched on).
    CheckForUpdates,
    /// Download and install the latest toolkit (first install or update).
    InstallToolkit,
    /// A toolkit folder was chosen: adopt it.
    AdoptToolkitFolder(PathBuf),
    /// The channel is stable again (already stored in the preferences): use the newest installed.
    UseStableToolkit,
    /// Use the AnyPlay engine (already stored in the preferences): install it if needed.
    UseEngineToolkit,
    /// Stay on this release (`v0.1.3`, already stored in the preferences): install it if needed.
    PinToolkit(String),
    /// Download the Japanese, Chinese and Korean fonts of the built-in set.
    DownloadCjkFonts,
    /// The OpenH264 row was turned on (download it) or off (remove it); the preference is
    /// already stored.
    Openh264(bool),
    /// Open the notes of this release (`0.1.3`).
    OpenWhatsNew(String),
    /// Delete this installed release (`0.1.2`); nothing uses it.
    RemoveToolkit(String),
    /// "Restart Now" on the About page: close, and start the staged launcher.
    RestartNow,
    /// "Cancel Restart" while the restart waits for a game.
    CancelRestart,
    /// "Check Now" on the launcher's own update row.
    CheckLauncherUpdate,
    /// Open the notes of the staged launcher update.
    OpenLauncherNotes,
    /// One of the About page's links (website, Discord, privacy, terms).
    OpenPage(&'static str),
    /// "Open AnyPlay Links with This Launcher" was switched: write or remove the registration.
    SetLinks(bool),
    /// "Add to Steam": put the launcher into the Steam library.
    AddToSteam,
    /// "Add My Games to Steam": one entry for each game.
    AddGamesToSteam,
    /// A row of the Phone as Controller page asks for something.
    Phone(PhoneAction),
    /// A LudusCtrl row of the same page asks for something.
    Ludus(LudusAction),
    Quit,
}

/// One category with its rows.
pub struct CategoryRows {
    pub category: Category,
    pub rows: Vec<Row>,
}

const IMAGE_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "bmp", "webp", "gif"];

fn backdrop_options(prefs: &Preferences, language: Language) -> Vec<String> {
    let t = |key: Key| language.text(key).to_owned();
    let mut options = vec![t(Key::BackgroundTheme), t(Key::BackgroundImage)];
    // A video background cannot be shown yet; a stored choice stays visible instead of vanishing.
    if prefs.backdrop == Backdrop::Video {
        options.push(t(Key::BackgroundVideo));
    }
    options
}

fn path_text(path: &Option<PathBuf>) -> String {
    path.as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "—".to_owned())
}

/// The rows of the unlocked games location: the place and its free space, a note that says what
/// that means, and the commands that fit the state (use the default, cancel or continue a move).
fn location_rows(prefs: &Preferences, location: &LocationEnv, language: Language) -> Vec<Row> {
    use crate::screens::location_strings as words;
    use anyps5_core::location::Availability;
    let custom = prefs.unlocked_folder.as_deref();
    let report = location.report.as_ref();
    let missing = report.is_some_and(|r| r.availability == Availability::Missing);
    let free = report.and_then(|r| r.free);
    let title = words::title(language);
    let mut rows = Vec::new();
    if let Some(moving) = location.moving {
        // A move in progress: the row is a status, nothing to pick.
        let percent = u32::from(moving.permille) / 10;
        rows.push(
            Row::info(
                "unlockedLocation",
                title,
                Some(words::moving(moving.done, moving.total, percent, language)),
            )
            .icon(Icon::HARD_DRIVE),
        );
        rows.push(Row::info("unlockedNote", words::note_moving(language), None).attached());
        rows.push(Row::command("moveCancel", words::cancel_move(language)).icon(Icon::X_CIRCLE));
        return rows;
    }
    rows.push(
        Row::action("unlockedLocation", title)
            .detail(words::detail(custom, free, missing, language))
            .icon(Icon::HARD_DRIVE),
    );
    if let Some((from, count)) = &location.left {
        rows.push(
            Row::info(
                "unlockedNote",
                words::note_left(*count, from, language),
                None,
            )
            .attached()
            .warning(),
        );
        rows.push(Row::command("moveContinue", words::continue_move(language)).icon(Icon::PLAY));
        rows.push(
            Row::command("moveLeave", words::leave(language))
                .detail(words::leave_detail(language))
                .icon(Icon::PROHIBIT),
        );
        return rows;
    }
    let note = if missing {
        Row::info("unlockedNote", words::note_missing(language), None).warning()
    } else if report.is_some_and(|r| r.network) {
        Row::info("unlockedNote", words::note_network(language), None).warning()
    } else if custom.is_some() {
        Row::info("unlockedNote", words::note_custom(language), None)
    } else {
        Row::info("unlockedNote", words::note_default(language), None)
    };
    rows.push(note.attached());
    if custom.is_some() {
        rows.push(
            Row::command("unlockedDefault", words::use_default(language))
                .icon(Icon::ARROW_COUNTER_CLOCKWISE),
        );
    }
    rows
}

/// The releases the pinned channel offers: the last ones, plus the pinned one and the installed
/// one when they are older than that.
fn pinned_options(prefs: &Preferences, env: &SettingsEnv<'_>) -> Vec<String> {
    let mut tags: Vec<String> = env.recent.to_vec();
    let installed = env.toolkit_version.and_then(compat::release_tag);
    for extra in [
        prefs.toolkit_pinned.clone(),
        installed.map(|t| format!("v{t}")),
    ]
    .into_iter()
    .flatten()
    {
        if !tags.contains(&extra) {
            tags.push(extra);
        }
    }
    tags
}

/// The release "What's New" is about: the one waiting to be installed, else the installed one.
/// `None` for a local build or no toolkit.
fn whats_new_tag(env: &SettingsEnv<'_>) -> Option<String> {
    match &env.update {
        UpdateState::Available(tag) => Some(tag.trim_start_matches('v').to_owned()),
        _ => env.toolkit_version.and_then(compat::release_tag),
    }
}

/// What the fonts row says a game gets: the player's folder, else the built-in set and whether
/// Japanese, Chinese and Korean are part of it.
/// What the OpenH264 row says under its title: off, the download, or what the share worker made of
/// the file.
fn openh264_detail(on: bool, codec: &Openh264Env, language: Language) -> String {
    use crate::screens::online_strings::OnlineText;
    if !on {
        return OnlineText::Openh264Off.text(language).to_owned();
    }
    match &codec.state {
        CodecState::Downloading => OnlineText::Openh264Downloading.text(language).to_owned(),
        CodecState::Missing => OnlineText::Openh264Missing.text(language).to_owned(),
        CodecState::Failed(why) => OnlineText::Openh264Failed.format(language, &[Arg::Str(why)]),
        CodecState::Ready => match &codec.worker {
            Some((state, _)) if state == "refused" => {
                OnlineText::Openh264Refused.text(language).to_owned()
            }
            Some((state, detail)) if state == "failed" => OnlineText::Openh264NotLoaded
                .format(language, &[Arg::Str(detail.as_deref().unwrap_or_default())]),
            _ => OnlineText::Openh264Ready.text(language).to_owned(),
        },
    }
}

fn fonts_detail(picked: Option<String>, cjk: CjkState, language: Language) -> String {
    if let Some(folder) = picked {
        return folder;
    }
    let cjk = match cjk {
        CjkState::Present => Extra::FontsCjkReady,
        CjkState::Missing => Extra::FontsCjkMissing,
        CjkState::Downloading => Extra::FontsCjkDownloading,
    };
    format!(
        "{} · {}",
        Extra::FontsBuiltIn.text(language),
        cjk.text(language)
    )
}

/// All categories and rows for these preferences.
pub fn build_categories(
    prefs: &Preferences,
    env: &SettingsEnv<'_>,
    language: Language,
) -> Vec<CategoryRows> {
    let t = |key: Key| language.text(key).to_owned();
    let home = home_dir();
    let abbreviated = |p: &std::path::Path| abbreviate_path(p, home.as_deref());

    // Appearance
    let accent = convert::effective_accent(prefs);
    let mut appearance = vec![
        Row::choice(
            "theme",
            t(Key::Theme),
            Theme::ALL
                .iter()
                .map(|x| x.title(language).to_owned())
                .collect(),
            Theme::ALL
                .iter()
                .position(|x| *x == convert::theme(prefs.theme))
                .unwrap_or(0),
        )
        .icon(Icon::PALETTE),
        Row::choice(
            "accent",
            t(Key::Accent),
            Accent::ALL
                .iter()
                .map(|x| x.title(language).to_owned())
                .collect(),
            Accent::ALL.iter().position(|x| *x == accent).unwrap_or(0),
        )
        .icon(Icon::CIRCLE_HALF),
        Row::choice(
            "backdrop",
            t(Key::Background),
            backdrop_options(prefs, language),
            match prefs.backdrop {
                Backdrop::Theme => 0,
                Backdrop::Image => 1,
                Backdrop::Video => 2,
            },
        )
        .icon(Icon::IMAGES),
    ];
    if prefs.backdrop != Backdrop::Theme {
        let mut row = Row::action("file", t(Key::ChooseFile)).icon(Icon::FOLDER);
        if let Some(name) = prefs
            .backdrop_file
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
        {
            row = row.detail(name);
        }
        appearance.push(row);
    }
    appearance
        .push(Row::toggle("artwork", t(Key::GameArtwork), prefs.game_artwork).icon(Icon::IMAGE));
    appearance.push(
        Row::choice(
            "motion",
            t(Key::Motion),
            vec![t(Key::MotionFull), t(Key::MotionReduced)],
            usize::from(prefs.reduced_motion),
        )
        .icon(Icon::WIND),
    );

    // Library
    let mut library = vec![
        Row::action("folder", t(Key::Folder))
            .detail(path_text(&prefs.games_folder))
            .icon(Icon::FOLDER),
        Row::command("rescan", t(Key::Rescan))
            .detail(language.format(Key::GamesFound, &[Arg::Int(env.games_found as i64)]))
            .icon(Icon::ARROW_CLOCKWISE),
    ];
    library.extend(location_rows(prefs, &env.location, language));

    // Language
    let languages = vec![
        Row::choice(
            "language",
            t(Key::Language),
            Language::ALL
                .iter()
                .map(|l| l.display_name().to_owned())
                .collect(),
            Language::ALL
                .iter()
                .position(|l| *l == convert::language(prefs.language))
                .unwrap_or(0),
        )
        .icon(Icon::GLOBE),
    ];

    // Behaviour
    let mut behaviour = vec![
        Row::text("name", t(Key::ProfileName), prefs.profile_name.clone()).icon(Icon::USER_CIRCLE),
        Row::toggle("sounds", t(Key::Sounds), prefs.sounds).icon(Icon::SPEAKER_HIGH),
        Row::toggle("music", Extra::HomeMusic.text(language), prefs.music)
            .detail(Extra::HomeMusicDetail.text(language))
            .icon(Icon::MUSIC_NOTES),
        Row::toggle(
            "gameMusic",
            Extra::GameMusic.text(language),
            prefs.game_music,
        )
        .detail(Extra::GameMusicDetail.text(language))
        .icon(Icon::GAME_CONTROLLER),
        Row::toggle(
            "discord",
            Extra::DiscordPresence.text(language),
            prefs.discord_presence,
        )
        .detail(Extra::DiscordPresenceDetail.text(language))
        .icon(Icon::DISCORD_LOGO),
    ];
    // The button belongs to the status: without the status there is nothing to put it on.
    if prefs.discord_presence {
        behaviour.push(
            Row::toggle(
                "discordJoin",
                DiscordText::Button.text(language),
                prefs.discord_join_button,
            )
            .detail(DiscordText::ButtonDetail.text(language))
            .icon(Icon::USER_PLUS),
        );
    }
    behaviour.push(
        Row::toggle(
            "prepare",
            crate::screens::prepare_strings::setting(language),
            prefs.prepare_games,
        )
        .detail(crate::screens::prepare_strings::setting_detail(language))
        .icon(Icon::ARROW_CIRCLE_DOWN),
    );
    behaviour.push(
        Row::toggle("hide", t(Key::HideWhilePlaying), prefs.hide_while_playing)
            .icon(Icon::EYE_SLASH),
    );
    behaviour.push(
        Row::toggle(
            "homeButton",
            InGameText::Setting.text(language),
            prefs.ps_button,
        )
        .detail(InGameText::SettingDetail.text(language))
        .icon(Icon::GAME_CONTROLLER),
    );
    behaviour.push(
        Row::choice(
            "vibration",
            Extra::Vibration.text(language),
            vec![
                t(Key::Off),
                Extra::VibrationLight.text(language).to_owned(),
                Extra::VibrationMedium.text(language).to_owned(),
                Extra::VibrationStrong.text(language).to_owned(),
            ],
            convert::vibration_index(prefs.vibration),
        )
        .detail(Extra::VibrationDetail.text(language))
        .icon(Icon::WAVE_SINE),
    );
    // Where a player looks when the launcher does not react to the pad.
    behaviour
        .push(Row::info("controllerHelp", Extra::ControllerHelp.text(language), None).attached());
    behaviour.push(
        Row::toggle(
            "hiddenTrophies",
            Extra::ShowHiddenTrophies.text(language),
            prefs.show_hidden_trophies,
        )
        .detail(Extra::ShowHiddenTrophiesDetail.text(language))
        .icon(Icon::TROPHY),
    );
    // The recorder is the share worker, which captures windows on Windows only.
    if cfg!(windows) {
        push_clips_rows(&mut behaviour, prefs, env, language);
    }
    // The registration lives in the registry, so only Windows has the row.
    if cfg!(windows) {
        let detail = match env.links {
            Handler::Other(_) => Extra::LinksDetailOther,
            _ => Extra::LinksDetail,
        };
        behaviour.push(
            Row::toggle(
                "links",
                Extra::Links.text(language),
                env.links == Handler::ThisExe,
            )
            .detail(detail.text(language))
            .icon(Icon::LINK),
        );
    }
    // Steam keeps its shortcuts in files of the user folder; the rows write them (DESIGN.md 9.9).
    if cfg!(any(windows, target_os = "linux")) {
        behaviour.push(
            Row::command("addToSteam", SteamText::Add.text(language))
                .detail(SteamText::AddDetail.text(language))
                .icon(Icon::STEAM_LOGO),
        );
        if env.games_found > 0 {
            behaviour.push(
                Row::command("addGamesToSteam", SteamText::AddGames.text(language))
                    .detail(SteamText::AddGamesDetail.text(language))
                    .icon(Icon::STEAM_LOGO),
            );
        }
    }

    // Phone as Controller
    let phone_rows = phone::page(prefs, env.phone, env.ludus, language);

    // Toolkit
    let channel = prefs.toolkit_channel();
    let toolkit_value = env.toolkit_version.map_or_else(
        || Extra::ToolkitMissing.text(language).to_owned(),
        str::to_owned,
    );
    // The header row is one line, always: a note under it would centre the title higher. The note
    // about a toolkit this launcher was not tested with hangs under the row as its own.
    let mut toolkit = vec![Row::info(
        "toolkit",
        t(Key::ToolkitVersion),
        Some(toolkit_value),
    )];
    match env.toolkit_version.map(compat::fit) {
        Some(Fit::Newer) => {
            toolkit.push(Row::info("toolkitFit", Extra::FitNewer.text(language), None).attached());
        }
        Some(Fit::Older) => {
            toolkit.push(Row::info("toolkitFit", Extra::FitOlder.text(language), None).attached());
        }
        _ => {}
    }
    if let (ToolkitChannel::Engine, Some(day)) = (channel, env.engine_built) {
        toolkit.push(
            Row::info(
                "engineBuilt",
                Extra::EngineBuilt.format(language, &[Arg::Str(day)]),
                None,
            )
            .attached(),
        );
    }
    if let Some(short) = env.glibc {
        toolkit.push(
            Row::info(
                "toolkitGlibc",
                crate::screens::glibc_strings::shortfall_text(&short, language),
                None,
            )
            .attached()
            // The amber ink of the warning line under Play on the home screen.
            .warning(),
        );
        toolkit.push(
            Row::command(
                "toolkitGlibcGuide",
                crate::screens::glibc_strings::guide_title(language),
            )
            .icon(Icon::FILE_TEXT),
        );
    }
    toolkit.push(
        Row::choice(
            "channel",
            Extra::ChannelTitle.text(language),
            vec![
                Extra::ChannelStable.text(language).to_owned(),
                Extra::ChannelEngine.text(language).to_owned(),
                Extra::ChannelPinned.text(language).to_owned(),
                Extra::ChannelLocal.text(language).to_owned(),
            ],
            match channel {
                ToolkitChannel::Stable => 0,
                ToolkitChannel::Engine => 1,
                ToolkitChannel::Pinned => 2,
                ToolkitChannel::Local => 3,
            },
        )
        .detail(
            match channel {
                ToolkitChannel::Stable => Extra::ChannelStableDetail,
                ToolkitChannel::Pinned => Extra::ChannelPinnedDetail,
                ToolkitChannel::Local => Extra::ChannelLocalDetail,
                ToolkitChannel::Engine => Extra::ChannelEngineDetail,
            }
            .text(language),
        )
        .icon(Icon::STACK),
    );
    match channel {
        ToolkitChannel::Pinned => {
            let tags = pinned_options(prefs, env);
            let selected = prefs
                .toolkit_pinned
                .as_ref()
                .and_then(|p| tags.iter().position(|t| t == p))
                .unwrap_or(0);
            toolkit.push(
                Row::choice(
                    "pinned",
                    Extra::PinnedVersion.text(language),
                    tags,
                    selected,
                )
                .icon(Icon::TAG),
            );
        }
        ToolkitChannel::Local => toolkit.push(
            Row::action("toolkitFolder", Extra::ToolkitChooseFolder.text(language))
                .icon(Icon::FOLDER),
        ),
        ToolkitChannel::Stable | ToolkitChannel::Engine => {}
    }
    // The notes of a nightly build are commit messages, not a release; the engine has none to read.
    let notes_tag = whats_new_tag(env).filter(|_| channel != ToolkitChannel::Engine);
    if let Some(tag) = notes_tag {
        let detail = match &env.update {
            UpdateState::Available(_) if channel == ToolkitChannel::Stable => {
                Extra::UpdateAvailable.format(language, &[Arg::Str(&format!("v{tag}"))])
            }
            _ => format!("v{tag}"),
        };
        toolkit.push(
            Row::action("whatsNew", Extra::WhatsNew.text(language))
                .detail(detail)
                .icon(Icon::FILE_TEXT),
        );
    }
    let missing = env.toolkit_version.is_none() && channel != ToolkitChannel::Local;
    let follows_updates = matches!(channel, ToolkitChannel::Stable | ToolkitChannel::Engine);
    let update_ready = follows_updates && matches!(env.update, UpdateState::Available(_));
    if missing || update_ready {
        let text = if missing {
            Extra::ToolkitInstall
        } else {
            Extra::ToolkitInstallUpdate
        };
        toolkit.push(Row::command("install", text.text(language)).icon(Icon::DOWNLOAD_SIMPLE));
    }
    if follows_updates {
        let status = match &env.update {
            UpdateState::Unknown => None,
            UpdateState::Checking => Some(Extra::Checking.text(language).to_owned()),
            UpdateState::Downloading => Some(Extra::ToolkitDownloading.text(language).to_owned()),
            UpdateState::UpToDate => Some(Extra::UpToDate.text(language).to_owned()),
            // The row above already names the update; this one says when it looked.
            UpdateState::Available(_) => None,
            UpdateState::Failed => Some(Extra::CheckFailed.text(language).to_owned()),
        };
        let last_checked = prefs.toolkit_checked_at.as_deref().map_or_else(
            || Extra::NeverChecked.text(language).to_owned(),
            |stamp| Extra::LastCheckedOn.format(language, &[Arg::Str(stamp)]),
        );
        toolkit.push(
            Row::toggle(
                "updateCheck",
                Extra::UpdateCheck.text(language),
                prefs.check_for_updates,
            )
            .detail(Extra::UpdateCheckDetail.text(language))
            .icon(Icon::ARROWS_CLOCKWISE)
            .group_start(),
        );
        // The command is its title alone; what the last check found hangs under it as a note.
        toolkit.push(
            Row::command("checkNow", Extra::CheckNow.text(language)).icon(Icon::ARROW_CLOCKWISE),
        );
        toolkit.push(Row::info("checkStatus", status.unwrap_or(last_checked), None).attached());
    }
    toolkit.push(
        Row::choice(
            "cpu",
            Extra::CpuConversion.text(language),
            vec![
                Extra::CpuAuto.text(language).to_owned(),
                Extra::CpuNone.text(language).to_owned(),
                Extra::CpuIntel.text(language).to_owned(),
            ],
            match prefs.cpu_conversion {
                CpuConversion::Auto => 0,
                CpuConversion::None => 1,
                CpuConversion::Intel => 2,
            },
        )
        .detail(Extra::CpuConversionDetail.text(language))
        .icon(Icon::GAUGE)
        .group_start(),
    );
    for (index, release) in env.unused.iter().take(REMOVE_ROWS.len()).enumerate() {
        toolkit.push(toolkit_pin::remove_row(
            index,
            &release.tag,
            release.bytes,
            language,
        ));
    }

    // Advanced
    let screenshots = prefs
        .screenshots_folder
        .as_deref()
        .unwrap_or(env.default_screenshots);
    let mut advanced = Vec::new();
    // F12 screenshots need a build newer than v0.1.3; an older toolkit would ignore the folder.
    match env.toolkit_version {
        Some(version) if !compat::supports(Feature::Screenshots, version) => {
            let needs = match Feature::Screenshots.since() {
                Since::Release(tag) => {
                    Extra::NeedsToolkit.format(language, &[Arg::Str(&format!("v{tag}"))])
                }
                Since::After(tag) => {
                    Extra::NeedsNewerThan.format(language, &[Arg::Str(&format!("v{tag}"))])
                }
            };
            advanced.push(
                Row::info("screenshots", t(Key::ScreenshotsFolder), Some(needs)).icon(Icon::CAMERA),
            );
        }
        _ => {
            advanced.push(
                Row::action("screenshots", t(Key::ScreenshotsFolder))
                    .detail(abbreviated(screenshots))
                    .icon(Icon::CAMERA),
            );
            advanced
                .push(Row::command("showScreenshots", t(Key::ShowScreenshots)).icon(Icon::IMAGES));
        }
    }
    advanced.push(
        Row::action("fonts", t(Key::FontsFolder))
            .detail(fonts_detail(
                prefs.fonts_folder.as_deref().map(abbreviated),
                env.cjk_fonts,
                language,
            ))
            .icon(Icon::TEXT_AA),
    );
    if prefs.fonts_folder.is_some() {
        advanced.push(Row::command("fontsReset", t(Key::FontsReset)).icon(Icon::X_CIRCLE));
    } else if env.cjk_fonts == CjkState::Missing {
        advanced.push(
            Row::command("fontsCjk", Extra::FontsDownloadCjk.text(language))
                .icon(Icon::DOWNLOAD_SIMPLE),
        );
    }
    advanced.push(Row::info("fontsHint", t(Key::FontsHint), None));
    {
        use crate::screens::online_strings::OnlineText;
        advanced.push(
            Row::toggle(
                "recordNetwork",
                OnlineText::RecordNetwork.text(language),
                prefs.record_network,
            )
            .detail(OnlineText::RecordNetworkDetail.text(language))
            .icon(Icon::FILE_TEXT)
            .group_start(),
        );
        if let Some(codec) = &env.openh264 {
            advanced.push(
                Row::toggle(
                    "openh264",
                    OnlineText::Openh264.text(language),
                    prefs.online_openh264,
                )
                .detail(openh264_detail(prefs.online_openh264, codec, language))
                .icon(Icon::APP_WINDOW)
                .group_start(),
            );
            // Cisco's licence: this text where the player turns the codec on and off.
            advanced.push(
                Row::info(
                    "openh264Notice",
                    OnlineText::Openh264Notice.text(language),
                    None,
                )
                .attached(),
            );
        }
    }

    // About
    let mut about = vec![
        Row::info("text", t(Key::AboutText), None),
        // The toolkit version lives on the Toolkit page only.
        Row::info("version", t(Key::Version), Some(env.app_version.to_owned())),
    ];
    about.extend(launcher_update_rows(
        &env.launcher,
        env.launcher_notes,
        language,
    ));
    {
        use crate::screens::about_strings::AboutText as A;
        about.push(
            Row::command("aboutWebsite", A::Website.text(language))
                .detail("anyplaynet.com")
                .icon(Icon::GLOBE)
                .group_start(),
        );
        about.push(
            Row::command("aboutDiscord", A::Discord.text(language))
                .detail(A::DiscordDetail.text(language))
                .icon(Icon::DISCORD_LOGO),
        );
        about.push(Row::command("aboutPrivacy", A::Privacy.text(language)).icon(Icon::FILE_TEXT));
        about.push(Row::command("aboutTerms", A::Terms.text(language)).icon(Icon::FILE_TEXT));
        about.push(Row::info("credits", A::Credits.text(language), None));
        about.push(Row::info("creditsLudus", A::LudusCredits.text(language), None).attached());
    }
    about.push(
        Row::command("quit", t(Key::Quit))
            .icon(Icon::POWER)
            .destructive(),
    );

    [
        appearance, library, languages, behaviour, phone_rows, toolkit, advanced, about,
    ]
    .into_iter()
    .zip(Category::ALL)
    .map(|(rows, category)| CategoryRows { category, rows })
    .collect()
}

/// The lengths of Clips as the choice row shows them, Off first.
fn clips_options(language: Language) -> Vec<String> {
    [
        ClipsText::Off,
        ClipsText::Seconds15,
        ClipsText::Seconds30,
        ClipsText::Minute1,
        ClipsText::Minutes2,
    ]
    .iter()
    .map(|t| t.text(language).to_owned())
    .collect()
}

/// Clips: the length, then (when on) the key, then the folder. Off, the folder row stays only
/// while there are clips to find.
fn push_clips_rows(
    rows: &mut Vec<Row>,
    prefs: &Preferences,
    env: &SettingsEnv<'_>,
    language: Language,
) {
    let seconds = anyps5_core::clips::length_or_off(prefs.clips_seconds);
    let selected = anyps5_core::clips::LENGTHS
        .iter()
        .position(|&l| l == seconds)
        .unwrap_or(0);
    rows.push(
        Row::choice(
            "clips",
            ClipsText::Title.text(language),
            clips_options(language),
            selected,
        )
        .detail(ClipsText::Detail.text(language))
        .icon(Icon::CAMERA)
        .group_start(),
    );
    if seconds > 0 {
        let hotkey = anyps5_core::clips::hotkey_or_default(prefs.clips_hotkey.as_deref());
        let sources = hotkey.parts().iter().map(|p| format!("KEY:{p}")).collect();
        // A detail line both ways, so that the rows below do not move when the row starts waiting.
        let detail = if env.clips.capturing {
            ClipsText::KeyPrompt
        } else {
            ClipsText::KeyDetail
        };
        let key = Row::keys(
            "clipsKey",
            ClipsText::Key.text(language),
            sources,
            env.clips.capturing,
        )
        .icon(Icon::KEYBOARD)
        .detail(detail.text(language));
        rows.push(key);
    }
    if seconds > 0 || env.clips.count > 0 {
        let count = match env.clips.count {
            0 => ClipsText::CountNone.text(language).to_owned(),
            1 => ClipsText::CountOne.text(language).to_owned(),
            n => ClipsText::CountMany.format(language, &[Arg::Int(n as i64)]),
        };
        rows.push(
            Row::command("showClips", ClipsText::Show.text(language))
                .detail(count)
                .icon(Icon::FOLDER_OPEN),
        );
    }
}

/// The rows under *Version* that tell where the launcher's own update stands. Nothing while
/// there is nothing to say. The status row is short; what it means follows as a note, and the
/// one thing to do next as a row.
fn launcher_update_rows(
    launcher: &LauncherUpdate,
    has_notes: bool,
    language: Language,
) -> Vec<Row> {
    use crate::screens::update_strings::UpdateText as U;
    let status = |value: String| Row::info("launcherUpdate", U::Row.text(language), Some(value));
    let note = |text: String| Row::info("launcherUpdateNote", text, None).attached();
    // What the note says is about the status above it; the row under it is the next thing to
    // do, a step apart, so the note does not lean on it.
    let check = || {
        Row::command("launcherCheck", Extra::CheckNow.text(language))
            .icon(Icon::ARROW_CLOCKWISE)
            .group_start()
    };
    let download = || {
        Row::command("launcherDownload", U::Download.text(language))
            .icon(Icon::DOWNLOAD_SIMPLE)
            .group_start()
    };
    match launcher {
        LauncherUpdate::None => Vec::new(),
        LauncherUpdate::Checking => vec![status(U::Checking.text(language).to_owned())],
        LauncherUpdate::UpToDate => vec![status(U::UpToDate.text(language).to_owned()), check()],
        LauncherUpdate::Available(v) => vec![
            status(U::Available.format(language, &version(v))),
            note(U::NoteDownloading.format(language, &version(v))),
        ],
        LauncherUpdate::Downloading {
            version: v,
            permille,
        } => {
            // The last tenth of a percent is the check of the file.
            let value = if *permille >= 1000 {
                U::Verifying.text(language).to_owned()
            } else {
                let percent = format!("{}%", permille / 10);
                U::Downloading.format(language, &[Arg::Str(&percent)])
            };
            vec![
                status(value),
                note(U::NoteDownloading.format(language, &version(v))),
            ]
        }
        LauncherUpdate::Ready(v) => {
            let mut rows = vec![
                status(U::Ready.format(language, &version(v))),
                note(U::NoteReady.text(language).to_owned()),
                Row::command("restartNow", Extra::RestartNow.text(language))
                    .icon(Icon::ARROW_CLOCKWISE)
                    .group_start(),
            ];
            if has_notes {
                rows.push(whats_new_row(v, language));
            }
            rows
        }
        LauncherUpdate::Holding(v) => {
            let mut rows = vec![
                status(U::WaitingForGame.text(language).to_owned()),
                note(U::NoteHolding.format(language, &version(v))),
                Row::command("cancelRestart", U::CancelRestart.text(language))
                    .icon(Icon::X)
                    .group_start(),
            ];
            if has_notes {
                rows.push(whats_new_row(v, language));
            }
            rows
        }
        LauncherUpdate::ReadOnly(v) => vec![
            status(U::Available.format(language, &version(v))),
            note(U::NoteReadOnly.text(language).to_owned()),
            download(),
        ],
        LauncherUpdate::Manual(v) => vec![
            status(U::Available.format(language, &version(v))),
            note(U::NoteManual.text(language).to_owned()),
            download(),
        ],
        LauncherUpdate::Failed(UpdateFailure::Offline) => vec![
            status(U::NotReachable.text(language).to_owned()),
            note(U::NoteOffline.text(language).to_owned()),
            check(),
        ],
        LauncherUpdate::Failed(failure) => vec![
            status(U::NotInstalled.text(language).to_owned()),
            note(
                match failure {
                    UpdateFailure::Refused => U::NoteRefused,
                    _ => U::NoteOther,
                }
                .text(language)
                .to_owned(),
            ),
            download(),
        ],
    }
}

fn version(v: &str) -> [Arg<'_>; 1] {
    [Arg::Str(v)]
}

fn whats_new_row(version: &str, language: Language) -> Row {
    use crate::screens::update_strings::UpdateText as U;
    Row::command(
        "launcherNotes",
        U::WhatsNewIn.format(language, &[Arg::Str(version)]),
    )
    .icon(Icon::SPARKLE)
}
/// The page an About row opens.
fn about_page(row: &str) -> Option<&'static str> {
    use crate::screens::about_strings::{DISCORD, INSTALL_GUIDE, PRIVACY, TERMS, WEBSITE};
    Some(match row {
        "aboutWebsite" => WEBSITE,
        "aboutDiscord" => DISCORD,
        "aboutPrivacy" => PRIVACY,
        "aboutTerms" => TERMS,
        "toolkitGlibcGuide" => INSTALL_GUIDE,
        _ => return None,
    })
}

/// Applies the preferences that the shell shows: theme and accent, language, motion, sounds. The
/// host calls it once after loading the preferences; the panel calls it after every change.
pub fn apply_preferences(shell: &mut Shell, ctx: &Context, prefs: &Preferences) {
    shell.set_theme(
        convert::theme(prefs.theme),
        prefs.accent.map(convert::accent),
    );
    shell.language = convert::language(prefs.language);
    shell.set_reduced_motion(ctx, prefs.reduced_motion);
    shell.sounds.enabled = prefs.sounds;
    shell.set_vibration(convert::vibration(prefs.vibration));
}

/// Writes one change into the preferences. Dialogs (a picture, a folder) are the caller's
/// business: this covers the plain values. Returns whether anything changed.
pub fn apply_value(prefs: &mut Preferences, change: &Change) -> bool {
    let before = prefs.clone();
    match change {
        Change::Choice {
            row: "theme",
            index,
        } => {
            if let Some(theme) = Theme::ALL.get(*index) {
                prefs.theme = convert::theme_to_core(*theme);
            }
        }
        Change::Choice {
            row: "accent",
            index,
        } => {
            if let Some(accent) = Accent::ALL.get(*index) {
                prefs.accent = Some(convert::accent_to_core(*accent));
            }
        }
        Change::Toggle { row: "artwork", on } => prefs.game_artwork = *on,
        Change::Choice {
            row: "motion",
            index,
        } => prefs.reduced_motion = *index == 1,
        Change::Choice {
            row: "language",
            index,
        } => {
            if let Some(language) = Language::ALL.get(*index) {
                prefs.language = convert::language_to_core(*language);
            }
        }
        Change::Toggle { row: "sounds", on } => prefs.sounds = *on,
        Change::Choice {
            row: "vibration",
            index,
        } => prefs.vibration = convert::vibration_to_core(*index),
        Change::Toggle { row: "music", on } => prefs.music = *on,
        Change::Toggle {
            row: "gameMusic",
            on,
        } => prefs.game_music = *on,
        Change::Toggle { row: "discord", on } => prefs.discord_presence = *on,
        Change::Toggle {
            row: "discordJoin",
            on,
        } => prefs.discord_join_button = *on,
        Change::Toggle { row: "hide", on } => prefs.hide_while_playing = *on,
        Change::Toggle { row: "prepare", on } => prefs.prepare_games = *on,
        Change::Toggle {
            row: "homeButton",
            on,
        } => prefs.ps_button = *on,
        Change::Toggle {
            row: "hiddenTrophies",
            on,
        } => prefs.show_hidden_trophies = *on,
        Change::Toggle {
            row: "recordNetwork",
            on,
        } => prefs.record_network = *on,
        Change::Toggle {
            row: "openh264",
            on,
        } => prefs.online_openh264 = *on,
        Change::Toggle {
            row: "updateCheck",
            on,
        } => prefs.check_for_updates = *on,
        Change::Choice { row: "cpu", index } => {
            prefs.cpu_conversion = match index {
                1 => CpuConversion::None,
                2 => CpuConversion::Intel,
                _ => CpuConversion::Auto,
            };
        }
        Change::Choice {
            row: "backdrop",
            index: 0,
        } => prefs.backdrop = Backdrop::Theme,
        Change::Run { row: "fontsReset" } => prefs.fonts_folder = None,
        Change::Toggle { row: "phone", on } => prefs.phone_remote = *on,
        Change::Toggle {
            row: "ludusctrl",
            on,
        } => prefs.phone_ludusctrl = *on,
        Change::Choice {
            row: "clips",
            index,
        } => {
            if let Some(seconds) = anyps5_core::clips::LENGTHS.get(*index) {
                prefs.clips_seconds = *seconds;
            }
        }
        _ => {}
    }
    *prefs != before
}

/// The settings screen.
pub struct SettingsPanel {
    open: bool,
    closing: bool,
    pub nav: PanelNav,
    /// Counts category changes: each one fades the new list in.
    switches: u32,
    last_category: usize,
    /// The clip key row waits for a key press.
    clip_capture: bool,
}

impl Default for SettingsPanel {
    fn default() -> Self {
        Self {
            open: false,
            closing: false,
            nav: PanelNav::settings(Category::ALL.len()),
            switches: 0,
            last_category: 0,
            clip_capture: false,
        }
    }
}

impl SettingsPanel {
    /// Opens at the category level on the first category.
    pub fn open(&mut self) {
        self.open_at(0, false);
    }

    /// Opens on a category, optionally already in its rows (previews).
    pub fn open_at(&mut self, category: usize, in_rows: bool) {
        self.open = true;
        self.closing = false;
        self.nav = PanelNav::settings(Category::ALL.len());
        self.nav.category = category.min(Category::ALL.len() - 1);
        self.nav.in_rows = in_rows;
        if in_rows {
            self.nav.row = 0;
        }
        self.last_category = self.nav.category;
    }

    /// Puts the focus on a row of the open category (the gallery shows the row that matters).
    pub fn focus_row(&mut self, row: usize) {
        self.nav.row = row;
    }

    pub fn close(&mut self) {
        if self.open {
            self.open = false;
            self.closing = true;
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The clip key row waits for a key press.
    pub fn clips_capturing(&self) -> bool {
        self.clip_capture
    }

    /// While the clip key row waits: the first key with its modifiers binds it. Esc and a pad's
    /// back keep the old one; a key that cannot be a hotkey (a bare letter) is ignored and the
    /// row keeps waiting.
    fn capture_clip_key(
        &mut self,
        shell: &mut Shell,
        events: &[InputEvent],
        prefs: &mut Preferences,
        out: &mut Output<SettingsAction>,
    ) {
        for event in events {
            match event {
                InputEvent::CapturedKey {
                    key,
                    physical_key,
                    modifiers,
                } => {
                    let key = physical_key.unwrap_or(*key);
                    if key == egui::Key::Escape {
                        self.clip_capture = false;
                    } else if let Some(hotkey) = anyps5_core::clips::Hotkey::from_press(
                        key.name(),
                        modifiers.ctrl,
                        modifiers.alt,
                        modifiers.shift,
                    ) {
                        prefs.clips_hotkey = Some(hotkey.to_string());
                        out.changed = true;
                        self.clip_capture = false;
                    }
                }
                InputEvent::Command(crate::ui::input::Command::Back) => self.clip_capture = false,
                _ => {}
            }
            if !self.clip_capture {
                break;
            }
        }
        shell.input.capture_keys = self.clip_capture;
    }

    pub fn is_active(&self) -> bool {
        self.open || self.closing
    }

    pub fn show(
        &mut self,
        shell: &mut Shell,
        ctx: &Context,
        events: &[InputEvent],
        prefs: &mut Preferences,
        env: &SettingsEnv<'_>,
    ) -> Output<SettingsAction> {
        let mut out = Output::default();
        if self.clip_capture && !self.open {
            self.clip_capture = false;
            shell.input.capture_keys = false;
        }
        if !self.is_active() {
            return out;
        }
        // The keys of a frame that ended the capture are not commands (Esc must not close the page).
        let was_capturing = self.clip_capture;
        if was_capturing {
            self.capture_clip_key(shell, events, prefs, &mut out);
        }
        if self.open && !was_capturing {
            for command in commands(events) {
                let cats = build_categories(prefs, env, convert::language(prefs.language));
                let rows = &cats[self.nav.category.min(cats.len() - 1)].rows;
                let driven = rows::drive(shell, &mut self.nav, rows, &[command]);
                for change in &driven.changes {
                    self.apply(shell, ctx, prefs, env, change, &mut out);
                }
                if driven.close {
                    self.close();
                    out.actions.push(SettingsAction::Close);
                    break;
                }
            }
        }
        if self.nav.category != self.last_category {
            self.last_category = self.nav.category;
            self.switches += 1;
        }
        let language = convert::language(prefs.language);
        let mut cats = build_categories(prefs, env, language);
        let open = self.open;
        let mut mouse = None;
        let mut picked_category = None;
        let nav = self.nav;
        let switches = self.switches;
        let accent = shell.accent_color();
        let drawn = fullscreen_panel(
            ctx,
            "settings-panel",
            open,
            GlassStyle::fullscreen_panel(),
            |ui, _| {
                draw(
                    ui,
                    &mut cats,
                    &nav,
                    switches,
                    language,
                    accent,
                    &mut mouse,
                    &mut picked_category,
                )
            },
        );
        if drawn.is_none() {
            self.closing = false;
        }
        if open {
            if let Some(index) = picked_category {
                self.nav.category = index;
                self.nav.row = 0;
                self.nav.in_rows = false;
                shell.play(Cue::Move);
            }
            if let Some(events) = mouse {
                let rows = &cats[self.nav.category.min(cats.len() - 1)].rows;
                let (changes, texts) = rows::apply_mouse(shell, &mut self.nav, rows, events);
                for change in &changes {
                    self.apply(shell, ctx, prefs, env, change, &mut out);
                }
                for (id, value) in texts {
                    if id == "name" && prefs.profile_name != value {
                        prefs.profile_name = value;
                        out.changed = true;
                    }
                }
            }
        }
        out
    }

    fn apply(
        &mut self,
        shell: &mut Shell,
        ctx: &Context,
        prefs: &mut Preferences,
        env: &SettingsEnv<'_>,
        change: &Change,
        out: &mut Output<SettingsAction>,
    ) {
        let language = convert::language(prefs.language);
        let mut changed = apply_value(prefs, change);
        match change {
            Change::Choice {
                row: "backdrop",
                index,
            } if *index >= 1 => {
                let kind = if *index == 2 {
                    Backdrop::Video
                } else {
                    Backdrop::Image
                };
                // Choosing a picture asks for one unless it is already the choice with a file.
                if prefs.backdrop_file.is_none() || prefs.backdrop != kind {
                    changed |= choose_backdrop(shell, prefs, language);
                }
            }
            Change::Run { row: "file" } => changed |= choose_backdrop(shell, prefs, language),
            Change::Run { row: "folder" } => {
                let start = prefs.games_folder.clone();
                if let Some(folder) =
                    dialogs::pick_folder(shell, language.text(Key::PickFolder), start.as_deref())
                {
                    prefs.games_folder = Some(folder.clone());
                    changed = true;
                    out.actions.push(SettingsAction::GamesFolderChosen(folder));
                }
            }
            Change::Edit { .. } => {
                ctx.data_mut(|d| d.insert_temp(egui::Id::unique("panel-text-focus"), true));
                ctx.request_repaint();
            }
            Change::Run { row: "rescan" } => out.actions.push(SettingsAction::Rescan),
            Change::Run {
                row: "unlockedLocation",
            } => {
                let start = prefs
                    .unlocked_folder
                    .clone()
                    .or_else(|| prefs.games_folder.clone());
                if let Some(folder) = dialogs::pick_folder(
                    shell,
                    crate::screens::location_strings::dialog(language),
                    start.as_deref(),
                ) {
                    out.actions
                        .push(SettingsAction::UnlockedFolderPicked(folder));
                }
            }
            Change::Run {
                row: "unlockedDefault",
            } => out.actions.push(SettingsAction::UseDefaultLocation),
            Change::Run { row: "moveCancel" } => out.actions.push(SettingsAction::CancelMove),
            Change::Run {
                row: "moveContinue",
            } => out.actions.push(SettingsAction::ContinueMove),
            Change::Run { row: "moveLeave" } => out.actions.push(SettingsAction::LeaveOldCopies),
            change if phone::action(env.phone, change).is_some() => {
                out.actions
                    .extend(phone::action(env.phone, change).map(SettingsAction::Phone));
            }
            change if ludus::action(env.ludus, change).is_some() => {
                out.actions
                    .extend(ludus::action(env.ludus, change).map(SettingsAction::Ludus));
            }
            change if ludus_web::action(&env.ludus.browser, change).is_some() => {
                out.actions.extend(
                    ludus_web::action(&env.ludus.browser, change).map(SettingsAction::Ludus),
                );
            }
            Change::Run { row: "screenshots" } => {
                let start = prefs.screenshots_folder.clone();
                if let Some(folder) = dialogs::pick_folder(
                    shell,
                    language.text(Key::ScreenshotsFolder),
                    start.as_deref(),
                ) {
                    prefs.screenshots_folder = Some(folder);
                    changed = true;
                }
            }
            Change::Run {
                row: "showScreenshots",
            } => out.actions.push(SettingsAction::ShowScreenshots),
            Change::Run { row: "showClips" } => out.actions.push(SettingsAction::ShowClips),
            Change::Run { row: "addToSteam" } => out.actions.push(SettingsAction::AddToSteam),
            Change::Run {
                row: "addGamesToSteam",
            } => out.actions.push(SettingsAction::AddGamesToSteam),
            Change::Capture { row: "clipsKey" } => {
                self.clip_capture = true;
                shell.input.capture_keys = true;
            }
            Change::Run { row: "fonts" } => {
                let start = prefs.fonts_folder.clone();
                if let Some(folder) =
                    dialogs::pick_folder(shell, language.text(Key::FontsFolder), start.as_deref())
                {
                    prefs.fonts_folder = Some(folder);
                    changed = true;
                }
            }
            Change::Run { row: "fontsCjk" } => out.actions.push(SettingsAction::DownloadCjkFonts),
            Change::Toggle {
                row: "openh264",
                on,
            } => out.actions.push(SettingsAction::Openh264(*on)),
            Change::Run { row: "checkNow" } => out.actions.push(SettingsAction::CheckForUpdates),
            Change::Toggle {
                row: "updateCheck",
                on: true,
            } => {
                out.actions.push(SettingsAction::CheckForUpdates);
            }
            Change::Run { row: "install" } => out.actions.push(SettingsAction::InstallToolkit),
            Change::Run { row: "whatsNew" } => {
                if let Some(tag) = whats_new_tag(env) {
                    out.actions.push(SettingsAction::OpenWhatsNew(tag));
                }
            }
            Change::Choice {
                row: "channel",
                index: 0,
            } => {
                prefs.toolkit = None;
                prefs.toolkit_engine = false;
                prefs.toolkit_pinned = None;
                changed = true;
                out.actions.push(SettingsAction::UseStableToolkit);
            }
            Change::Choice {
                row: "channel",
                index: 1,
            } => {
                prefs.toolkit = None;
                prefs.toolkit_pinned = None;
                prefs.toolkit_engine = true;
                changed = true;
                out.actions.push(SettingsAction::UseEngineToolkit);
            }
            Change::Choice {
                row: "channel",
                index: 2,
            } => {
                // Stay on what is installed now; the list offers the other releases.
                let tag = env
                    .toolkit_version
                    .and_then(compat::release_tag)
                    .map(|t| format!("v{t}"))
                    .or_else(|| env.recent.first().cloned());
                if let Some(tag) = tag {
                    prefs.toolkit_engine = false;
                    prefs.toolkit = None;
                    prefs.toolkit_pinned = Some(tag.clone());
                    changed = true;
                    out.actions.push(SettingsAction::PinToolkit(tag));
                }
            }
            Change::Choice {
                row: "channel",
                index: 3,
            } if prefs.toolkit.is_none() => {
                if let Some(folder) =
                    dialogs::pick_folder(shell, Extra::ToolkitChooseFolder.text(language), None)
                {
                    out.actions.push(SettingsAction::AdoptToolkitFolder(folder));
                }
            }
            Change::Choice {
                row: "pinned",
                index,
            } => {
                if let Some(tag) = pinned_options(prefs, env).get(*index).cloned() {
                    prefs.toolkit_pinned = Some(tag.clone());
                    changed = true;
                    out.actions.push(SettingsAction::PinToolkit(tag));
                }
            }
            Change::Run { row } if REMOVE_ROWS.contains(row) => {
                if let Some(release) = REMOVE_ROWS
                    .iter()
                    .position(|r| r == row)
                    .and_then(|index| env.unused.get(index))
                {
                    out.actions
                        .push(SettingsAction::RemoveToolkit(release.tag.clone()));
                }
            }
            Change::Run { row: "restartNow" } => out.actions.push(SettingsAction::RestartNow),
            Change::Run {
                row: "cancelRestart",
            } => out.actions.push(SettingsAction::CancelRestart),
            Change::Run {
                row: "launcherCheck",
            } => out.actions.push(SettingsAction::CheckLauncherUpdate),
            Change::Run {
                row: "launcherNotes",
            } => out.actions.push(SettingsAction::OpenLauncherNotes),
            Change::Run {
                row: "launcherDownload",
            } => out.actions.push(SettingsAction::OpenPage(
                crate::screens::update_strings::RELEASES,
            )),
            Change::Run { row } if about_page(row).is_some() => {
                if let Some(page) = about_page(row) {
                    out.actions.push(SettingsAction::OpenPage(page));
                }
            }
            Change::Run {
                row: "toolkitFolder",
            } => {
                if let Some(folder) =
                    dialogs::pick_folder(shell, Extra::ToolkitChooseFolder.text(language), None)
                {
                    out.actions.push(SettingsAction::AdoptToolkitFolder(folder));
                }
            }
            Change::Toggle { row: "links", on } => out.actions.push(SettingsAction::SetLinks(*on)),
            Change::Run { row: "quit" } => out.actions.push(SettingsAction::Quit),
            _ => {}
        }
        if changed {
            out.changed = true;
            apply_preferences(shell, ctx, prefs);
        }
    }
}

/// Asks for a picture and stores it as the backdrop. Returns whether the preferences changed.
fn choose_backdrop(shell: &mut Shell, prefs: &mut Preferences, language: Language) -> bool {
    let start = prefs
        .backdrop_file
        .as_deref()
        .and_then(|p| p.parent())
        .map(|p| p.to_owned());
    match dialogs::pick_file(
        shell,
        language.text(Key::ChooseFile),
        ("Images", &IMAGE_EXTENSIONS),
        start.as_deref(),
    ) {
        Some(file) => {
            prefs.backdrop_file = Some(file);
            prefs.backdrop = Backdrop::Image;
            true
        }
        None => false,
    }
}

#[allow(clippy::too_many_arguments)]
fn draw(
    ui: &mut Ui,
    cats: &mut [CategoryRows],
    nav: &PanelNav,
    switches: u32,
    language: Language,
    accent: Color32,
    mouse: &mut Option<Vec<rows::ListEvent>>,
    picked: &mut Option<usize>,
) {
    let ctx = ui.ctx().clone();
    let painter = ui.painter().clone();
    let area = Rect::from_min_max(
        pos2(
            ui.max_rect().min.x + pad::SETTINGS.0,
            ui.max_rect().min.y + pad::SETTINGS.1,
        ),
        pos2(
            ui.max_rect().max.x - pad::SETTINGS.0,
            ui.max_rect().max.y - pad::SETTINGS.2,
        ),
    );

    // Title and the category list.
    let title = text::paint(
        &painter,
        area.min,
        Align2::LEFT_TOP,
        ty::SCREEN_TITLE,
        language.text(Key::Settings),
        Color32::WHITE,
    );
    let list_top = title.max.y + 31.0;
    let mut y = list_top;
    let column_w = size::SETTINGS_CATEGORY_WIDTH;
    for (index, cat) in cats.iter().enumerate() {
        let rect = Rect::from_min_size(
            pos2(area.min.x, y),
            vec2(column_w, size::SETTINGS_CATEGORY_ROW),
        );
        y += size::SETTINGS_CATEGORY_ROW + 6.0;
        let selected = index == nav.category;
        let focused = selected && !nav.in_rows;
        let response = ui.interact(
            rect,
            ui.make_persistent_id(("category", index)),
            Sense::click(),
        );
        crate::ui::a11y::follow(&ctx, response.id, focused);
        let title_text = cat.category.title(language);
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::Role::Tab, true, selected, title_text)
        });
        if input::hovered(&ctx, &response) {
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if response.clicked() {
            *picked = Some(index);
        }
        let t_focus = ctx.focus_t(("category-focus", index), focused);
        let t_sel = ctx.choice_t(("category-selected", index), selected);
        let hover_t = ctx.animate_to(
            ("category-hover", index),
            f32::from(input::hovered(&ctx, &response) && !selected),
            Motion::Hover,
        );
        // The selected category keeps its quiet fill; the one with the focus gets the ring.
        components::paint_surface(
            &painter,
            rect,
            radius::ROW,
            white(0.12 * t_sel),
            hover_t,
            t_focus,
            0.0,
            size::FOCUS_RING,
        );
        let ink = white(lerp(0.7, 1.0, t_sel));
        painter.text(
            pos2(rect.min.x + 18.0 + 13.0, rect.center().y),
            Align2::CENTER_CENTER,
            cat.category.icon().glyph(),
            cat.category.icon().font(18.0, false),
            ink,
        );
        let style = if selected {
            ty::CATEGORY_SELECTED
        } else {
            ty::CATEGORY
        };
        text::paint(
            &painter,
            pos2(rect.min.x + 18.0 + 26.0 + 14.0, rect.center().y),
            Align2::LEFT_CENTER,
            style,
            title_text,
            ink,
        );
    }

    // The rows of the current category, fading in from the side when the category changes.
    let current = nav.category.min(cats.len() - 1);
    let fade_in = ctx.animate_from_to(("settings-switch", switches), 0.0, 1.0, Motion::Focus);
    let x = area.min.x + column_w + 36.0;
    let list_rect = Rect::from_min_max(
        pos2(x + 18.0 * (1.0 - fade_in), list_top - size::LIST_INSET),
        area.max,
    );
    let mut child = ui.new_child(UiBuilder::new().max_rect(list_rect));
    child.set_opacity(fade_in.clamp(0.0, 1.0));
    let options = ListOptions {
        accent,
        language,
        focus: nav.in_rows.then_some(nav.row),
        force: None,
    };
    let events = rows::list(
        &mut child,
        ("settings-rows", current),
        &mut cats[current].rows,
        &options,
    );
    *mouse = Some(events);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(toolkit: Option<&'a str>, update: UpdateState) -> SettingsEnv<'a> {
        SettingsEnv {
            unused: &[],
            recent: &[],
            games_found: 9,
            app_version: "0.1.0",
            toolkit_version: toolkit,
            engine_built: None,
            glibc: None,
            update,
            launcher: LauncherUpdate::None,
            launcher_notes: false,
            default_screenshots: std::path::Path::new("C:\\Users\\me\\Pictures\\AnyPS5"),
            links: Handler::Absent,
            cjk_fonts: CjkState::Missing,
            phone: Box::leak(Box::new(PhoneState::default())),
            ludus: Box::leak(Box::new(LudusState::default())),
            clips: ClipsEnv::default(),
            location: LocationEnv::default(),
            openh264: None,
        }
    }

    fn ids(cats: &[CategoryRows], category: Category) -> Vec<&'static str> {
        cats.iter()
            .find(|c| c.category == category)
            .unwrap()
            .rows
            .iter()
            .map(|r| r.id)
            .collect()
    }

    #[test]
    fn the_categories_come_in_the_order_of_the_list() {
        let prefs = Preferences::first_run();
        let cats = build_categories(&prefs, &env(None, UpdateState::Unknown), Language::English);
        let order: Vec<_> = cats.iter().map(|c| c.category).collect();
        assert_eq!(order, Category::ALL);
    }

    #[test]
    fn appearance_rows_follow_the_backdrop_choice() {
        let mut prefs = Preferences::first_run();
        let e = env(None, UpdateState::Unknown);
        let cats = build_categories(&prefs, &e, Language::English);
        assert_eq!(
            ids(&cats, Category::Appearance),
            ["theme", "accent", "backdrop", "artwork", "motion"]
        );
        prefs.backdrop = Backdrop::Image;
        let cats = build_categories(&prefs, &e, Language::English);
        assert_eq!(
            ids(&cats, Category::Appearance),
            ["theme", "accent", "backdrop", "file", "artwork", "motion"],
            "the file row appears with a picture backdrop"
        );
    }

    #[test]
    fn vibration_is_a_choice_that_reaches_the_preferences() {
        let mut prefs = Preferences::default();
        assert_eq!(prefs.vibration, anyps5_core::settings::Vibration::Medium);
        for (index, want) in [
            (0, anyps5_core::settings::Vibration::Off),
            (1, anyps5_core::settings::Vibration::Light),
            (2, anyps5_core::settings::Vibration::Medium),
            (3, anyps5_core::settings::Vibration::Strong),
        ] {
            apply_value(
                &mut prefs,
                &Change::Choice {
                    row: "vibration",
                    index,
                },
            );
            assert_eq!(prefs.vibration, want);
            assert_eq!(convert::vibration_index(prefs.vibration), index);
        }
    }

    #[test]
    fn a_stored_video_choice_stays_visible() {
        let mut prefs = Preferences::first_run();
        let rows_of = |prefs: &Preferences| {
            build_categories(prefs, &env(None, UpdateState::Unknown), Language::English)
                .remove(0)
                .rows
                .into_iter()
                .find(|r| r.id == "backdrop")
                .unwrap()
        };
        let rows::Control::Choice { options, .. } = rows_of(&prefs).control else {
            panic!()
        };
        assert_eq!(options.len(), 2, "no video choice without one stored");
        prefs.backdrop = Backdrop::Video;
        let rows::Control::Choice { options, selected } = rows_of(&prefs).control else {
            panic!()
        };
        assert_eq!((options.len(), selected), (3, 2));
    }

    #[test]
    fn the_toolkit_rows_depend_on_what_is_installed() {
        let prefs = Preferences::first_run();
        let none = build_categories(&prefs, &env(None, UpdateState::Unknown), Language::English);
        assert!(
            ids(&none, Category::Toolkit).contains(&"install"),
            "nothing installed: offer it"
        );
        let current = build_categories(
            &prefs,
            &env(Some("0.1.3-abc"), UpdateState::UpToDate),
            Language::English,
        );
        assert!(!ids(&current, Category::Toolkit).contains(&"install"));
        let outdated = build_categories(
            &prefs,
            &env(Some("0.1.3-abc"), UpdateState::Available("0.1.4".into())),
            Language::English,
        );
        assert!(
            ids(&outdated, Category::Toolkit).contains(&"install"),
            "an update is offered"
        );
    }

    fn about_rows(launcher: LauncherUpdate) -> Vec<(&'static str, Option<String>)> {
        let prefs = Preferences::first_run();
        let mut e = env(None, UpdateState::Unknown);
        e.launcher = launcher;
        let cats = build_categories(&prefs, &e, Language::English);
        let about = cats.iter().find(|c| c.category == Category::About).unwrap();
        about
            .rows
            .iter()
            .map(|r| match &r.control {
                rows::Control::Info(value) => (r.id, value.clone()),
                _ => (r.id, None),
            })
            .collect()
    }

    fn row_ids(rows: &[(&'static str, Option<String>)]) -> Vec<&'static str> {
        rows.iter().map(|(id, _)| *id).collect()
    }

    #[test]
    fn about_always_shows_the_running_version_and_offers_the_restart_only_with_a_staged_update() {
        let plain = about_rows(LauncherUpdate::None);
        assert!(plain.contains(&("version", Some("0.1.0".into()))));
        assert!(!row_ids(&plain).contains(&"launcherUpdate"));
        assert!(!row_ids(&plain).contains(&"restartNow"));

        let ready = about_rows(LauncherUpdate::Ready("0.2.0".into()));
        // The version row keeps the version that runs; the update has its own row.
        assert!(ready.contains(&("version", Some("0.1.0".into()))));
        assert!(ready.contains(&("launcherUpdate", Some("0.2.0 ready".into()))));
        let ids = row_ids(&ready);
        let (restart, quit) = (
            ids.iter().position(|i| *i == "restartNow"),
            ids.iter().position(|i| *i == "quit"),
        );
        assert!(
            restart.is_some() && restart < quit,
            "Restart Now sits before Quit"
        );
        assert!(!ids.contains(&"launcherNotes"), "the notes row needs notes");
    }

    #[test]
    fn every_state_of_the_update_says_one_thing_and_offers_at_most_one_next_step() {
        let status = |u: LauncherUpdate| {
            about_rows(u)
                .into_iter()
                .find(|(id, _)| *id == "launcherUpdate")
                .and_then(|(_, v)| v)
        };
        assert_eq!(
            status(LauncherUpdate::Checking).as_deref(),
            Some("Checking…")
        );
        assert_eq!(
            status(LauncherUpdate::UpToDate).as_deref(),
            Some("Up to date")
        );
        assert_eq!(
            status(LauncherUpdate::Downloading {
                version: "0.2.0".into(),
                permille: 427
            })
            .as_deref(),
            Some("Downloading 42%")
        );
        assert_eq!(
            status(LauncherUpdate::Downloading {
                version: "0.2.0".into(),
                permille: 1000
            })
            .as_deref(),
            Some("Verifying")
        );
        assert_eq!(
            status(LauncherUpdate::Holding("0.2.0".into())).as_deref(),
            Some("Waiting for your game")
        );
        let holding = row_ids(&about_rows(LauncherUpdate::Holding("0.2.0".into())));
        assert!(holding.contains(&"cancelRestart") && !holding.contains(&"restartNow"));

        // A folder that cannot be written to and a refused release lead to the download page.
        for failed in [
            LauncherUpdate::ReadOnly("0.2.0".into()),
            LauncherUpdate::Manual("0.2.0".into()),
            LauncherUpdate::Failed(UpdateFailure::Refused),
            LauncherUpdate::Failed(UpdateFailure::Other),
        ] {
            let rows = row_ids(&about_rows(failed));
            assert!(rows.contains(&"launcherDownload"), "{rows:?}");
            assert!(!rows.contains(&"restartNow"));
        }
        // Offline: a retry, never a download.
        let offline = row_ids(&about_rows(LauncherUpdate::Failed(UpdateFailure::Offline)));
        assert!(offline.contains(&"launcherCheck") && !offline.contains(&"launcherDownload"));
    }

    #[test]
    fn the_notes_row_appears_with_notes() {
        let prefs = Preferences::first_run();
        let mut e = env(None, UpdateState::Unknown);
        e.launcher = LauncherUpdate::Ready("0.2.0".into());
        e.launcher_notes = true;
        let cats = build_categories(&prefs, &e, Language::English);
        let about = cats.iter().find(|c| c.category == Category::About).unwrap();
        let row = about.rows.iter().find(|r| r.id == "launcherNotes").unwrap();
        assert_eq!(row.title, "What's New in 0.2.0");
    }
    /// The Advanced rows with the codec row's facts, and the codec row's detail.
    fn codec_rows(on: bool, codec: Option<Openh264Env>) -> (Vec<&'static str>, Option<String>) {
        let mut prefs = Preferences::first_run();
        prefs.online_openh264 = on;
        let mut e = env(None, UpdateState::Unknown);
        e.openh264 = codec;
        let cats = build_categories(&prefs, &e, Language::English);
        let advanced = cats
            .iter()
            .find(|c| c.category == Category::Advanced)
            .unwrap();
        let detail = advanced
            .rows
            .iter()
            .find(|r| r.id == "openh264")
            .and_then(|r| r.detail.clone());
        (ids(&cats, Category::Advanced), detail)
    }

    fn codec(state: CodecState, worker: Option<(&str, Option<&str>)>) -> Option<Openh264Env> {
        Some(Openh264Env {
            state,
            worker: worker.map(|(s, d)| (s.to_owned(), d.map(str::to_owned))),
        })
    }

    #[test]
    fn the_codec_row_shows_cisco_s_notice_under_it_and_only_with_share_screen_on_linux() {
        let (rows, _) = codec_rows(false, None);
        assert!(!rows.contains(&"openh264") && !rows.contains(&"openh264Notice"));

        let (rows, detail) = codec_rows(false, codec(CodecState::Missing, None));
        let at = rows.iter().position(|r| *r == "openh264").unwrap();
        assert_eq!(rows[at + 1], "openh264Notice");
        assert_eq!(
            detail.as_deref(),
            Some("Share Screen cannot send or show video while it is off")
        );
        let mut prefs = Preferences::first_run();
        let mut e = env(None, UpdateState::Unknown);
        e.openh264 = codec(CodecState::Missing, None);
        for language in [Language::English, Language::Turkish] {
            prefs.language = convert::language_to_core(language);
            let cats = build_categories(&prefs, &e, language);
            let advanced = cats
                .iter()
                .find(|c| c.category == Category::Advanced)
                .unwrap();
            let notice = advanced
                .rows
                .iter()
                .find(|r| r.id == "openh264Notice")
                .unwrap();
            // Cisco's words, unchanged in every language.
            assert_eq!(
                notice.title,
                "OpenH264 Video Codec provided by Cisco Systems, Inc."
            );
        }
    }

    #[test]
    fn the_codec_row_says_what_the_download_and_the_worker_made_of_it() {
        let detail = |state, worker| codec_rows(true, codec(state, worker)).1.unwrap();
        assert_eq!(
            detail(CodecState::Downloading, None),
            "Downloading from Cisco"
        );
        assert_eq!(detail(CodecState::Missing, None), "Not downloaded yet");
        assert_eq!(
            detail(CodecState::Failed("timed out".into()), None),
            "The download failed: timed out"
        );
        assert_eq!(
            detail(CodecState::Ready, Some(("ready", None))),
            "Downloaded from Cisco"
        );
        assert_eq!(
            detail(CodecState::Ready, Some(("refused", Some("x")))),
            "The file is not Cisco's build. Turn it off and on to download it again."
        );
        assert_eq!(
            detail(CodecState::Ready, Some(("failed", Some("no such symbol")))),
            "It did not load: no such symbol"
        );
    }

    #[test]
    fn turning_the_codec_row_on_and_off_stores_it() {
        let mut prefs = Preferences::first_run();
        assert!(!prefs.online_openh264, "off until the player turns it on");
        apply_value(
            &mut prefs,
            &Change::Toggle {
                row: "openh264",
                on: true,
            },
        );
        assert!(prefs.online_openh264);
        apply_value(
            &mut prefs,
            &Change::Toggle {
                row: "openh264",
                on: false,
            },
        );
        assert!(!prefs.online_openh264);
    }

    fn fonts_row(prefs: &Preferences, cjk: CjkState) -> (String, Vec<&'static str>) {
        let mut e = env(None, UpdateState::Unknown);
        e.cjk_fonts = cjk;
        let cats = build_categories(prefs, &e, Language::English);
        let advanced = cats
            .iter()
            .find(|c| c.category == Category::Advanced)
            .unwrap();
        let detail = advanced
            .rows
            .iter()
            .find(|r| r.id == "fonts")
            .and_then(|r| r.detail.clone())
            .unwrap();
        (detail, ids(&cats, Category::Advanced))
    }

    #[test]
    fn the_fonts_row_names_the_set_a_game_gets() {
        let mut prefs = Preferences::first_run();
        let (detail, rows) = fonts_row(&prefs, CjkState::Missing);
        assert_eq!(
            detail,
            "Built-in Noto (Latin, Thai, Arabic) · Japanese, Chinese and Korean not downloaded"
        );
        assert!(rows.contains(&"fontsCjk"));
        let (detail, rows) = fonts_row(&prefs, CjkState::Present);
        assert_eq!(
            detail,
            "Built-in Noto (Latin, Thai, Arabic) · Japanese, Chinese and Korean included"
        );
        assert!(!rows.contains(&"fontsCjk"));
        let (detail, rows) = fonts_row(&prefs, CjkState::Downloading);
        assert!(detail.ends_with("Downloading Japanese, Chinese and Korean"));
        assert!(!rows.contains(&"fontsCjk"));
        // A picked folder is what the game gets: no built-in text, no download offer.
        prefs.fonts_folder = Some("/nonexistent-home-x/Fonts".into());
        let (detail, rows) = fonts_row(&prefs, CjkState::Missing);
        assert_eq!(detail, "/nonexistent-home-x/Fonts");
        assert!(!rows.contains(&"fontsCjk"));
    }

    #[test]
    fn advanced_offers_the_reset_only_with_a_fonts_folder() {
        let mut prefs = Preferences::first_run();
        let e = env(None, UpdateState::Unknown);
        let without = build_categories(&prefs, &e, Language::English);
        assert!(!ids(&without, Category::Advanced).contains(&"fontsReset"));
        prefs.fonts_folder = Some("D:\\Fonts".into());
        let with = build_categories(&prefs, &e, Language::English);
        assert!(ids(&with, Category::Advanced).contains(&"fontsReset"));
    }

    #[test]
    fn about_credits_ludusctrl_under_the_credits() {
        let rows = about_rows(LauncherUpdate::None);
        let ids = row_ids(&rows);
        let at = |id: &str| ids.iter().position(|i| *i == id).unwrap();
        assert_eq!(at("creditsLudus"), at("credits") + 1);
        let prefs = Preferences::first_run();
        let cats = build_categories(&prefs, &env(None, UpdateState::Unknown), Language::English);
        let about = cats.iter().find(|c| c.category == Category::About).unwrap();
        let row = about.rows.iter().find(|r| r.id == "creditsLudus").unwrap();
        assert_eq!(
            row.title,
            "LudusCtrl protocol and app by Josip Parlov (@parl0v)"
        );
        assert!(row.attached);
    }

    #[test]
    fn the_ludusctrl_switch_is_stored_and_sits_on_the_phone_page() {
        let mut prefs = Preferences::first_run();
        assert!(apply_value(
            &mut prefs,
            &Change::Toggle {
                row: "ludusctrl",
                on: true
            }
        ));
        assert!(prefs.phone_ludusctrl && !prefs.phone_remote);
        let cats = build_categories(&prefs, &env(None, UpdateState::Unknown), Language::English);
        assert_eq!(
            ids(&cats, Category::Phone),
            ["phone"],
            "an option of the phone switch"
        );
        prefs.phone_remote = true;
        let cats = build_categories(&prefs, &env(None, UpdateState::Unknown), Language::English);
        let phone = ids(&cats, Category::Phone);
        let at = |id: &str| phone.iter().position(|i| *i == id).unwrap();
        assert!(at("phonePaired") < at("ludusctrl") && at("ludusctrl") < at("ludusApp"));
    }

    #[test]
    fn about_ends_with_the_destructive_quit() {
        let prefs = Preferences::first_run();
        let cats = build_categories(&prefs, &env(None, UpdateState::Unknown), Language::English);
        let about = &cats.last().unwrap().rows;
        let quit = about.last().unwrap();
        assert_eq!(quit.id, "quit");
        assert!(quit.destructive);
    }

    #[test]
    fn the_join_button_row_sits_under_the_status_and_follows_it() {
        let mut prefs = Preferences::first_run();
        assert!(
            prefs.discord_presence && prefs.discord_join_button,
            "on by default"
        );
        let shown = build_categories(&prefs, &env(None, UpdateState::Unknown), Language::English);
        let rows = ids(&shown, Category::Behaviour);
        let status = rows.iter().position(|id| *id == "discord").unwrap();
        assert_eq!(
            rows[status + 1],
            "discordJoin",
            "right under the status row"
        );
        prefs.discord_presence = false;
        let hidden = build_categories(&prefs, &env(None, UpdateState::Unknown), Language::English);
        assert!(
            !ids(&hidden, Category::Behaviour).contains(&"discordJoin"),
            "no status, nothing to put a button on"
        );
    }

    #[test]
    fn values_land_in_the_preferences() {
        let mut prefs = Preferences::first_run();
        assert!(apply_value(
            &mut prefs,
            &Change::Choice {
                row: "theme",
                index: 2
            }
        ));
        assert_eq!(prefs.theme, anyps5_core::settings::Theme::Sunset);
        assert!(apply_value(
            &mut prefs,
            &Change::Choice {
                row: "accent",
                index: 3
            }
        ));
        assert_eq!(prefs.accent, Some(anyps5_core::settings::Accent::Violet));
        assert!(apply_value(
            &mut prefs,
            &Change::Toggle {
                row: "sounds",
                on: false
            }
        ));
        assert!(!prefs.sounds);
        assert!(apply_value(
            &mut prefs,
            &Change::Toggle {
                row: "music",
                on: false
            }
        ));
        assert!(!prefs.music);
        assert!(apply_value(
            &mut prefs,
            &Change::Toggle {
                row: "discord",
                on: false
            }
        ));
        assert!(!prefs.discord_presence);
        assert!(!prefs.show_hidden_trophies);
        assert!(apply_value(
            &mut prefs,
            &Change::Toggle {
                row: "hiddenTrophies",
                on: true
            }
        ));
        assert!(prefs.show_hidden_trophies);
        assert!(apply_value(
            &mut prefs,
            &Change::Toggle {
                row: "discordJoin",
                on: false
            }
        ));
        assert!(!prefs.discord_join_button);
        assert!(apply_value(
            &mut prefs,
            &Change::Choice {
                row: "motion",
                index: 1
            }
        ));
        assert!(prefs.reduced_motion);
        assert!(apply_value(
            &mut prefs,
            &Change::Choice {
                row: "language",
                index: 0
            }
        ));
        assert_eq!(prefs.language, anyps5_core::settings::Language::Turkish);
        assert!(apply_value(
            &mut prefs,
            &Change::Choice {
                row: "cpu",
                index: 2
            }
        ));
        assert_eq!(prefs.cpu_conversion, CpuConversion::Intel);
        assert!(apply_value(
            &mut prefs,
            &Change::Toggle {
                row: "updateCheck",
                on: false
            }
        ));
        assert!(!prefs.check_for_updates);
        assert!(!apply_value(&mut prefs, &Change::Run { row: "rescan" }));
    }

    #[test]
    fn every_row_has_text_in_both_languages() {
        let prefs = Preferences::first_run();
        for language in Language::ALL {
            let cats = build_categories(
                &prefs,
                &env(Some("0.1.3"), UpdateState::Available("0.1.4".into())),
                language,
            );
            for cat in &cats {
                assert!(!cat.category.title(language).is_empty());
                for row in &cat.rows {
                    assert!(!row.title.is_empty(), "{:?} {}", cat.category, row.id);
                }
            }
        }
    }

    fn toolkit_ids(prefs: &Preferences, version: Option<&str>) -> Vec<&'static str> {
        let cats = build_categories(
            prefs,
            &env(version, UpdateState::UpToDate),
            Language::English,
        );
        ids(&cats, Category::Toolkit)
    }

    #[test]
    fn unused_releases_can_be_removed_from_the_toolkit_page() {
        let prefs = Preferences::default();
        let unused = [
            InstalledRelease {
                tag: "0.1.2".into(),
                bytes: 412 * 1024 * 1024,
            },
            InstalledRelease {
                tag: "0.1.1".into(),
                bytes: 300 * 1024 * 1024,
            },
        ];
        let mut with = env(Some("0.1.3-abc"), UpdateState::UpToDate);
        with.unused = &unused;
        let cats = build_categories(&prefs, &with, Language::English);
        let rows = &cats
            .iter()
            .find(|c| c.category == Category::Toolkit)
            .unwrap()
            .rows;
        let remove: Vec<&Row> = rows
            .iter()
            .filter(|r| REMOVE_ROWS.contains(&r.id))
            .collect();
        assert_eq!(remove.len(), 2);
        assert_eq!(remove[0].title, "Remove v0.1.2");
        assert!(remove[0].group_start && !remove[1].group_start);
        assert!(
            remove[0]
                .detail
                .as_deref()
                .unwrap()
                .contains("No game uses it")
        );
        assert!(
            !toolkit_ids(&prefs, Some("0.1.3-abc"))
                .iter()
                .any(|id| REMOVE_ROWS.contains(id)),
            "nothing to remove, no row"
        );
    }

    #[test]
    fn the_channel_decides_which_toolkit_rows_exist() {
        let mut prefs = Preferences::default();
        let stable = toolkit_ids(&prefs, Some("0.1.3-abc"));
        for row in ["channel", "whatsNew", "updateCheck", "checkNow", "cpu"] {
            assert!(stable.contains(&row), "stable has {row}");
        }
        assert!(!stable.contains(&"pinned"));
        prefs.toolkit_pinned = Some("v0.1.2".into());
        let pinned = toolkit_ids(&prefs, Some("0.1.2-abc"));
        assert!(pinned.contains(&"pinned") && pinned.contains(&"whatsNew"));
        assert!(!pinned.contains(&"checkNow") && !pinned.contains(&"updateCheck"));
        prefs.toolkit = Some("D:/build".into());
        let local = toolkit_ids(&prefs, Some("local-abc"));
        assert!(local.contains(&"toolkitFolder") && !local.contains(&"pinned"));
        assert!(!local.contains(&"checkNow") && !local.contains(&"install"));
        assert!(
            !local.contains(&"whatsNew"),
            "a local build has no release notes"
        );
    }

    #[test]
    fn the_engine_channel_follows_updates_but_has_no_release_notes() {
        let prefs = Preferences {
            toolkit_engine: true,
            ..Default::default()
        };
        let mut with = env(
            Some("anyplay-30d0fe9"),
            UpdateState::Available("anyplay-a1b2c3d".into()),
        );
        with.engine_built = Some("2026-10-25");
        let cats = build_categories(&prefs, &with, Language::English);
        let rows = &cats
            .iter()
            .find(|c| c.category == Category::Toolkit)
            .unwrap()
            .rows;
        let ids: Vec<&str> = rows.iter().map(|r| r.id).collect();
        for row in [
            "channel",
            "engineBuilt",
            "updateCheck",
            "checkNow",
            "install",
        ] {
            assert!(ids.contains(&row), "the engine has {row}");
        }
        assert!(!ids.contains(&"whatsNew") && !ids.contains(&"pinned"));
        let channel = rows.iter().find(|r| r.id == "channel").unwrap();
        assert_eq!(
            channel.detail.as_deref(),
            Some(Extra::ChannelEngineDetail.text(Language::English))
        );
        let built = rows.iter().find(|r| r.id == "engineBuilt").unwrap();
        assert_eq!(built.title, "Built on 2026-10-25");
        assert!(built.attached, "the day hangs under the version row");
    }

    #[test]
    fn the_build_day_shows_only_on_the_engine_channel() {
        let mut with = env(Some("0.1.3-abc"), UpdateState::UpToDate);
        with.engine_built = Some("2026-10-25");
        let cats = build_categories(&Preferences::default(), &with, Language::English);
        let rows = &cats
            .iter()
            .find(|c| c.category == Category::Toolkit)
            .unwrap()
            .rows;
        assert!(rows.iter().all(|r| r.id != "engineBuilt"));
    }

    #[test]
    fn the_engine_without_a_build_offers_the_download() {
        let prefs = Preferences {
            toolkit_engine: true,
            ..Default::default()
        };
        let ids = toolkit_ids(&prefs, None);
        assert!(ids.contains(&"install"));
    }

    #[test]
    fn the_pinned_list_keeps_the_pinned_and_the_installed_release() {
        let prefs = Preferences {
            toolkit_pinned: Some("v0.0.9".into()),
            ..Default::default()
        };
        let recent = ["v0.1.3".to_owned(), "v0.1.2".to_owned()];
        let mut e = env(Some("0.1.0-abc"), UpdateState::Unknown);
        e.recent = &recent;
        assert_eq!(
            pinned_options(&prefs, &e),
            ["v0.1.3", "v0.1.2", "v0.0.9", "v0.1.0"]
        );
    }

    #[test]
    fn screenshots_need_a_build_newer_than_the_last_release() {
        let advanced = |version| {
            let cats = build_categories(
                &Preferences::default(),
                &env(Some(version), UpdateState::Unknown),
                Language::English,
            );
            ids(&cats, Category::Advanced)
        };
        let released = advanced("0.1.3-abc");
        assert!(released.contains(&"screenshots") && !released.contains(&"showScreenshots"));
        for version in ["0.1.4-abc", "local-abc"] {
            assert!(advanced(version).contains(&"showScreenshots"), "{version}");
        }
    }

    #[test]
    fn the_notes_are_about_the_update_when_one_waits() {
        let waiting = env(Some("0.1.3-abc"), UpdateState::Available("0.1.4".into()));
        assert_eq!(whats_new_tag(&waiting).as_deref(), Some("0.1.4"));
        let current = env(Some("0.1.3-abc"), UpdateState::UpToDate);
        assert_eq!(whats_new_tag(&current).as_deref(), Some("0.1.3"));
        assert_eq!(whats_new_tag(&env(None, UpdateState::Unknown)), None);
    }
}
