//! The Captures area in the panel host: reads the screenshots folder, matches every capture to a
//! game of the library, hands the screen its sections and pictures, and carries out what it asks
//! for (open, reveal, copy, delete). The screen itself never touches a file.

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyps5_core::captures::{self, Capture};
use anyps5_core::links::SettingsPage;
use anyps5_core::platform;

use super::PanelHost;
use crate::app::captures::images::{Images, Key, Kind as ImageKind};
use crate::app::captures::sections;
use crate::app::overlay::OverlayCx;
use crate::app::textures::Textures;
use crate::screens::captures::{CapturesAction, CapturesData, Media, Thumb};
use crate::screens::captures_strings::CapturesText;
use crate::screens::format;
use crate::ui::chrome::TileImage;
use crate::ui::i18n::Arg;
use crate::ui::icons::Icon;

/// The size game icons are asked for: the one the library grid has already loaded.
const GAME_ICON: u32 = 256;

/// The pictures for the screen, asked for from the cache.
struct HostMedia<'a> {
    images: &'a mut Images,
}

impl Media for HostMedia<'_> {
    fn thumb(&mut self, capture: &Capture) -> Thumb {
        let kind = match capture.kind {
            captures::Kind::Screenshot => ImageKind::Thumb,
            captures::Kind::Clip => ImageKind::Clip,
        };
        self.images.get(Key {
            path: capture.path.clone(),
            kind,
        })
    }

    fn full(&mut self, capture: &Capture) -> Thumb {
        let kind = match capture.kind {
            captures::Kind::Screenshot => ImageKind::Full,
            captures::Kind::Clip => ImageKind::ClipFull,
        };
        self.images.get(Key {
            path: capture.path.clone(),
            kind,
        })
    }

    fn duration(&mut self, capture: &Capture) -> Option<Duration> {
        self.images.duration(&capture.path)
    }
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

impl PanelHost {
    pub(super) fn captures_ui(&mut self, cx: &mut OverlayCx<'_>, wanted: bool) {
        let folder = cx.model.prefs.screenshots_dir(&cx.model.paths);
        if wanted && !self.captures.is_active() {
            self.captures.open();
            self.capture_service.begin(&folder);
        } else if !wanted && self.captures.is_active() {
            self.captures.close();
        }
        let events = self.take_events(wanted);
        if !self.captures.is_active() {
            if self.capture_service.images.bytes() > 0
                || !self.capture_service.captures().is_empty()
            {
                self.capture_service.release();
            }
            return;
        }
        // A folder chosen in the settings while this is open is read at once.
        if wanted && self.capture_service.folder() != folder.as_path() {
            self.capture_service.refresh(&folder);
        }
        self.capture_service.poll();
        self.capture_service.images.poll();
        let language = cx.model.language();
        let accent = cx.shell.accent_color();
        let now = now_seconds();
        let home = format::home_dir();
        let shown = format::abbreviate_path(&folder, home.as_deref());
        let filter = self.captures.filter();
        let games = &cx.model.games;
        let textures: &mut Textures = cx.textures;
        let (list, grouped, images, load) = self.capture_service.frame(games, filter);
        let secs = sections(
            games,
            grouped,
            CapturesText::OtherGame.text(language),
            |game| {
                game.icon.as_ref().and_then(|path| {
                    textures.get(path, GAME_ICON).map(|loaded| TileImage {
                        texture: loaded.id,
                        aspect: loaded.aspect,
                    })
                })
            },
        );
        let (actions, targets) = {
            let mut media = HostMedia { images };
            let mut data = CapturesData {
                load,
                folder: &shown,
                captures: list,
                sections: &secs,
                any: !list.is_empty(),
                clips_available: cfg!(windows),
                language,
                accent,
                now,
                utc_offset: format::local_offset_seconds(now * 1_000_000),
                media: &mut media,
            };
            let actions = self.captures.show(cx.shell, cx.ctx, &events, &mut data);
            // The captures the actions are about, taken before the list is let go of.
            let targets: Vec<Option<Capture>> = actions
                .iter()
                .map(|a| a.capture().and_then(|i| list.get(i).cloned()))
                .collect();
            (actions, targets)
        };
        self.capture_service.images.end_frame();
        if let Some(answer) = self.capture_service.images.copied() {
            match answer {
                Some(image) => {
                    cx.ctx.copy_image(image);
                    cx.model
                        .toast(CapturesText::Copied.text(language), Icon::CHECK_CIRCLE);
                }
                None => cx
                    .model
                    .toast(CapturesText::CopyFailed.text(language), Icon::WARNING),
            }
        }
        for (action, target) in actions.into_iter().zip(targets) {
            self.captures_action(cx, action, target, &folder);
        }
        // The slide-out is over: the textures and the list go; the next opening reads the folder again.
        if !self.captures.is_active() {
            self.capture_service.release();
        }
    }

    fn captures_action(
        &mut self,
        cx: &mut OverlayCx<'_>,
        action: CapturesAction,
        target: Option<Capture>,
        folder: &Path,
    ) {
        let language = cx.model.language();
        let model = &mut *cx.model;
        match action {
            CapturesAction::Close => model.dismiss_overlay(),
            CapturesAction::Refresh => self.capture_service.refresh(folder),
            CapturesAction::OpenClipsSettings => model.show_settings(Some(SettingsPage::Behavior)),
            CapturesAction::OpenFolderSettings => {
                model.show_settings(Some(SettingsPage::Advanced));
            }
            CapturesAction::OpenInPlayer(_) => {
                if let Some(c) = &target
                    && let Err(error) = platform::open_file(&c.path)
                {
                    tracing::warn!("captures: {} did not open: {error}", c.path.display());
                    model.toast(CapturesText::PlayerFailed.text(language), Icon::WARNING);
                }
            }
            CapturesAction::ShowInFolder(_) => {
                if let Some(c) = &target
                    && let Err(error) = platform::reveal(&c.path)
                {
                    tracing::warn!("captures: {} was not shown: {error}", c.path.display());
                    model.toast(CapturesText::FolderFailed.text(language), Icon::WARNING);
                }
            }
            CapturesAction::CopyImage(_) => {
                if let Some(c) = &target {
                    self.capture_service.images.start_copy(c.path.clone());
                }
            }
            CapturesAction::Delete(_) => {
                let Some(c) = &target else { return };
                let name = c.file_name();
                match captures::delete(&c.path) {
                    Ok(()) => {
                        tracing::info!("captures: deleted {}", c.path.display());
                        self.capture_service.remove(&c.path);
                        let text = CapturesText::Deleted.format(language, &[Arg::Str(&name)]);
                        model.toast(text, Icon::TRASH);
                    }
                    Err(error) => {
                        tracing::warn!("captures: {} was not deleted: {error}", c.path.display());
                        let text = CapturesText::DeleteFailed.format(language, &[Arg::Str(&name)]);
                        model.toast(text, Icon::WARNING);
                    }
                }
            }
        }
    }
}
