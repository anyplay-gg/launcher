//! Drawing of the Captures area: the grid, the states without tiles, the view of one capture and
//! the question before a delete. Everything the pointer pressed is written to [`Pressed`] and
//! turned into focus changes and actions after the frame, like the library grid does.

use anyps5_core::captures::{Capture, Filter, Kind};
use egui::{Align, Align2, Color32, Layout as UiLayout, Rect, Sense, Ui, UiBuilder, pos2, vec2};

use super::layout::{Layout, Metrics, columns_for};
use super::{
    ButtonKind, CapturesAction, CapturesData, CapturesScreen, Load, Mode, Thumb, ViewAction,
    ViewFocus, Zone, duration_text, filter_text,
};
use crate::screens::captures_strings::CapturesText;
use crate::screens::dialog;
use crate::screens::format;
use crate::screens::rows::{ListEvent, Row};
use crate::screens::scroll::Follow;
use crate::screens::{clamped, right_aligned_hints};
use crate::ui::anim::{AnimExt, Motion};
use crate::ui::chrome::TileImage;
use crate::ui::components::{
    self, ConsoleButton, ControlSize, corner, paint_focus_glow, paint_focus_stroke, paint_hairline,
    paint_spinner_arc,
};
use crate::ui::i18n::{Arg, Language};
use crate::ui::icons::Icon;
use crate::ui::input;
use crate::ui::text;
use crate::ui::theme::{self, color, pad, size, ty, white};

/// The tile a row aims for; the real width fills the row.
const TILE_TARGET: f32 = 300.0;
const TILE_GAP: f32 = 20.0;
/// Room around the tiles for the focus ring and its glow.
const GRID_PADDING: f32 = 14.0;
const CAPTION: f32 = 26.0;
const SECTION_HEADER: f32 = 52.0;
const TILE_RADIUS: f32 = 12.0;
const GAME_ICON: f32 = 30.0;

/// What the pointer did in one frame.
#[derive(Default)]
pub(super) struct Pressed {
    pub filter: Option<usize>,
    pub tile: Option<usize>,
    pub button: bool,
    pub back: bool,
    /// -1 or 1: the arrows beside the picture.
    pub step: Option<i32>,
    pub action: Option<usize>,
    /// The play button on a clip.
    pub play: bool,
    pub confirm: Option<usize>,
}

pub(super) fn metrics(tile_width: f32) -> Metrics {
    Metrics {
        header: SECTION_HEADER,
        tile_row: tile_height(tile_width) + CAPTION,
        row_gap: 18.0,
        section_gap: 34.0,
    }
}

fn tile_height(width: f32) -> f32 {
    (width * 9.0 / 16.0).round()
}

/// The tile width the rows get in a grid `width` wide, and how many fit.
pub(super) fn grid_measures(width: f32) -> (usize, f32) {
    let columns = columns_for(width, TILE_TARGET, TILE_GAP);
    let tile = ((width - TILE_GAP * (columns - 1) as f32) / columns as f32).floor();
    (columns, tile)
}

// --- the states without tiles ------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum State {
    Loading,
    Missing,
    Failed,
    /// The folder holds no capture.
    Empty,
    /// The filter hides every capture there is.
    FilterEmpty,
    Items,
}

pub(super) fn state(data: &CapturesData<'_>) -> State {
    match data.load {
        Load::Loading => State::Loading,
        Load::Missing => State::Missing,
        Load::Failed => State::Failed,
        Load::Ready if !data.any => State::Empty,
        Load::Ready if data.sections.is_empty() => State::FilterEmpty,
        Load::Ready => State::Items,
    }
}

/// The one button of a state without tiles.
pub(super) struct ButtonSpec {
    pub kind: ButtonKind,
    pub label: CapturesText,
}

pub(super) fn button_for(data: &CapturesData<'_>) -> Option<ButtonSpec> {
    let spec = |kind, label| Some(ButtonSpec { kind, label });
    match state(data) {
        State::Loading | State::Items => None,
        State::Missing => spec(
            ButtonKind::Action(CapturesAction::OpenFolderSettings),
            CapturesText::OpenFolderSettings,
        ),
        State::Failed => spec(
            ButtonKind::Action(CapturesAction::Refresh),
            CapturesText::TryAgain,
        ),
        State::Empty if data.clips_available => spec(
            ButtonKind::Action(CapturesAction::OpenClipsSettings),
            CapturesText::OpenClipsSettings,
        ),
        State::Empty => None,
        State::FilterEmpty => spec(ButtonKind::ShowAll, CapturesText::ShowAll),
    }
}

// --- the frame ---------------------------------------------------------------------------------

pub(super) fn draw(
    ui: &mut Ui,
    screen: &mut CapturesScreen,
    data: &mut CapturesData<'_>,
    flat: &[usize],
    pressed: &mut Pressed,
) {
    let area = ui.max_rect().shrink2(vec2(pad::LIBRARY.0, pad::LIBRARY.1));
    match screen.mode {
        Mode::Browse => {
            browse(ui, area, screen, data, flat, pressed);
            dialog::close(ui.ctx(), CONFIRM_CARD);
        }
        Mode::View { place, focus } => {
            view(ui, area, data, flat, place, Some(focus), pressed);
            // The question has just been answered: its card leaves.
            dialog::close(ui.ctx(), CONFIRM_CARD);
        }
        Mode::Confirm { place, button } => {
            view(ui, area, data, flat, place, None, pressed);
            confirm(ui, data, flat, place, button, pressed);
        }
    }
}

/// The title line: the name on the left, and on the right the hints and a button that leaves.
fn header(
    ui: &mut Ui,
    area: Rect,
    language: Language,
    title: &str,
    count: Option<usize>,
    pressed: &mut Pressed,
) -> Rect {
    let painter = ui.painter().clone();
    let title_h = ty::SCREEN_TITLE.line_height();
    let row = Rect::from_min_size(area.min, vec2(area.width(), title_h));
    let shown = text::middle_truncate(&painter, ty::SCREEN_TITLE, title, area.width() * 0.5);
    let title = text::paint(
        &painter,
        pos2(area.min.x, row.center().y),
        Align2::LEFT_CENTER,
        ty::SCREEN_TITLE,
        &shown,
        Color32::WHITE,
    );
    if let Some(count) = count {
        text::paint(
            &painter,
            pos2(title.max.x + 16.0, row.center().y + 2.0),
            Align2::LEFT_CENTER,
            ty::LIBRARY_COUNT,
            &count.to_string(),
            color::text_count(),
        );
    }
    let close = size::CONTROL_M;
    let button = Rect::from_min_size(
        pos2(row.max.x - close, row.center().y - close * 0.5),
        vec2(close, close),
    );
    let mut child = ui.new_child(UiBuilder::new().max_rect(button));
    let name = language.text(crate::ui::i18n::Key::HintBack);
    if ConsoleButton::icon_only(Icon::X, name)
        .size(ControlSize::Medium)
        .show(&mut child)
        .clicked()
    {
        pressed.back = true;
    }
    right_aligned_hints(
        ui,
        Rect::from_min_max(
            row.min,
            pos2(button.min.x - size::CONTROL_GAP * 2.0, row.max.y),
        ),
        language,
    );
    row
}

fn browse(
    ui: &mut Ui,
    area: Rect,
    screen: &mut CapturesScreen,
    data: &mut CapturesData<'_>,
    flat: &[usize],
    pressed: &mut Pressed,
) {
    let language = data.language;
    let state = state(data);
    let head = header(
        ui,
        area,
        language,
        CapturesText::Title.text(language),
        (state == State::Items).then_some(flat.len()),
        pressed,
    );
    // The filters.
    let chips = Rect::from_min_size(
        pos2(area.min.x, head.max.y + 18.0),
        vec2(area.width(), size::CONTROL_M),
    );
    let mut row = ui.new_child(
        UiBuilder::new()
            .max_rect(chips)
            .layout(UiLayout::left_to_right(Align::Center)),
    );
    row.spacing_mut().item_spacing.x = size::CONTROL_GAP;
    for (i, filter) in Filter::ALL.into_iter().enumerate() {
        let chosen = filter == screen.filter;
        let focused = screen.zone == Zone::Filter(i);
        let chip = ConsoleButton::new(filter_text(filter).text(language))
            .size(ControlSize::Medium)
            .prominent(chosen)
            .focused(focused)
            .show(&mut row);
        crate::ui::a11y::follow(ui.ctx(), chip.id, focused);
        if chip.clicked() {
            pressed.filter = Some(i);
        }
    }
    let content = Rect::from_min_max(pos2(area.min.x, chips.max.y + 22.0), area.max);
    if state == State::Items {
        grid(ui, area, content, screen, data, flat, pressed);
    } else {
        status(ui, content, screen, data, state, pressed);
    }
}

// --- the grid ----------------------------------------------------------------------------------

fn grid(
    ui: &mut Ui,
    area: Rect,
    content: Rect,
    screen: &mut CapturesScreen,
    data: &mut CapturesData<'_>,
    flat: &[usize],
    pressed: &mut Pressed,
) {
    let ctx = ui.ctx().clone();
    let (columns, tile_w) = grid_measures(area.width());
    let tile_h = tile_height(tile_w);
    screen.columns = columns;
    screen.tile_w = tile_w;
    let sizes: Vec<usize> = data.sections.iter().map(|s| s.items.len()).collect();
    let layout = Layout::build(&sizes, columns, metrics(tile_w));

    // The list runs to the bottom of the screen, so rows slide out under the edge instead of
    // being cut off above it; the last row can still be scrolled clear of it.
    let list = Rect::from_min_max(
        pos2(content.min.x - GRID_PADDING, content.min.y),
        pos2(content.max.x + GRID_PADDING, ui.max_rect().max.y),
    );
    let foot = ui.max_rect().max.y - content.max.y;
    let mut child = ui.new_child(UiBuilder::new().max_rect(list));
    let id = child.make_persistent_id("captures-scroll");
    let focused_place = match screen.zone {
        Zone::Grid(p) => Some(p),
        _ => None,
    };
    let (follow, scroll_to) =
        Follow::begin(&ctx, id, focused_place.and_then(|p| layout.band_of(p)));
    let mut scroll = egui::ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink([false, false])
        .scroll_bar_visibility(egui::containers::scroll_area::ScrollBarVisibility::AlwaysHidden);
    if let Some(offset) = scroll_to {
        scroll = scroll.vertical_scroll_offset(offset);
    }
    let output = scroll.show(&mut child, |ui| {
        let top = ui.cursor().min.y + GRID_PADDING;
        ui.allocate_exact_size(
            vec2(
                ui.available_width(),
                layout.height + 2.0 * GRID_PADDING + foot,
            ),
            Sense::hover(),
        );
        // Only the rows on screen, and one beyond: a library of thousands costs a handful of tiles.
        let seen = ui.clip_rect();
        let rows = layout.visible(seen.min.y - top - 40.0, seen.max.y - top + 40.0);
        for bi in rows {
            let band = layout.bands[bi];
            let section = &data.sections[band.section];
            if band.header {
                section_header(ui, pos2(area.min.x, top + band.top), section);
            }
            for k in 0..band.len {
                let place = band.first + k;
                let capture = &data.captures[flat[place]];
                let rect = Rect::from_min_size(
                    pos2(
                        area.min.x + k as f32 * (tile_w + TILE_GAP),
                        top + band.tiles_top,
                    ),
                    vec2(tile_w, tile_h),
                );
                let focused = focused_place == Some(place);
                let thumb = data.media.thumb(capture);
                let length = (capture.kind == Kind::Clip)
                    .then(|| data.media.duration(capture))
                    .flatten();
                let label = format!(
                    "{}, {}, {}",
                    section.title,
                    kind_text(capture.kind).text(data.language),
                    when(capture, data)
                );
                let response = tile(
                    ui,
                    TileCx {
                        rect,
                        capture,
                        thumb,
                        length,
                        focused,
                        caption: when(capture, data),
                        label,
                    },
                );
                if response.clicked() {
                    pressed.tile = Some(place);
                }
            }
        }
    });
    follow.end(
        &ctx,
        layout.geometry(),
        output.content_size.y,
        output.inner_rect.height(),
        output.state.clamped_offset().y,
    );
}

fn kind_text(kind: Kind) -> CapturesText {
    match kind {
        Kind::Screenshot => CapturesText::Screenshot,
        Kind::Clip => CapturesText::Clip,
    }
}

fn when(capture: &Capture, data: &CapturesData<'_>) -> String {
    format::message_time(capture.modified, data.now, data.utc_offset, data.language)
}

fn section_header(ui: &mut Ui, at: egui::Pos2, section: &super::Section) {
    let painter = ui.painter().clone();
    let icon = Rect::from_min_size(
        pos2(at.x, at.y + (SECTION_HEADER - GAME_ICON) * 0.5 - 4.0),
        vec2(GAME_ICON, GAME_ICON),
    );
    painter.rect_filled(icon, corner(8.0), color::surface_tile_placeholder());
    match section.icon {
        Some(image) => paint_image(&painter, icon, 8.0, image, 1.0),
        None => {
            painter.text(
                icon.center(),
                Align2::CENTER_CENTER,
                Icon::GAME_CONTROLLER.glyph(),
                Icon::GAME_CONTROLLER.font(16.0, true),
                white(0.5),
            );
        }
    }
    paint_hairline(&painter, icon, 8.0, white(0.10));
    let x = icon.max.x + 12.0;
    let name = text::paint(
        &painter,
        pos2(x, icon.center().y),
        Align2::LEFT_CENTER,
        ty::TROPHY_NAME,
        &section.title,
        Color32::WHITE,
    );
    text::paint(
        &painter,
        pos2(name.max.x + 10.0, icon.center().y),
        Align2::LEFT_CENTER,
        ty::STATE_LABEL,
        &section.items.len().to_string(),
        color::text_count(),
    );
}

struct TileCx<'a> {
    rect: Rect,
    capture: &'a Capture,
    thumb: Thumb,
    length: Option<std::time::Duration>,
    focused: bool,
    caption: String,
    label: String,
}

/// One tile: the picture (or the plain tile), what marks a clip, and the time under it.
fn tile(ui: &mut Ui, tile: TileCx<'_>) -> egui::Response {
    let ctx = ui.ctx().clone();
    let TileCx {
        rect,
        capture,
        thumb,
        length,
        focused,
        caption,
        label,
    } = tile;
    let painter = ui.painter().clone();
    let hit = Rect::from_min_max(rect.min, pos2(rect.max.x, rect.max.y + CAPTION));
    let id = ui.make_persistent_id(("capture", &capture.path));
    let response = ui.interact(hit, id, Sense::click());
    crate::ui::a11y::follow(&ctx, response.id, focused);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label.clone()));
    let hovered = input::hovered(&ctx, &response);
    if hovered {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if !ui.is_rect_visible(hit) {
        return response;
    }
    let focus = ctx.focus_t((id, "focus"), focused);
    let hover = ctx.animate_to((id, "hover"), f32::from(hovered), Motion::Hover);
    paint_focus_glow(&painter, rect, TILE_RADIUS, size::FOCUS_RING_LARGE, focus);
    painter.rect_filled(rect, corner(TILE_RADIUS), color::surface_tile_placeholder());
    // The picture fades in when it arrives; one that is there at once does not.
    let shown = ctx.animate_to(
        (id, "shown"),
        f32::from(matches!(thumb, Thumb::Ready(_))),
        Motion::Hover,
    );
    match thumb {
        Thumb::Ready(image) => paint_image(&painter, rect, TILE_RADIUS, image, shown),
        Thumb::Pending | Thumb::Unavailable => {
            if capture.kind == Kind::Screenshot {
                painter.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    Icon::IMAGE.glyph(),
                    Icon::IMAGE.font(text::quantize_size(rect.height() * 0.28), false),
                    white(0.3),
                );
            }
        }
    }
    if capture.kind == Kind::Clip {
        play_badge(&painter, rect.center(), 44.0, thumb_is_ready(thumb));
        if let Some(length) = length {
            duration_chip(&painter, rect, &duration_text(length));
        }
    }
    paint_hairline(&painter, rect, TILE_RADIUS, white(0.10));
    let ring = focus.max(0.45 * hover);
    paint_focus_stroke(&painter, rect, TILE_RADIUS, size::FOCUS_RING_LARGE, ring);
    // The time, under the tile.
    let ink = if focused {
        Color32::WHITE
    } else {
        color::text_detail()
    };
    let galley = clamped(&painter, ty::STATE_LABEL, &caption, ink, rect.width(), 1);
    painter.galley(pos2(rect.min.x, rect.max.y + 10.0), galley, ink);
    response
}

fn thumb_is_ready(thumb: Thumb) -> bool {
    matches!(thumb, Thumb::Ready(_))
}

/// The disc with a play triangle that marks a clip.
fn play_badge(painter: &egui::Painter, centre: egui::Pos2, diameter: f32, over_picture: bool) {
    painter.circle_filled(
        centre,
        diameter * 0.5,
        theme::black(if over_picture { 0.5 } else { 0.28 }),
    );
    painter.circle_stroke(
        centre,
        diameter * 0.5 - 0.5,
        egui::Stroke::new(1.0, white(0.35)),
    );
    let glyph = diameter * 0.46;
    painter.text(
        centre + vec2(Icon::PLAY.optical_dx(glyph), 0.0),
        Align2::CENTER_CENTER,
        Icon::PLAY.glyph(),
        Icon::PLAY.font(text::quantize_size(glyph), true),
        Color32::WHITE,
    );
}

/// The length in the corner of a clip.
fn duration_chip(painter: &egui::Painter, tile: Rect, text_: &str) {
    let style = ty::STATE_LABEL_SMALL;
    let w = text::tabular_width(painter, style, text_);
    let chip = Rect::from_min_size(
        pos2(tile.max.x - 8.0 - w - 16.0, tile.max.y - 8.0 - 24.0),
        vec2(w + 16.0, 24.0),
    );
    components::paint_capsule(painter, chip, theme::black(0.62));
    text::paint_tabular(
        painter,
        pos2(
            chip.min.x + 8.0,
            chip.center().y - style.line_height() * 0.5,
        ),
        style,
        text_,
        Color32::WHITE,
    );
}

/// A texture that fills `rect`, cropped around the middle, with rounded corners.
fn paint_image(painter: &egui::Painter, rect: Rect, radius: f32, image: TileImage, opacity: f32) {
    let want = rect.width() / rect.height().max(1.0);
    let uv = if image.aspect >= want {
        let w = want / image.aspect;
        Rect::from_min_max(pos2((1.0 - w) * 0.5, 0.0), pos2((1.0 + w) * 0.5, 1.0))
    } else {
        let h = image.aspect / want;
        Rect::from_min_max(pos2(0.0, (1.0 - h) * 0.5), pos2(1.0, (1.0 + h) * 0.5))
    };
    painter.add(
        egui::epaint::RectShape::filled(
            rect,
            corner(radius),
            Color32::WHITE.gamma_multiply(opacity.clamp(0.0, 1.0)),
        )
        .with_texture(image.texture, uv),
    );
}

// --- no tiles ----------------------------------------------------------------------------------

fn status(
    ui: &mut Ui,
    rect: Rect,
    screen: &CapturesScreen,
    data: &CapturesData<'_>,
    state: State,
    pressed: &mut Pressed,
) {
    let painter = ui.painter().clone();
    let language = data.language;
    let (icon, title, body): (Icon, CapturesText, Option<String>) = match state {
        State::Loading => (Icon::IMAGES, CapturesText::Loading, None),
        State::Missing => (
            Icon::FOLDER_SIMPLE_DASHED,
            CapturesText::MissingTitle,
            Some(CapturesText::MissingBody.format(language, &[Arg::Str(data.folder)])),
        ),
        State::Failed => (
            Icon::WARNING,
            CapturesText::FailedTitle,
            Some(CapturesText::FailedBody.format(language, &[Arg::Str(data.folder)])),
        ),
        State::Empty => (
            Icon::IMAGES,
            CapturesText::EmptyTitle,
            Some(
                if data.clips_available {
                    CapturesText::EmptyBody
                } else {
                    CapturesText::EmptyBodyShotsOnly
                }
                .text(language)
                .to_owned(),
            ),
        ),
        State::FilterEmpty => {
            if screen.filter == Filter::Clips {
                (
                    Icon::CAMERA,
                    CapturesText::NoClips,
                    data.clips_available
                        .then(|| CapturesText::NoClipsBody.text(language).to_owned()),
                )
            } else {
                (Icon::IMAGE, CapturesText::NoScreenshots, None)
            }
        }
        State::Items => return,
    };
    let wrap = 520.0f32.min(rect.width() - 40.0);
    let body_galley = body.as_ref().map(|b| centred_galley(&painter, b, wrap));
    let button = button_for(data);
    let body_h = body_galley.as_ref().map_or(0.0, |g| g.size().y + 14.0);
    let button_h = if button.is_some() {
        28.0 + size::CONTROL_L
    } else {
        0.0
    };
    let block = 44.0 + 18.0 + ty::EMPTY_TITLE.line_height() + body_h + button_h;
    let mut y = (rect.center().y - block * 0.5 - 24.0).max(rect.min.y + 8.0);
    let cx = rect.center().x;
    if state == State::Loading {
        let time = ui.ctx().clock();
        paint_spinner_arc(&painter, pos2(cx, y + 22.0), 14.0, time, 3.0, white(0.7));
        ui.ctx().request_repaint();
    } else {
        painter.text(
            pos2(cx, y + 22.0),
            Align2::CENTER_CENTER,
            icon.glyph(),
            icon.font(text::quantize_size(40.0), false),
            white(0.5),
        );
    }
    y += 44.0 + 18.0;
    text::paint(
        &painter,
        pos2(cx, y),
        Align2::CENTER_TOP,
        ty::EMPTY_TITLE,
        title.text(language),
        Color32::WHITE,
    );
    y += ty::EMPTY_TITLE.line_height();
    if let Some(galley) = body_galley {
        y += 14.0;
        let at = pos2(cx - galley.rect.center().x, y);
        let h = galley.size().y;
        painter.galley(at, galley, color::text_empty());
        y += h;
    }
    if let Some(spec) = button {
        y += 28.0;
        let area = Rect::from_min_size(pos2(rect.min.x, y), vec2(rect.width(), size::CONTROL_L));
        let mut row = ui.new_child(
            UiBuilder::new()
                .max_rect(area)
                .layout(UiLayout::top_down(Align::Center)),
        );
        let focused = screen.zone == Zone::Button;
        let response = ConsoleButton::new(spec.label.text(language))
            .prominent(true)
            .focused(focused)
            .show(&mut row);
        crate::ui::a11y::follow(ui.ctx(), response.id, focused);
        if response.clicked() {
            pressed.button = true;
        }
    }
}

/// Centred text, wrapped to `width`, in the colour the host paints it with.
fn centred_galley(
    painter: &egui::Painter,
    text_: &str,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let style = ty::QUESTION;
    let mut job =
        egui::text::LayoutJob::simple(text_.to_owned(), style.font(), Color32::WHITE, width);
    job.halign = Align::Center;
    job.sections[0].format.line_height = Some(style.line_height() + 4.0);
    painter.layout_job(job)
}

// --- one capture -------------------------------------------------------------------------------

fn view(
    ui: &mut Ui,
    area: Rect,
    data: &mut CapturesData<'_>,
    flat: &[usize],
    place: usize,
    focus: Option<ViewFocus>,
    pressed: &mut Pressed,
) {
    let ctx = ui.ctx().clone();
    let painter = ui.painter().clone();
    let language = data.language;
    let capture = &data.captures[flat[place]];
    let total = flat.len();
    let group = data
        .sections
        .iter()
        .find(|s| s.items.contains(&flat[place]));
    let section = group.map_or_else(String::new, |s| s.title.clone());
    // "3 of 4" counts within the game the title names.
    let (within, of) = group.map_or((place, total), |s| {
        (
            s.items.iter().position(|&i| i == flat[place]).unwrap_or(0),
            s.items.len(),
        )
    });

    // The top: where it is from, and when.
    let head = header(ui, area, language, &section, None, pressed);
    let date = format::date_text(capture.modified * 1_000_000, data.utc_offset, language);
    let position = CapturesText::Position.format(
        language,
        &[Arg::Int(within as i64 + 1), Arg::Int(of as i64)],
    );
    let sub_y = head.max.y + 6.0;
    let sub = format!("{date}   {position}");
    text::paint(
        &painter,
        pos2(area.min.x, sub_y),
        Align2::LEFT_TOP,
        ty::STATE_LABEL,
        &sub,
        color::text_subtitle(),
    );

    // The bottom: what can be done.
    let actions_rect = Rect::from_min_size(
        pos2(area.min.x, area.max.y - size::CONTROL_M),
        vec2(area.width(), size::CONTROL_M),
    );
    let buttons = ViewAction::list(capture.kind, capture.deletable());
    centred_row(ui, actions_rect, |row| {
        for (i, action) in buttons.iter().enumerate() {
            let (label, icon) = match action {
                ViewAction::OpenInPlayer => (CapturesText::OpenInPlayer, Icon::PLAY),
                ViewAction::CopyImage => (CapturesText::CopyImage, Icon::IMAGE),
                ViewAction::ShowInFolder => (CapturesText::ShowInFolder, Icon::FOLDER_OPEN),
                ViewAction::Delete => (CapturesText::Delete, Icon::TRASH),
            };
            let focused = focus == Some(ViewFocus::Action(i));
            let response = ConsoleButton::new(label.text(language))
                .icon(icon)
                .icon_fill(*action == ViewAction::OpenInPlayer)
                .size(ControlSize::Medium)
                .prominent(*action == ViewAction::OpenInPlayer)
                .destructive(*action == ViewAction::Delete)
                .focused(focused)
                .show(row);
            crate::ui::a11y::follow(row.ctx(), response.id, focused);
            if response.clicked() {
                pressed.action = Some(i);
            }
        }
    });

    // The file's name, over the buttons.
    let name_y = actions_rect.min.y - 18.0 - ty::DETAIL.line_height();
    let name = text::middle_truncate(
        &painter,
        ty::DETAIL,
        &capture.file_name(),
        area.width() * 0.7,
    );
    text::paint(
        &painter,
        pos2(area.center().x, name_y),
        Align2::CENTER_TOP,
        ty::DETAIL,
        &name,
        color::text_detail(),
    );

    // The middle: the picture, with the arrows beside it.
    let arrow = size::CONTROL_M;
    let media_area = Rect::from_min_max(
        pos2(
            area.min.x + arrow + 24.0,
            sub_y + ty::STATE_LABEL.line_height() + 22.0,
        ),
        pos2(area.max.x - arrow - 24.0, name_y - 18.0),
    );
    let thumb = data.media.thumb(capture);
    let full = data.media.full(capture);
    let aspect = match (full, thumb) {
        (Thumb::Ready(i), _) | (_, Thumb::Ready(i)) => i.aspect,
        _ => 16.0 / 9.0,
    }
    .clamp(0.3, 4.0);
    let fit = contain(media_area, aspect);
    let ready = match (full, thumb) {
        (Thumb::Ready(i), _) | (_, Thumb::Ready(i)) => Some(i),
        _ => None,
    };
    // A picture that has to be waited for fades in; one that is there at once, as when stepping
    // through the captures, is just there.
    let appear = ctx.animate_to(
        ("captures-view-appear", &capture.path),
        f32::from(ready.is_some()),
        Motion::Hover,
    );
    // Behind a picture, black; a clip without one is a plain tile like the grid's.
    if ready.is_some() {
        painter.rect_filled(fit, corner(14.0), theme::black(0.35 * appear));
    } else {
        painter.rect_filled(fit, corner(14.0), color::surface_tile_placeholder());
    }
    match ready {
        Some(image) => paint_image(&painter, fit, 14.0, image, appear),
        None if matches!(full, Thumb::Pending) || matches!(thumb, Thumb::Pending) => {
            paint_spinner_arc(&painter, fit.center(), 14.0, ctx.clock(), 3.0, white(0.7));
            ctx.request_repaint();
        }
        None => {}
    }
    paint_hairline(&painter, fit, 14.0, white(0.10));
    if capture.kind == Kind::Clip {
        let hit = ui.interact(
            fit,
            ui.make_persistent_id(("play", &capture.path)),
            Sense::click(),
        );
        if input::hovered(&ctx, &hit) {
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if hit.clicked() {
            pressed.play = true;
        }
        let focused = focus == Some(ViewFocus::Media);
        let big = 84.0;
        if focused {
            paint_focus_stroke(
                &painter,
                Rect::from_center_size(fit.center(), vec2(big, big)),
                big * 0.5,
                size::FOCUS_RING_LARGE,
                1.0,
            );
        }
        play_badge(&painter, fit.center(), big, ready.is_some());
        if let Some(length) = data.media.duration(capture) {
            duration_chip(&painter, fit, &duration_text(length));
        }
        if ready.is_none() {
            text::paint(
                &painter,
                pos2(fit.center().x, fit.center().y + big * 0.5 + 16.0),
                Align2::CENTER_TOP,
                ty::NOTE,
                CapturesText::ClipNote.text(language),
                color::text_note(),
            );
        }
    } else if focus == Some(ViewFocus::Media) {
        paint_focus_stroke(&painter, fit, 14.0, size::FOCUS_RING_LARGE, 1.0);
    }

    // The arrows.
    let middle = media_area.center().y;
    for (delta, icon, enabled, label) in [
        (-1, Icon::CARET_LEFT, place > 0, CapturesText::Previous),
        (1, Icon::CARET_RIGHT, place + 1 < total, CapturesText::Next),
    ] {
        if !enabled {
            continue;
        }
        let x = if delta < 0 {
            area.min.x
        } else {
            area.max.x - arrow
        };
        let at = Rect::from_min_size(pos2(x, middle - arrow * 0.5), vec2(arrow, arrow));
        let mut child = ui.new_child(UiBuilder::new().max_rect(at));
        if ConsoleButton::icon_only(icon, label.text(language))
            .size(ControlSize::Medium)
            .show(&mut child)
            .clicked()
        {
            pressed.step = Some(delta);
        }
    }
}

/// The largest rectangle of `aspect` that fits in `bounds`, centred.
fn contain(bounds: Rect, aspect: f32) -> Rect {
    let (w, h) = (bounds.width().max(1.0), bounds.height().max(1.0));
    let size = if w / h > aspect {
        vec2(h * aspect, h)
    } else {
        vec2(w, w / aspect)
    };
    Rect::from_center_size(bounds.center(), size)
}

/// A row of controls, centred in `rect` with the usual gap.
fn centred_row(ui: &mut Ui, rect: Rect, mut add: impl FnMut(&mut Ui)) {
    let mut probe = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(UiLayout::left_to_right(Align::Center))
            .invisible(),
    );
    probe.spacing_mut().item_spacing.x = size::CONTROL_GAP;
    add(&mut probe);
    let width = probe.min_rect().width();
    let at = Rect::from_center_size(rect.center(), vec2(width, rect.height()));
    let mut row = ui.new_child(
        UiBuilder::new()
            .max_rect(at)
            .layout(UiLayout::left_to_right(Align::Center)),
    );
    row.spacing_mut().item_spacing.x = size::CONTROL_GAP;
    add(&mut row);
}

/// How much of the card's width the file name in the sentence may take.
const FILE_NAME_SHARE: f32 = 0.8;
/// The animation identity of the delete question's card.
const CONFIRM_CARD: &str = "captures-confirm";

// --- the question before a delete ---------------------------------------------------------------

fn confirm(
    ui: &mut Ui,
    data: &CapturesData<'_>,
    flat: &[usize],
    place: usize,
    button: usize,
    pressed: &mut Pressed,
) {
    let ctx = ui.ctx().clone();
    let painter = ui.painter().clone();
    let language = data.language;
    let capture = &data.captures[flat[place]];
    let (title, verb) = match capture.kind {
        Kind::Screenshot => (
            CapturesText::DeleteScreenshotTitle,
            CapturesText::DeleteScreenshot,
        ),
        Kind::Clip => (CapturesText::DeleteClipTitle, CapturesText::DeleteClip),
    };
    let name = text::middle_truncate(
        &painter,
        ty::QUESTION,
        &capture.file_name(),
        dialog::inner_width(&ctx) * FILE_NAME_SHARE,
    );
    let body = CapturesText::DeleteBody.format(language, &[Arg::Str(&name)]);
    // The card of every question (screens::dialog): Delete first, Cancel last, the focus on the
    // button the model holds (0 = Cancel, 1 = Delete).
    let rows = vec![
        Row::command("delete", verb.text(language)).destructive(),
        Row::command("cancel", CapturesText::Cancel.text(language)).refusal(),
    ];
    let card = dialog::Card {
        id: CONFIRM_CARD,
        title: title.text(language),
        message: Some(&body),
        language,
        focus: usize::from(button == 0),
        force: None,
    };
    for event in dialog::draw(&ctx, true, &card, &rows).unwrap_or_default() {
        if let ListEvent::Effect { row, .. } = event {
            pressed.confirm = Some(usize::from(row == 0));
        }
    }
}
