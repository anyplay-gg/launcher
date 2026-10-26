//! The panel family of a game (docs/DESIGN.md 5.9): game settings, controls, advanced, saves, the
//! recommended settings, the toolkit pin, the Home-button panel and the confirmations are panels on
//! one frame. The frame is a full-screen area like Friends and Captures: the Back row on top, a
//! content column under it, the enter and exit of every full-screen area. Switching from one panel
//! to another keeps the area on screen and only swaps the content. A confirmation is the same area
//! without the Back row, its question centred higher up, so the choice stands alone.

use egui::{Context, Ui, UiBuilder, pos2, vec2};

use crate::screens::rows::{self, ListEvent, ListOptions, Row};
use crate::screens::{BACK_ROW, back_button, footer_hints};
use crate::ui::chrome;
use crate::ui::components::{Force, HintSet};
use crate::ui::focus::PanelNav;
use crate::ui::glass::GlassStyle;
use crate::ui::i18n::Language;
use crate::ui::input::{Command, InputEvent};
use crate::ui::text;
use crate::ui::theme::{color, pad, size, space, ty};

/// The animation identity all the panels of the family share.
const SIDE_ID: &str = "game-side-panel";

/// The content column: as wide as a row list reads well, centred in the area.
const COLUMN: f32 = 720.0;
/// Where a confirmation's question starts, as a share of the screen height.
const QUESTION_TOP: f32 = 0.28;
/// The leading of a confirmation's question, and of a quiet subtitle.
const QUESTION_LEADING: f32 = 1.45;
const SUBTITLE_LEADING: f32 = 1.2;

/// Open and closing state plus the focus model of one side panel.
#[derive(Clone, Copy, Debug)]
pub struct SideState {
    open: bool,
    closing: bool,
    pub nav: PanelNav,
}

impl Default for SideState {
    fn default() -> Self {
        Self {
            open: false,
            closing: false,
            nav: PanelNav::single(0),
        }
    }
}

impl SideState {
    /// Opens at `row` (game settings 0, controls 1, advanced 1, confirm 0).
    pub fn open_at(&mut self, row: usize) {
        self.open = true;
        self.closing = false;
        self.nav = PanelNav::single(row);
    }

    /// Leaves with the exit of a full-screen area.
    pub fn close(&mut self) {
        if self.open {
            self.open = false;
            self.closing = true;
        }
    }

    /// Leaves without an exit: another panel of the family takes over the frame.
    pub fn dismiss(&mut self) {
        self.open = false;
        self.closing = false;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// True while the panel is open or still leaving; the host keeps calling `show`.
    pub fn is_active(&self) -> bool {
        self.open || self.closing
    }

    /// Called when the exit has finished.
    pub fn finished(&mut self) {
        self.closing = false;
    }
}

/// The commands among a frame's input events.
pub fn commands(events: &[InputEvent]) -> Vec<Command> {
    events
        .iter()
        .filter_map(|e| match e {
            InputEvent::Command(c) => Some(*c),
            _ => None,
        })
        .collect()
}

/// What a panel frame produced: actions for the host and whether the settings it edits changed
/// (the host saves them).
#[derive(Debug)]
pub struct Output<A> {
    pub actions: Vec<A>,
    pub changed: bool,
}

impl<A> Default for Output<A> {
    fn default() -> Self {
        Self {
            actions: Vec::new(),
            changed: false,
        }
    }
}

/// What a panel shows and how.
pub struct SideView<'a> {
    pub title: &'a str,
    pub subtitle: Option<&'a str>,
    /// The subtitle is the question of a confirmation: set in body size and ink, not as a quiet line.
    pub question: bool,
    pub list_id: &'static str,
    pub nav: &'a PanelNav,
    pub accent: egui::Color32,
    pub language: Language,
    pub force: Option<(usize, Force)>,
}

/// What came back from drawing.
pub struct SideDrawn {
    /// The Back button was clicked: the same as the Back command.
    pub back_clicked: bool,
    pub events: Vec<ListEvent>,
}

/// Draws the panel (or its exit). `None` once it is fully closed.
pub fn draw(ctx: &Context, open: bool, view: &SideView<'_>, rows: &mut [Row]) -> Option<SideDrawn> {
    draw_with_header(ctx, open, view, rows, |_| {})
}

/// [`draw`] with something above the first row that scrolls with the rows: the profile page's
/// banner.
pub fn draw_with_header(
    ctx: &Context,
    open: bool,
    view: &SideView<'_>,
    rows: &mut [Row],
    header: impl FnOnce(&mut Ui),
) -> Option<SideDrawn> {
    let opts = ListOptions {
        accent: view.accent,
        language: view.language,
        focus: Some(view.nav.row),
        force: view.force,
    };
    panel(
        ctx,
        open,
        view.title,
        view.subtitle,
        view.question,
        view.language,
        |ui| rows::list_with_header(ui, view.list_id, rows, &opts, header),
    )
    .map(|(back_clicked, events)| SideDrawn {
        back_clicked,
        events,
    })
}

/// The frame: a full-screen area with the Back row, the title and subtitle on the content column's
/// edge and `contents` under them. Returns whether the Back button was clicked and what `contents`
/// returned; `None` once the area is fully closed.
fn panel<R>(
    ctx: &Context,
    open: bool,
    title: &str,
    subtitle: Option<&str>,
    question: bool,
    language: crate::ui::i18n::Language,
    contents: impl FnOnce(&mut Ui) -> R,
) -> Option<(bool, R)> {
    chrome::fullscreen_panel(ctx, SIDE_ID, open, GlassStyle::game_panel(), |ui, _| {
        let screen = ui.max_rect();
        let area = screen.shrink2(vec2(pad::TROPHIES.0, pad::TROPHIES.1));
        let painter = ui.painter().clone();
        let mut back = false;
        // The hint bar is the footer's, as everywhere (DESIGN.md 3.12); "Back" is left out where a
        // Back button is on screen.
        let top = if question {
            footer_hints(ui, screen, language, HintSet::BASIC);
            (screen.min.y + screen.height() * QUESTION_TOP).max(area.min.y)
        } else {
            footer_hints(ui, screen, language, HintSet::BASIC.without_back());
            back = back_button(ui, area, language);
            area.min.y + BACK_ROW
        };
        let width = COLUMN.min(area.width());
        let x = area.center().x - width * 0.5;
        // Title and subtitle start on the text of the rows below them (the icon's edge).
        let edge = x + size::LIST_INSET + size::ROW_PADDING_X;
        // Text wraps inside the rows' right edge, as far in as it starts from the left.
        let text_width = width - (edge - x) * 2.0;
        let title_galley = text::layout(
            &painter,
            ty::SIDE_PANEL_TITLE,
            title,
            color::text(),
            text_width,
        );
        let mut y = top;
        let title_h = title_galley.size().y;
        painter.galley(pos2(edge, y), title_galley, color::text());
        y += title_h + space::S;
        if let Some(subtitle) = subtitle {
            // Wraps (the confirmation sentence is long, and longer in Turkish). A
            // confirmation's question is what the player decides on: body size and ink, with
            // room between the lines. Any other subtitle is a quiet line.
            let (style, ink, leading) = if question {
                (ty::QUESTION, color::text_label(), QUESTION_LEADING)
            } else {
                (ty::SENTENCE, color::text_detail(), SUBTITLE_LEADING)
            };
            let galley = text::layout_font_pretty(
                &painter,
                style.font(),
                style.size * leading,
                style.tracking,
                subtitle,
                ink,
                text_width,
            );
            let height = galley.size().y;
            painter.galley(pos2(edge, y), galley, ink);
            y += height + space::S;
        }
        y += space::M;
        // The list runs down to the foot of the screen: the hint bar sits at the trailing
        // edge, clear of the column.
        let column =
            egui::Rect::from_min_max(pos2(x, y), pos2(x + width, screen.max.y - space::XL));
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(column)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        (back, contents(&mut child))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_close_and_switch() {
        let mut s = SideState::default();
        assert!(!s.is_active());
        s.open_at(1);
        assert!(s.is_open() && s.nav.row == 1 && s.nav.in_rows);
        s.close();
        assert!(!s.is_open() && s.is_active(), "still leaving");
        s.finished();
        assert!(!s.is_active());
        s.open_at(0);
        s.dismiss();
        assert!(!s.is_active(), "a switch does not leave");
    }

    #[test]
    fn only_commands_are_taken_from_events() {
        let events = [
            InputEvent::Scroll(Command::Left),
            InputEvent::Command(Command::Down),
            InputEvent::OpenSettings,
            InputEvent::Command(Command::Select),
        ];
        assert_eq!(commands(&events), vec![Command::Down, Command::Select]);
    }
}
