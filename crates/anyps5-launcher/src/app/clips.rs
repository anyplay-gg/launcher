//! Clips while a game runs (docs/DESIGN.md 9.12): starts the recorder for the running game when
//! Clips is on, keeps the hotkey registered, and turns the recorder's answers into toasts.
//!
//! Nothing runs when Clips is off or no game runs: no worker, no hotkey. The recorder is the
//! share worker in its clips mode (network spec/clips.md); it keeps the pictures, this module
//! only asks it to save and says what came of it.

use std::sync::Arc;

use anyps5_core::clips::{self, HotkeyListener, Recorder, WorkerEvent};
use anyps5_core::paths::GameId;

use super::model::Model;
use super::pipeline::Pipeline;
use crate::screens::clips_strings::ClipsText;
use crate::ui::i18n::Arg;
use crate::ui::icons::Icon;
use crate::ui::sound::Cue;

/// The recorder of one running game.
struct Active {
    game: GameId,
    seconds: u32,
    recorder: Recorder,
    /// The worker reported `ready`: pictures are being kept.
    ready: bool,
}

pub struct ClipsService {
    wake: Arc<dyn Fn() + Send + Sync>,
    /// Previews and tests never start a process or take a hotkey.
    simulated: bool,
    active: Option<Active>,
    /// The game whose recorder ended (no encoder, no window): it is not started again for it.
    gave_up: Option<GameId>,
    hotkey: Option<(String, HotkeyListener)>,
    /// The key the player was last told is taken, so that the toast comes once.
    told_taken: Option<String>,
}

impl ClipsService {
    pub fn new(wake: Arc<dyn Fn() + Send + Sync>, simulated: bool) -> Self {
        ClipsService {
            wake,
            simulated,
            active: None,
            gave_up: None,
            hotkey: None,
            told_taken: None,
        }
    }

    /// Once per frame: starts or ends the recorder and the hotkey as the game and the
    /// preferences ask, and takes in what they report.
    pub fn frame(&mut self, model: &mut Model, pipeline: &dyn Pipeline) {
        let seconds = clips::length_or_off(model.prefs.clips_seconds);
        let running = model.running.clone();
        let wanted = !self.simulated && cfg!(windows) && seconds > 0;
        let Some(game) = running.filter(|_| wanted) else {
            self.active = None;
            self.hotkey = None;
            self.gave_up = None;
            return;
        };
        if self
            .active
            .as_ref()
            .is_some_and(|a| a.game != game || a.seconds != seconds)
        {
            self.active = None;
        }
        if self.gave_up.as_ref().is_some_and(|g| *g != game) {
            self.gave_up = None;
        }
        if self.active.is_none() && self.gave_up.as_ref() != Some(&game) {
            self.start(model, pipeline, &game, seconds);
        }
        self.sync_hotkey(model);
        self.drain(model);
    }

    fn start(&mut self, model: &mut Model, pipeline: &dyn Pipeline, game: &GameId, seconds: u32) {
        // The game has no process yet while it starts; the next frame asks again.
        let Some(target) = pipeline.game_target(game).filter(|t| !t.pids.is_empty()) else {
            return;
        };
        let Some(worker) = super::online::share_worker_path(model) else {
            self.gave_up = Some(game.clone());
            let text = ClipsText::NoWorker.text(model.language());
            model.toast_failure(text);
            return;
        };
        let title = model.game(game).map(|g| g.name.clone()).unwrap_or_default();
        let wake = self.wake.clone();
        match Recorder::start(
            &worker,
            seconds,
            target.window,
            &target.pids,
            &title,
            move || wake(),
        ) {
            Ok(recorder) => {
                tracing::info!("clips: recording {seconds} s of {title}");
                self.active = Some(Active {
                    game: game.clone(),
                    seconds,
                    recorder,
                    ready: false,
                });
            }
            Err(error) => {
                tracing::warn!("clips: the recorder could not start: {error}");
                self.gave_up = Some(game.clone());
                let text = ClipsText::NoWorker.text(model.language());
                model.toast(text, Icon::WARNING);
            }
        }
    }

    /// The hotkey follows the preference: registered while a game runs with Clips on.
    fn sync_hotkey(&mut self, model: &mut Model) {
        let wanted = clips::hotkey_or_default(model.prefs.clips_hotkey.as_deref());
        let wanted_text = wanted.to_string();
        if self
            .hotkey
            .as_ref()
            .is_some_and(|(held, _)| *held == wanted_text)
        {
            return;
        }
        self.hotkey = None;
        if self.told_taken.as_deref() == Some(&wanted_text) {
            return;
        }
        let wake = self.wake.clone();
        match HotkeyListener::start(&wanted, move || wake()) {
            Ok(listener) => self.hotkey = Some((wanted_text, listener)),
            Err(error) => {
                tracing::warn!("clips: the key {wanted_text} could not be taken: {error}");
                let text = ClipsText::KeyTaken.format(model.language(), &[Arg::Str(&wanted_text)]);
                model.toast(text, Icon::WARNING);
                self.told_taken = Some(wanted_text);
            }
        }
    }

    fn drain(&mut self, model: &mut Model) {
        let pressed = self
            .hotkey
            .as_ref()
            .is_some_and(|(_, l)| l.presses.try_iter().count() > 0);
        if pressed {
            self.save(model);
        }
        let mut ended = None;
        if let Some(active) = &mut self.active {
            while let Ok(event) = active.recorder.events.try_recv() {
                match event {
                    WorkerEvent::Ready { .. } => active.ready = true,
                    WorkerEvent::Saved { path, seconds, .. } => {
                        tracing::info!("clips: saved {path} ({seconds:.1} s)");
                        let text = ClipsText::Saved.text(model.language());
                        model.toast(text, Icon::CAMERA);
                        model.sound(Cue::Unlocked);
                    }
                    WorkerEvent::SaveFailed { reason } => {
                        tracing::warn!("clips: not saved: {reason}");
                        let text = if reason == "empty" {
                            ClipsText::NothingYet
                        } else {
                            ClipsText::SaveFailed
                        };
                        // "Nothing yet" is a notice and fades; a save that failed stays.
                        if matches!(text, ClipsText::SaveFailed) {
                            model.toast_failure(text.text(model.language()));
                        } else {
                            model.toast(text.text(model.language()), Icon::WARNING);
                        }
                    }
                    WorkerEvent::Ended { reason } => ended = Some(reason),
                }
            }
            if ended.is_none() && !active.recorder.alive() {
                ended = Some("exited".to_owned());
            }
        }
        if let Some(reason) = ended {
            self.ended(model, &reason);
        }
    }

    /// The recorder is gone: say why, once, and do not start it again for this game.
    fn ended(&mut self, model: &mut Model, reason: &str) {
        let game = self.active.take().map(|a| a.game);
        tracing::info!("clips: the recorder ended: {reason}");
        let text = match reason {
            "no_encoder" => ClipsText::NoEncoder,
            "no_window" => ClipsText::NoWindow,
            "quit" => return,
            _ => ClipsText::Stopped,
        };
        self.gave_up = game;
        model.toast_failure(text.text(model.language()));
    }

    /// The hotkey, the controller's Create button or the phone's: save the last seconds.
    pub fn save(&mut self, model: &mut Model) {
        let language = model.language();
        let Some(active) = self.active.as_mut() else {
            // Create was pressed in a game with Clips off: say how to turn it on, once the game
            // really runs (the button also means other things before one does).
            if model.running.is_some() && model.prefs.clips_seconds == 0 && cfg!(windows) {
                model.toast(ClipsText::IsOff.text(language), Icon::CAMERA);
            }
            return;
        };
        if !active.ready {
            model.toast(ClipsText::NothingYet.text(language), Icon::WARNING);
            return;
        }
        let folder = model.prefs.screenshots_dir(&model.paths);
        let title = model
            .game(&active.game)
            .map(|g| g.name.clone())
            .unwrap_or_default();
        let path = clips::clip_path(&folder, &title, &stamp());
        if let Err(error) = active.recorder.save(&path) {
            tracing::warn!("clips: the save command failed: {error}");
            model.toast_failure(ClipsText::SaveFailed.text(language));
        }
    }
}

/// The local time as it goes into a file name: `2026-10-25 18-04-31`.
fn stamp() -> String {
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    let format = time::macros::format_description!("[year]-[month]-[day] [hour]-[minute]-[second]");
    now.format(&format).unwrap_or_else(|_| "clip".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stamp_is_a_safe_file_name_part() {
        let stamp = stamp();
        assert_eq!(stamp.len(), 19, "{stamp}");
        assert!(!stamp.contains([':', '/', '\\']));
    }
}
