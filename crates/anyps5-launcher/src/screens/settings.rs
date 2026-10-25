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
use anyps5_core::settings::{Backdrop, CpuConversion, Preferences, ToolkitChannel};
use anyps5_core::toolkit::InstalledRelease;
use egui::{Align2, Color32, Context, Rect, Sense, Ui, UiBuilder, pos2, vec2};

use crate::app::phone::PhoneState;
use crate::screens::convert;
use crate::screens::dialogs;
use crate::screens::discord_strings::DiscordText;
use crate::screens::format::{abbreviate_path, home_dir};
use crate::screens::phone::{self, PhoneAction};
use crate::screens::phone_strings::PhoneText;
use crate::screens::rows::{self, Change, ListOptions, Row};
use crate::screens::side::{Output, commands};
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

/// Where the launcher's own update stands (the About page).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum LauncherUpdate {
    /// Nothing to show: up to date, not checked, or the check failed.
    #[default]
    None,
    /// A newer version is downloaded and verified: its version.
    Ready(String),
    /// A newer version exists but this folder cannot be written to: its version.
    ReadOnly(String),
}

/// What the settings show that is not a preference.
pub struct SettingsEnv<'a> {
    pub games_found: usize,
    pub app_version: &'a str,
    /// The installed toolkit's version, `None` when there is none.
    pub toolkit_version: Option<&'a str>,
    /// The toolkit needs a newer glibc than this system has (Linux).
    pub glibc: Option<anyps5_core::glibc::Shortfall>,
    pub update: UpdateState,
    /// The last releases (`v0.1.3`), for the pinned channel; empty until fetched.
    pub recent: &'a [String],
    /// Installed releases that neither the toolkit in use nor a game nor the pinned channel needs,
    /// newest first, with their size: the page offers to remove them.
    pub unused: &'a [InstalledRelease],
    pub launcher: LauncherUpdate,
    /// The folder screenshots go to when the preference is unset.
    pub default_screenshots: &'a std::path::Path,
    /// Who answers `anyps5://` links on this PC.
    pub links: Handler,
    /// Whether the built-in fonts have the Japanese, Chinese and Korean set.
    pub cjk_fonts: CjkState,
    /// What the phone server is doing.
    pub phone: &'a PhoneState,
}

/// What the host has to do after a frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingsAction {
    Close,
    /// A new games folder was chosen (already stored in the preferences): rescan and watch it.
    GamesFolderChosen(PathBuf),
    Rescan,
    ShowScreenshots,
    /// "Check now" (or the check was switched on).
    CheckForUpdates,
    /// Download and install the latest toolkit (first install or update).
    InstallToolkit,
    /// A toolkit folder was chosen: adopt it.
    AdoptToolkitFolder(PathBuf),
    /// The channel is stable again (already stored in the preferences): use the newest installed.
    UseStableToolkit,
    /// Stay on this release (`v0.1.3`, already stored in the preferences): install it if needed.
    PinToolkit(String),
    /// Download the Japanese, Chinese and Korean fonts of the built-in set.
    DownloadCjkFonts,
    /// Open the notes of this release (`0.1.3`).
    OpenWhatsNew(String),
    /// Delete this installed release (`0.1.2`); nothing uses it.
    RemoveToolkit(String),
    /// "Restart Now" on the About page: close, and start the staged launcher.
    RestartNow,
    /// One of the About page's links (website, Discord, privacy, terms).
    OpenPage(&'static str),
    /// "Open AnyPlay Links with This Launcher" was switched: write or remove the registration.
    SetLinks(bool),
    /// A row of the Phone as Controller page asks for something.
    Phone(PhoneAction),
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
    let library = vec![
        Row::action("folder", t(Key::Folder))
            .detail(path_text(&prefs.games_folder))
            .icon(Icon::FOLDER),
        Row::command("rescan", t(Key::Rescan))
            .detail(language.format(Key::GamesFound, &[Arg::Int(env.games_found as i64)]))
            .icon(Icon::ARROW_CLOCKWISE),
        Row::info("clones", t(Key::InstalledSize), None),
    ];

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
        Row::toggle("hide", t(Key::HideWhilePlaying), prefs.hide_while_playing)
            .icon(Icon::EYE_SLASH),
    );
    behaviour.push(
        Row::toggle(
            "hiddenTrophies",
            Extra::ShowHiddenTrophies.text(language),
            prefs.show_hidden_trophies,
        )
        .detail(Extra::ShowHiddenTrophiesDetail.text(language))
        .icon(Icon::TROPHY),
    );
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

    // Phone as Controller
    let phone_rows = phone::rows(prefs, env.phone, language);

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
    if let Some(short) = env.glibc {
        toolkit.push(
            Row::info(
                "toolkitGlibc",
                crate::screens::glibc_strings::shortfall_text(&short, language),
                None,
            )
            .attached(),
        );
    }
    toolkit.push(
        Row::choice(
            "channel",
            Extra::ChannelTitle.text(language),
            vec![
                Extra::ChannelStable.text(language).to_owned(),
                Extra::ChannelPinned.text(language).to_owned(),
                Extra::ChannelLocal.text(language).to_owned(),
            ],
            match channel {
                ToolkitChannel::Stable => 0,
                ToolkitChannel::Pinned => 1,
                ToolkitChannel::Local => 2,
            },
        )
        .detail(
            match channel {
                ToolkitChannel::Stable => Extra::ChannelStableDetail,
                ToolkitChannel::Pinned => Extra::ChannelPinnedDetail,
                ToolkitChannel::Local => Extra::ChannelLocalDetail,
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
        ToolkitChannel::Stable => {}
    }
    if let Some(tag) = whats_new_tag(env) {
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
    let update_ready =
        channel == ToolkitChannel::Stable && matches!(env.update, UpdateState::Available(_));
    if missing || update_ready {
        let text = if missing {
            Extra::ToolkitInstall
        } else {
            Extra::ToolkitInstallUpdate
        };
        toolkit.push(Row::command("install", text.text(language)).icon(Icon::DOWNLOAD_SIMPLE));
    }
    if channel == ToolkitChannel::Stable {
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
    }

    // About
    let version = match &env.launcher {
        LauncherUpdate::Ready(v) => Extra::LauncherReady.format(language, &[Arg::Str(v)]),
        _ => env.app_version.to_owned(),
    };
    let mut about = vec![
        Row::info("text", t(Key::AboutText), None),
        // The toolkit version lives on the Toolkit page only.
        Row::info("version", t(Key::Version), Some(version)),
    ];
    match &env.launcher {
        LauncherUpdate::Ready(_) => about.push(
            Row::command("restartNow", Extra::RestartNow.text(language))
                .icon(Icon::ARROW_CLOCKWISE),
        ),
        LauncherUpdate::ReadOnly(v) => about.push(Row::info(
            "launcherUpdate",
            t(Key::Status),
            Some(Extra::LauncherReadOnly.format(language, &[Arg::Str(v)])),
        )),
        LauncherUpdate::None => {}
    }
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

/// The page an About row opens.
fn about_page(row: &str) -> Option<&'static str> {
    use crate::screens::about_strings::{DISCORD, PRIVACY, TERMS, WEBSITE};
    Some(match row {
        "aboutWebsite" => WEBSITE,
        "aboutDiscord" => DISCORD,
        "aboutPrivacy" => PRIVACY,
        "aboutTerms" => TERMS,
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
        Change::Toggle { row: "music", on } => prefs.music = *on,
        Change::Toggle { row: "discord", on } => prefs.discord_presence = *on,
        Change::Toggle {
            row: "discordJoin",
            on,
        } => prefs.discord_join_button = *on,
        Change::Toggle { row: "hide", on } => prefs.hide_while_playing = *on,
        Change::Toggle {
            row: "hiddenTrophies",
            on,
        } => prefs.show_hidden_trophies = *on,
        Change::Toggle {
            row: "recordNetwork",
            on,
        } => prefs.record_network = *on,
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
}

impl Default for SettingsPanel {
    fn default() -> Self {
        Self {
            open: false,
            closing: false,
            nav: PanelNav::settings(Category::ALL.len()),
            switches: 0,
            last_category: 0,
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
        if !self.is_active() {
            return out;
        }
        if self.open {
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
            change if phone::action(env.phone, change).is_some() => {
                out.actions
                    .extend(phone::action(env.phone, change).map(SettingsAction::Phone));
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
                prefs.toolkit_pinned = None;
                changed = true;
                out.actions.push(SettingsAction::UseStableToolkit);
            }
            Change::Choice {
                row: "channel",
                index: 1,
            } => {
                // Stay on what is installed now; the list offers the other releases.
                let tag = env
                    .toolkit_version
                    .and_then(compat::release_tag)
                    .map(|t| format!("v{t}"))
                    .or_else(|| env.recent.first().cloned());
                if let Some(tag) = tag {
                    prefs.toolkit = None;
                    prefs.toolkit_pinned = Some(tag.clone());
                    changed = true;
                    out.actions.push(SettingsAction::PinToolkit(tag));
                }
            }
            Change::Choice {
                row: "channel",
                index: 2,
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
        let t_sel = ctx.focus_t(("category-selected", index), selected);
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
            glibc: None,
            update,
            launcher: LauncherUpdate::None,
            default_screenshots: std::path::Path::new("C:\\Users\\me\\Pictures\\AnyPS5"),
            links: Handler::Absent,
            cjk_fonts: CjkState::Missing,
            phone: Box::leak(Box::new(PhoneState::default())),
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

    #[test]
    fn about_shows_the_version_and_offers_the_restart_only_with_a_staged_update() {
        let plain = about_rows(LauncherUpdate::None);
        assert!(plain.contains(&("version", Some("0.1.0".into()))));
        assert!(!plain.iter().any(|(id, _)| *id == "restartNow"));

        let ready = about_rows(LauncherUpdate::Ready("0.2.0".into()));
        assert!(ready.contains(&("version", Some("0.2.0 ready".into()))));
        let ids: Vec<_> = ready.iter().map(|(id, _)| *id).collect();
        let (restart, quit) = (
            ids.iter().position(|i| *i == "restartNow"),
            ids.iter().position(|i| *i == "quit"),
        );
        assert!(
            restart.is_some() && restart < quit,
            "Restart Now sits before Quit"
        );

        let read_only = about_rows(LauncherUpdate::ReadOnly("0.2.0".into()));
        assert!(read_only.contains(&("version", Some("0.1.0".into()))));
        assert!(read_only.iter().any(|(id, _)| *id == "launcherUpdate"));
        assert!(!read_only.iter().any(|(id, _)| *id == "restartNow"));
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
