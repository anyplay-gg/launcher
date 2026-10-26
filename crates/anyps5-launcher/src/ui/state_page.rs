//! The page that has no cards (docs/DESIGN.md 5.21): loading, offline, failed, private, hidden or
//! empty. A quiet disc with a glyph (a spinner while it loads), the headline, a note that says
//! what to do, and at most one primary button that does it. Profile, Compare and the feed share
//! it, so a state looks and stands the same on every page.

use egui::{Align, Align2, Id, Layout, Rect, Role, Sense, Ui, UiBuilder, WidgetInfo, pos2, vec2};

use crate::ui::anim::AnimExt;
use crate::ui::components::{ConsoleButton, paint_spinner_arc};
use crate::ui::icons::Icon;
use crate::ui::text;
use crate::ui::theme::{color, size, space, ty};

/// The disc behind the glyph, and the glyph.
const DISC: f32 = 56.0;
const GLYPH: f32 = 24.0;
/// The headline wraps at this width, the note at [`NOTE_WIDTH`].
const TITLE_WIDTH: f32 = 520.0;
const NOTE_WIDTH: f32 = 420.0;
/// The block stands a little above the middle of the room.
const LIFT: f32 = space::XL;

/// What a state page says.
pub struct StatePage<'a> {
    pub icon: Icon,
    pub title: &'a str,
    pub note: Option<&'a str>,
    /// A spinner in place of the glyph (a still glyph under reduced motion).
    pub loading: bool,
    /// The label of the one button, if the state has a way out.
    pub action: Option<&'a str>,
}

/// The text of a page laid out, and the height of the block.
struct Laid {
    title: std::sync::Arc<egui::Galley>,
    note: Option<std::sync::Arc<egui::Galley>>,
    height: f32,
}

fn lay_out(painter: &egui::Painter, page: &StatePage<'_>) -> Laid {
    let title = crate::screens::clamped(
        painter,
        ty::STATE_TITLE,
        page.title,
        egui::Color32::PLACEHOLDER,
        TITLE_WIDTH,
        1,
    );
    let note = page.note.filter(|n| !n.is_empty()).map(|note| {
        let mut job = egui::text::LayoutJob::simple(
            note.to_owned(),
            ty::NOTE.font(),
            egui::Color32::PLACEHOLDER,
            NOTE_WIDTH,
        );
        job.halign = Align::Center;
        job.sections[0].format.line_height = Some(1.45 * ty::NOTE.size);
        painter.layout_job(job)
    });
    let height = DISC
        + space::L
        + title.size().y
        + note.as_ref().map_or(0.0, |n| space::S + n.size().y)
        + if page.action.is_some() {
            space::XL + size::BUTTON_MIN_HEIGHT
        } else {
            0.0
        };
    Laid {
        title,
        note,
        height,
    }
}

/// How tall the block of a page is, for a screen that places it in a flow.
pub fn height(painter: &egui::Painter, page: &StatePage<'_>) -> f32 {
    lay_out(painter, page).height
}

/// Draws the page centred in `room`; `focused` is whether the button holds the focus. Returns
/// whether the button was clicked.
pub fn show(ui: &mut Ui, room: Rect, page: &StatePage<'_>, focused: bool) -> bool {
    let ctx = ui.ctx().clone();
    let painter = ui.painter().clone();
    let Laid {
        title,
        note,
        height,
    } = lay_out(&painter, page);
    let button = size::BUTTON_MIN_HEIGHT;
    let top = room.min.y + ((room.height() - height) * 0.5 - LIFT).max(space::M);
    let centre = room.center().x;

    // The words of the page, for a screen reader: they are only painted.
    let said = match page.note {
        Some(note) if !note.is_empty() => format!("{}. {note}", page.title),
        _ => page.title.to_owned(),
    };
    let block = Rect::from_center_size(
        pos2(centre, top + height * 0.5),
        vec2(TITLE_WIDTH, height),
    );
    ui.interact(block, Id::unique("state-page"), Sense::hover())
        .widget_info(|| WidgetInfo::labeled(Role::Label, true, said.as_str()));

    let disc = Rect::from_center_size(pos2(centre, top + DISC * 0.5), vec2(DISC, DISC));
    painter.circle_filled(disc.center(), DISC * 0.5, color::surface_row());
    if page.loading && !ctx.reduced_motion() {
        paint_spinner_arc(
            &painter,
            disc.center(),
            12.0,
            ctx.input(|i| i.time),
            2.5,
            color::text_detail(),
        );
        ctx.request_repaint();
    } else {
        painter.text(
            disc.center(),
            Align2::CENTER_CENTER,
            page.icon.glyph(),
            page.icon.font(GLYPH, false),
            color::text_detail(),
        );
    }
    let mut y = disc.max.y + space::L;
    let title_height = title.size().y;
    painter.galley(
        pos2(centre - title.size().x * 0.5, y),
        title,
        color::text(),
    );
    y += title_height;
    if let Some(note) = note {
        y += space::S;
        let note_height = note.size().y;
        painter.galley(pos2(centre, y), note, color::text_detail());
        y += note_height;
    }
    let Some(label) = page.action else {
        return false;
    };
    y += space::XL;
    let width = text::width(&painter, ty::BUTTON, label) + 2.0 * size::BUTTON_PADDING;
    let mut row = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_size(
                pos2(centre - width * 0.5 - space::XS, y),
                vec2(width + 2.0 * space::XS, button + space::S),
            ))
            .layout(Layout::left_to_right(Align::Min)),
    );
    ConsoleButton::new(label)
        .prominent(true)
        .focused(focused)
        .show(&mut row)
        .clicked()
}
