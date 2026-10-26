//! The trophy comparison (docs/DESIGN.md 17, "Trophy compare"): the player's trophies in one game
//! next to another player's. The game on top; under it a head for each player (who, level and
//! the way to the next, the game's trophies by grade and their points); then one row per trophy
//! with its picture and name from the player's own copy of the game, and for each player whether
//! and when it was earned.
//!
//! What is shown is decided in [`view`], plain data tested without a window; this module lays it
//! out and paints it, moves the focus and reports what the host has to do. Up and down walk the
//! rows, select shows a secret, back and options leave.

mod view;

use std::collections::HashSet;

use anyps5_core::trophies::TrophySet;
use egui::{
    Align, Align2, Color32, Context, Id, Layout, Rect, Role, Sense, Ui, UiBuilder, WidgetInfo,
    pos2, vec2,
};

pub use view::{Body, Cell, CompareView, Inputs, Press, Ready, Row, view};

use crate::screens::compare_strings::CompareText as C;
use crate::screens::online::paint_avatar;
use crate::screens::online_feed_strings::FeedText;
use crate::screens::scroll::Follow;
use crate::screens::side::commands;
use crate::screens::strings::Extra;
use crate::screens::trophies::{IconCache, paint_trophy_icon};
use crate::screens::{BACK_ROW, back_row, clamped};
use crate::ui::Shell;
use crate::ui::anim::{AnimExt, Motion};
use crate::ui::chrome::fullscreen_panel;
use crate::ui::components::{self, ConsoleButton, corner, paint_progress, paint_spinner_arc};
use crate::ui::glass::GlassStyle;
use crate::ui::i18n::{Key, Language};
use crate::ui::icons::Icon;
use crate::ui::input::{Command, InputEvent};
use crate::ui::sound::Cue;
use crate::ui::text;
use crate::ui::theme::{Grade, TextStyle, Weight, color, fade, pad, radius, size, ty, white};

/// Room around the scrolling rows for the focused card's grow and shadow.
const LIST_MARGIN: f32 = 8.0;
/// Rows fade out over this distance at the edge of the list, and start to once the cut is this
/// far over them: the padding above a row's text. A line is gone before it is cut.
const EDGE_FADE: f32 = 20.0;
/// The width of each player's column, and the room right of it.
const COLUMN: f32 = 236.0;
const ROW_PAD: f32 = 18.0;
const ROW_ICON: f32 = 52.0;
const ROW_GAP: f32 = 10.0;
const HEAD_ART: f32 = 96.0;
const HEAD_GAP: f32 = 24.0;
const AVATAR: f32 = 32.0;
const BUTTON_HEIGHT: f32 = 54.0;
/// The "Show" pill on a focused secret, as on the trophy list.
const SHOW_HEIGHT: f32 = 34.0;
const SHOW_PAD: f32 = 18.0;
const SHOW_GAP: f32 = 26.0;
const SHOW_REACH: f32 = 5.0;
const SHOW_RADIUS: f32 = 12.0;

const NAME: TextStyle = TextStyle::new(15.0, Weight::SemiBold);
const FACT: TextStyle = TextStyle::new(14.0, Weight::Medium);
const STATE_TITLE: TextStyle = TextStyle::new(22.0, Weight::Medium);

/// What the host has to do after a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompareAction {
    /// Back or options: leave the screen.
    Close,
    /// Ask the cloud again.
    Retry,
    /// The player showed the secret with this number.
    Revealed(u32),
}

/// How the screen is drawn besides what it shows.
#[derive(Clone, Copy)]
pub struct Look {
    pub language: Language,
    pub accent: Color32,
}

/// The screen: whether it is open, where the focus stands and what the player showed.
#[derive(Default)]
pub struct CompareScreen {
    open: bool,
    closing: bool,
    /// The focused row, or the Try Again button of a failed comparison.
    index: usize,
    icons: IconCache,
    /// The secrets shown since the screen opened, by trophy number.
    revealed: HashSet<u32>,
    /// What the last frame drew, for the keys of the next.
    rows: Vec<Press>,
    /// Counts the visits, so a row's reveal animation never carries over to the next one.
    visit: u32,
    /// The Back button was clicked while drawing: the frame answers it like the Back command.
    back_clicked: bool,
}

impl CompareScreen {
    /// Opens the screen on the first row.
    pub fn open(&mut self) {
        self.open = true;
        self.closing = false;
        self.index = 0;
        self.revealed.clear();
        self.rows.clear();
        self.visit = self.visit.wrapping_add(1);
    }

    pub fn close(&mut self) {
        if self.open {
            self.open = false;
            self.closing = true;
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// True while the screen is open or still fading out; the host keeps calling `show`.
    pub fn is_active(&self) -> bool {
        self.open || self.closing
    }

    /// The secrets shown on this screen, for the view.
    pub fn revealed(&self) -> &HashSet<u32> {
        &self.revealed
    }

    /// Puts the focus on a row (the gallery shows a focused row).
    pub fn set_focus(&mut self, index: usize) {
        self.index = index;
    }

    /// Shows a secret for this visit.
    pub fn reveal(&mut self, number: u32) {
        self.revealed.insert(number);
    }

    pub fn show(
        &mut self,
        shell: &mut Shell,
        ctx: &Context,
        events: &[InputEvent],
        model: &CompareView,
        set: Option<&TrophySet>,
        look: Look,
    ) -> Vec<CompareAction> {
        let Look { language, accent } = look;
        let mut actions = Vec::new();
        if !self.is_active() {
            return actions;
        }
        if self.open {
            for command in commands(events) {
                self.command(shell, command, model, &mut actions);
            }
        }
        let open = self.open;
        let mut clicked = None;
        let mut shown = None;
        let mut rows = Vec::new();
        let drawn = fullscreen_panel(
            ctx,
            "trophy-compare",
            open,
            GlassStyle::fullscreen_panel(),
            |ui, _| {
                self.draw(
                    ui,
                    model,
                    set,
                    language,
                    accent,
                    open,
                    &mut clicked,
                    &mut shown,
                    &mut rows,
                )
            },
        );
        if drawn.is_none() {
            self.closing = false;
        }
        if std::mem::take(&mut self.back_clicked) && open {
            shell.play(Cue::Back);
            actions.push(CompareAction::Close);
        }
        if drawn.is_some() {
            self.rows = rows;
        }
        if open {
            if let Some(i) = clicked {
                if matches!(model.body, Body::Failed) {
                    shell.play(Cue::Select);
                    actions.push(CompareAction::Retry);
                } else if i != self.index {
                    self.index = i;
                    shell.play(Cue::Move);
                }
            }
            if let Some(number) = shown {
                self.reveal(number);
                actions.push(CompareAction::Revealed(number));
                shell.play(Cue::Select);
            }
        }
        actions
    }

    fn command(
        &mut self,
        shell: &mut Shell,
        command: Command,
        model: &CompareView,
        actions: &mut Vec<CompareAction>,
    ) {
        let count = self.rows.len();
        match command {
            // The app answers: "back" to the profile, "close" to Home (7.6).
            Command::Back | Command::Options => {
                actions.push(CompareAction::Close);
            }
            Command::Select => {
                if matches!(model.body, Body::Failed) {
                    shell.play(Cue::Select);
                    actions.push(CompareAction::Retry);
                } else if let Some(Press::Reveal(number)) = self.rows.get(self.index).copied() {
                    self.reveal(number);
                    actions.push(CompareAction::Revealed(number));
                    shell.play(Cue::Select);
                }
            }
            Command::Up | Command::Down => {
                let next = if command == Command::Up {
                    self.index.saturating_sub(1)
                } else {
                    (self.index + 1).min(count.saturating_sub(1))
                };
                if next != self.index {
                    self.index = next;
                    shell.play(Cue::Move);
                }
            }
            Command::Left | Command::Right => {}
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw(
        &mut self,
        ui: &mut Ui,
        model: &CompareView,
        set: Option<&TrophySet>,
        language: Language,
        accent: Color32,
        open: bool,
        clicked: &mut Option<usize>,
        shown: &mut Option<u32>,
        rows: &mut Vec<Press>,
    ) {
        let area = ui
            .max_rect()
            .shrink2(vec2(pad::TROPHIES.0, pad::TROPHIES.1));
        if back_row(ui, area, language) {
            self.back_clicked = true;
        }
        let header = Rect::from_min_size(
            pos2(area.min.x, area.min.y + BACK_ROW),
            vec2(area.width(), HEAD_ART),
        );
        let painter = ui.painter().clone();
        self.draw_header(ui, header, model, language);
        // A whole-screen name for a screen reader, whatever the state.
        ui.interact(header, Id::unique("compare-header"), Sense::hover())
            .widget_info(|| WidgetInfo::labeled(Role::Label, true, model.name.as_str()));
        match &model.body {
            Body::Ready(ready) => {
                let heads_top = header.max.y + HEAD_GAP;
                let heads = self.draw_heads(ui, area, heads_top, ready, accent);
                if let Some(note) = &ready.note {
                    let width =
                        (area.max.x - ROW_PAD - 2.0 * COLUMN - 32.0 - area.min.x).max(200.0);
                    let galley = clamped(&painter, ty::NOTE, note, Color32::PLACEHOLDER, width, 3);
                    painter.galley(
                        pos2(area.min.x, heads_top + 4.0),
                        galley,
                        color::text_detail(),
                    );
                }
                let list = Rect::from_min_max(
                    pos2(area.min.x - LIST_MARGIN, heads.max.y + 16.0 - LIST_MARGIN),
                    pos2(area.max.x + LIST_MARGIN, area.max.y),
                );
                let mut child = ui.new_child(UiBuilder::new().max_rect(list));
                self.draw_rows(
                    &mut child, ready, set, language, accent, open, clicked, shown, rows,
                );
            }
            _ => self.draw_state(ui, area, header, model, language, open, clicked),
        }
    }

    fn draw_header(&self, ui: &mut Ui, header: Rect, model: &CompareView, language: Language) {
        let painter = ui.painter().clone();
        let art = Rect::from_min_size(header.min, vec2(HEAD_ART, HEAD_ART));
        paint_trophy_icon(ui, art, model.art, true);
        let overline = ty::TROPHIES_OVERLINE;
        let title = ty::TROPHY_LIST_TITLE;
        let sub = ty::ROW_INFO;
        let total = overline.line_height() + 10.0 + title.line_height() + 8.0 + sub.line_height();
        let x = art.max.x + 26.0;
        let top = header.center().y - total * 0.5;
        text::paint(
            &painter,
            pos2(x, top),
            Align2::LEFT_TOP,
            overline,
            &text::to_upper(C::Overline.text(language), language),
            color::text_overline(),
        );
        let galley = clamped(
            &painter,
            title,
            &model.game,
            Color32::PLACEHOLDER,
            (header.max.x - x).max(200.0),
            1,
        );
        let title_y = top + overline.line_height() + 10.0;
        let title_h = galley.size().y;
        painter.galley(pos2(x, title_y), galley, Color32::WHITE);
        if !model.handle.is_empty() {
            let line = C::With.format(language, &[crate::ui::i18n::Arg::Str(&model.handle)]);
            text::paint(
                &painter,
                pos2(x, title_y + title_h + 8.0),
                Align2::LEFT_TOP,
                sub,
                &line,
                color::text_subtitle(),
            );
        }
    }

    /// The two heads over the columns; returns the rectangle they take.
    fn draw_heads(
        &self,
        ui: &mut Ui,
        area: Rect,
        top: f32,
        ready: &Ready,
        accent: Color32,
    ) -> Rect {
        let painter = ui.painter().clone();
        let left = area.max.x - ROW_PAD - 2.0 * COLUMN;
        let mut bottom = top;
        for (n, side) in [&ready.you, &ready.them].into_iter().enumerate() {
            let x = left + n as f32 * COLUMN;
            let mut y = top;
            // Who.
            let avatar = Rect::from_min_size(pos2(x, y), vec2(AVATAR, AVATAR));
            paint_avatar(&painter, avatar, side.avatar, side.account, &side.handle);
            let name = clamped(
                &painter,
                NAME,
                &side.name,
                Color32::PLACEHOLDER,
                COLUMN - AVATAR - 10.0 - 12.0,
                1,
            );
            let name_h = name.size().y;
            painter.galley(
                pos2(avatar.max.x + 10.0, avatar.center().y - name_h * 0.5),
                name,
                Color32::WHITE,
            );
            y += AVATAR + 12.0;
            // The level, its points and the way to the next.
            let level = text::paint(
                &painter,
                pos2(x, y),
                Align2::LEFT_TOP,
                FACT,
                &side.level_text,
                white(0.92),
            );
            text::paint_tabular(
                &painter,
                pos2(level.max.x + 10.0, y),
                FACT,
                &side.points_text,
                color::text_detail(),
            );
            y += FACT.line_height() + 8.0;
            let bar = Rect::from_min_size(pos2(x, y), vec2(COLUMN - 40.0, 4.0));
            paint_progress(&painter, bar, side.fraction, accent);
            y += 4.0 + 12.0;
            // This game's trophies by grade and their points.
            let mut cx = x;
            for (grade, count) in Grade::ALL.into_iter().zip(side.grades) {
                if count == 0 {
                    continue;
                }
                painter.text(
                    pos2(cx + 6.5, y + 8.0),
                    Align2::CENTER_CENTER,
                    Icon::TROPHY.glyph(),
                    Icon::TROPHY.font(13.0, true),
                    grade.color(),
                );
                cx += 17.0;
                let w = text::paint_tabular(
                    &painter,
                    pos2(cx, y),
                    ty::DETAIL_LARGE,
                    &count.to_string(),
                    white(0.8),
                )
                .width();
                cx += w + 12.0;
            }
            y += 20.0 + 4.0;
            text::paint(
                &painter,
                pos2(x, y),
                Align2::LEFT_TOP,
                ty::DETAIL_LARGE,
                &side.game_points_text,
                color::text_detail(),
            );
            y += ty::DETAIL_LARGE.line_height();
            bottom = bottom.max(y);
            ui.interact(
                Rect::from_min_max(pos2(x, top), pos2(x + COLUMN - 12.0, y)),
                Id::unique(("compare-head", n)),
                Sense::hover(),
            )
            .widget_info(|| WidgetInfo::labeled(Role::Label, true, side.label.as_str()));
        }
        Rect::from_min_max(pos2(area.min.x, top), pos2(area.max.x, bottom))
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_rows(
        &mut self,
        ui: &mut Ui,
        ready: &Ready,
        set: Option<&TrophySet>,
        language: Language,
        accent: Color32,
        open: bool,
        clicked: &mut Option<usize>,
        shown: &mut Option<u32>,
        presses: &mut Vec<Press>,
    ) {
        let ctx = ui.ctx().clone();
        let id = ui.make_persistent_id("compare-rows");
        let focus = open.then_some(self.index.min(ready.rows.len().saturating_sub(1)));
        let (follow, scroll_to) = Follow::begin(&ctx, id, focus);
        let mut area = egui::ScrollArea::vertical()
            .id_salt(id)
            .auto_shrink([false, false])
            .scroll_bar_visibility(
                egui::containers::scroll_area::ScrollBarVisibility::AlwaysHidden,
            );
        if let Some(offset) = scroll_to {
            area = area.vertical_scroll_offset(offset);
        }
        let mut geometry = Vec::with_capacity(ready.rows.len());
        // The rows are as high as the last frame left them (they are all alike).
        let height_id = id.with("row-height");
        let row_height: f32 = ctx.data(|d| d.get_temp(height_id)).unwrap_or(80.0);
        let clip = ui.clip_rect().intersect(ui.max_rect()).shrink(LIST_MARGIN);
        let output = area.show(ui, |ui| {
            egui::Frame::NONE
                .inner_margin(egui::Margin::same(LIST_MARGIN as i8))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = ROW_GAP;
                    let origin = ui.cursor().min.y;
                    for (i, row) in ready.rows.iter().enumerate() {
                        presses.push(row.press);
                        let focused = open && i == self.index;
                        let top = ui.cursor().min.y;
                        let edge = (top - clip.min.y).min(clip.max.y - (top + row_height));
                        let visible = ((edge + EDGE_FADE) / EDGE_FADE).clamp(0.0, 1.0);
                        let (rect, pill) = ui
                            .scope(|ui| {
                                ui.multiply_opacity(visible);
                                self.draw_row(ui, row, set, language, accent, focused, i)
                            })
                            .inner;
                        ctx.data_mut(|d| d.insert_temp(height_id, rect.height()));
                        geometry.push((rect.min.y - origin, rect.height()));
                        let pressable = row.press != Press::None;
                        let response =
                            ui.interact(rect, id.with(("compare-row", i)), Sense::click());
                        response.widget_info(|| {
                            WidgetInfo::labeled(
                                if pressable { Role::Button } else { Role::Label },
                                true,
                                row.label.as_str(),
                            )
                        });
                        if response.clicked() {
                            *clicked = Some(i);
                        }
                        // After the row, so the pill wins a click on both.
                        if let (Some(pill), Press::Reveal(number)) = (pill, row.press) {
                            let name = format!(
                                "{}, {}",
                                Extra::TrophyShow.text(language),
                                language.text(Key::TrophyHidden)
                            );
                            let show = ui.interact(
                                pill.expand(SHOW_REACH),
                                id.with(("compare-show", i)),
                                Sense::click(),
                            );
                            show.widget_info(|| {
                                WidgetInfo::labeled(Role::Button, true, name.as_str())
                            });
                            if show.clicked() {
                                *shown = Some(number);
                                *clicked = None;
                            }
                        }
                    }
                });
        });
        follow.end(
            &ctx,
            geometry,
            output.content_size.y,
            output.inner_rect.height(),
            output.state.clamped_offset().y,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_row(
        &mut self,
        ui: &mut Ui,
        row: &Row,
        set: Option<&TrophySet>,
        language: Language,
        _accent: Color32,
        focused: bool,
        index: usize,
    ) -> (Rect, Option<Rect>) {
        let ctx = ui.ctx().clone();
        let painter = ui.painter().clone();
        let width = ui.available_width();
        let show_label = Extra::TrophyShow.text(language);
        let show_style = TextStyle::new(15.0, Weight::SemiBold);
        let show_w = if row.secret {
            text::width(&painter, show_style, show_label) + 2.0 * SHOW_PAD
        } else {
            0.0
        };
        let cells_left_offset = ROW_PAD + 2.0 * COLUMN;
        let text_x = ROW_PAD + ROW_ICON + 18.0;
        let mut text_w = (width - text_x - cells_left_offset - 16.0).max(80.0);
        if row.secret {
            text_w = (text_w - show_w - SHOW_GAP).max(80.0);
        }
        let name_g = clamped(
            &painter,
            ty::TROPHY_NAME,
            &row.name,
            Color32::PLACEHOLDER,
            text_w,
            1,
        );
        let detail_g = clamped(
            &painter,
            ty::DETAIL_LARGE,
            &row.detail,
            Color32::PLACEHOLDER,
            text_w,
            2,
        );
        let block = name_g.size().y + 6.0 + detail_g.size().y;
        let height = ROW_ICON.max(block) + 28.0;
        let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        if !ui.is_rect_visible(rect) {
            return (rect, None);
        }
        let t = ctx.focus_t(("compare-row-focus", index), focused);
        let under_pointer = ctx.pointer_hover_pos().is_some_and(|p| rect.contains(p));
        let hover_t = ctx.animate_to(
            ("compare-row-hover", index),
            f32::from(under_pointer),
            Motion::Hover,
        );
        components::paint_surface(
            &painter,
            rect,
            radius::TROPHY_ROW,
            color::surface_trophy_row(),
            hover_t,
            t,
            0.0,
            size::FOCUS_RING,
        );
        let you_earned = matches!(row.you, Cell::Earned(_));

        // The picture, or the lock of a secret.
        let icon_rect = Rect::from_center_size(
            pos2(rect.min.x + ROW_PAD + ROW_ICON * 0.5, rect.center().y),
            vec2(ROW_ICON, ROW_ICON),
        );
        if row.secret {
            painter.rect_filled(icon_rect, corner(12.0), white(0.07));
            painter.text(
                icon_rect.center(),
                Align2::CENTER_CENTER,
                Icon::LOCK.glyph(),
                Icon::LOCK.font(22.0, true),
                white(0.5),
            );
        } else {
            let trophy = row.index.and_then(|i| set.and_then(|s| s.trophies.get(i)));
            let texture = trophy.and_then(|t| {
                let key = format!("compare/{}/{}", t.service_label, t.number);
                self.icons.get(&ctx, &key, t.icon.as_deref(), you_earned)
            });
            paint_trophy_icon(ui, icon_rect, texture, you_earned);
        }

        // Name and detail.
        let x = rect.min.x + text_x;
        let top = rect.center().y - block * 0.5;
        let name_h = name_g.size().y;
        painter.galley(
            pos2(x, top),
            name_g,
            fade(Color32::WHITE, if you_earned { 1.0 } else { 0.86 }),
        );
        painter.galley(pos2(x, top + name_h + 6.0), detail_g, color::text_detail());
        if let Some(grade) = row.grade
            && !row.secret
        {
            // The grade's colour on the picture's corner: never the only cue, the detail says it.
            painter.circle_filled(
                icon_rect.right_bottom() - vec2(4.0, 4.0),
                5.0,
                grade.color(),
            );
        }

        // The two columns.
        let cells_left = rect.max.x - cells_left_offset;
        for (n, cell) in [&row.you, &row.them].into_iter().enumerate() {
            let cx = cells_left + n as f32 * COLUMN;
            match cell {
                Cell::Earned(day) => {
                    painter.text(
                        pos2(cx + 9.0, rect.center().y),
                        Align2::CENTER_CENTER,
                        Icon::CHECK_CIRCLE.glyph(),
                        Icon::CHECK_CIRCLE.font(17.0, true),
                        white(0.88),
                    );
                    text::paint(
                        &painter,
                        pos2(cx + 28.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        ty::DETAIL_LARGE,
                        day,
                        white(0.88),
                    );
                }
                Cell::NotYet => {
                    text::paint(
                        &painter,
                        pos2(cx + 28.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        ty::DETAIL_LARGE,
                        C::NotYet.text(language),
                        color::text_detail(),
                    );
                }
            }
        }

        // The "Show" pill of a focused secret, left of the columns.
        let mut pill = None;
        if row.secret && t > 0.0 {
            let right_edge = cells_left - SHOW_GAP;
            let mut area = Rect::from_center_size(
                pos2(right_edge - show_w * 0.5, rect.center().y),
                vec2(show_w, SHOW_HEIGHT),
            );
            let hovered = focused
                && ctx
                    .pointer_hover_pos()
                    .is_some_and(|p| area.expand(SHOW_REACH).contains(p));
            if hovered && ctx.input(|i| i.pointer.primary_down()) {
                area = area.shrink2(vec2(show_w, SHOW_HEIGHT) * 0.02);
            }
            let fill = white(if hovered { 0.28 } else { 0.18 });
            painter.rect_filled(area, corner(SHOW_RADIUS), fade(fill, t));
            text::paint(
                &painter,
                area.center(),
                Align2::CENTER_CENTER,
                show_style,
                show_label,
                fade(Color32::WHITE, t),
            );
            pill = focused.then_some(area);
        }
        (rect, pill)
    }

    /// Loading, offline, failed, hidden and empty: one block of words under the header.
    #[allow(clippy::too_many_arguments)]
    fn draw_state(
        &self,
        ui: &mut Ui,
        area: Rect,
        header: Rect,
        model: &CompareView,
        language: Language,
        open: bool,
        clicked: &mut Option<usize>,
    ) {
        let ctx = ui.ctx().clone();
        let painter = ui.painter().clone();
        let handle = crate::ui::i18n::Arg::Str(&model.handle);
        let (icon, title, note): (Icon, String, String) = match &model.body {
            Body::Loading => (
                Icon::CIRCLE_DASHED,
                C::LoadingNote.text(language).to_owned(),
                String::new(),
            ),
            Body::Offline => (
                Icon::CLOUD,
                C::OfflineTitle.text(language).to_owned(),
                C::OfflineNote.text(language).to_owned(),
            ),
            Body::Failed => (
                Icon::WARNING,
                C::FailedTitle.text(language).to_owned(),
                C::FailedNote.text(language).to_owned(),
            ),
            Body::Hidden => (
                Icon::LOCK,
                C::HiddenTitle.text(language).to_owned(),
                C::HiddenNote.format(language, &[handle]),
            ),
            Body::Empty => (
                Icon::TROPHY,
                C::EmptyTitle.text(language).to_owned(),
                C::EmptyNote.text(language).to_owned(),
            ),
            Body::Ready(_) => return,
        };
        // A screen reader hears the words of the state, which are only painted.
        let block_rect = Rect::from_center_size(
            pos2(area.center().x, header.max.y + 120.0),
            vec2(560.0, 240.0),
        );
        let said = if note.is_empty() {
            title.clone()
        } else {
            format!("{title}. {note}")
        };
        ui.interact(block_rect, Id::unique("compare-state"), Sense::hover())
            .widget_info(|| WidgetInfo::labeled(Role::Label, true, said.as_str()));
        let failed = matches!(model.body, Body::Failed);
        let block =
            56.0 + 18.0 + 30.0 + 8.0 + 44.0 + if failed { 24.0 + BUTTON_HEIGHT } else { 0.0 };
        let body_top = header.max.y + 28.0;
        let room = area.max.y - body_top;
        let top = body_top + ((room - block) * 0.5 - 24.0).max(12.0);
        let centre = area.center().x;
        let disc = Rect::from_center_size(pos2(centre, top + 28.0), vec2(56.0, 56.0));
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
        let galley = clamped(
            &painter,
            STATE_TITLE,
            &title,
            Color32::PLACEHOLDER,
            520.0,
            1,
        );
        let title_h = galley.size().y;
        painter.galley(
            pos2(centre - galley.size().x * 0.5, y),
            galley,
            Color32::WHITE,
        );
        y += title_h + 8.0;
        if !note.is_empty() {
            let mut job =
                egui::text::LayoutJob::simple(note, ty::NOTE.font(), Color32::PLACEHOLDER, 420.0);
            job.halign = Align::Center;
            job.sections[0].format.line_height = Some(1.45 * ty::NOTE.size);
            let galley = painter.layout_job(job);
            painter.galley(pos2(centre, y), galley, color::text_detail());
        }
        if failed {
            let label = FeedText::TryAgain.text(language);
            let w = text::width(&painter, ty::BUTTON, label) + 2.0 * size::BUTTON_PADDING;
            let button_y = top + 56.0 + 18.0 + 30.0 + 8.0 + 44.0 + 24.0;
            let mut row = ui.new_child(
                UiBuilder::new()
                    .max_rect(Rect::from_min_size(
                        pos2(centre - w * 0.5 - 3.0, button_y),
                        vec2(w + 12.0, BUTTON_HEIGHT + 8.0),
                    ))
                    .layout(Layout::left_to_right(Align::Min)),
            );
            let response = ConsoleButton::new(label)
                .prominent(true)
                .focused(open)
                .show(&mut row);
            if response.clicked() {
                *clicked = Some(0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_screen_starts_closed_and_opens_on_the_first_row() {
        let mut screen = CompareScreen::default();
        assert!(!screen.is_active());
        screen.reveal(3);
        screen.open();
        assert!(screen.is_open() && screen.revealed().is_empty());
        screen.close();
        assert!(
            !screen.is_open() && screen.is_active(),
            "it fades out first"
        );
    }
}
