//! The Captures area (docs/DESIGN.md 9.10): a full-screen gallery of the screenshots and clips the
//! player took, one group per game with the game's own icon and name, newest first, and a view of
//! one capture with what can be done to it. Arrows, select and back do everything; a click does too.
//!
//! The screen knows nothing of files or the library. The host hands it the sections to show
//! ([`CapturesData`]) and a [`Media`] that answers with pictures as they load; the screen answers
//! with [`CapturesAction`]s. What it keeps is where the focus is and which step of the flow the
//! player is on: browsing, looking at one capture, or answering the question before a delete.

mod layout;
mod paint;
pub mod sample;
#[cfg(test)]
mod tests;

use std::time::Duration;

use anyps5_core::captures::{Capture, Filter, Kind};
use egui::{Color32, Context};

use crate::screens::captures_strings::CapturesText;
use crate::screens::side::commands;
use crate::ui::Shell;
use crate::ui::chrome::{TileImage, fullscreen_panel};
use crate::ui::glass::GlassStyle;
use crate::ui::i18n::Language;
use crate::ui::input::{Command, InputEvent};
use crate::ui::sound::Cue;

pub use layout::{Layout, step};

/// The filters in the order of their chips.
const FILTERS: [Filter; 3] = Filter::ALL;

/// A picture that may still be on its way.
#[derive(Clone, Copy, Debug)]
pub enum Thumb {
    /// Decoding; ask again next frame.
    Pending,
    Ready(TileImage),
    /// There is none and there will be none (a clip the system gives no picture for, a file that
    /// is not a picture): draw the plain tile.
    Unavailable,
}

/// Pictures and facts about captures, asked for as they come on screen.
pub trait Media {
    /// The small picture of a capture, for the grid.
    fn thumb(&mut self, capture: &Capture) -> Thumb;
    /// The big picture of a screenshot, for the full view.
    fn full(&mut self, capture: &Capture) -> Thumb;
    /// The length of a clip, once read.
    fn duration(&mut self, capture: &Capture) -> Option<Duration>;
}

/// One group of the grid: a game, and the captures of it (positions in [`CapturesData::captures`]).
#[derive(Clone, Debug)]
pub struct Section {
    pub title: String,
    pub icon: Option<TileImage>,
    pub items: Vec<usize>,
}

/// Where the list stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Load {
    Loading,
    Ready,
    /// The folder is not there.
    Missing,
    /// The folder is there and cannot be read.
    Failed,
}

pub struct CapturesData<'a> {
    pub load: Load,
    /// The folder, as the player reads it (`~/Pictures/AnyPS5`).
    pub folder: &'a str,
    pub captures: &'a [Capture],
    /// For the filter this screen asked for ([`CapturesScreen::filter`]).
    pub sections: &'a [Section],
    /// Whether the folder holds anything at all, whatever the filter hides.
    pub any: bool,
    /// Clips can be made on this system.
    pub clips_available: bool,
    pub language: Language,
    pub accent: Color32,
    /// Now, and the local offset from UTC, in seconds: for the dates.
    pub now: i64,
    pub utc_offset: i32,
    pub media: &'a mut dyn Media,
}

/// What the host has to do after a frame. Captures are named by their position in
/// [`CapturesData::captures`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapturesAction {
    Close,
    /// Read the folder again.
    Refresh,
    OpenClipsSettings,
    OpenFolderSettings,
    OpenInPlayer(usize),
    ShowInFolder(usize),
    CopyImage(usize),
    Delete(usize),
}

impl CapturesAction {
    /// The capture an action is about.
    pub fn capture(self) -> Option<usize> {
        match self {
            CapturesAction::OpenInPlayer(i)
            | CapturesAction::ShowInFolder(i)
            | CapturesAction::CopyImage(i)
            | CapturesAction::Delete(i) => Some(i),
            _ => None,
        }
    }
}

/// Where the focus is while browsing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Zone {
    Filter(usize),
    /// A tile, by its place.
    Grid(usize),
    /// The button of an empty, missing or failed state.
    Button,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ViewFocus {
    /// The picture: left and right step through the captures.
    Media,
    Action(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Browse,
    View {
        place: usize,
        focus: ViewFocus,
    },
    /// The question before a delete; the button is 0 for Cancel, 1 for Delete.
    Confirm {
        place: usize,
        button: usize,
    },
}

/// The actions of the full view, in the order of the buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ViewAction {
    OpenInPlayer,
    CopyImage,
    ShowInFolder,
    Delete,
}

impl ViewAction {
    /// The buttons of the view; a file the launcher did not name has no Delete.
    pub(crate) fn list(kind: Kind, deletable: bool) -> &'static [ViewAction] {
        match (kind, deletable) {
            (Kind::Clip, true) => &[
                ViewAction::OpenInPlayer,
                ViewAction::ShowInFolder,
                ViewAction::Delete,
            ],
            (Kind::Clip, false) => &[ViewAction::OpenInPlayer, ViewAction::ShowInFolder],
            (Kind::Screenshot, true) => &[
                ViewAction::CopyImage,
                ViewAction::ShowInFolder,
                ViewAction::Delete,
            ],
            (Kind::Screenshot, false) => &[ViewAction::CopyImage, ViewAction::ShowInFolder],
        }
    }
}

/// What the focus model needs to know of the current frame.
pub(crate) struct Nav<'a> {
    pub layout: &'a Layout,
    /// The capture at every place.
    pub flat: &'a [usize],
    pub kinds: &'a [Kind],
    /// Whether the capture at a place may be deleted.
    pub deletable: &'a [bool],
    /// What the state without tiles offers: a button, or nothing.
    pub button: Option<ButtonKind>,
}

/// What the button of a state without tiles does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ButtonKind {
    /// Lift the filter.
    ShowAll,
    Action(CapturesAction),
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct Outcome {
    pub cues: Vec<Cue>,
    pub actions: Vec<CapturesAction>,
}

/// The screen.
pub struct CapturesScreen {
    open: bool,
    closing: bool,
    filter: Filter,
    zone: Zone,
    mode: Mode,
    /// The tiles of one row on the last frame, so the arrows move by the rows the player sees.
    columns: usize,
    /// ... and how wide they were.
    tile_w: f32,
}

impl Default for CapturesScreen {
    fn default() -> Self {
        Self {
            open: false,
            closing: false,
            filter: Filter::All,
            zone: Zone::Filter(0),
            mode: Mode::Browse,
            columns: 4,
            tile_w: 300.0,
        }
    }
}

impl CapturesScreen {
    /// Opens on the filter All, the focus on the first capture.
    pub fn open(&mut self) {
        *self = CapturesScreen {
            open: true,
            zone: Zone::Grid(0),
            ..CapturesScreen::default()
        };
    }

    /// Opens on a capture's full view (previews).
    pub fn open_view(&mut self, place: usize, focus_action: Option<usize>) {
        self.open();
        self.zone = Zone::Grid(place);
        self.mode = Mode::View {
            place,
            focus: focus_action.map_or(ViewFocus::Media, ViewFocus::Action),
        };
    }

    /// Puts the focus on a capture (previews).
    pub fn focus_place(&mut self, place: usize) {
        self.zone = Zone::Grid(place);
    }

    /// Opens with the question before a delete (previews).
    pub fn open_confirm(&mut self, place: usize) {
        self.open();
        self.zone = Zone::Grid(place);
        self.mode = Mode::Confirm { place, button: 0 };
    }

    pub fn set_filter(&mut self, filter: Filter) {
        self.filter = filter;
    }

    pub fn close(&mut self) {
        if self.open {
            self.open = false;
            self.closing = true;
        }
    }

    pub fn is_active(&self) -> bool {
        self.open || self.closing
    }

    /// The filter the host builds the sections for.
    pub fn filter(&self) -> Filter {
        self.filter
    }

    /// One frame. `events` are the commands of the frame.
    pub fn show(
        &mut self,
        shell: &mut Shell,
        ctx: &Context,
        events: &[InputEvent],
        data: &mut CapturesData<'_>,
    ) -> Vec<CapturesAction> {
        let mut actions = Vec::new();
        if !self.is_active() {
            return actions;
        }
        let flat: Vec<usize> = data
            .sections
            .iter()
            .flat_map(|s| s.items.iter().copied())
            .collect();
        let kinds: Vec<Kind> = flat.iter().map(|&i| data.captures[i].kind).collect();
        let deletable: Vec<bool> = flat.iter().map(|&i| data.captures[i].deletable()).collect();
        let sizes: Vec<usize> = data.sections.iter().map(|s| s.items.len()).collect();
        let layout = Layout::build(&sizes, self.columns, paint::metrics(self.tile_w));
        let button = paint::button_for(data).map(|b| b.kind);
        self.settle(&flat, data.load, button.is_some());
        if self.open {
            let nav = Nav {
                layout: &layout,
                flat: &flat,
                kinds: &kinds,
                deletable: &deletable,
                button,
            };
            for command in commands(events) {
                let outcome = self.handle(command, &nav);
                for cue in outcome.cues {
                    shell.play(cue);
                }
                actions.extend(outcome.actions);
                if !self.open {
                    break;
                }
            }
        }
        let open = self.open;
        let mut pressed = paint::Pressed::default();
        let drawn = fullscreen_panel(
            ctx,
            "captures",
            open,
            GlassStyle::fullscreen_panel(),
            |ui, _| paint::draw(ui, self, data, &flat, &mut pressed),
        );
        if drawn.is_none() {
            self.closing = false;
        }
        if open {
            let nav = Nav {
                layout: &layout,
                flat: &flat,
                kinds: &kinds,
                deletable: &deletable,
                button: paint::button_for(data).map(|b| b.kind),
            };
            let outcome = self.pressed(pressed, &nav);
            for cue in outcome.cues {
                shell.play(cue);
            }
            actions.extend(outcome.actions);
        }
        actions
    }

    /// Keeps the focus on something that is there: the list shrank, or the state changed.
    pub(crate) fn settle(&mut self, flat: &[usize], load: Load, has_button: bool) {
        let total = flat.len();
        let browsing_items = load == Load::Ready && total > 0;
        match self.mode {
            Mode::View { .. } | Mode::Confirm { .. } if total == 0 => {
                self.mode = Mode::Browse;
                self.zone = Zone::Filter(self.filter_index());
            }
            Mode::View { place, focus } => {
                self.mode = Mode::View {
                    place: place.min(total - 1),
                    focus,
                };
            }
            Mode::Confirm { place, button } => {
                self.mode = Mode::Confirm {
                    place: place.min(total - 1),
                    button,
                };
            }
            Mode::Browse => {}
        }
        self.zone = match self.zone {
            Zone::Grid(p) if browsing_items => Zone::Grid(p.min(total - 1)),
            // A state with one button opens on it: it is the one thing to do there.
            Zone::Grid(_) | Zone::Button if has_button => Zone::Button,
            Zone::Grid(_) | Zone::Button => Zone::Filter(self.filter_index()),
            zone => zone,
        };
    }

    fn filter_index(&self) -> usize {
        FILTERS.iter().position(|f| *f == self.filter).unwrap_or(0)
    }

    /// One command.
    pub(crate) fn handle(&mut self, command: Command, nav: &Nav<'_>) -> Outcome {
        let mut out = Outcome::default();
        let total = nav.flat.len();
        match self.mode {
            Mode::Browse => match self.zone {
                Zone::Filter(i) => match command {
                    Command::Left | Command::Right => {
                        let next = if command == Command::Left {
                            i.checked_sub(1)
                        } else {
                            (i + 1 < FILTERS.len()).then_some(i + 1)
                        };
                        match next {
                            Some(next) => {
                                self.zone = Zone::Filter(next);
                                out.cues.push(Cue::Move);
                            }
                            None => out.cues.push(Cue::Bump),
                        }
                    }
                    Command::Down if total > 0 => {
                        self.zone = Zone::Grid(0);
                        out.cues.push(Cue::Move);
                    }
                    Command::Down if nav.button.is_some() => {
                        self.zone = Zone::Button;
                        out.cues.push(Cue::Move);
                    }
                    Command::Up | Command::Down => out.cues.push(Cue::Bump),
                    Command::Select => {
                        self.choose_filter(FILTERS[i], &mut out);
                    }
                    Command::Back | Command::Options => self.leave(&mut out),
                },
                Zone::Grid(place) => match command {
                    Command::Left | Command::Right | Command::Up | Command::Down => {
                        match step(nav.layout, place, total, command) {
                            Some(next) => {
                                self.zone = Zone::Grid(next);
                                out.cues.push(Cue::Move);
                            }
                            None if command == Command::Up => {
                                self.zone = Zone::Filter(self.filter_index());
                                out.cues.push(Cue::Move);
                            }
                            None => out.cues.push(Cue::Bump),
                        }
                    }
                    Command::Select => {
                        self.mode = Mode::View {
                            place,
                            focus: ViewFocus::Media,
                        };
                        out.cues.push(Cue::Open);
                    }
                    Command::Back | Command::Options => self.leave(&mut out),
                },
                Zone::Button => match command {
                    Command::Up => {
                        self.zone = Zone::Filter(self.filter_index());
                        out.cues.push(Cue::Move);
                    }
                    Command::Select => self.press_button(nav.button, &mut out),
                    Command::Back | Command::Options => self.leave(&mut out),
                    _ => out.cues.push(Cue::Bump),
                },
            },
            Mode::View { place, focus } => {
                let kind = nav.kinds[place.min(total.saturating_sub(1))];
                let buttons =
                    ViewAction::list(kind, nav.deletable[place.min(total.saturating_sub(1))]);
                match (focus, command) {
                    (_, Command::Back | Command::Options) => {
                        self.mode = Mode::Browse;
                        self.zone = Zone::Grid(place);
                        out.cues.push(Cue::Close);
                    }
                    (ViewFocus::Media, Command::Left | Command::Right) => {
                        let next = if command == Command::Left {
                            place.checked_sub(1)
                        } else {
                            (place + 1 < total).then_some(place + 1)
                        };
                        match next {
                            Some(next) => {
                                self.mode = Mode::View { place: next, focus };
                                self.zone = Zone::Grid(next);
                                out.cues.push(Cue::Move);
                            }
                            None => out.cues.push(Cue::Bump),
                        }
                    }
                    (ViewFocus::Media, Command::Down) => {
                        self.mode = Mode::View {
                            place,
                            focus: ViewFocus::Action(0),
                        };
                        out.cues.push(Cue::Move);
                    }
                    (ViewFocus::Media, Command::Select) if kind == Kind::Clip => {
                        out.cues.push(Cue::Select);
                        out.actions
                            .push(CapturesAction::OpenInPlayer(nav.flat[place]));
                    }
                    (ViewFocus::Media, Command::Select) => {
                        self.mode = Mode::View {
                            place,
                            focus: ViewFocus::Action(0),
                        };
                        out.cues.push(Cue::Move);
                    }
                    (ViewFocus::Media, _) => out.cues.push(Cue::Bump),
                    (ViewFocus::Action(i), Command::Left | Command::Right) => {
                        let next = if command == Command::Left {
                            i.checked_sub(1)
                        } else {
                            (i + 1 < buttons.len()).then_some(i + 1)
                        };
                        match next {
                            Some(next) => {
                                self.mode = Mode::View {
                                    place,
                                    focus: ViewFocus::Action(next),
                                };
                                out.cues.push(Cue::Move);
                            }
                            None => out.cues.push(Cue::Bump),
                        }
                    }
                    (ViewFocus::Action(_), Command::Up) => {
                        self.mode = Mode::View {
                            place,
                            focus: ViewFocus::Media,
                        };
                        out.cues.push(Cue::Move);
                    }
                    (ViewFocus::Action(i), Command::Select) => {
                        self.run(buttons[i.min(buttons.len() - 1)], place, nav, &mut out);
                    }
                    (ViewFocus::Action(_), Command::Down) => out.cues.push(Cue::Bump),
                }
            }
            Mode::Confirm { place, button } => match command {
                Command::Left | Command::Right => {
                    // Delete stands left, Cancel right (screens::dialog).
                    let next = if command == Command::Left { 1 } else { 0 };
                    if next == button {
                        out.cues.push(Cue::Bump);
                    } else {
                        self.mode = Mode::Confirm {
                            place,
                            button: next,
                        };
                        out.cues.push(Cue::Move);
                    }
                }
                Command::Select if button == 1 => {
                    out.cues.push(Cue::Select);
                    out.actions.push(CapturesAction::Delete(nav.flat[place]));
                    // The capture is gone: the view stays on whatever comes to its place.
                    self.mode = Mode::View {
                        place,
                        focus: ViewFocus::Media,
                    };
                }
                Command::Select | Command::Back | Command::Options => {
                    self.mode = Mode::View {
                        place,
                        focus: ViewFocus::Action(
                            ViewAction::list(nav.kinds[place], nav.deletable[place]).len() - 1,
                        ),
                    };
                    out.cues.push(Cue::Back);
                }
                _ => out.cues.push(Cue::Bump),
            },
        }
        out
    }

    fn press_button(&mut self, button: Option<ButtonKind>, out: &mut Outcome) {
        match button {
            Some(ButtonKind::Action(action)) => {
                out.cues.push(Cue::Select);
                out.actions.push(action);
            }
            Some(ButtonKind::ShowAll) => {
                self.filter = Filter::All;
                self.zone = Zone::Filter(0);
                out.cues.push(Cue::Select);
            }
            None => out.cues.push(Cue::Bump),
        }
    }

    fn leave(&mut self, out: &mut Outcome) {
        self.close();
        out.cues.push(Cue::Close);
        out.actions.push(CapturesAction::Close);
    }

    fn choose_filter(&mut self, filter: Filter, out: &mut Outcome) {
        if self.filter != filter {
            self.filter = filter;
            out.cues.push(Cue::Select);
        }
    }

    fn run(&mut self, action: ViewAction, place: usize, nav: &Nav<'_>, out: &mut Outcome) {
        let capture = nav.flat[place];
        match action {
            ViewAction::OpenInPlayer => out.actions.push(CapturesAction::OpenInPlayer(capture)),
            ViewAction::CopyImage => out.actions.push(CapturesAction::CopyImage(capture)),
            ViewAction::ShowInFolder => out.actions.push(CapturesAction::ShowInFolder(capture)),
            ViewAction::Delete => {
                // Opens on Cancel: Enter must never delete by accident.
                self.mode = Mode::Confirm { place, button: 0 };
                out.cues.push(Cue::Open);
                return;
            }
        }
        out.cues.push(Cue::Select);
    }

    /// What the pointer asked for in the frame that was just drawn.
    fn pressed(&mut self, pressed: paint::Pressed, nav: &Nav<'_>) -> Outcome {
        let mut out = Outcome::default();
        let total = nav.flat.len();
        if let Some(i) = pressed.filter {
            self.zone = Zone::Filter(i);
            self.choose_filter(FILTERS[i], &mut out);
        }
        if let Some(place) = pressed.tile.filter(|p| *p < total) {
            self.zone = Zone::Grid(place);
            self.mode = Mode::View {
                place,
                focus: ViewFocus::Media,
            };
            out.cues.push(Cue::Open);
        }
        if pressed.button {
            self.press_button(nav.button, &mut out);
        }
        if pressed.back {
            match self.mode {
                Mode::Browse => self.leave(&mut out),
                Mode::View { place, .. } => {
                    self.mode = Mode::Browse;
                    self.zone = Zone::Grid(place);
                    out.cues.push(Cue::Close);
                }
                // A click beside the card is Cancel: back to where Delete was pressed.
                Mode::Confirm { place, .. } => {
                    self.mode = Mode::View {
                        place,
                        focus: ViewFocus::Action(
                            ViewAction::list(nav.kinds[place], nav.deletable[place]).len() - 1,
                        ),
                    };
                    out.cues.push(Cue::Back);
                }
            }
        }
        if let Mode::View { place, focus } = self.mode {
            if let Some(delta) = pressed.step {
                let next = if delta < 0 {
                    place.checked_sub(1)
                } else {
                    (place + 1 < total).then_some(place + 1)
                };
                if let Some(next) = next {
                    self.mode = Mode::View { place: next, focus };
                    self.zone = Zone::Grid(next);
                    out.cues.push(Cue::Move);
                }
            }
            if let Some(i) = pressed.action {
                let buttons = ViewAction::list(nav.kinds[place], nav.deletable[place]);
                if let Some(&action) = buttons.get(i) {
                    self.mode = Mode::View {
                        place,
                        focus: ViewFocus::Action(i),
                    };
                    self.run(action, place, nav, &mut out);
                }
            }
            if pressed.play && nav.kinds[place] == Kind::Clip {
                out.cues.push(Cue::Select);
                out.actions
                    .push(CapturesAction::OpenInPlayer(nav.flat[place]));
            }
        }
        if let Mode::Confirm { place, .. } = self.mode
            && let Some(button) = pressed.confirm
        {
            self.mode = Mode::Confirm { place, button };
            let command = Command::Select;
            let again = self.handle(command, nav);
            out.cues.extend(again.cues);
            out.actions.extend(again.actions);
        }
        out
    }

    /// What the full view or the question shows, for the previews and the tests.
    #[cfg(test)]
    pub(crate) fn mode(&self) -> Mode {
        self.mode
    }

    #[cfg(test)]
    pub(crate) fn zone(&self) -> Zone {
        self.zone
    }
}

/// The words of a filter chip.
pub fn filter_text(filter: Filter) -> CapturesText {
    match filter {
        Filter::All => CapturesText::FilterAll,
        Filter::Screenshots => CapturesText::FilterScreenshots,
        Filter::Clips => CapturesText::FilterClips,
    }
}

/// `m:ss`, or `h:mm:ss` from an hour on.
pub fn duration_text(duration: Duration) -> String {
    let seconds = duration.as_secs_f64().round() as u64;
    let (h, m, s) = (seconds / 3600, seconds % 3600 / 60, seconds % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}
