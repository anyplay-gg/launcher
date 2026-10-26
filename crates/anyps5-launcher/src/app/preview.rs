//! Made-up data and the hidden `--gallery=<scene>` / `--demo` modes.
//!
//! ```text
//! anyplay-launcher --gallery=home [--screenshot=out.png] [--size=1600x900] [--tr] [--pad] [--intro] [--frames=100]
//! anyplay-launcher --demo
//! ```
//!
//! Scenes: `onboarding0`, `onboarding1`, `onboarding2` (folder chosen), `onboarding2-empty`,
//! `onboarding3` (signing in, with a cloud),
//! `home`, `home-actions`, `home-trophies`, `home-locked`, `home-unlocking`, `home-relinking`,
//! `home-failed`, `home-queued`, `home-running`, `home-outdated`, `home-diagnosis`, `home-messages` (the
//! profile button with unread messages), `home-friends` (signed in, three friends online),
//! `home-appearing` (signed in, appearing offline), `home-friends-quiet` (nobody online), `home-phone` (a phone is connected), `home-ludus` (a phone and a LudusCtrl phone are connected), `home-browser` (a phone and a LudusCtrl browser client are connected), `home-friends-focus` and `home-signin-focus` (the top bar's
//! two pills focused), `home-theme1`,
//! `home-ask` (signed in, a session just ended: "How did it run?"), `home-runs` (the community verdict),
//! `home-glibc` and `home-glibc-locked` (the system's glibc is older than the libraries ask for),
//! `home-friends-playing` (friends playing the game), `home-theme2`, `home-noartwork`, `home-toolkit`, `home-toolkit-missing`, `home-empty`, `home-scanning`, `home-nofolder`, `starting`,
//! `toast`. The games, their icons and artwork are generated here (nothing of any real game is in
//! the repository); the clock of the backdrop is fixed, so a scene always renders the same.
//!
//! `--demo` opens the real window with the same library, a simulated pipeline (an unlock takes five
//! seconds, a start shows its window after two and a half) and no saving, to try the flows by hand.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyps5_core::library::{ExecutableKind, Game};
use anyps5_core::online::api::{
    Account, AccountId, Activity, Friend, Presence, RunSummary, Verdict, VerdictCounts,
};
use anyps5_core::paths::{AppPaths, GameId};
use anyps5_core::settings::{Language as CoreLanguage, Preferences, Theme as CoreTheme};
use anyps5_core::trophies::{Grade, Trophy, TrophySet};
use egui::{ColorImage, Context, Ui};
use image::{Rgba, RgbaImage};

use crate::app::model::Effect;
use crate::app::pipeline::{SimPipeline, UnlockStep};
use crate::app::{Config, LauncherApp};
use crate::ui::Shell;
use crate::ui::anim::AnimExt;
use crate::ui::background::{Backdrop, BackgroundClock, STILL_TIME_FULL};
use crate::ui::focus::HomeFocus;
use crate::ui::icons::Icon;
use crate::ui::input::{Command, InputEvent};
use crate::ui::theme::Theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    Onboarding(usize),
    OnboardingEmpty,
    Home,
    HomeActions,
    HomeTrophies,
    HomeLocked,
    HomeUnlocking,
    /// The launcher prepares the selected game on its own: progress, time left, the quiet tile band.
    HomePreparing,
    /// ... and it stands still because the PC is busy.
    HomePreparingPaused,
    /// ... before the work has started.
    HomePreparingWaiting,
    HomeRelinking,
    HomeFailed,
    HomeFailedLong,
    HomeLongTitle,
    HomeQueued,
    HomeRunning,
    /// The running game with the Home button's quick panel in front.
    HomeInGame,
    /// The question before the running game is closed.
    HomeInGameClose,
    HomeOutdated,
    HomeDiagnosis,
    /// Signed in with unread messages: the count on the profile button.
    HomeMessages,
    /// Signed in with three friends online: the count on the friends button.
    HomeFriends,
    /// Signed in and appearing offline: the ring on the profile button and its quiet words.
    HomeAppearing,
    /// Signed in, nobody online: the plain friends button.
    HomeFriendsQuiet,
    /// A paired phone is connected: its name after the title.
    HomePhone,
    /// The phone server's phone and a LudusCtrl phone are connected: both after the title.
    HomeLudus,
    HomeBrowser,
    /// The friends button focused, twelve friends online (two digits).
    HomeFriendsFocus,
    /// Signed out with the sign-in button focused.
    HomeSignInFocus,
    /// Signed in, the session just ended: the question "How did it run?" below the facts.
    HomeAsk,
    /// Signed in, enough players reported: the Runs fact.
    HomeRuns,
    /// Signed in, two friends play the game now.
    HomeFriendsPlaying,
    /// Signed in, the selected game can be played together: the Play Together button.
    HomeTogether,
    HomeTheme(u32),
    HomeNoArtwork,
    /// The selected game stays on a release of its own (a fact says so).
    HomeToolkit,
    /// The release the selected game stays on is gone: the question.
    HomeToolkitMissing,
    /// Linux with a glibc older than the libraries ask for, on an unlocked game (the Play button).
    HomeGlibc,
    /// The same on a locked game (the Unlock button).
    HomeGlibcLocked,
    HomeEmpty,
    HomeScanning,
    HomeNoFolder,
    Starting,
    Toast,
    /// A verified launcher update waits: the notice after the title.
    HomeUpdate,
    /// The same with the notice's first button focused.
    HomeUpdateFocus,
    /// "Restart Now" was pressed while a game runs: the restart waits.
    HomeUpdateHolding,
    /// "Later" was pressed: no notice, a dot on the settings button.
    HomeUpdateLater,
}

impl Scene {
    pub fn from_name(name: &str) -> Option<Scene> {
        Some(match name {
            "onboarding0" | "onboarding" => Scene::Onboarding(0),
            "onboarding1" => Scene::Onboarding(1),
            "onboarding2" => Scene::Onboarding(2),
            "onboarding3" => Scene::Onboarding(3),
            "onboarding2-empty" => Scene::OnboardingEmpty,
            "home" => Scene::Home,
            "home-actions" => Scene::HomeActions,
            "home-trophies" => Scene::HomeTrophies,
            "home-locked" => Scene::HomeLocked,
            "home-unlocking" => Scene::HomeUnlocking,
            "home-preparing" => Scene::HomePreparing,
            "home-preparing-paused" => Scene::HomePreparingPaused,
            "home-preparing-waiting" => Scene::HomePreparingWaiting,
            "home-relinking" => Scene::HomeRelinking,
            "home-failed" => Scene::HomeFailed,
            "home-failed-long" => Scene::HomeFailedLong,
            "home-long-title" => Scene::HomeLongTitle,
            "home-queued" => Scene::HomeQueued,
            "home-running" => Scene::HomeRunning,
            "home-ingame" => Scene::HomeInGame,
            "home-ingame-close" => Scene::HomeInGameClose,
            "home-outdated" => Scene::HomeOutdated,
            "home-diagnosis" => Scene::HomeDiagnosis,
            "home-messages" => Scene::HomeMessages,
            "home-friends" => Scene::HomeFriends,
            "home-appearing" => Scene::HomeAppearing,
            "home-friends-quiet" => Scene::HomeFriendsQuiet,
            "home-phone" => Scene::HomePhone,
            "home-ludus" => Scene::HomeLudus,
            "home-browser" => Scene::HomeBrowser,
            "home-friends-focus" => Scene::HomeFriendsFocus,
            "home-signin-focus" => Scene::HomeSignInFocus,
            "home-ask" => Scene::HomeAsk,
            "home-runs" => Scene::HomeRuns,
            "home-friends-playing" => Scene::HomeFriendsPlaying,
            "home-together" => Scene::HomeTogether,
            "home-theme1" => Scene::HomeTheme(1),
            "home-theme2" => Scene::HomeTheme(2),
            "home-noartwork" => Scene::HomeNoArtwork,
            "home-toolkit" => Scene::HomeToolkit,
            "home-toolkit-missing" => Scene::HomeToolkitMissing,
            "home-glibc" => Scene::HomeGlibc,
            "home-glibc-locked" => Scene::HomeGlibcLocked,
            "home-empty" => Scene::HomeEmpty,
            "home-scanning" => Scene::HomeScanning,
            "home-nofolder" => Scene::HomeNoFolder,
            "starting" => Scene::Starting,
            "toast" => Scene::Toast,
            "home-update" => Scene::HomeUpdate,
            "home-update-focus" => Scene::HomeUpdateFocus,
            "home-update-holding" => Scene::HomeUpdateHolding,
            "home-update-later" => Scene::HomeUpdateLater,
            _ => return None,
        })
    }

    pub const ALL_NAMES: [&'static str; 48] = [
        "onboarding0",
        "onboarding1",
        "onboarding2",
        "onboarding2-empty",
        "onboarding3",
        "home",
        "home-actions",
        "home-trophies",
        "home-locked",
        "home-unlocking",
        "home-preparing",
        "home-preparing-paused",
        "home-preparing-waiting",
        "home-relinking",
        "home-failed",
        "home-failed-long",
        "home-long-title",
        "home-queued",
        "home-running",
        "home-ingame",
        "home-ingame-close",
        "home-outdated",
        "home-diagnosis",
        "home-messages",
        "home-friends",
        "home-appearing",
        "home-friends-quiet",
        "home-phone",
        "home-ludus",
        "home-browser",
        "home-friends-focus",
        "home-signin-focus",
        "home-ask",
        "home-runs",
        "home-friends-playing",
        "home-together",
        "home-theme1",
        "home-theme2",
        "home-noartwork",
        "home-toolkit",
        "home-toolkit-missing",
        "home-glibc",
        "home-glibc-locked",
        "home-empty",
        "home-scanning",
        "home-nofolder",
        "starting",
        "toast",
    ];
}

/// Parsed command line of the preview modes.
#[derive(Clone, Debug)]
pub struct Options {
    /// `None`: the interactive demo.
    pub scene: Option<Scene>,
    pub screenshot: Option<PathBuf>,
    pub size: (f32, f32),
    /// `--at=X,Y`: the window's top-left corner on the screen (recordings).
    pub at: Option<(f32, f32)>,
    /// `--fullscreen`: fill the screen the window opens on, as the launcher does.
    pub fullscreen: bool,
    pub turkish: bool,
    /// `--pad`: hint bars show gamepad buttons instead of keys.
    pub pad: bool,
    /// `--theme=aurora|sunset|cosmic`: the backdrop theme (the scenes `home-theme1` and `-2` set it too).
    pub theme: Option<Theme>,
    /// `--bright`: the selected game's artwork is a pale picture (the UI's white on a light backdrop).
    pub bright: bool,
    /// `--intro`: play the launch intro (to screenshot it part way with `--frames`).
    pub intro: bool,
    /// `--hover=X,Y`: where the pointer rests, in dp (to screenshot a hover state).
    pub hover: Option<(f32, f32)>,
    pub frames: u32,
    /// Commands fed to the app one after the other (`--script=right,select,wait60`).
    pub script: Vec<Step>,
    /// A real dump whose `icon0.png` and `pic0.png` replace the second game's generated art, for
    /// local comparisons with the original (never committed, never uploaded).
    pub real_art: Option<PathBuf>,
    /// `ANYPS5_PREVIEW_GAMES`: a folder of dump-shaped game folders (eboot.bin, sce_sys/param.json,
    /// icon0.png, pic0.png) that replace the generated library, in folder-name order. For the
    /// README's pictures; the art stays where it is.
    pub showcase: Option<PathBuf>,
}

/// One step of a scripted run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Command(Command),
    /// Wait this many frames.
    Wait(u32),
    /// `Ctrl+,`
    OpenSettings,
}

fn parse_script(text: &str) -> Vec<Step> {
    text.split(',')
        .filter_map(|word| {
            Some(match word.trim() {
                "left" => Step::Command(Command::Left),
                "right" => Step::Command(Command::Right),
                "up" => Step::Command(Command::Up),
                "down" => Step::Command(Command::Down),
                "select" => Step::Command(Command::Select),
                "back" => Step::Command(Command::Back),
                "options" => Step::Command(Command::Options),
                "settings" => Step::OpenSettings,
                other => Step::Wait(other.strip_prefix("wait")?.parse().ok()?),
            })
        })
        .collect()
}

/// `--gallery=<scene>` of a home-screen scene, or `--demo`.
pub fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Option<Options> {
    let mut options = Options {
        scene: None,
        screenshot: None,
        size: (1600.0, 900.0),
        at: None,
        fullscreen: false,
        turkish: false,
        pad: false,
        theme: None,
        bright: false,
        intro: false,
        hover: None,
        frames: 100,
        script: Vec::new(),
        real_art: std::env::var_os("ANYPS5_PREVIEW_REAL_ART").map(PathBuf::from),
        showcase: std::env::var_os("ANYPS5_PREVIEW_GAMES").map(PathBuf::from),
    };
    let mut seen = false;
    for arg in args {
        if arg == "--demo" {
            seen = true;
        } else if let Some(name) = arg.strip_prefix("--gallery=")
            && let Some(scene) = Scene::from_name(name)
        {
            seen = true;
            options.scene = Some(scene);
        } else if let Some(path) = arg.strip_prefix("--screenshot=") {
            options.screenshot = Some(PathBuf::from(path));
        } else if let Some(dims) = arg.strip_prefix("--size=") {
            if let Some((Ok(w), Ok(h))) = dims.split_once('x').map(|(w, h)| (w.parse(), h.parse()))
            {
                options.size = (w, h);
            }
        } else if let Some(at) = arg.strip_prefix("--at=") {
            if let Some((Ok(x), Ok(y))) = at.split_once(',').map(|(x, y)| (x.parse(), y.parse())) {
                options.at = Some((x, y));
            }
        } else if let Some(frames) = arg.strip_prefix("--frames=") {
            options.frames = frames.parse().unwrap_or(100);
        } else if let Some(script) = arg.strip_prefix("--script=") {
            options.script = parse_script(script);
        } else if let Some(at) = arg.strip_prefix("--hover=") {
            options.hover = at
                .split_once(',')
                .and_then(|(x, y)| Some((x.parse().ok()?, y.parse().ok()?)));
        } else if arg == "--pad" {
            options.pad = true;
        } else if let Some(name) = arg.strip_prefix("--theme=") {
            options.theme = crate::ui::theme::Theme::ALL.into_iter().find(|t| {
                t.title(crate::ui::i18n::Language::English)
                    .eq_ignore_ascii_case(name)
            });
        } else if arg == "--bright" {
            options.bright = true;
        } else if arg == "--intro" {
            options.intro = true;
        } else if arg == "--fullscreen" {
            options.fullscreen = true;
        } else if arg == "--tr" {
            options.turkish = true;
        }
    }
    seen.then_some(options)
}

// ---------------------------------------------------------------------------------------------
// Generated games
// ---------------------------------------------------------------------------------------------

fn splitmix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn unit(seed: u64, k: u64) -> f32 {
    (splitmix(seed.wrapping_mul(31).wrapping_add(k)) >> 40) as f32 / (1u64 << 24) as f32
}

fn hsv(h: f32, s: f32, v: f32) -> [f32; 3] {
    let h = h.rem_euclid(1.0) * 6.0;
    let (i, f) = (h.floor() as i32, h.fract());
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match i {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

/// Abstract artwork: a diagonal gradient with soft discs and a ring. Deterministic per seed.
fn make_art(width: u32, height: u32, seed: u64) -> RgbaImage {
    let hue = unit(seed, 1);
    let dark = hsv(hue, 0.7, 0.30);
    let light = hsv(hue + 0.08, 0.75, 0.90);
    let discs: Vec<(f32, f32, f32, [f32; 3])> = (0..6)
        .map(|i| {
            (
                unit(seed, 10 + i),
                unit(seed, 20 + i),
                0.08 + 0.22 * unit(seed, 30 + i),
                hsv(hue + 0.15 + 0.12 * i as f32, 0.55, 0.95),
            )
        })
        .collect();
    let aspect = width as f32 / height as f32;
    RgbaImage::from_fn(width, height, |x, y| {
        let (u, v) = (x as f32 / width as f32, y as f32 / height as f32);
        let t = ((u + v) * 0.5).clamp(0.0, 1.0);
        let mut c = [
            dark[0] + (light[0] - dark[0]) * t,
            dark[1] + (light[1] - dark[1]) * t,
            dark[2] + (light[2] - dark[2]) * t,
        ];
        for &(cx, cy, r, tint) in &discs {
            let d = (((u - cx) * aspect).powi(2) + (v - cy).powi(2)).sqrt();
            let a = (1.0 - d / r).clamp(0.0, 1.0).powi(2) * 0.45;
            for k in 0..3 {
                c[k] += (tint[k] - c[k]) * a;
            }
        }
        let ring = (((u - 0.5) * aspect).powi(2) + (v - 0.5).powi(2)).sqrt();
        let ring_a = (1.0 - ((ring - 0.22).abs() / 0.018)).clamp(0.0, 1.0) * 0.55;
        for channel in &mut c {
            *channel += (1.0 - *channel) * ring_a;
        }
        Rgba([
            (c[0].clamp(0.0, 1.0) * 255.0) as u8,
            (c[1].clamp(0.0, 1.0) * 255.0) as u8,
            (c[2].clamp(0.0, 1.0) * 255.0) as u8,
            255,
        ])
    })
}

/// A pale picture: cream sky, white haze, soft pastel discs. The worst case for white controls and
/// white text laid over the artwork.
fn bright_art(width: u32, height: u32) -> RgbaImage {
    RgbaImage::from_fn(width, height, |x, y| {
        let (u, v) = (x as f32 / width as f32, y as f32 / height as f32);
        let sky = [0.97 - 0.06 * v, 0.95 - 0.03 * v, 0.88 + 0.06 * v];
        let glow =
            (1.0 - (((u - 0.7).powi(2) * 2.0 + (v - 0.35).powi(2)).sqrt() / 0.5)).clamp(0.0, 1.0);
        let c = [
            sky[0] + (1.0 - sky[0]) * glow,
            sky[1] + (1.0 - sky[1]) * glow,
            sky[2] + (1.0 - sky[2]) * glow * 0.6,
        ];
        Rgba([
            (c[0] * 255.0) as u8,
            (c[1] * 255.0) as u8,
            (c[2] * 255.0) as u8,
            255,
        ])
    })
}

/// The made-up names; none of them is a real title.
const NAMES: [&str; 9] = [
    "Lanternfall",
    "Hollow Orchard",
    "Starlit Courier",
    "Pale Harbor",
    "Ember Cartographer",
    "Quiet Machines",
    "Tidepool Kingdom",
    "Nightbloom",
    "Copper Sky Racing",
];

/// Writes the generated icons and artwork under `dir` and returns the games.
pub fn make_games(dir: &Path) -> Vec<Game> {
    NAMES
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let folder = dir.join(name);
            let sys = folder.join("sce_sys");
            let _ = std::fs::create_dir_all(&sys);
            let icon = sys.join("icon0.png");
            let background = sys.join("pic0.png");
            let seed = 1000 + i as u64 * 7;
            if !icon.exists() {
                let _ = make_art(512, 512, seed).save(&icon);
            }
            if !background.exists() {
                let _ = make_art(1280, 720, seed + 3).save(&background);
            }
            Game {
                id: GameId::from_path(&folder),
                folder,
                title_id: format!("PPSA9{:04}", 100 + i * 17),
                name: (*name).to_owned(),
                names: Default::default(),
                version: format!("01.000.0{:02}", 10 + i),
                content_id: None,
                icon: Some(icon),
                background: Some(background),
                music: None,
                executable: ExecutableKind::Elf,
                size: None,
                unlocked_with: None,
            }
        })
        .collect()
}

fn make_trophies(game: &Game, unlocked: usize) -> TrophySet {
    let grades = [Grade::Bronze, Grade::Silver, Grade::Gold, Grade::Bronze];
    TrophySet {
        title: game.name.clone(),
        icon: None,
        trophies: (0..34)
            .map(|n| Trophy {
                service_label: 0,
                number: n as u32,
                grade: if n == 0 {
                    Grade::Platinum
                } else {
                    grades[n % 4]
                },
                hidden: false,
                name: format!("Trophy {n}"),
                detail: "Made up for the preview".to_owned(),
                icon: None,
                unlocked: (n < unlocked)
                    .then_some(1_760_000_000_000_000 + n as i64 * 3_600_000_000),
                progress: None,
            })
            .collect(),
    }
}

// ---------------------------------------------------------------------------------------------
// Scenes
// ---------------------------------------------------------------------------------------------

/// Sets the library and the state of a scene.
pub fn apply_scene(m: &mut crate::app::model::Model, scene: Scene, games: Vec<Game>) {
    let folder = PathBuf::from("D:\\Games\\PS5");
    m.edit_prefs(|p| {
        p.games_folder = Some(folder.clone());
        p.profile_name = "Mira".to_owned();
        p.sounds = false;
        p.onboarded = !matches!(scene, Scene::Onboarding(_) | Scene::OnboardingEmpty);
        if let Scene::HomeTheme(index) = scene {
            p.theme = match index {
                1 => CoreTheme::Aurora,
                _ => CoreTheme::Sunset,
            };
        }
        p.game_artwork = scene != Scene::HomeNoArtwork;
        // The online step needs a cloud to sign in to.
        if scene == Scene::Onboarding(3) {
            p.online_cloud = Some("https://anyplaynet.com".to_owned());
        }
    });
    m.toolkit = Some("0.1.0".to_owned());

    let with_games = !matches!(
        scene,
        Scene::HomeEmpty | Scene::HomeScanning | Scene::HomeNoFolder | Scene::OnboardingEmpty
    );
    if matches!(scene, Scene::HomeNoFolder) {
        m.edit_prefs(|p| p.games_folder = None);
    }
    if matches!(scene, Scene::HomeScanning | Scene::OnboardingEmpty) {
        m.scanning = matches!(scene, Scene::HomeScanning);
    }
    if scene == Scene::OnboardingEmpty {
        // The folder step with a folder that holds no games.
        m.onboarding.step = 2;
    }
    if !with_games {
        return;
    }

    let mut games = games;
    // A mix of states along the row: installed, installed, locked, unreadable, locked, ...
    for (i, game) in games.iter_mut().enumerate() {
        match i {
            0 | 1 | 5 | 6 => game.unlocked_with = Some("0.1.0".to_owned()),
            3 => game.executable = ExecutableKind::Unreadable,
            _ => {}
        }
    }
    let ids: Vec<GameId> = games.iter().map(|g| g.id.clone()).collect();
    // Scenes name games by position; a shorter library (a showcase) uses its last game instead.
    let at = |i: usize| i.min(ids.len().saturating_sub(1));
    m.games = games;
    m.selected = at(1);
    for (i, id) in ids.iter().enumerate() {
        m.sizes.insert(
            id.clone(),
            crate::app::model::SizeState::Known(8_390_000_000 + i as u64 * 1_730_000_000),
        );
    }
    // The first two have been played: a made-up 45 minutes and 12 hours; the rest never.
    m.edit_prefs(|p| {
        for (i, seconds) in [(0, 45 * 60), (1, 12 * 3600 + 34 * 60)] {
            p.count_session(&ids[at(i)]);
            p.add_playtime(&ids[at(i)], seconds);
        }
    });
    let trophy_game = m.games[at(1)].clone();
    m.trophies
        .insert(trophy_game.id.clone(), make_trophies(&trophy_game, 8));

    match scene {
        Scene::Onboarding(step) => {
            m.onboarding.step = step;
        }

        Scene::HomeActions => m.focus = HomeFocus::Actions(0),
        Scene::HomeTogether => {
            sign_in(m);
            let id = ids[at(1)].clone();
            if let Some(game) = m.games.iter_mut().find(|g| g.id == id) {
                game.title_id = "PPSA17221".into();
                game.name = "Minecraft".into();
            }
            m.focus = HomeFocus::Actions(1);
        }
        Scene::HomeTrophies => m.focus = HomeFocus::Actions(1),
        Scene::HomeLocked => m.selected = 2,
        Scene::HomeGlibc => m.focus = HomeFocus::Actions(0),
        Scene::HomeGlibcLocked => {
            m.selected = 2;
            m.focus = HomeFocus::Actions(0);
        }
        Scene::HomeUnlocking => {
            m.selected = at(2);
            m.progress
                .insert(ids[at(2)].clone(), (UnlockStep::Copy, 0.4));
            m.focus = HomeFocus::Actions(0);
        }
        Scene::HomePreparing | Scene::HomePreparingPaused | Scene::HomePreparingWaiting => {
            m.selected = at(2);
            let id = ids[at(2)].clone();
            m.prep.background.insert(id.clone());
            m.prep.background.insert(ids[at(4)].clone());
            if scene != Scene::HomePreparingWaiting {
                m.progress.insert(id.clone(), (UnlockStep::Relink, 0.45));
                m.preview_eta(&id, 130.0);
            }
            if scene == Scene::HomePreparingPaused {
                m.prep.paused.insert(id);
            }
            m.focus = HomeFocus::Actions(0);
        }
        Scene::HomeRelinking => {
            m.selected = at(2);
            m.progress
                .insert(ids[at(2)].clone(), (UnlockStep::Relink, 0.0));
        }
        Scene::HomeFailed => {
            m.selected = at(4);
            m.failures.insert(
                ids[at(4)].clone(),
                crate::screens::prepare_strings::failure(
                    anyps5_core::unlock::FailureKind::DiskFull,
                    m.language(),
                )
                .to_owned(),
            );
        }
        Scene::HomeFailedLong => {
            m.selected = at(4);
            m.failures.insert(
                ids[at(4)].clone(),
                crate::screens::prepare_strings::failure(
                    anyps5_core::unlock::FailureKind::LocationMissing,
                    m.language(),
                )
                .to_owned(),
            );
        }
        Scene::HomeLongTitle => {
            m.selected = at(1);
            if let Some(game) = m.games.iter_mut().find(|g| g.id == ids[at(1)]) {
                game.name = "The Extraordinarily Long Title of a Remastered Collection: Director's Cut, Complete Edition".into();
            }
            m.focus = HomeFocus::Actions(0);
        }
        Scene::HomeQueued => {
            m.selected = at(4);
            m.queue.push(ids[at(4)].clone());
            m.progress
                .insert(ids[at(2)].clone(), (UnlockStep::Relink, 0.3));
        }
        Scene::HomeRunning => m.running = Some(ids[at(1)].clone()),
        Scene::HomeInGame | Scene::HomeInGameClose => {
            m.running = Some(ids[at(1)].clone());
            m.summoned = true;
            m.switch_overlay(crate::app::overlay::Overlay::InGame);
            if scene == Scene::HomeInGameClose {
                m.switch_overlay(crate::app::overlay::Overlay::ConfirmCloseGame);
            }
        }
        Scene::HomeOutdated => m.toolkit = Some("0.2.0".to_owned()),
        Scene::HomeToolkit | Scene::HomeToolkitMissing => {
            let id = ids[at(1)].clone();
            // The simulated pipeline has v0.1.2 beside its own toolkit, and not v0.1.1.
            let pin = if scene == Scene::HomeToolkit {
                "v0.1.2"
            } else {
                "v0.1.1"
            };
            m.edit_prefs(|p| {
                let mut settings = p.game_settings(&id);
                settings.toolkit = Some(pin.to_owned());
                p.set_game_settings(&id, settings);
            });
            if scene == Scene::HomeToolkitMissing {
                m.toolkit_question = Some(id);
                m.switch_overlay(crate::app::overlay::Overlay::ToolkitMissing);
            }
        }
        Scene::HomeDiagnosis => {
            m.diagnoses.insert(
                ids[at(1)].clone(),
                "The game crashed (0xC0000005 ACCESS_VIOLATION).".to_owned(),
            );
            m.focus = HomeFocus::Actions(0);
        }
        Scene::HomeFriends => {
            sign_in(m);
            add_friends(m, 3, 1);
        }
        Scene::HomeAppearing => {
            sign_in(m);
            add_friends(m, 3, 1);
            m.online.set_appear_offline(true);
            m.online.take_outbox();
        }
        Scene::HomePhone => {
            m.phone.connected = Some("Pixel 8".into());
        }
        Scene::HomeLudus => {
            m.phone.connected = Some("Pixel 8".into());
            m.ludus.connected = Some("Galaxy Tab S9".into());
        }
        Scene::HomeBrowser => {
            m.phone.connected = Some("Pixel 8".into());
            m.ludus.browser.peer = Some("192.168.178.41".into());
            m.ludus.browser.device = Some("iPhone".into());
        }
        Scene::HomeFriendsQuiet => {
            sign_in(m);
            add_friends(m, 0, 4);
        }
        Scene::HomeFriendsFocus => {
            sign_in(m);
            add_friends(m, 12, 2);
            m.focus = HomeFocus::FRIENDS;
        }
        Scene::HomeSignInFocus => m.focus = HomeFocus::PROFILE,
        Scene::HomeMessages => {
            use anyps5_core::online::api::{
                Account, AccountId, Conversation, ConversationKind, Conversations,
            };
            sign_in(m);
            let chat = |id: &str, with: (u64, &str), last, read, request| Conversation {
                id: id.into(),
                kind: ConversationKind::Direct,
                name: None,
                members: [(1, "Mira"), with]
                    .iter()
                    .map(|(id, handle)| Account {
                        id: AccountId(*id),
                        handle: (*handle).into(),
                    })
                    .collect(),
                last,
                read,
                marks: Vec::new(),
                updated: last,
                request,
                party: None,
                unread: last.saturating_sub(read),
                muted: false,
                pinned: false,
                rev: 0,
            };
            m.online.social.set_conversations(Conversations {
                conversations: vec![chat("a", (2, "Lumen"), 7, 5, false)],
                requests: vec![chat("r", (9, "Nyx"), 1, 0, true)],
            });
        }
        Scene::HomeAsk | Scene::HomeRuns | Scene::HomeFriendsPlaying => {
            sign_in(m);
            let game = m.games[at(1)].clone();
            // The game stays on a toolkit release of its own, so the Toolkit fact stands beside Runs.
            m.edit_prefs(|p| {
                let mut settings = p.game_settings(&game.id);
                settings.toolkit = Some("v0.1.2".to_owned());
                p.set_game_settings(&game.id, settings);
            });
            if scene != Scene::HomeFriendsPlaying {
                m.online.runs.keep(
                    &game.title_id,
                    Some(&game.version),
                    RunSummary {
                        title_id: game.title_id.clone(),
                        version: Some(game.version.clone()),
                        reports: 12,
                        verdict: Some(Verdict::Playable),
                        counts: VerdictCounts {
                            no_start: 1,
                            menus: 2,
                            in_game: 3,
                            playable: 6,
                        },
                    },
                );
            }
            if scene != Scene::HomeRuns {
                let playing = |id, handle: &str, title: &str| Friend {
                    account: Account {
                        id: AccountId(id),
                        handle: handle.into(),
                    },
                    since: 1,
                    presence: Presence {
                        online: true,
                        activity: Some(Activity {
                            title_id: title.into(),
                            title: game.name.clone(),
                            version: game.version.clone(),
                            session: None,
                        }),
                        revision: 1,
                    },
                };
                m.online.friends.friends = vec![
                    playing(2, "Mira", &game.title_id),
                    playing(3, "Sam", &game.title_id),
                ];
            }
            if scene == Scene::HomeAsk {
                m.online.friends.friends.clear();
                m.run_prompt = Some(crate::app::run_question::RunPrompt {
                    game: game.id.clone(),
                    title_id: game.title_id.clone(),
                    version: game.version.clone(),
                    toolkit: "0.1.0".to_owned(),
                });
                m.focus = HomeFocus::Ask(3);
            }
        }
        Scene::Starting => {
            m.starting = Some(ids[at(1)].clone());
            m.running = Some(ids[at(1)].clone());
        }
        Scene::HomeUpdateFocus => m.focus = HomeFocus::Update(0),
        // The restart waits because a game runs.
        Scene::HomeUpdateHolding => m.running = Some(ids[at(1)].clone()),
        Scene::HomeUpdateLater => m.update_dismissed = Some("0.1.4".to_owned()),
        Scene::Toast => {
            m.toast("Lanternfall · Ready to play", Icon::LOCK_OPEN);
            if let Some(toast) = m.toast.as_mut() {
                toast.until = 1.0e6;
            }
        }
        _ => {}
    }
    // The preview has no workers: what the model asked for is dropped.
    m.effects.retain(|e| matches!(e, Effect::Sound(_)));
}

/// Signs the preview in as Mira, the cloud answering.
fn sign_in(m: &mut crate::app::model::Model) {
    use anyps5_core::online::api::AccountId;
    use anyps5_core::online::{Cloud, SignedIn, Status};
    m.online.status = Status::Running;
    m.online.cloud = Some(Cloud::Online);
    m.online.account = Some(SignedIn {
        id: AccountId(1),
        handle: "Mira".into(),
        email: "mira@example.org".into(),
        appear_offline: false,
    });
}

/// Invented friends: `online` of them online, then `offline` who are not.
fn add_friends(m: &mut crate::app::model::Model, online: usize, offline: usize) {
    use anyps5_core::online::api::{Account, AccountId, Friend, Presence};
    m.online.friends.friends = (0..online + offline)
        .map(|i| Friend {
            account: Account {
                id: AccountId(10 + i as u64),
                handle: format!("Friend{}", i + 1),
            },
            since: 0,
            presence: Presence {
                online: i < online,
                activity: None,
                revision: 0,
            },
        })
        .collect();
}

/// The games of a showcase folder, read like the library reads dumps.
fn showcase_games(folder: &Path) -> Vec<Game> {
    let mut dumps: Vec<PathBuf> = std::fs::read_dir(folder)
        .map(|entries| entries.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    dumps.sort();
    dumps
        .iter()
        .filter_map(|dump| anyps5_core::library::inspect(dump))
        .collect()
}

/// Points a preview game at the icon and artwork of a real dump on this PC (and gives it the
/// folder's title). Local checks only: the files are read from where they are, never copied.
fn use_real_art(game: &mut Game, dump: &Path) {
    let sys = dump.join("sce_sys");
    let (icon, pic) = (sys.join("icon0.png"), sys.join("pic0.png"));
    if icon.is_file() {
        game.icon = Some(icon);
    }
    if pic.is_file() {
        game.background = Some(pic);
    }
    if let Some(name) = dump.file_name().and_then(|n| n.to_str()) {
        let title = name.split("  ").next().unwrap_or(name).trim();
        if !title.is_empty() {
            game.name = title.to_owned();
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Runner
// ---------------------------------------------------------------------------------------------

/// Opens the preview or demo window.
pub fn run(options: Options, inbox: Option<crate::instance::Inbox>) -> eframe::Result {
    let mut viewport = egui::ViewportBuilder::default();
    if let Some((x, y)) = options.at {
        viewport = viewport.with_position([x, y]);
    }
    let mut native = eframe::NativeOptions {
        // Frameless like the launcher, so the demo shows its caption as users see it; the Mac
        // keeps its own frame.
        viewport: crate::macstyle::MacStyle::viewport(
            viewport
                .with_title("AnyPlay Launcher preview")
                .with_decorations(false)
                .with_inner_size([options.size.0, options.size.1])
                .with_resizable(options.scene.is_none()),
        ),
        ..Default::default()
    };
    crate::ui::restrict_backends(&mut native);
    crate::startup::mark("preview: window requested");
    eframe::run_native(
        "AnyPlay Launcher preview",
        native,
        Box::new(move |cc| {
            crate::startup::mark("window and GPU ready");
            let app = PreviewApp::new(cc, options, inbox);
            crate::startup::mark("app built");
            Ok(Box::new(app))
        }),
    )
}

struct PreviewApp {
    inner: LauncherApp,
    options: Options,
    frames: u32,
    requested: bool,
    script: std::collections::VecDeque<Step>,
    wait: u32,
}

impl PreviewApp {
    fn new(
        cc: &eframe::CreationContext<'_>,
        options: Options,
        inbox: Option<crate::instance::Inbox>,
    ) -> Self {
        let dir = crate::app::preview_dir();
        let games = match &options.showcase {
            Some(folder) => showcase_games(folder),
            None => make_games(&dir),
        };
        crate::startup::mark("preview games made");
        let demo = options.scene.is_none();
        let theme = options.theme.unwrap_or(match options.scene {
            Some(Scene::HomeTheme(1)) => Theme::Aurora,
            Some(Scene::HomeTheme(_)) => Theme::Sunset,
            _ => Theme::Cosmic,
        });
        let prefs = Preferences {
            language: if options.turkish {
                CoreLanguage::Turkish
            } else {
                CoreLanguage::English
            },
            onboarded: demo,
            theme: crate::screens::convert::theme_to_core(theme),
            // `--solid`: the Mac window as it is without the glass (screenshots).
            glass_window: !std::env::args().any(|a| a == "--solid"),
            ..Preferences::default()
        };
        let mut shell = Shell::headless(cc, theme);
        if demo {
            // The demo sounds like the launcher: cues and the home music.
            shell.sounds = crate::ui::sound::Sounds::new();
        }
        if options.pad {
            crate::ui::input::set_device(&cc.egui_ctx, crate::ui::input::Device::Gamepad);
        }
        shell.language = if options.turkish {
            crate::ui::i18n::Language::Turkish
        } else {
            crate::ui::i18n::Language::English
        };
        if !demo {
            let clock = BackgroundClock::starting_at(f64::from(STILL_TIME_FULL));
            shell.backdrop = Backdrop::with_clock(theme, clock);
        }
        shell.sounds.enabled = demo;
        let config = Config {
            prefs,
            paths: AppPaths::under(&dir.join("appdata")),
            persist: false,
            pipeline: Box::new({
                let mut pipeline = SimPipeline::new("0.1.0");
                pipeline.installed = vec!["0.1.2".to_owned()];
                if matches!(
                    options.scene,
                    Some(Scene::HomeGlibc | Scene::HomeGlibcLocked)
                ) {
                    pipeline.glibc = Some(anyps5_core::glibc::Shortfall {
                        needed: anyps5_core::glibc::Version::new(2, 38),
                        system: anyps5_core::glibc::Version::new(2, 35),
                    });
                }
                pipeline
            }),
            panels: Box::new({
                use crate::screens::settings::LauncherUpdate;
                let host = crate::app::panels::PanelHost::new(&cc.egui_ctx, true);
                match options.scene {
                    Some(Scene::HomeUpdate | Scene::HomeUpdateFocus | Scene::HomeUpdateLater) => {
                        host.showing_launcher_update(
                            &cc.egui_ctx,
                            LauncherUpdate::Ready("0.1.4".into()),
                        )
                    }
                    Some(Scene::HomeUpdateHolding) => host.showing_launcher_update(
                        &cc.egui_ctx,
                        LauncherUpdate::Holding("0.1.4".into()),
                    ),
                    _ => host,
                }
            }),
            scan_on_start: false,
        };
        let mut inner = LauncherApp::from_shell(shell, &cc.egui_ctx, config);
        inner.intro = options.intro;
        inner.offline = true;
        // The quick panel of the Home button only exists while the launcher is in front.
        inner.assume_focus = (demo && std::env::var_os("ANYPS5_DEMO_FOCUS").is_some())
            || matches!(
                options.scene,
                Some(Scene::HomeInGame | Scene::HomeInGameClose)
            );
        inner.freeze_backdrop = !demo;
        inner.glass_anyway = std::env::args().any(|a| a == "--glass-anyway");
        if demo {
            crate::ui::home_button::test_game_from_env(&inner.shell.home);
        }
        let mut games = games;
        if options.bright
            && let Some(path) = &games[1].background
        {
            let _ = bright_art(1280, 720).save(path);
        }
        if let Some(real) = &options.real_art {
            use_real_art(&mut games[1], real);
        }
        apply_scene(
            &mut inner.model,
            options.scene.unwrap_or(Scene::Home),
            games,
        );
        if inbox.is_some() {
            inner.attach_links(&cc.egui_ctx, inbox, &[]);
        }
        if demo {
            inner.model.edit_prefs(|p| p.sounds = true);
        }
        Self {
            inner,
            frames: 0,
            requested: false,
            script: options.script.iter().copied().collect(),
            wait: 0,
            options,
        }
    }
}

impl PreviewApp {
    /// Feeds the next scripted command, one every few frames.
    fn run_script(&mut self, ctx: &Context) {
        if self.wait > 0 {
            self.wait -= 1;
            return;
        }
        match self.script.pop_front() {
            Some(Step::Command(command)) => {
                self.inner.handle_event(ctx, InputEvent::Command(command));
                self.wait = 6;
            }
            Some(Step::Wait(frames)) => self.wait = frames,
            Some(Step::OpenSettings) => {
                self.inner.handle_event(ctx, InputEvent::OpenSettings);
                self.wait = 6;
            }
            None => {}
        }
    }
}

impl eframe::App for PreviewApp {
    #[cfg(target_os = "macos")]
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.inner.clear_color()
    }

    fn raw_input_hook(&mut self, _ctx: &Context, raw_input: &mut egui::RawInput) {
        if let Some((x, y)) = self.options.hover {
            raw_input
                .events
                .push(egui::Event::PointerMoved(egui::pos2(x, y)));
        }
    }

    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        if self.options.scene.is_none() && self.frames == 0 {
            crate::app::window_chrome::apply(frame);
        }
        let ctx = ui.ctx().clone();
        let glass_setting = self.inner.model.prefs.glass_window;
        self.inner.dress_window(&ctx, frame, glass_setting);
        if self.frames <= 3 {
            crate::startup::mark(&format!("frame {} begins", self.frames + 1));
        }
        // Like the launcher: fullscreen once the window is shown.
        if self.options.fullscreen && self.frames == 1 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
        }
        self.run_script(&ctx);
        self.inner.step(&ctx);
        // With `--pad` the pointer rests where `--hover` says, like a mouse nobody touches: the pad
        // stays the device in use (the injected move would hand it back) and the hover must light
        // nothing (see `ui::input::hovered`).
        if self.options.pad {
            crate::ui::input::set_device(&ctx, crate::ui::input::Device::Gamepad);
        }
        self.inner.paint(ui);
        self.inner.finish_frame(&ctx);
        self.frames += 1;
        // Scenes and screenshots draw every frame; the demo idles like the launcher does.
        if self.options.scene.is_some() || self.options.screenshot.is_some() {
            ctx.request_repaint();
        }
        if let Some(path) = self.options.screenshot.clone()
            && self.frames >= self.options.frames
            && !self.requested
        {
            self.requested = true;
            let close = ctx.clone();
            ctx.request_screenshot(move |image| {
                match save_png(&path, &image) {
                    Ok(()) => println!("screenshot written to {}", path.display()),
                    Err(error) => eprintln!("screenshot failed: {error}"),
                }
                close.send_viewport_cmd(egui::ViewportCommand::Close);
            });
        }
    }
}

fn save_png(path: &Path, image: &Arc<ColorImage>) -> Result<(), String> {
    let [w, h] = image.size;
    let mut bytes = Vec::with_capacity(w * h * 4);
    for pixel in &image.pixels {
        bytes.extend_from_slice(&pixel.to_array());
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    image::save_buffer(path, &bytes, w as u32, h as u32, image::ColorType::Rgba8)
        .map_err(|e| e.to_string())
}

/// Used by the tests: a context-free check that every scene name parses.
pub fn all_scenes_parse() -> bool {
    Scene::ALL_NAMES
        .iter()
        .all(|n| Scene::from_name(n).is_some())
}

/// The real home screen with the made-up library, for the gallery pages that show a panel over it:
/// the same code that paints the app paints the screen under the panel, so a screenshot of a panel
/// matches the launcher (the top bar, the clock, the hero) and cannot fall behind it.
pub struct HomeStand {
    model: crate::app::model::Model,
    textures: crate::app::textures::Textures,
    artwork: Vec<GameId>,
}

impl HomeStand {
    /// `covered` hides the focus ring, as it is hidden while a panel is open.
    pub fn new(ctx: &Context, prefs: Preferences, scene: Scene, covered: bool) -> Self {
        let dir = crate::app::preview_dir();
        let mut model =
            crate::app::model::Model::new(prefs, AppPaths::under(&dir.join("appdata")), false);
        apply_scene(&mut model, scene, make_games(&dir));
        if covered {
            model.overlay = Some(crate::app::overlay::Overlay::Library);
        }
        Self {
            model,
            textures: crate::app::textures::Textures::new(ctx),
            artwork: Vec::new(),
        }
    }

    /// Paints the game artwork and the home screen over what is already drawn (the backdrop).
    pub fn paint(&mut self, ui: &mut Ui, shell: &Shell) {
        let ctx = ui.ctx().clone();
        let screen = ctx.content_rect();
        self.model.tick(ctx.clock());
        crate::app::paint_artwork_layer(
            &ctx,
            screen,
            &self.model,
            &mut self.textures,
            &mut self.artwork,
            shell.backdrop.paused,
        );
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(screen));
        let mut cx = crate::screens::ScreenCx {
            model: &mut self.model,
            shell,
            textures: &mut self.textures,
            ctx: &ctx,
        };
        crate::screens::home::show(&mut child, &mut cx);
    }
}

#[allow(dead_code)]
fn _unused(_: &Context) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_names_parse() {
        assert!(all_scenes_parse());
        assert_eq!(Scene::from_name("home"), Some(Scene::Home));
        assert_eq!(Scene::from_name("nope"), None);
    }

    #[test]
    fn args_pick_scene_and_options() {
        let args = [
            "--gallery=home-trophies",
            "--screenshot=a.png",
            "--size=1100x650",
            "--tr",
        ]
        .map(String::from);
        let o = parse_args(args).unwrap();
        assert_eq!(o.scene, Some(Scene::HomeTrophies));
        assert_eq!(o.size, (1100.0, 650.0));
        assert!(o.turkish);
        // The foundation's own gallery pages are not ours.
        assert!(parse_args(["--gallery=panels".to_owned()]).is_none());
        assert!(parse_args(["--demo".to_owned()]).unwrap().scene.is_none());
    }

    #[test]
    fn generated_art_is_deterministic_and_varied() {
        let a = make_art(32, 32, 1);
        let b = make_art(32, 32, 1);
        let c = make_art(32, 32, 2);
        assert_eq!(a.as_raw(), b.as_raw());
        assert_ne!(a.as_raw(), c.as_raw());
    }
}
