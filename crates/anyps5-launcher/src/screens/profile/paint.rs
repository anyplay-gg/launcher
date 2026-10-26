//! Lays the profile out and paints it. The page is one scrolling surface; everything is placed by
//! rectangles measured first, so the focus can follow a row and the geometry of the last frame can
//! steer the next move ([`super::nav`]).
//!
//! Type: the name is the page's title (40 light); a section is a 20 semibold caption over rows of
//! 17 medium titles and 14 details; figures are tabular. Surfaces: none at rest. A hovered row is a
//! faint surface (white 0.07, like the other lists); the focused row is the white card of every
//! other panel with dark ink, grown by 3 dp.

use std::sync::Arc;

use egui::{
    Align, Align2, Color32, Galley, Id, Layout, Painter, Rect, Response, Role, ScrollArea, Sense,
    Ui, UiBuilder, WidgetInfo, pos2, vec2,
};

use super::layout::{self, Kind};
use super::nav::{Column, Slot};
use super::view::{ActivityRow, Body, GameRow, Press, ProfileView, Ready, Stat, TrophyRow};
use crate::screens::online::paint_avatar;
use crate::screens::profile_strings::ProfileText as P;
use crate::screens::{BACK_ROW, back_row, clamped};
use crate::ui::anim::AnimExt;
use crate::ui::components::{ConsoleButton, corner, paint_spinner_arc};
use crate::ui::i18n::Language;
use crate::ui::icons::Icon;
use crate::ui::text;
use crate::ui::theme::{self, Grade, TextStyle, Weight, color, fade, pad, radius, ty, white};

// Sizes in dp.
const AVATAR: f32 = 120.0;
const AVATAR_GAP: f32 = 32.0;
const SCROLL_MARGIN: f32 = 8.0;
const HINT_ROOM: f32 = BACK_ROW;
const SECTION_GAP: f32 = 40.0;
const CAPTION_HEIGHT: f32 = 28.0;
const CAPTION_GAP: f32 = 14.0;
const ROW_GAP: f32 = 8.0;
const ROW_PAD: f32 = 14.0;
const GAME_HEIGHT: f32 = 72.0;
const GAME_ART: f32 = 52.0;
const TROPHY_HEIGHT: f32 = 64.0;
const TROPHY_TILE: f32 = 44.0;
const ACTIVITY_AVATAR: f32 = 36.0;
const BUTTON_HEIGHT: f32 = 54.0;
const BUTTON_GAP: f32 = 12.0;
const STAT_GAP: f32 = 44.0;
const BOTTOM: f32 = 64.0;
const BODY_GAP: f32 = 28.0;
/// The height of the block of words of a state: disc, title, note.
const STATE_BLOCK: f32 = 56.0 + 18.0 + 30.0 + 8.0 + 44.0;

const NAME: TextStyle = ty::SCREEN_TITLE;
const ABOUT: TextStyle = TextStyle::new(17.0, Weight::Regular);
const SECTION: TextStyle = TextStyle::new(20.0, Weight::SemiBold);
const FIGURE: TextStyle = TextStyle::new(34.0, Weight::Light);
const GRADE_COUNT: TextStyle = TextStyle::new(14.0, Weight::Medium);
const FACT: TextStyle = TextStyle::new(15.0, Weight::SemiBold);
const SENTENCE: TextStyle = TextStyle::new(15.0, Weight::Regular);
const STATE_TITLE: TextStyle = TextStyle::new(22.0, Weight::Medium);

/// What the painter needs besides the view.
pub struct Settings {
    pub language: Language,
    pub accent: Color32,
    /// The focused slot, `None` while the screen fades out.
    pub focus: Option<usize>,
    /// The scroll offset the focus asks for.
    pub scroll_to: Option<f32>,
}

/// What a frame learned, for the next one.
#[derive(Default)]
pub struct Drawn {
    pub slots: Vec<Slot>,
    pub presses: Vec<Press>,
    /// `(top, height)` of each slot on the page, for the scroll that follows the focus.
    pub geometry: Vec<(f32, f32)>,
    pub content_height: f32,
    pub viewport_height: f32,
    pub offset: f32,
    /// The Back button was clicked.
    pub back_clicked: bool,
}

#[derive(Clone, Copy, Debug)]
enum ItemKind {
    Game(usize),
    Trophy(usize),
    Activity(usize),
}

/// A row of a column, placed from the top-left corner of the scrolling content.
#[derive(Clone, Debug)]
struct Item {
    kind: ItemKind,
    rect: Rect,
    column: Column,
}

struct Caption {
    kind: Kind,
    pos: egui::Pos2,
    count: Option<usize>,
    /// The player's own words for an empty section.
    note: Option<(egui::Pos2, Arc<Galley>)>,
}

/// The header, placed from its own top-left corner. It does not scroll.
struct Header {
    height: f32,
    name: Arc<Galley>,
    /// Where the identity column starts.
    identity_x: f32,
    chip: Option<(Rect, Arc<Galley>)>,
    status: Option<(egui::Pos2, Arc<Galley>, bool)>,
    about: Option<(egui::Pos2, Arc<Galley>)>,
    note: Option<(egui::Pos2, Arc<Galley>)>,
    stats: Vec<egui::Pos2>,
    /// Where the buttons start (they lay themselves out from here).
    buttons_at: Option<egui::Pos2>,
}

/// The columns, placed from the top-left corner of the scrolling content.
struct Columns {
    height: f32,
    captions: Vec<Caption>,
    items: Vec<Item>,
}

fn wrapped(
    painter: &Painter,
    style: TextStyle,
    text: &str,
    width: f32,
    rows: usize,
    line: f32,
) -> Arc<Galley> {
    let mut job =
        egui::text::LayoutJob::simple(text.to_owned(), style.font(), Color32::PLACEHOLDER, width);
    job.wrap.max_rows = rows;
    job.sections[0].format.line_height = Some(line * style.size);
    painter.layout_job(job)
}

fn stat_width(painter: &Painter, stat: &Stat, language: Language) -> f32 {
    let figure = text::tabular_width(painter, FIGURE, &stat.figure);
    let label = text::width(
        painter,
        ty::INFO_OVERLINE,
        &text::to_upper(&stat.label, language),
    );
    let grades = stat.grades.map_or(0.0, |g| grades_width(painter, g));
    let points = stat.level.as_ref().map_or(0.0, |l| {
        text::tabular_width(painter, GRADE_COUNT, &l.points).max(96.0)
    });
    figure.max(label).max(grades).max(points)
}

fn grades_width(painter: &Painter, grades: [u32; 4]) -> f32 {
    grades
        .iter()
        .filter(|n| **n > 0)
        .map(|n| 17.0 + text::tabular_width(painter, GRADE_COUNT, &n.to_string()) + 12.0)
        .sum::<f32>()
        - 12.0
}

fn activity_text_width(width: f32) -> f32 {
    width - ROW_PAD - ACTIVITY_AVATAR - 12.0 - ROW_PAD
}

fn activity_height(painter: &Painter, row: &ActivityRow, width: f32) -> f32 {
    let sentence = wrapped(
        painter,
        SENTENCE,
        &row.text,
        activity_text_width(width),
        2,
        1.4,
    );
    (ROW_PAD + sentence.size().y + 4.0 + ty::DETAIL.line_height() + ROW_PAD).max(64.0)
}

fn button_width(painter: &Painter, label: &str) -> f32 {
    text::width(painter, ty::BUTTON, label) + 2.0 * theme::size::BUTTON_PADDING
}

/// Measures the header: where the name, the line under it, the text, the figures and the buttons
/// stand, from the header's own top-left corner.
fn measure_header(
    painter: &Painter,
    model: &ProfileView,
    width: f32,
    language: Language,
) -> Header {
    let ready: Option<&Ready> = match &model.body {
        Body::Ready(ready) => Some(ready),
        _ => None,
    };
    let stats = ready.map_or(&[][..], |r| r.stats.as_slice());
    let stat_widths: Vec<f32> = stats
        .iter()
        .map(|s| stat_width(painter, s, language))
        .collect();
    let stats_total: f32 =
        stat_widths.iter().sum::<f32>() + STAT_GAP * stat_widths.len().saturating_sub(1) as f32;
    let identity_x = AVATAR + AVATAR_GAP;
    let identity_width = if stats.is_empty() {
        width - identity_x
    } else {
        (width - identity_x - stats_total - 56.0).max(360.0)
    };

    let name = clamped(
        painter,
        NAME,
        &model.handle,
        Color32::PLACEHOLDER,
        identity_width,
        1,
    );
    let mut y = name.size().y;
    let mut header = Header {
        height: 0.0,
        name,
        identity_x,
        chip: None,
        status: None,
        about: None,
        note: None,
        stats: Vec::new(),
        buttons_at: None,
    };
    if let Some(ready) = ready {
        // The line about how you know each other and what they do now.
        if ready.chip.is_some() || ready.status.is_some() {
            y += 8.0;
            let row_height = 28.0;
            let mut x = identity_x;
            if let Some(chip) = &ready.chip {
                let galley =
                    text::layout(painter, ty::CHIP, chip, Color32::PLACEHOLDER, f32::INFINITY);
                let rect =
                    Rect::from_min_size(pos2(x, y), vec2(galley.size().x + 24.0, row_height));
                x = rect.max.x + 12.0;
                header.chip = Some((rect, galley));
            }
            if let Some(status) = &ready.status {
                let dot = if status.online { 18.0 } else { 0.0 };
                let galley = clamped(
                    painter,
                    ty::ROW_INFO,
                    &status.text,
                    Color32::PLACEHOLDER,
                    (identity_width - (x - identity_x) - dot).max(120.0),
                    1,
                );
                let top = y + (row_height - galley.size().y) * 0.5;
                header.status = Some((pos2(x + dot, top), galley, status.online));
            }
            y += row_height;
        }
        if let Some(about) = &ready.about {
            y += 14.0;
            let galley = wrapped(painter, ABOUT, about, identity_width.min(640.0), 3, 1.45);
            y += galley.size().y;
            header.about = Some((pos2(identity_x, y - galley.size().y), galley));
        }
        if let Some(note) = &ready.note {
            y += 12.0;
            let galley = wrapped(painter, ty::NOTE, note, identity_width, 2, 1.4);
            y += galley.size().y;
            header.note = Some((pos2(identity_x, y - galley.size().y), galley));
        }
        if !ready.actions.is_empty() {
            y += 24.0;
            header.buttons_at = Some(pos2(identity_x, y));
            y += BUTTON_HEIGHT;
        }
        // The figures, right-aligned, level with the name.
        let mut x = width - stats_total;
        for w in &stat_widths {
            header.stats.push(pos2(x, 6.0));
            x += w + STAT_GAP;
        }
    }
    header.height = y.max(AVATAR);
    header
}

/// Measures the columns of a profile that is there.
fn measure_columns(painter: &Painter, ready: &Ready, width: f32) -> Columns {
    let placements = layout::placements(
        ready.games.shown(),
        ready.trophies.shown(),
        ready.activity.shown(),
    );
    let two = placements.iter().any(|(_, c)| *c == Column::Right);
    let (left, right, right_x) = layout::columns(width, two);
    let mut columns = Columns {
        height: 0.0,
        captions: Vec::new(),
        items: Vec::new(),
    };
    let mut ys = [0.0_f32; 2];
    for (kind, column) in placements {
        let (x, w, slot) = match column {
            Column::Right => (right_x, right, 1),
            _ => (0.0, left, 0),
        };
        let mut at = ys[slot];
        let (count, empty, rows) = match kind {
            Kind::Games => (
                Some(ready.games.total).filter(|n| *n > 0),
                ready.games.empty.as_deref(),
                ready.games.rows.len(),
            ),
            Kind::Trophies => (
                None,
                ready.trophies.empty.as_deref(),
                ready.trophies.rows.len(),
            ),
            Kind::Activity => (
                None,
                ready.activity.empty.as_deref(),
                ready.activity.rows.len(),
            ),
        };
        let pos = pos2(x, at);
        at += CAPTION_HEIGHT + CAPTION_GAP;
        let mut note = None;
        if rows == 0
            && let Some(empty) = empty
        {
            let galley = wrapped(painter, ty::NOTE, empty, w.min(420.0), 3, 1.45);
            at += galley.size().y;
            note = Some((pos2(x, at - galley.size().y), galley));
        }
        columns.captions.push(Caption {
            kind,
            pos,
            count,
            note,
        });
        for n in 0..rows {
            let (item, height) = match kind {
                Kind::Games => (ItemKind::Game(n), GAME_HEIGHT),
                Kind::Trophies => (ItemKind::Trophy(n), TROPHY_HEIGHT),
                Kind::Activity => (
                    ItemKind::Activity(n),
                    activity_height(painter, &ready.activity.rows[n], w),
                ),
            };
            columns.items.push(Item {
                kind: item,
                rect: Rect::from_min_size(pos2(x, at), vec2(w, height)),
                column,
            });
            at += height + ROW_GAP;
        }
        ys[slot] = if rows > 0 { at - ROW_GAP } else { at } + SECTION_GAP;
    }
    columns.height = ys[0].max(ys[1]);
    columns
}

/// A soft wash of the player's colour in the top-left corner, behind everything: the one thing on
/// the page that is only this player's. A gradient of the avatar's colour from 22 % at the corner
/// to nothing 520 dp down and 70 % across, so text on it keeps its contrast with the glass.
fn paint_wash(painter: &Painter, screen: Rect, tint: [u8; 3]) {
    let corner_alpha = 0.22;
    let at = |alpha: f32| {
        Color32::from_rgba_premultiplied(
            (f32::from(tint[0]) * alpha) as u8,
            (f32::from(tint[1]) * alpha) as u8,
            (f32::from(tint[2]) * alpha) as u8,
            (255.0 * alpha) as u8,
        )
    };
    let rect = Rect::from_min_size(
        screen.min,
        vec2(
            (screen.width() * 0.7).min(1100.0),
            520.0_f32.min(screen.height()),
        ),
    );
    let mut mesh = egui::epaint::Mesh::default();
    for (pos, colour) in [
        (rect.left_top(), at(corner_alpha)),
        (rect.right_top(), at(0.0)),
        (rect.right_bottom(), at(0.0)),
        (rect.left_bottom(), at(0.0)),
    ] {
        mesh.vertices.push(egui::epaint::Vertex {
            pos,
            uv: egui::epaint::WHITE_UV,
            color: colour,
        });
    }
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    painter.add(egui::Shape::mesh(mesh));
}

/// Draws the screen into the panel `ui`: a profile that is there is one scrolling page (header,
/// then the columns); one that is not is a header and a block of words.
pub fn draw(
    ui: &mut Ui,
    model: &ProfileView,
    settings: Settings,
    drawn: &mut Drawn,
    clicked: &mut Option<usize>,
) {
    let screen = ui.max_rect();
    paint_wash(ui.painter(), screen, model.tint);
    let area = screen.shrink2(vec2(pad::TROPHIES.0, pad::TROPHIES.1));
    drawn.back_clicked = back_row(ui, area, settings.language);
    drawn.slots.clear();
    drawn.presses.clear();
    drawn.geometry.clear();
    drawn.content_height = 0.0;
    drawn.viewport_height = 0.0;
    drawn.offset = 0.0;

    // The page starts under the hint bar and runs to the bottom of the screen; what scrolls past
    // either edge fades out.
    let list = Rect::from_min_max(
        pos2(
            area.min.x - SCROLL_MARGIN,
            area.min.y + HINT_ROOM - SCROLL_MARGIN,
        ),
        pos2(area.max.x + SCROLL_MARGIN, screen.max.y),
    );
    match &model.body {
        Body::Ready(ready) => paint_page(ui, list, model, ready, &settings, drawn, clicked),
        _ => paint_state_page(ui, list, area, model, &settings, drawn, clicked),
    }
}

/// A profile that is not there: the header, and under it one block of words.
fn paint_state_page(
    ui: &mut Ui,
    list: Rect,
    area: Rect,
    model: &ProfileView,
    settings: &Settings,
    drawn: &mut Drawn,
    clicked: &mut Option<usize>,
) {
    let ctx = ui.ctx().clone();
    let painter = ui.painter().clone();
    let origin = pos2(area.min.x, list.min.y + SCROLL_MARGIN);
    let header = measure_header(&painter, model, area.width(), settings.language);
    paint_header(ui, origin, model, &header, settings, drawn, clicked);
    let body_top = origin.y + header.height + BODY_GAP;
    let failed = matches!(model.body, Body::Failed);
    let block = STATE_BLOCK + if failed { 24.0 + BUTTON_HEIGHT } else { 0.0 };
    let room = area.max.y - body_top;
    let top = body_top + ((room - block) * 0.5 - 24.0).max(12.0);
    let retry = failed.then(|| {
        drawn.slots.push(Slot {
            column: Column::Header,
            y: 0.0,
        });
        drawn.presses.push(Press::Retry);
        drawn.geometry.push((0.0, 0.0));
        drawn.slots.len() - 1
    });
    paint_state(
        ui,
        &ctx,
        pos2(area.center().x, top),
        model,
        settings,
        retry,
        clicked,
    );
}

/// How far from the edge of the scrolling list a row starts to fade out (dp).
const EDGE: f32 = 20.0;

/// How much of a row shows at the clipped edges of the list: it fades out over its last 20 dp
/// before the edge, as the rows of the other lists do.
fn edge_fade(rect: Rect, clip: Rect) -> f32 {
    let centre = rect.center().y;
    let room = rect.height() * 0.5 + EDGE;
    ((centre - clip.min.y).min(clip.max.y - centre) / room).clamp(0.0, 1.0)
}

/// The page: the header, then the columns, in one scroll area.
fn paint_page(
    ui: &mut Ui,
    list: Rect,
    model: &ProfileView,
    ready: &Ready,
    settings: &Settings,
    drawn: &mut Drawn,
    clicked: &mut Option<usize>,
) {
    let ctx = ui.ctx().clone();
    let mut child = ui.new_child(UiBuilder::new().max_rect(list));
    let mut scroll = ScrollArea::vertical()
        .id_salt(Id::unique("profile-screen-scroll"))
        .auto_shrink([false, false])
        .scroll_bar_visibility(egui::containers::scroll_area::ScrollBarVisibility::AlwaysHidden);
    if let Some(offset) = settings.scroll_to {
        scroll = scroll.vertical_scroll_offset(offset);
    }
    let output = scroll.show(&mut child, |ui| {
        egui::Frame::NONE
            .inner_margin(egui::Margin::same(SCROLL_MARGIN as i8))
            .show(ui, |ui| {
                let painter = ui.painter().clone();
                let width = ui.available_width();
                let header = measure_header(&painter, model, width, settings.language);
                let columns = measure_columns(&painter, ready, width);
                let body = header.height + BODY_GAP;
                let (page, _) = ui.allocate_exact_size(
                    vec2(width, body + columns.height + BOTTOM),
                    Sense::hover(),
                );
                let clip = list.shrink(SCROLL_MARGIN);
                // The header leaves by fading, once its lower half is past the top edge.
                let leaving = ((page.min.y + header.height - clip.min.y) / (header.height * 0.5))
                    .clamp(0.0, 1.0);
                ui.set_opacity(leaving);
                paint_header(ui, page.min, model, &header, settings, drawn, clicked);
                ui.set_opacity(1.0);
                let columns_at = page.min + vec2(0.0, body);
                paint_columns(
                    ui, &ctx, columns_at, body, clip, ready, &columns, settings, drawn, clicked,
                );
            });
    });
    drawn.content_height = output.content_size.y;
    drawn.viewport_height = output.inner_rect.height();
    drawn.offset = output.state.clamped_offset().y;
}

/// The header: the avatar, the name, the line under it, the text, the figures and the buttons.
/// The buttons are the first slots of the focus.
fn paint_header(
    ui: &mut Ui,
    origin: egui::Pos2,
    model: &ProfileView,
    header: &Header,
    settings: &Settings,
    drawn: &mut Drawn,
    clicked: &mut Option<usize>,
) {
    let painter = ui.painter().clone();
    let language = settings.language;
    let at = |p: egui::Pos2| p + origin.to_vec2();
    let avatar = Rect::from_min_size(origin, vec2(AVATAR, AVATAR));
    paint_avatar(&painter, avatar, model.avatar, model.account, &model.handle);
    painter.galley(
        at(pos2(header.identity_x, 0.0)),
        header.name.clone(),
        Color32::WHITE,
    );
    if let Some((rect, galley)) = &header.chip {
        let rect = rect.translate(origin.to_vec2());
        painter.rect_filled(rect, corner(rect.height() * 0.5), white(0.12));
        painter.galley(
            pos2(rect.min.x + 12.0, rect.center().y - galley.size().y * 0.5),
            galley.clone(),
            white(0.92),
        );
    }
    if let Some((pos, galley, online)) = &header.status {
        let pos = at(*pos);
        if *online {
            painter.circle_filled(
                pos2(pos.x - 11.0, pos.y + galley.size().y * 0.5),
                4.0,
                settings.accent,
            );
        }
        painter.galley(pos, galley.clone(), color::text_subtitle());
    }
    if let Some((pos, galley)) = &header.about {
        painter.galley(at(*pos), galley.clone(), white(0.86));
    }
    if let Some((pos, galley)) = &header.note {
        painter.galley(at(*pos), galley.clone(), color::text_detail());
    }
    let Body::Ready(ready) = &model.body else {
        // The words of the header for a screen reader, whatever the state.
        let rect = Rect::from_min_size(origin, vec2(ui.max_rect().width() * 0.5, header.height));
        ui.interact(rect, Id::unique("profile-header"), Sense::hover())
            .widget_info(|| WidgetInfo::labeled(Role::Label, true, model.name.as_str()));
        return;
    };
    // The header reads as one thing, and each figure as another.
    let words: Vec<&str> = [
        Some(model.name.as_str()),
        ready.chip.as_deref(),
        ready.status.as_ref().map(|s| s.text.as_str()),
        ready.about.as_deref(),
        ready.note.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect();
    let rect = Rect::from_min_size(origin, vec2(AVATAR + AVATAR_GAP + 640.0, header.height));
    ui.interact(rect, Id::unique("profile-header"), Sense::hover())
        .widget_info(|| WidgetInfo::labeled(Role::Label, true, words.join(", ")));
    for (stat, pos) in ready.stats.iter().zip(&header.stats) {
        paint_stat(&painter, at(*pos), stat, language, settings.accent);
        let rect = Rect::from_min_size(at(*pos), vec2(120.0, 70.0));
        ui.interact(
            rect,
            Id::unique(("profile-stat", &stat.name)),
            Sense::hover(),
        )
        .widget_info(|| WidgetInfo::labeled(Role::Label, true, stat.name.as_str()));
    }
    // The buttons are the shared console buttons; they lay themselves out in a row.
    if let Some(pos) = header.buttons_at {
        let mut row = ui.new_child(
            UiBuilder::new()
                .max_rect(Rect::from_min_size(
                    at(pos),
                    vec2(ui.max_rect().max.x - at(pos).x, BUTTON_HEIGHT + 8.0),
                ))
                .layout(Layout::left_to_right(Align::Min)),
        );
        row.spacing_mut().item_spacing.x = BUTTON_GAP;
        for (n, action) in ready.actions.iter().enumerate() {
            let slot = drawn.slots.len();
            drawn.slots.push(Slot {
                column: Column::Header,
                y: pos.y,
            });
            drawn.presses.push(action.press.clone());
            drawn.geometry.push((pos.y + SCROLL_MARGIN, BUTTON_HEIGHT));
            let response = ConsoleButton::new(&action.label)
                .prominent(n == 0)
                .focused(settings.focus == Some(slot))
                .show(&mut row);
            if response.clicked() {
                *clicked = Some(slot);
            }
        }
    }
}

/// The captions and rows of the columns, from `origin` down. A row that does something is a
/// stop of the focus; one that does not is just text.
#[allow(clippy::too_many_arguments)]
fn paint_columns(
    ui: &mut Ui,
    ctx: &egui::Context,
    origin: egui::Pos2,
    body: f32,
    clip: Rect,
    ready: &Ready,
    columns: &Columns,
    settings: &Settings,
    drawn: &mut Drawn,
    clicked: &mut Option<usize>,
) {
    let language = settings.language;
    let shift = origin.to_vec2();
    let at = |p: egui::Pos2| p + shift;

    // Captions and the empty words under them.
    for caption in &columns.captions {
        let title = match caption.kind {
            Kind::Games => P::SectionGames,
            Kind::Trophies => P::SectionTrophies,
            Kind::Activity => P::SectionActivity,
        };
        let pos = at(caption.pos);
        let shown = Rect::from_min_size(pos, vec2(200.0, CAPTION_HEIGHT));
        let mut painter = ui.painter().clone();
        painter.set_opacity(edge_fade(shown, clip));
        let rect = text::paint(
            &painter,
            pos,
            Align2::LEFT_TOP,
            SECTION,
            title.text(language),
            white(0.94),
        );
        if let Some(count) = caption.count {
            painter.text(
                pos2(rect.max.x + 10.0, rect.max.y - 3.0),
                Align2::LEFT_BOTTOM,
                count.to_string(),
                ty::NOTE.font(),
                color::text_count(),
            );
        }
        if let Some((pos, galley)) = &caption.note {
            painter.galley(at(*pos), galley.clone(), color::text_detail());
        }
    }

    // The rows, in the order the focus walks them.
    for item in &columns.items {
        let rect = item.rect.translate(shift);
        let press = match item.kind {
            ItemKind::Game(n) => ready.games.rows[n].press.clone(),
            ItemKind::Trophy(n) => ready.trophies.rows[n].press.clone(),
            ItemKind::Activity(n) => ready.activity.rows[n].press.clone(),
        };
        let stop = !matches!(press, Press::None);
        let slot = drawn.slots.len();
        let focused = stop && settings.focus == Some(slot);
        if stop {
            drawn.slots.push(Slot {
                column: item.column,
                y: item.rect.center().y,
            });
            drawn.presses.push(press);
            drawn
                .geometry
                .push((item.rect.min.y + body + SCROLL_MARGIN, item.rect.height()));
        }
        // A focused row stays whole: the focus scrolls it into the middle.
        let fade = if focused { 1.0 } else { edge_fade(rect, clip) };
        ui.set_opacity(fade);
        let response = match item.kind {
            ItemKind::Game(n) => {
                paint_game(ui, ctx, rect, slot, &ready.games.rows[n], focused, settings)
            }
            ItemKind::Trophy(n) => paint_trophy(
                ui,
                ctx,
                rect,
                slot,
                &ready.trophies.rows[n],
                focused,
                language,
            ),
            ItemKind::Activity(n) => paint_activity(
                ui,
                ctx,
                rect,
                slot,
                &ready.activity.rows[n],
                focused,
                language,
            ),
        };
        ui.set_opacity(1.0);
        if stop && response.clicked() {
            *clicked = Some(slot);
        }
    }
}

fn paint_stat(
    painter: &Painter,
    pos: egui::Pos2,
    stat: &Stat,
    language: Language,
    accent: Color32,
) {
    text::paint_tabular(painter, pos, FIGURE, &stat.figure, Color32::WHITE);
    let label_y = pos.y + FIGURE.line_height() + 2.0;
    text::paint(
        painter,
        pos2(pos.x, label_y),
        Align2::LEFT_TOP,
        ty::INFO_OVERLINE,
        &text::to_upper(&stat.label, language),
        color::text_overline(),
    );
    if let Some(level) = &stat.level {
        // The points, and under them the way to the next level: a track and its fill.
        let y = label_y + ty::INFO_OVERLINE.line_height() + 8.0;
        let words = text::paint_tabular(
            painter,
            pos2(pos.x, y),
            GRADE_COUNT,
            &level.points,
            white(0.8),
        );
        let bar = Rect::from_min_size(
            pos2(pos.x, words.max.y + 8.0),
            vec2(words.width().max(96.0), 4.0),
        );
        painter.rect_filled(bar, corner(2.0), white(0.16));
        paint_progress_fill(painter, bar, level.fraction, accent);
    }
    if let Some(grades) = stat.grades {
        let mut x = pos.x;
        let y = label_y + ty::INFO_OVERLINE.line_height() + 8.0;
        for (grade, count) in Grade::ALL.into_iter().zip(grades) {
            if count == 0 {
                continue;
            }
            painter.text(
                pos2(x + 6.5, y + 8.0),
                Align2::CENTER_CENTER,
                Icon::TROPHY.glyph(),
                Icon::TROPHY.font(13.0, true),
                grade.color(),
            );
            x += 17.0;
            let w = text::paint_tabular(
                painter,
                pos2(x, y),
                GRADE_COUNT,
                &count.to_string(),
                white(0.8),
            )
            .width();
            x += w + 12.0;
        }
    }
}

/// A row's surface: nothing at rest, a faint one under the pointer, the white card when focused.
struct Surface {
    response: Response,
    /// The ink of the row: white on the page, black on the focused card.
    ink: Color32,
    /// Focus amount.
    t: f32,
}

impl Surface {
    fn dim(&self, factor: f32) -> Color32 {
        fade(self.ink, factor)
    }
}

fn surface(
    ui: &mut Ui,
    ctx: &egui::Context,
    rect: Rect,
    id: usize,
    focused: bool,
    pressable: bool,
    name: &str,
) -> Surface {
    let response = ui.interact(
        rect,
        Id::unique(("profile-row", id)),
        if pressable {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    response.widget_info(|| {
        WidgetInfo::labeled(
            if pressable { Role::Button } else { Role::Label },
            true,
            name,
        )
    });
    crate::ui::a11y::follow(ctx, response.id, focused && pressable);
    let hovered = pressable && crate::ui::input::hovered(ctx, &response);
    if hovered {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let t = ctx.focus_t((response.id, "focus"), focused);
    let painter = ui.painter();
    if ui.is_rect_visible(rect) {
        // The ring system of DESIGN.md 3.9: lifted under the pointer, lifted more and ringed with the focus.
        let hover_t = ctx.animate_to(
            (response.id, "hover"),
            f32::from(hovered),
            crate::ui::anim::Motion::Hover,
        );
        crate::ui::components::paint_surface(
            painter,
            rect,
            radius::ROW,
            Color32::TRANSPARENT,
            hover_t,
            t,
            0.0,
            crate::ui::theme::size::FOCUS_RING,
        );
    }
    Surface {
        response,
        ink: Color32::WHITE,
        t,
    }
}

fn paint_game(
    ui: &mut Ui,
    ctx: &egui::Context,
    rect: Rect,
    slot: usize,
    row: &GameRow,
    focused: bool,
    settings: &Settings,
) -> Response {
    let s = surface(
        ui,
        ctx,
        rect,
        slot,
        focused,
        !matches!(row.press, Press::None),
        &row.name_a11y,
    );
    let painter = ui.painter().clone();
    let art = Rect::from_center_size(
        pos2(rect.min.x + ROW_PAD - 2.0 + GAME_ART * 0.5, rect.center().y),
        vec2(GAME_ART, GAME_ART),
    );
    // A game the player does not have is a quiet tile; the art comes only from their own copy.
    let mut style = crate::ui::chrome::TileStyle::new(settings.accent);
    style.image = row.art;
    style.opacity = if row.owned { 1.0 } else { 0.7 };
    crate::ui::chrome::paint_tile(&painter, art, &style);
    let trailing = row
        .trailing
        .as_deref()
        .map(|t| (t, text::tabular_width(&painter, FACT, t)));
    let bar_width = if row.progress.is_some() { 96.0 } else { 0.0 };
    let trailing_width = trailing.map_or(0.0, |(_, w)| w).max(bar_width);
    let text_x = art.max.x + 14.0;
    let text_width = (rect.max.x - ROW_PAD - trailing_width - 16.0 - text_x).max(80.0);
    let name = clamped(
        &painter,
        ty::ROW_TITLE,
        &row.name,
        Color32::PLACEHOLDER,
        text_width,
        1,
    );
    let detail = clamped(
        &painter,
        ty::NOTE,
        &row.detail,
        Color32::PLACEHOLDER,
        text_width,
        1,
    );
    let has_detail = !row.detail.is_empty();
    let block = name.size().y
        + if has_detail {
            3.0 + detail.size().y
        } else {
            0.0
        };
    let top = rect.center().y - block * 0.5;
    let name_h = name.size().y;
    let ink = if row.owned { s.ink } else { s.dim(0.78) };
    painter.galley(pos2(text_x, top), name, ink);
    if has_detail {
        painter.galley(pos2(text_x, top + name_h + 3.0), detail, s.dim(0.68));
    }
    if let Some((label, width)) = trailing {
        let x = rect.max.x - ROW_PAD - width;
        let stack = FACT.line_height() + if row.progress.is_some() { 8.0 } else { 0.0 };
        let y = rect.center().y - (stack + if row.progress.is_some() { 4.0 } else { 0.0 }) * 0.5;
        text::paint_tabular(&painter, pos2(x, y), FACT, label, s.ink);
        if let Some(fraction) = row.progress {
            let bar = Rect::from_min_size(
                pos2(
                    rect.max.x - ROW_PAD - bar_width,
                    y + FACT.line_height() + 8.0,
                ),
                vec2(bar_width, 4.0),
            );
            painter.rect_filled(bar, corner(2.0), white(0.16));
            let fill = settings.accent;
            paint_progress_fill(&painter, bar, fraction, fill);
        }
    }
    s.response
}

fn paint_progress_fill(painter: &Painter, bar: Rect, fraction: f32, fill: Color32) {
    let width =
        (bar.width() * fraction.clamp(0.0, 1.0)).max(if fraction > 0.0 { 4.0 } else { 0.0 });
    if width > 0.0 {
        painter.rect_filled(
            Rect::from_min_size(bar.min, vec2(width, bar.height())),
            corner(2.0),
            fill,
        );
    }
}

fn paint_trophy(
    ui: &mut Ui,
    ctx: &egui::Context,
    rect: Rect,
    slot: usize,
    row: &TrophyRow,
    focused: bool,
    _language: Language,
) -> Response {
    let s = surface(
        ui,
        ctx,
        rect,
        slot,
        focused,
        !matches!(row.press, Press::None),
        &format!("{}, {}", row.title, row.detail),
    );
    let painter = ui.painter().clone();
    let tile = Rect::from_center_size(
        pos2(rect.min.x + ROW_PAD + TROPHY_TILE * 0.5, rect.center().y),
        vec2(TROPHY_TILE, TROPHY_TILE),
    );
    let glyph = row.grade.color();
    painter.rect_filled(tile, corner(radius::ROW), fade(glyph, 0.16));
    painter.text(
        tile.center(),
        Align2::CENTER_CENTER,
        Icon::TROPHY.glyph(),
        Icon::TROPHY.font(20.0, true),
        glyph,
    );
    let trailing = row.trailing.as_deref();
    let trailing_galley = trailing.map(|t| {
        clamped(
            &painter,
            ty::DETAIL,
            t,
            Color32::PLACEHOLDER,
            rect.width() * 0.3,
            1,
        )
    });
    let trailing_width = trailing_galley.as_ref().map_or(0.0, |g| g.size().x);
    let text_x = tile.max.x + 14.0;
    let text_width = (rect.max.x - ROW_PAD - trailing_width - 16.0 - text_x).max(80.0);
    let title = clamped(
        &painter,
        ty::ROW_TITLE,
        &row.title,
        Color32::PLACEHOLDER,
        text_width,
        1,
    );
    let detail = clamped(
        &painter,
        ty::NOTE,
        &row.detail,
        Color32::PLACEHOLDER,
        text_width,
        1,
    );
    let block = title.size().y + 3.0 + detail.size().y;
    let top = rect.center().y - block * 0.5;
    let title_h = title.size().y;
    painter.galley(pos2(text_x, top), title, s.ink);
    painter.galley(pos2(text_x, top + title_h + 3.0), detail, s.dim(0.68));
    if let Some(galley) = trailing_galley {
        let h = galley.size().y;
        painter.galley(
            pos2(
                rect.max.x - ROW_PAD - galley.size().x,
                rect.center().y - h * 0.5,
            ),
            galley,
            s.dim(0.68),
        );
    }
    s.response
}

fn paint_activity(
    ui: &mut Ui,
    ctx: &egui::Context,
    rect: Rect,
    slot: usize,
    row: &ActivityRow,
    focused: bool,
    _language: Language,
) -> Response {
    let meta = if row.reactions.is_empty() {
        row.when.clone()
    } else {
        format!("{} · {}", row.when, row.reactions)
    };
    let s = surface(
        ui,
        ctx,
        rect,
        slot,
        focused,
        !matches!(row.press, Press::None),
        &format!("{}, {meta}", row.text),
    );
    let painter = ui.painter().clone();
    let avatar = Rect::from_min_size(
        pos2(rect.min.x + ROW_PAD, rect.min.y + ROW_PAD),
        vec2(ACTIVITY_AVATAR, ACTIVITY_AVATAR),
    );
    paint_avatar(&painter, avatar, row.avatar, row.account, &row.handle);
    let text_x = avatar.max.x + 12.0;
    let sentence = wrapped(
        &painter,
        SENTENCE,
        &row.text,
        activity_text_width(rect.width()),
        2,
        1.4,
    );
    let sentence_h = sentence.size().y;
    painter.galley(pos2(text_x, rect.min.y + ROW_PAD), sentence, s.ink);
    let meta = clamped(
        &painter,
        ty::DETAIL,
        &meta,
        Color32::PLACEHOLDER,
        activity_text_width(rect.width()),
        1,
    );
    painter.galley(
        pos2(text_x, rect.min.y + ROW_PAD + sentence_h + 4.0),
        meta,
        s.dim(0.66),
    );
    s.response
}

fn paint_state(
    ui: &mut Ui,
    ctx: &egui::Context,
    top_centre: egui::Pos2,
    model: &ProfileView,
    settings: &Settings,
    retry_slot: Option<usize>,
    clicked: &mut Option<usize>,
) {
    let painter = ui.painter().clone();
    let language = settings.language;
    let (icon, title, note): (Icon, String, String) = match &model.body {
        Body::Loading => (
            Icon::CIRCLE_DASHED,
            P::LoadingNote.text(language).to_owned(),
            String::new(),
        ),
        Body::Offline => (
            Icon::CLOUD,
            P::OfflineTitle.text(language).to_owned(),
            P::OfflineNote.text(language).to_owned(),
        ),
        Body::Failed => (
            Icon::WARNING,
            P::FailedTitle.text(language).to_owned(),
            P::FailedNote.text(language).to_owned(),
        ),
        Body::Private => (
            Icon::LOCK,
            P::PrivateTitle.text(language).to_owned(),
            P::PrivateNote.format(language, &[crate::ui::i18n::Arg::Str(&model.handle)]),
        ),
        Body::Empty => (
            Icon::USER_CIRCLE,
            P::EmptyTitle.text(language).to_owned(),
            P::EmptyNote.format(language, &[crate::ui::i18n::Arg::Str(&model.handle)]),
        ),
        Body::Ready(_) => return,
    };
    let disc = Rect::from_center_size(pos2(top_centre.x, top_centre.y + 28.0), vec2(56.0, 56.0));
    painter.circle_filled(disc.center(), 28.0, color::surface_row());
    if matches!(model.body, Body::Loading) && !ctx.reduced_motion() {
        let time = ctx.input(|i| i.time);
        paint_spinner_arc(&painter, disc.center(), 12.0, time, 2.5, white(0.8));
        ctx.request_repaint();
    } else {
        painter.text(
            disc.center(),
            Align2::CENTER_CENTER,
            icon.glyph(),
            icon.font(24.0, false),
            white(0.75),
        );
    }
    let mut y = disc.max.y + 18.0;
    let title_g = clamped(
        &painter,
        STATE_TITLE,
        &title,
        Color32::PLACEHOLDER,
        520.0,
        1,
    );
    let title_h = title_g.size().y;
    painter.galley(
        pos2(top_centre.x - title_g.size().x * 0.5, y),
        title_g,
        Color32::WHITE,
    );
    y += title_h + 8.0;
    if !note.is_empty() {
        let mut job =
            egui::text::LayoutJob::simple(note, ty::NOTE.font(), Color32::PLACEHOLDER, 420.0);
        job.halign = Align::Center;
        job.sections[0].format.line_height = Some(1.45 * ty::NOTE.size);
        let galley = painter.layout_job(job);
        painter.galley(pos2(top_centre.x, y), galley, color::text_detail());
    }
    if let Some(slot) = retry_slot {
        let label = crate::screens::online_feed_strings::FeedText::TryAgain.text(language);
        let w = button_width(&painter, label);
        let y = top_centre.y + 56.0 + 18.0 + 30.0 + 8.0 + 44.0 + 24.0;
        let mut row = ui.new_child(
            UiBuilder::new()
                .max_rect(Rect::from_min_size(
                    pos2(top_centre.x - w * 0.5 - 3.0, y),
                    vec2(w + 12.0, BUTTON_HEIGHT + 8.0),
                ))
                .layout(Layout::left_to_right(Align::Min)),
        );
        let response = ConsoleButton::new(label)
            .prominent(true)
            .focused(settings.focus == Some(slot))
            .show(&mut row);
        if response.clicked() {
            *clicked = Some(slot);
        }
    }
}
