//! Paints the frame of the Friends screen: the header with who you are, the rail of sections and
//! the title of the page. The page itself is the rows of `screens::online`.
//!
//! Type: the handle is the screen's title (40 light); a page's title is 30 light over a 15 detail;
//! a rail entry is a 17 medium title. Surfaces: none at rest. The current section is a faint
//! surface with a small bar at its edge, the focused entry the white ring of every other control
//! (docs/DESIGN.md 3.9). The only accent is the online dot.

use egui::{Align2, Color32, Id, Painter, Rect, Role, Sense, Ui, WidgetInfo, pos2, vec2};

use crate::screens::clamped;
use crate::ui::anim::{AnimExt, Motion};
use crate::ui::chrome::{RowAvatar, paint_row_avatar};
use crate::ui::components::{self, corner, paint_capsule};
use crate::ui::icons::Icon;
use crate::ui::text;
use crate::ui::theme::{self, TextStyle, Weight, color, fade, radius, size, ty, white};

/// The width of the rail (dp).
pub const RAIL_WIDTH: f32 = 248.0;
/// The room between the rail and the page.
pub const RAIL_GAP: f32 = 56.0;
/// The widest the page grows.
pub const PAGE_MAX: f32 = 940.0;
const ITEM_HEIGHT: f32 = 52.0;
const ITEM_GAP: f32 = 8.0;
/// The extra air above the entries that are not sections.
const ACTION_GAP: f32 = 16.0;
const AVATAR: f32 = 64.0;
const AVATAR_GAP: f32 = 20.0;

const HANDLE: TextStyle = theme::ty::SCREEN_TITLE;
const PAGE_TITLE: TextStyle = theme::ty::SIDE_PANEL_TITLE;
const PAGE_NOTE: TextStyle = TextStyle::new(15.0, Weight::Regular);
const BADGE: TextStyle = TextStyle::new(13.0, Weight::SemiBold);

/// Who is signed in, for the header.
pub struct Header<'a> {
    pub handle: &'a str,
    pub status: &'a str,
    /// The player is connected: the status gets the dot.
    pub connected: bool,
    pub avatar: RowAvatar,
    pub accent: Color32,
    /// The screen reader's reading of the whole header.
    pub name: &'a str,
}

/// The height the header takes.
pub const HEADER_HEIGHT: f32 = AVATAR;

pub fn header(ui: &mut Ui, at: egui::Pos2, width: f32, header: &Header<'_>) {
    let painter = ui.painter().clone();
    let avatar = Rect::from_min_size(at, vec2(AVATAR, AVATAR));
    paint_row_avatar(&painter, avatar, header.avatar);
    let x = avatar.max.x + AVATAR_GAP;
    let room = (width - AVATAR - AVATAR_GAP).max(120.0);
    let name = clamped(
        &painter,
        HANDLE,
        header.handle,
        Color32::PLACEHOLDER,
        room,
        1,
    );
    let status = clamped(
        &painter,
        ty::ROW_INFO,
        header.status,
        Color32::PLACEHOLDER,
        room - 18.0,
        1,
    );
    let block = name.size().y + 4.0 + status.size().y;
    let top = avatar.center().y - block * 0.5;
    let name_h = name.size().y;
    painter.galley(pos2(x, top), name, Color32::WHITE);
    let dot = if header.connected { 18.0 } else { 0.0 };
    let line_y = top + name_h + 4.0;
    if header.connected {
        painter.circle_filled(
            pos2(x + 4.0, line_y + status.size().y * 0.5),
            4.0,
            header.accent,
        );
    }
    painter.galley(pos2(x + dot, line_y), status, color::text_subtitle());
    // The header reads as one thing.
    let rect = Rect::from_min_size(at, vec2(width.min(AVATAR + AVATAR_GAP + 640.0), AVATAR));
    ui.interact(rect, Id::unique("friends-header"), Sense::hover())
        .widget_info(|| WidgetInfo::labeled(Role::Label, true, header.name));
}

/// An entry of the rail.
pub struct Entry {
    pub icon: Icon,
    pub label: String,
    /// What waits in it (requests, unread messages); 0 for nothing.
    pub waiting: u32,
    /// The page on screen belongs to this section.
    pub current: bool,
    /// The entry does something instead of showing a section.
    pub action: bool,
    /// The accessible name, with what waits.
    pub name: String,
}

/// Draws the rail down from `at`. Returns the entry the pointer pressed.
pub fn rail(ui: &mut Ui, at: egui::Pos2, entries: &[Entry], focus: Option<usize>) -> Option<usize> {
    let ctx = ui.ctx().clone();
    let painter = ui.painter().clone();
    let mut clicked = None;
    let mut y = at.y;
    for (index, entry) in entries.iter().enumerate() {
        if entry.action && index > 0 && !entries[index - 1].action {
            y += ACTION_GAP;
        }
        let rect = Rect::from_min_size(pos2(at.x, y), vec2(RAIL_WIDTH, ITEM_HEIGHT));
        y += ITEM_HEIGHT + ITEM_GAP;
        let response = ui.interact(rect, Id::unique(("friends-rail", index)), Sense::click());
        response.widget_info(|| {
            WidgetInfo::selected(Role::Button, true, entry.current, entry.name.as_str())
        });
        let focused = focus == Some(index);
        crate::ui::a11y::follow(&ctx, response.id, focused);
        let hovered = crate::ui::input::hovered(&ctx, &response);
        if hovered {
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let t = ctx.focus_t((response.id, "focus"), focused);
        let hover_t = ctx.animate_to((response.id, "hover"), f32::from(hovered), Motion::Hover);
        let rest = if entry.current {
            white(0.10)
        } else {
            Color32::TRANSPARENT
        };
        components::paint_surface(
            &painter,
            rect,
            radius::ROW,
            rest,
            hover_t,
            t,
            0.0,
            size::FOCUS_RING,
        );
        if entry.current {
            // The section on screen has a mark that is not a colour.
            let bar =
                Rect::from_center_size(pos2(rect.min.x + 6.0, rect.center().y), vec2(3.0, 22.0));
            painter.rect_filled(bar, corner(1.5), Color32::WHITE);
        }
        let ink = if entry.current || focused {
            Color32::WHITE
        } else {
            white(0.80)
        };
        painter.text(
            pos2(
                rect.min.x + 18.0 + size::ROW_ICON_BOX * 0.5,
                rect.center().y,
            ),
            Align2::CENTER_CENTER,
            entry.icon.glyph(),
            entry.icon.font(size::ROW_ICON, false),
            fade(ink, 0.9),
        );
        let label_x = rect.min.x + 18.0 + size::ROW_ICON_BOX + 14.0;
        let badge_room = if entry.waiting > 0 { 34.0 } else { 0.0 };
        let label = clamped(
            &painter,
            ty::ROW_TITLE,
            &entry.label,
            Color32::PLACEHOLDER,
            rect.max.x - 14.0 - badge_room - label_x,
            1,
        );
        painter.galley(
            pos2(label_x, rect.center().y - label.size().y * 0.5),
            label,
            ink,
        );
        if entry.waiting > 0 {
            paint_badge(&painter, rect, entry.waiting);
        }
        if response.clicked() {
            clicked = Some(index);
        }
    }
    clicked
}

/// The count of what waits: a light capsule with dark ink, as on the top bar.
fn paint_badge(painter: &Painter, row: Rect, count: u32) {
    let label = if count > 9 {
        "9+".to_owned()
    } else {
        count.to_string()
    };
    let width = (text::tabular_width(painter, BADGE, &label) + 14.0).max(22.0);
    let rect = Rect::from_center_size(
        pos2(row.max.x - 14.0 - width * 0.5, row.center().y),
        vec2(width, 22.0),
    );
    paint_capsule(painter, rect, white(0.92));
    text::paint_tabular_aligned(
        painter,
        rect,
        egui::Align::Center,
        BADGE,
        &label,
        color::on_primary(),
    );
}

/// The title and the line under it, from `at`. Returns the height they take.
pub fn page_title(ui: &mut Ui, at: egui::Pos2, width: f32, title: &str, note: Option<&str>) -> f32 {
    let painter = ui.painter().clone();
    let title = clamped(&painter, PAGE_TITLE, title, Color32::PLACEHOLDER, width, 1);
    let mut height = title.size().y;
    ui.interact(
        Rect::from_min_size(at, vec2(width, height)),
        Id::unique("friends-page-title"),
        Sense::hover(),
    )
    .widget_info(|| WidgetInfo::labeled(Role::Heading, true, title.text()));
    painter.galley(at, title, Color32::WHITE);
    if let Some(note) = note {
        height += 6.0;
        let note = clamped(&painter, PAGE_NOTE, note, Color32::PLACEHOLDER, width, 2);
        let note_h = note.size().y;
        // The line under the title is text on the painter: a label gives it to the screen reader.
        ui.interact(
            Rect::from_min_size(pos2(at.x, at.y + height), vec2(width, note_h)),
            Id::unique("friends-page-note"),
            Sense::hover(),
        )
        .widget_info(|| WidgetInfo::labeled(Role::Label, true, note.text()));
        painter.galley(pos2(at.x, at.y + height), note, color::text_detail());
        height += note_h;
    }
    height
}
