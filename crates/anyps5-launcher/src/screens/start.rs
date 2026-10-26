//! The launch splash, "the start screen with a timer" (docs/DESIGN.md 5.5): from the press of Play
//! until the game's first window shows. The game's icon breathes in the accent colour, the elapsed
//! `m:ss` counts up, an endless sweep says the launcher is waiting, and the Cancel button (or Esc,
//! or the pad's back) stops the start. While it is up the host swallows all other input.

use egui::text::LayoutJob;
use egui::{Align, Align2, Color32, Context, Layout, Rect, Ui, UiBuilder, pos2, vec2};

use crate::screens::format::elapsed_text;
use crate::screens::side::commands;
use crate::ui::Shell;
use crate::ui::anim::{self, AnimExt, lerp};
use crate::ui::chrome::{TileImage, TileStyle, fullscreen_panel, paint_tile};
use crate::ui::components::{ConsoleButton, ControlSize, IndeterminateLine, paint_shadow};
use crate::ui::glass::GlassStyle;
use crate::ui::i18n::{Key, Language};
use crate::ui::icons::Icon;
use crate::ui::input::{Command, InputEvent};
use crate::ui::sound::Cue;
use crate::ui::text;
use crate::ui::theme::{self, color, fade, radius, size, ty, white};

/// What the host has to do after a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartAction {
    /// Esc or the pad's back: kill the game process and close the splash.
    Cancel,
}

/// The game being started, borrowed for one frame.
pub struct StartData<'a> {
    pub game_name: &'a str,
    pub icon: Option<TileImage>,
    pub language: Language,
    pub accent: Color32,
    /// Animations: Reduced stops the breathing glow.
    pub reduced_motion: bool,
}

/// The splash.
#[derive(Default)]
pub struct StartScreen {
    open: bool,
    closing: bool,
    since: f64,
}

impl StartScreen {
    /// Shows the splash; the timer starts at 0:00 now.
    pub fn open(&mut self, ctx: &Context) {
        self.open_at(ctx.clock());
    }

    /// Shows the splash with the timer starting at a given clock time (previews).
    pub fn open_at(&mut self, since: f64) {
        self.open = true;
        self.closing = false;
        self.since = since;
    }

    /// The game's window appeared (or the start failed): the splash leaves.
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
        data: &StartData<'_>,
    ) -> Vec<StartAction> {
        let mut actions = Vec::new();
        if !self.is_active() {
            return actions;
        }
        // Only back counts; everything else is the host's to swallow.
        if self.open && commands(events).contains(&Command::Back) {
            shell.play(Cue::Back);
            self.close();
            actions.push(StartAction::Cancel);
        }
        let since = self.since;
        let drawn = fullscreen_panel(
            ctx,
            "launch-splash",
            self.open,
            GlassStyle::splash(),
            |ui, _| draw(ui, data, since),
        );
        match drawn {
            None => self.closing = false,
            // The pointer path: the same stop as Esc.
            Some(true) if self.open => {
                self.close();
                actions.push(StartAction::Cancel);
            }
            Some(_) => {}
        }
        actions
    }
}

/// A centred, wrapped paragraph.
fn centred(
    painter: &egui::Painter,
    style: theme::TextStyle,
    text: &str,
    color: Color32,
    width: f32,
    max_rows: usize,
) -> std::sync::Arc<egui::Galley> {
    let mut job = LayoutJob::simple(text.to_owned(), style.font(), color, width);
    job.halign = Align::Center;
    job.wrap.max_rows = max_rows;
    job.sections[0].format.line_height = Some(style.line_height());
    painter.layout_job(job)
}

/// The status block under the name, and the room the name has either side of it.
const COLUMN_WIDTH: f32 = 380.0;
const TITLE_MARGIN: f32 = 160.0;
const TITLE_MIN: f32 = 200.0;
const TITLE_MAX: f32 = 900.0;
/// Between the tile, the name and the status block; between the parts of the status block.
const COLUMN_GAP: f32 = 24.0;
const BLOCK_GAP: f32 = 12.0;
/// Between the Cancel button and the bottom edge, and its width.
const CANCEL_BOTTOM: f32 = 40.0;
const CANCEL_WIDTH: f32 = 240.0;

/// Paints the splash; true when the Cancel button was pressed.
fn draw(ui: &mut Ui, data: &StartData<'_>, since: f64) -> bool {
    let ctx = ui.ctx().clone();
    let painter = ui.painter().clone();
    let screen = ui.max_rect();
    let language = data.language;
    let time = ctx.clock();

    // The column is centred as a whole: tile, name, the 380-wide status block.
    let tile_side = size::SPLASH_TILE;
    let column_w = COLUMN_WIDTH;
    let title_w = (screen.width() - TITLE_MARGIN).clamp(TITLE_MIN, TITLE_MAX);
    let title = centred(
        &painter,
        ty::SPLASH_TITLE,
        data.game_name,
        Color32::WHITE,
        title_w,
        2,
    );
    let info = centred(
        &painter,
        ty::DETAIL_LARGE,
        language.text(Key::StartingText),
        color::text_detail(),
        column_w + 80.0,
        usize::MAX,
    );
    let status_h = ty::CHOICE_VALUE.line_height();
    let block_h = status_h + BLOCK_GAP + size::PROGRESS_HEIGHT + BLOCK_GAP + info.size().y;
    let total = tile_side + COLUMN_GAP + title.size().y + COLUMN_GAP + block_h;
    let top = screen.center().y - total * 0.5;
    let cx = screen.center().x;

    // The tile breathes: 0.985 to 1.015, the glow 0.2 to 0.55 in alpha and 18 to 40 in radius.
    let breathe = if data.reduced_motion {
        0.0
    } else {
        ctx.request_repaint();
        anim::breathing(time)
    };
    let side = tile_side * lerp(0.985, 1.015, breathe);
    let tile = Rect::from_center_size(pos2(cx, top + tile_side * 0.5), vec2(side, side));
    paint_shadow(
        &painter,
        tile,
        radius::tile(side),
        theme::shadow(
            fade(data.accent, lerp(0.2, 0.55, breathe)),
            lerp(18.0, 40.0, breathe),
            0.0,
        ),
    );
    let mut style = TileStyle::new(data.accent);
    style.image = data.icon;
    paint_tile(&painter, tile, &style);

    // Name.
    let mut y = top + tile_side + COLUMN_GAP;
    painter.galley(pos2(cx, y), title.clone(), Color32::WHITE);
    y += title.size().y + COLUMN_GAP;

    // "Starting..." with the timer on the right, the sweep, the explanation.
    let left = cx - column_w * 0.5;
    let right = left + column_w;
    const STATUS: theme::TextStyle = ty::TOAST;
    let ink = white(0.88);
    text::paint(
        &painter,
        pos2(left, y),
        Align2::LEFT_TOP,
        STATUS,
        language.text(Key::Starting),
        ink,
    );
    // The timer in the same weight as the word beside it.
    let clock = elapsed_text((time - since).max(0.0) as u64);
    let w = text::tabular_width(&painter, STATUS, &clock);
    text::paint_tabular(&painter, pos2(right - w, y), STATUS, &clock, ink);
    y += status_h + BLOCK_GAP;
    let mut bar = ui.new_child(UiBuilder::new().max_rect(Rect::from_min_size(
        pos2(left, y),
        vec2(column_w, size::PROGRESS_HEIGHT),
    )));
    IndeterminateLine::new(data.accent)
        .width(column_w)
        .show(&mut bar);
    y += size::PROGRESS_HEIGHT + BLOCK_GAP;
    painter.galley(pos2(cx, y), info, color::text_detail());

    // Cancel, 40 above the bottom edge: a button for the pointer; Esc and the pad's back do the same.
    let cancel_rect = Rect::from_center_size(
        pos2(
            cx,
            screen.max.y - CANCEL_BOTTOM - ControlSize::Medium.height() * 0.5,
        ),
        vec2(CANCEL_WIDTH, ControlSize::Medium.height()),
    );
    let mut cancel = ui.new_child(
        UiBuilder::new()
            .max_rect(cancel_rect)
            .layout(Layout::top_down(Align::Center)),
    );
    ConsoleButton::new(language.text(Key::Cancel))
        .icon(Icon::X)
        .size(ControlSize::Medium)
        .show(&mut cancel)
        .clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_and_close() {
        let mut s = StartScreen::default();
        assert!(!s.is_active());
        s.open_at(5.0);
        assert!(s.is_open());
        s.close();
        assert!(!s.is_open() && s.is_active(), "fading out");
    }
}
