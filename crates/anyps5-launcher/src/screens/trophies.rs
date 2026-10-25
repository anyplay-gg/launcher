//! The trophy list (docs/DESIGN.md 5.11): the set's icon, title and progress on top, one row per
//! trophy below with its icon, grade and earned date. Secret trophies stay locked until earned,
//! unless the player reveals one (the "Show" pill on the focused row, or select) for this visit, or
//! has "Always Show Hidden Trophies" on. Nothing of that leaves the launcher.
//! Up and down move, select reveals a secret, back and options close; clicking a row focuses it.
//!
//! The data is the core's [`TrophySet`]; trophy icons are PNG bytes inside it, decoded here into
//! textures (greyscale and dimmed until a trophy is earned).

use std::collections::{HashMap, HashSet};

use anyps5_core::trophies::{Trophy, TrophySet};
use egui::{
    Align2, Color32, ColorImage, Context, Rect, Sense, TextureHandle, TextureId, TextureOptions,
    Ui, UiBuilder, pos2, vec2,
};

use crate::screens::clamped;
use crate::screens::convert;
use crate::screens::format;
use crate::screens::scroll::Follow;
use crate::screens::side::commands;
use crate::screens::strings::Extra;
use crate::ui::Shell;
use crate::ui::anim::{AnimExt, Motion};
use crate::ui::chrome::fullscreen_panel;
use crate::ui::components::{self, corner, paint_progress};
use crate::ui::glass::GlassStyle;
use crate::ui::i18n::{Key, Language};
use crate::ui::icons::Icon;
use crate::ui::input::{Command, InputEvent};
use crate::ui::sound::Cue;
use crate::ui::text;
use crate::ui::theme::{self, Grade, color, fade, radius, size, ty, white};

/// Room around the scrolling rows for the focused card's grow and shadow.
const LIST_MARGIN: f32 = 8.0;

/// The "Show" pill on a focused secret: height, side padding, and the gap to the grade column.
const SHOW_HEIGHT: f32 = 34.0;
const SHOW_PAD: f32 = 18.0;
const SHOW_GAP: f32 = 26.0;
/// The pill is a small target; the click area reaches this far past it.
const SHOW_REACH: f32 = 5.0;
/// Concentric with the row's radius 16 at the pill's distance from the card edge.
const SHOW_RADIUS: f32 = 12.0;

/// What the host has to do after a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrophyAction {
    /// The list closed (back, options).
    Close,
}

/// The set to show, borrowed for one frame.
pub struct TrophyData<'a> {
    /// Identifies the game in the icon cache (the game id).
    pub game_key: &'a str,
    pub game_name: &'a str,
    pub set: &'a TrophySet,
    pub language: Language,
    pub accent: Color32,
    /// "Always Show Hidden Trophies": no trophy is secret.
    pub show_hidden: bool,
}

/// Decodes and keeps trophy icon textures.
#[derive(Default)]
pub struct IconCache {
    textures: HashMap<(String, bool), Option<TextureHandle>>,
}

impl IconCache {
    /// The texture of an icon, decoded on first use. Unearned icons are greyscale.
    pub fn get(
        &mut self,
        ctx: &Context,
        key: &str,
        data: Option<&[u8]>,
        earned: bool,
    ) -> Option<TextureId> {
        self.textures
            .entry((key.to_owned(), earned))
            .or_insert_with(|| {
                let image = decode_icon(data?, earned)?;
                Some(ctx.load_texture(
                    format!("trophy/{key}/{earned}"),
                    image,
                    TextureOptions::LINEAR,
                ))
            })
            .as_ref()
            .map(TextureHandle::id)
    }
}

/// PNG bytes to an image, greyscale when `earned` is false.
pub fn decode_icon(data: &[u8], earned: bool) -> Option<ColorImage> {
    let decoded = image::load_from_memory(data).ok()?.to_rgba8();
    let (w, h) = (decoded.width() as usize, decoded.height() as usize);
    let mut pixels = decoded.into_raw();
    if !earned {
        for px in pixels.as_chunks_mut::<4>().0 {
            let luma =
                (0.2126 * f32::from(px[0]) + 0.7152 * f32::from(px[1]) + 0.0722 * f32::from(px[2]))
                    .round() as u8;
            px[0] = luma;
            px[1] = luma;
            px[2] = luma;
        }
    }
    Some(ColorImage::from_rgba_unmultiplied([w, h], &pixels))
}

/// Paints a trophy icon tile: the picture (or a trophy glyph) clipped to a rounded square.
pub fn paint_trophy_icon(ui: &mut Ui, rect: Rect, texture: Option<TextureId>, earned: bool) {
    let side = rect.width();
    let r = radius::trophy_icon(side);
    ui.painter().rect_filled(rect, corner(r), white(0.07));
    match texture {
        Some(id) => {
            let tint = if earned {
                Color32::WHITE
            } else {
                fade(Color32::WHITE, 0.45)
            };
            egui::Image::new(egui::load::SizedTexture::new(id, rect.size()))
                .corner_radius(corner(r))
                .tint(tint)
                .paint_at(ui, rect);
        }
        None => {
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                Icon::TROPHY.glyph(),
                Icon::TROPHY.font(side * 0.38, true),
                white(0.4),
            );
        }
    }
}

/// The accessible name of a trophy row: name, grade and whether it is earned.
fn trophy_label(trophy: &Trophy, secret: bool, language: Language) -> String {
    let name = if secret {
        language.text(Key::TrophyHidden)
    } else {
        trophy.name.as_str()
    };
    let grade = convert::grade(trophy.grade).title(language);
    let state = match trophy.unlocked {
        Some(micros) => format::local_date_text(micros, language),
        None => language.text(Key::TrophyLocked).to_owned(),
    };
    format!("{name}, {grade}, {state}")
}

/// The trophy list overlay.
#[derive(Default)]
pub struct TrophyListPanel {
    open: bool,
    closing: bool,
    index: usize,
    icons: IconCache,
    /// Secrets the player revealed since the list opened (by position in the set).
    revealed: HashSet<usize>,
    /// Counts the visits, so a row's reveal animation never carries over to the next one.
    visit: u32,
}

impl TrophyListPanel {
    /// Opens on the first trophy (the original keeps the index of the last visit; `open_at` does too).
    pub fn open(&mut self) {
        self.open_at(self.index);
    }

    pub fn open_at(&mut self, index: usize) {
        self.open = true;
        self.closing = false;
        self.index = index;
        self.revealed.clear();
        self.visit = self.visit.wrapping_add(1);
    }

    /// Reveals one secret for this visit.
    pub fn reveal(&mut self, index: usize) {
        self.revealed.insert(index);
    }

    pub fn close(&mut self) {
        if self.open {
            self.open = false;
            self.closing = true;
        }
    }

    /// Whether the trophy at `index` still shows as a secret.
    fn is_secret(&self, trophy: &Trophy, index: usize, data: &TrophyData<'_>) -> bool {
        trophy.hidden
            && !trophy.is_unlocked()
            && !data.show_hidden
            && !self.revealed.contains(&index)
    }

    pub fn is_active(&self) -> bool {
        self.open || self.closing
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn show(
        &mut self,
        shell: &mut Shell,
        ctx: &Context,
        events: &[InputEvent],
        data: &TrophyData<'_>,
    ) -> Vec<TrophyAction> {
        let mut actions = Vec::new();
        if !self.is_active() {
            return actions;
        }
        let count = data.set.trophies.len();
        self.index = self.index.min(count.saturating_sub(1));
        if self.open {
            for command in commands(events) {
                match command {
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
                    Command::Back | Command::Options => {
                        shell.play(Cue::Back);
                        self.close();
                        actions.push(TrophyAction::Close);
                    }
                    Command::Select => {
                        let secret = data
                            .set
                            .trophies
                            .get(self.index)
                            .is_some_and(|t| self.is_secret(t, self.index, data));
                        if secret {
                            self.reveal(self.index);
                            shell.play(Cue::Select);
                        }
                    }
                    Command::Left | Command::Right => {}
                }
            }
        }
        let open = self.open;
        let mut clicked = None;
        let mut shown = None;
        let drawn = fullscreen_panel(
            ctx,
            "trophy-list",
            open,
            GlassStyle::fullscreen_panel(),
            |ui, _| self.draw(ui, data, &mut clicked, &mut shown),
        );
        if drawn.is_none() {
            self.closing = false;
        }
        if let Some(i) = clicked.filter(|i| open && *i != self.index) {
            self.index = i;
            shell.play(Cue::Move);
        }
        if let Some(i) = shown.filter(|_| open) {
            self.reveal(i);
            shell.play(Cue::Select);
        }
        actions
    }

    fn draw(
        &mut self,
        ui: &mut Ui,
        data: &TrophyData<'_>,
        clicked: &mut Option<usize>,
        shown: &mut Option<usize>,
    ) {
        let area = ui
            .max_rect()
            .shrink2(vec2(theme::pad::TROPHIES.0, theme::pad::TROPHIES.1));
        let header = Rect::from_min_size(area.min, vec2(area.width(), size::TROPHY_HEADER_ICON));
        self.draw_header(ui, header, data);
        // The scroll margin puts the rows on the header's edges.
        let list = Rect::from_min_max(
            pos2(area.min.x - LIST_MARGIN, header.max.y + 28.0 - LIST_MARGIN),
            pos2(area.max.x + LIST_MARGIN, area.max.y),
        );
        let mut child = ui.new_child(UiBuilder::new().max_rect(list));
        self.draw_rows(&mut child, data, clicked, shown);
    }

    fn draw_header(&mut self, ui: &mut Ui, header: Rect, data: &TrophyData<'_>) {
        let ctx = ui.ctx().clone();
        let language = data.language;
        let set = data.set;
        let painter = ui.painter().clone();
        let icon_rect = Rect::from_min_size(header.min, vec2(header.height(), header.height()));
        let texture = self.icons.get(
            &ctx,
            &format!("{}/set", data.game_key),
            set.icon.as_deref(),
            true,
        );
        paint_trophy_icon(ui, icon_rect, texture, true);

        // Overline, title and the progress row, centred against the icon.
        let overline = ty::TROPHIES_OVERLINE;
        let title_style = ty::TROPHY_LIST_TITLE;
        let row_h = ty::PERCENT.line_height();
        let total = overline.line_height() + 12.0 + title_style.line_height() + 12.0 + row_h;
        let x = icon_rect.max.x + 26.0;
        let top = header.center().y - total * 0.5;
        text::paint(
            &painter,
            pos2(x, top),
            Align2::LEFT_TOP,
            overline,
            &text::to_upper(language.text(Key::Trophies), language),
            color::text_overline(),
        );
        let title = if set.title.is_empty() {
            data.game_name
        } else {
            set.title.as_str()
        };
        let hint_w = 520.0;
        let title_galley = clamped(
            &painter,
            title_style,
            title,
            Color32::PLACEHOLDER,
            (header.max.x - x - hint_w).max(200.0),
            1,
        );
        let title_y = top + overline.line_height() + 12.0;
        painter.galley(pos2(x, title_y), title_galley, Color32::WHITE);

        let row_y = title_y + title_style.line_height() + 12.0;
        let percent = set.percent();
        let mut cx = x;
        let percent_text = format!("{percent}%");
        let w = text::tabular_width(&painter, ty::PERCENT, &percent_text);
        text::paint_tabular(
            &painter,
            pos2(cx, row_y),
            ty::PERCENT,
            &percent_text,
            Color32::WHITE,
        );
        cx += w + 16.0;
        let bar = Rect::from_center_size(pos2(cx + 130.0, row_y + row_h * 0.5), vec2(260.0, 4.0));
        paint_progress(&painter, bar, percent as f32 / 100.0, data.accent);
        cx = bar.max.x + 16.0;
        let count_style = theme::TextStyle::new(15.0, theme::Weight::SemiBold);
        for grade in anyps5_core::trophies::Grade::ALL {
            let (unlocked, total) = set.count(grade);
            if total == 0 {
                continue;
            }
            let ui_grade = convert::grade(grade);
            let mid = row_y + row_h * 0.5;
            painter.text(
                pos2(cx + 8.0, mid),
                Align2::CENTER_CENTER,
                Icon::TROPHY.glyph(),
                Icon::TROPHY.font(15.0, true),
                ui_grade.color(),
            );
            cx += 22.0;
            let label = format!("{unlocked}/{total}");
            let w = text::tabular_width(&painter, count_style, &label);
            text::paint_tabular(
                &painter,
                pos2(cx, mid - count_style.line_height() * 0.5),
                count_style,
                &label,
                fade(Color32::WHITE, 0.85),
            );
            cx += w + 16.0;
        }

        // The hint bar, right aligned and centred on the header.
        crate::screens::right_aligned_hints(ui, header, language);
    }

    fn draw_rows(
        &mut self,
        ui: &mut Ui,
        data: &TrophyData<'_>,
        clicked: &mut Option<usize>,
        shown: &mut Option<usize>,
    ) {
        let ctx = ui.ctx().clone();
        let id = ui.make_persistent_id("trophy-rows");
        let (follow, scroll_to) = Follow::begin(&ctx, id, Some(self.index));
        let mut area = egui::ScrollArea::vertical()
            .id_salt(id)
            .auto_shrink([false, false])
            .scroll_bar_visibility(
                egui::containers::scroll_area::ScrollBarVisibility::AlwaysHidden,
            );
        if let Some(offset) = scroll_to {
            area = area.vertical_scroll_offset(offset);
        }
        let mut geometry = Vec::with_capacity(data.set.trophies.len());
        let output = area.show(ui, |ui| {
            egui::Frame::NONE
                .inner_margin(egui::Margin::same(LIST_MARGIN as i8))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 10.0;
                    let origin = ui.cursor().min.y;
                    for (i, trophy) in data.set.trophies.iter().enumerate() {
                        let (rect, pill) = self.draw_row(ui, data, trophy, i, i == self.index);
                        geometry.push((rect.min.y - origin, rect.height()));
                        // Hit-test after drawing so the click reads the row's own response.
                        let response =
                            ui.interact(rect, id.with(("trophy-row", i)), Sense::click());
                        let secret = self.is_secret(trophy, i, data);
                        let label = trophy_label(trophy, secret, data.language);
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::Role::Button, true, label.as_str())
                        });
                        if response.clicked() {
                            *clicked = Some(i);
                        }
                        // After the row, so the pill wins a click on both.
                        if let Some(pill) = pill {
                            let name = format!(
                                "{}, {}",
                                Extra::TrophyShow.text(data.language),
                                data.language.text(Key::TrophyHidden)
                            );
                            let show = ui.interact(
                                pill.expand(SHOW_REACH),
                                id.with(("trophy-show", i)),
                                Sense::click(),
                            );
                            show.widget_info(|| {
                                egui::WidgetInfo::labeled(egui::Role::Button, true, name.as_str())
                            });
                            if show.clicked() {
                                *shown = Some(i);
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

    fn draw_row(
        &mut self,
        ui: &mut Ui,
        data: &TrophyData<'_>,
        trophy: &Trophy,
        index: usize,
        focused: bool,
    ) -> (Rect, Option<Rect>) {
        let ctx = ui.ctx().clone();
        let language = data.language;
        let earned = trophy.is_unlocked();
        let secret = self.is_secret(trophy, index, data);
        // 0 while the lock tile and the placeholder texts show, 1 once the trophy is shown.
        let revealable = trophy.hidden && !earned;
        let shown_t = if revealable {
            ctx.focus_t(("trophy-reveal", self.visit, index), !secret)
        } else {
            1.0
        };
        let show_label = Extra::TrophyShow.text(language);
        let show_style = theme::TextStyle::new(15.0, theme::Weight::SemiBold);
        let show_w = if secret {
            text::width(ui.painter(), show_style, show_label) + 2.0 * SHOW_PAD
        } else {
            0.0
        };
        let width = ui.available_width();
        let painter = ui.painter().clone();
        let grade: Grade = convert::grade(trophy.grade);

        // Right column first: it decides how much room the text has.
        let grade_style = theme::TextStyle::new(14.0, theme::Weight::SemiBold);
        let grade_text = grade.title(language);
        let date_text = match trophy.unlocked {
            Some(micros) => format::local_date_text(micros, language),
            None => language.text(Key::TrophyLocked).to_owned(),
        };
        let date_style = ty::DETAIL;
        let right_w = (text::width(&painter, grade_style, grade_text) + 22.0)
            .max(text::width(&painter, date_style, &date_text));

        let icon_size = size::TROPHY_ICON;
        let text_x = 18.0 + icon_size + 20.0;
        let text_w = (width - text_x - 18.0 - right_w - 12.0).max(60.0);
        // A secret keeps the room of its "Show" pill from the start, so focus never reflows it.
        let text_w = if secret {
            (text_w - show_w - SHOW_GAP).max(60.0)
        } else {
            text_w
        };
        let name = if secret {
            language.text(Key::TrophyHidden)
        } else {
            trophy.name.as_str()
        };
        let detail = if secret {
            language.text(Key::TrophyHiddenDetail)
        } else {
            trophy.detail.as_str()
        };
        let name_g = clamped(
            &painter,
            ty::TROPHY_NAME,
            name,
            Color32::PLACEHOLDER,
            text_w,
            1,
        );
        let detail_g = clamped(
            &painter,
            ty::DETAIL_LARGE,
            detail,
            Color32::PLACEHOLDER,
            text_w,
            2,
        );
        let progress = (!earned && !secret).then_some(trophy.progress).flatten();
        let mut block = name_g.size().y + 6.0 + detail_g.size().y;
        if progress.is_some() {
            block += 6.0 + 14.4;
        }
        let height = icon_size.max(block) + 28.0;
        let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        if !ui.is_rect_visible(rect) {
            return (rect, None);
        }
        let t = ctx.focus_t(("trophy-row-focus", index), focused);
        let r = radius::TROPHY_ROW;
        // The focused row is a row of every other panel: lifted, with the ring.
        let under_pointer = ctx.pointer_hover_pos().is_some_and(|p| rect.contains(p));
        let hover_t = ctx.animate_to(
            ("trophy-row-hover", index),
            f32::from(under_pointer),
            Motion::Hover,
        );
        components::paint_surface(
            &painter,
            rect,
            r,
            color::surface_trophy_row(),
            hover_t,
            t,
            0.0,
            size::FOCUS_RING,
        );
        let ink = |alpha: f32| fade(Color32::WHITE, alpha);

        // Icon or the secret's lock tile.
        let icon_rect = Rect::from_center_size(
            pos2(rect.min.x + 18.0 + icon_size * 0.5, rect.center().y),
            vec2(icon_size, icon_size),
        );
        if shown_t < 1.0 {
            let a = 1.0 - shown_t;
            painter.rect_filled(icon_rect, corner(12.0), ink(0.07 * a));
            painter.text(
                icon_rect.center(),
                Align2::CENTER_CENTER,
                Icon::LOCK.glyph(),
                Icon::LOCK.font(22.0, true),
                ink(0.5 * a),
            );
        }
        if shown_t > 0.0 {
            let key = format!("{}/{}", data.game_key, trophy.id());
            let texture = self.icons.get(&ctx, &key, trophy.icon.as_deref(), earned);
            if shown_t < 1.0 {
                // Fades in over the lock tile.
                let mut fading = ui.new_child(UiBuilder::new().max_rect(rect));
                fading.set_opacity(shown_t);
                paint_trophy_icon(&mut fading, icon_rect, texture, earned);
            } else {
                paint_trophy_icon(ui, icon_rect, texture, earned);
            }
        }

        // Name, detail and the progress line.
        let x = rect.min.x + text_x;
        let top = rect.center().y - block * 0.5;
        let name_color = ink(if earned { 1.0 } else { 0.8 });
        let name_h = name_g.size().y;
        let detail_y = top + name_h + 6.0;
        let detail_h = detail_g.size().y;
        if shown_t < 1.0 && !secret {
            // Just revealed: the placeholder words fade out where the real ones fade in.
            let ghost_name = clamped(
                &painter,
                ty::TROPHY_NAME,
                language.text(Key::TrophyHidden),
                Color32::PLACEHOLDER,
                text_w,
                1,
            );
            let ghost_detail = clamped(
                &painter,
                ty::DETAIL_LARGE,
                language.text(Key::TrophyHiddenDetail),
                Color32::PLACEHOLDER,
                text_w,
                2,
            );
            let a = 1.0 - shown_t;
            painter.galley(pos2(x, top), ghost_name, fade(name_color, a));
            painter.galley(pos2(x, detail_y), ghost_detail, fade(ink(0.6), a));
        }
        // A secret shows its placeholder words at full strength; a shown trophy fades them in.
        let text_a = if secret { 1.0 } else { shown_t };
        painter.galley(pos2(x, top), name_g, fade(name_color, text_a));
        painter.galley(pos2(x, detail_y), detail_g, fade(ink(0.6), text_a));
        if let Some((value, target)) = progress {
            let y = detail_y + detail_h + 6.0;
            let bar = Rect::from_center_size(pos2(x + 90.0, y + 7.2), vec2(180.0, 4.0));
            let fraction = if target == 0 {
                0.0
            } else {
                value as f32 / target as f32
            };
            painter.rect_filled(bar, corner(2.0), white(0.16 * t));
            paint_progress(&painter, bar, fraction.clamp(0.0, 1.0), data.accent);
            text::paint_tabular(
                &painter,
                pos2(bar.max.x + 10.0, y),
                theme::TextStyle::new(12.0, theme::Weight::Medium),
                &format!("{value}/{target}"),
                ink(0.6),
            );
        }

        // Grade and date, right aligned.
        let right = rect.max.x - 18.0;
        let grade_color = grade.color();
        let grade_color = if earned {
            grade_color
        } else {
            fade(grade_color, 0.55)
        };
        let rows_h = grade_style.line_height() + 6.0 + date_style.line_height();
        let gy = rect.center().y - rows_h * 0.5;
        let label = text::paint(
            &painter,
            pos2(right, gy),
            Align2::RIGHT_TOP,
            grade_style,
            grade_text,
            grade_color,
        );
        painter.text(
            pos2(label.min.x - 14.0, gy + grade_style.line_height() * 0.5),
            Align2::CENTER_CENTER,
            Icon::TROPHY.glyph(),
            Icon::TROPHY.font(14.0, true),
            grade_color,
        );
        text::paint(
            &painter,
            pos2(right, gy + grade_style.line_height() + 6.0),
            Align2::RIGHT_TOP,
            date_style,
            &date_text,
            ink(if earned { 0.7 } else { 0.4 }),
        );

        // The "Show" pill of a focused secret, left of the grade column.
        let mut pill = None;
        if secret && t > 0.0 {
            let right_edge = right - right_w - SHOW_GAP;
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
            // Only a focused row can be asked to show; a fading pill is not a target yet.
            pill = focused.then_some(area);
        }
        (rect, pill)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(rgba: [u8; 4]) -> Vec<u8> {
        let mut bytes = Vec::new();
        let image = image::RgbaImage::from_pixel(2, 2, image::Rgba(rgba));
        image
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    }

    #[test]
    fn earned_icons_keep_their_colour_and_unearned_go_grey() {
        let data = png([200, 40, 40, 255]);
        let colour = decode_icon(&data, true).unwrap();
        assert_eq!(colour.pixels[0], Color32::from_rgb(200, 40, 40));
        let grey = decode_icon(&data, false).unwrap();
        let px = grey.pixels[0];
        assert_eq!(px.r(), px.g());
        assert_eq!(px.g(), px.b());
        assert!(px.r() > 40 && px.r() < 200);
    }

    #[test]
    fn a_broken_icon_is_none_not_a_panic() {
        assert!(decode_icon(b"not a png", true).is_none());
        assert!(decode_icon(&[], false).is_none());
    }

    fn hidden(unlocked: Option<i64>) -> Trophy {
        Trophy {
            service_label: 0,
            number: 0,
            grade: anyps5_core::trophies::Grade::Gold,
            hidden: true,
            name: "Name".into(),
            detail: "Detail".into(),
            icon: None,
            unlocked,
            progress: None,
        }
    }

    fn data(set: &TrophySet, show_hidden: bool) -> TrophyData<'_> {
        TrophyData {
            game_key: "g",
            game_name: "G",
            set,
            language: Language::English,
            accent: Color32::WHITE,
            show_hidden,
        }
    }

    #[test]
    fn a_secret_opens_for_one_visit_or_always() {
        let set = TrophySet {
            title: String::new(),
            icon: None,
            trophies: Vec::new(),
        };
        let mut panel = TrophyListPanel::default();
        panel.open_at(0);
        let locked = hidden(None);
        assert!(panel.is_secret(&locked, 0, &data(&set, false)));
        assert!(!panel.is_secret(&locked, 0, &data(&set, true)));
        panel.reveal(0);
        assert!(!panel.is_secret(&locked, 0, &data(&set, false)));
        assert!(panel.is_secret(&locked, 1, &data(&set, false)));
        // Leaving and coming back hides it again.
        panel.close();
        panel.open_at(0);
        assert!(panel.is_secret(&locked, 0, &data(&set, false)));
        // An earned trophy is never a secret.
        assert!(!panel.is_secret(&hidden(Some(1)), 0, &data(&set, false)));
    }

    #[test]
    fn the_index_stays_inside_the_set() {
        let mut panel = TrophyListPanel::default();
        panel.open_at(40);
        assert!(panel.is_active());
        panel.close();
        assert!(panel.is_active(), "still fading out");
    }
}
