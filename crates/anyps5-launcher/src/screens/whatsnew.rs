//! "What's New": the notes of one toolkit release in one panel. A header with the release and its
//! date, the highlights (new features and speed-ups) grouped by area, then everything that
//! changed. Every change links to its pull request. The list is drawn row by row from the visible
//! range only, because a release can have two thousand changes.

use std::collections::HashSet;

use anyps5_core::notes::{Change, Group, ReleaseNotes};
use anyps5_core::releases::Published;
use egui::{
    Align, Align2, Color32, Context, CursorIcon, Layout, Rect, ScrollArea, Sense, Ui, UiBuilder,
    pos2, vec2,
};

use crate::screens::side::commands;
use crate::screens::strings::Extra;
use crate::ui::Shell;
use crate::ui::anim::{AnimExt, Motion};
use crate::ui::chrome::fullscreen_panel;
use crate::ui::components::{self, ConsoleButton};
use crate::ui::glass::GlassStyle;
use crate::ui::i18n::{Arg, Language};
use crate::ui::icons::Icon;
use crate::ui::input::{self, Command, InputEvent};
use crate::ui::sound::Cue;
use crate::ui::text;
use crate::ui::theme::{TextStyle, color, pad, radius, size, ty};

/// Areas shown on their own under Highlights; the rest are gathered in one more group.
const AREAS: usize = 7;
const ROW_HEIGHT: f32 = 44.0;
const ROW_GAP: f32 = 4.0;
const RELEASE_PAGE: &str = "https://github.com/boykopovar/AnyPS5/releases/tag";

/// Release notes for the demo and the previews, in the shape of upstream's.
pub const SAMPLE_NOTES: &str = "## What's Changed\n\
* feat(agc): add a mesh shader path by @a in https://github.com/boykopovar/AnyPS5/pull/6401\n\
* feat(agc): support depth bounds tests by @b in https://github.com/boykopovar/AnyPS5/pull/6422\n\
* perf(agc): skip redundant barriers by @a in https://github.com/boykopovar/AnyPS5/pull/6430\n\
* feat(libSceSaveData): keep memory saves per user by @c in https://github.com/boykopovar/AnyPS5/pull/6388\n\
* feat(relinker): keep TLS alignment of the main module by @d in https://github.com/boykopovar/AnyPS5/pull/6377\n\
* feat(relinker): write the patch manifest next to the output by @d in https://github.com/boykopovar/AnyPS5/pull/6314\n\
* perf(shader): cache recompiled shaders on disk by @e in https://github.com/boykopovar/AnyPS5/pull/6351\n\
* fix(libkernel): wake waiters of a destroyed semaphore by @f in https://github.com/boykopovar/AnyPS5/pull/6402\n\
* feat(libSceNpTrophy): write the trophy list per user by @i in https://github.com/boykopovar/AnyPS5/pull/6411\n\
* feat(libSceVideoOut): add a mailbox present mode by @j in https://github.com/boykopovar/AnyPS5/pull/6392\n\
* perf(libc): speed up memcpy for small blocks by @k in https://github.com/boykopovar/AnyPS5/pull/6417\n\
* feat(tools): list the environment variables by @l in https://github.com/boykopovar/AnyPS5/pull/6366\n\
* docs: describe the environment variables by @g in https://github.com/boykopovar/AnyPS5/pull/6360\n\
* ci: run the Windows tests on every pull request by @h in https://github.com/boykopovar/AnyPS5/pull/6399\n";

/// What the notes are doing.
pub enum NotesView<'a> {
    Loading,
    Unavailable,
    Ready(&'a Published),
}

pub struct WhatsNewData<'a> {
    /// The release, without a leading `v`.
    pub tag: &'a str,
    pub view: NotesView<'a>,
    pub language: Language,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WhatsNewAction {
    Close,
    /// Open this address in the browser (always a page of the toolkit's repository).
    OpenUrl(String),
}

/// One line of the list.
#[derive(Clone, Debug)]
enum Line<'a> {
    Section(&'static str),
    /// A highlighted area; `key` identifies it, `open` shows its changes below.
    Group {
        key: String,
        count: usize,
        open: bool,
    },
    Change {
        change: &'a Change,
        /// Show the area in front of the subject (the list is not grouped by it).
        with_scope: bool,
    },
}

impl Line<'_> {
    fn selectable(&self) -> bool {
        !matches!(self, Line::Section(_))
    }
}

/// What a player calls the part of the toolkit a change is about: the highlights speak of
/// graphics, trophies or saves, not of the engine's module names. Unknown areas keep their name;
/// the full list under them keeps the exact scopes.
fn area_title(scope: &str, language: Language) -> String {
    let scope = scope.to_ascii_lowercase();
    let has = |parts: &[&str]| parts.iter().any(|part| scope.contains(part));
    let (english, turkish) = if has(&["trophy"]) {
        ("Trophies", "Kupalar")
    } else if has(&["savedata", "save"]) {
        ("Saves", "Kayıtlar")
    } else if has(&[
        "agc", "gnm", "gpu", "vulkan", "shader", "videoout", "graphics",
    ]) {
        ("Graphics", "Grafik")
    } else if has(&["audio", "sound", "ngs"]) {
        ("Sound", "Ses")
    } else if has(&["pad", "input", "mouse", "keyboard"]) {
        ("Controllers", "Kontrolcüler")
    } else if has(&["net", "np", "http", "ssl"]) {
        ("Online", "Çevrimiçi")
    } else if has(&["relink", "loader", "elf", "module"]) {
        ("Starting games", "Oyunları başlatma")
    } else if has(&["libc", "kernel", "system", "thread"]) {
        ("System", "Sistem")
    } else {
        return scope;
    };
    match language {
        Language::Turkish => turkish.to_owned(),
        Language::English => english.to_owned(),
    }
}

/// Highlights as (area, changes): the busiest areas under the names players know, then
/// everything else as `other`. Areas that share a name are shown as one.
fn highlight_groups<'a>(
    notes: &'a ReleaseNotes,
    other: &str,
    language: Language,
) -> Vec<(String, Vec<&'a Change>, bool)> {
    let groups: Vec<Group<'a>> = notes.highlights();
    let mut out: Vec<(String, Vec<&Change>, bool)> = Vec::new();
    let mut rest: Vec<&Change> = Vec::new();
    for (index, group) in groups.into_iter().enumerate() {
        if index < AREAS {
            let title = area_title(group.scope, language);
            match out.iter_mut().find(|(known, ..)| *known == title) {
                Some((_, changes, _)) => changes.extend(group.changes),
                None => out.push((title, group.changes, false)),
            }
        } else {
            rest.extend(group.changes);
        }
    }
    if !rest.is_empty() {
        out.push((other.to_owned(), rest, true));
    }
    out
}

fn lines<'a>(
    notes: &'a ReleaseNotes,
    expanded: &HashSet<String>,
    language: Language,
) -> Vec<Line<'a>> {
    let mut out = vec![Line::Section(Extra::NotesHighlights.text(language))];
    let other = Extra::NotesOther.text(language);
    for (key, changes, mixed) in highlight_groups(notes, other, language) {
        let open = expanded.contains(&key);
        out.push(Line::Group {
            key,
            count: changes.len(),
            open,
        });
        if open {
            out.extend(changes.into_iter().map(|change| Line::Change {
                change,
                with_scope: mixed,
            }));
        }
    }
    out.push(Line::Section(Extra::NotesEverything.text(language)));
    out.extend(notes.changes.iter().map(|change| Line::Change {
        change,
        with_scope: true,
    }));
    out
}

/// The panel.
#[derive(Default)]
pub struct WhatsNewPanel {
    open: bool,
    closing: bool,
    cursor: usize,
    expanded: HashSet<String>,
    /// Set when the cursor moved by command: the next frame scrolls to it.
    follow: bool,
}

impl WhatsNewPanel {
    pub fn open(&mut self) {
        self.open = true;
        self.closing = false;
        self.cursor = 1;
        self.expanded.clear();
        self.follow = true;
    }

    /// Opens with some areas already unfolded (previews).
    pub fn open_expanded(&mut self, notes: &ReleaseNotes, language: Language, count: usize) {
        self.open();
        let other = Extra::NotesOther.text(language);
        self.expanded = highlight_groups(notes, other, language)
            .into_iter()
            .take(count)
            .map(|(key, ..)| key)
            .collect();
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

    pub fn show(
        &mut self,
        shell: &mut Shell,
        ctx: &Context,
        events: &[InputEvent],
        data: &WhatsNewData<'_>,
    ) -> Vec<WhatsNewAction> {
        let mut actions = Vec::new();
        if !self.is_active() {
            return actions;
        }
        if self.open {
            for command in commands(events) {
                self.command(shell, command, data, &mut actions);
            }
        }
        let mut clicked: Vec<Pick> = Vec::new();
        let drawn = fullscreen_panel(
            ctx,
            "whats-new",
            self.open,
            GlassStyle::fullscreen_panel(),
            |ui, _| self.draw(ui, data, &mut clicked),
        );
        if drawn.is_none() {
            self.closing = false;
        }
        if self.open {
            for pick in clicked {
                match pick {
                    Pick::Close => {
                        shell.play(Cue::Back);
                        self.close();
                        actions.push(WhatsNewAction::Close);
                    }
                    Pick::Release => {
                        shell.play(Cue::Select);
                        actions.push(WhatsNewAction::OpenUrl(format!(
                            "{RELEASE_PAGE}/v{}",
                            data.tag
                        )));
                    }
                    Pick::Line(index) => {
                        self.cursor = index;
                        self.activate(shell, data, &mut actions);
                    }
                }
            }
        }
        actions
    }

    fn current_lines<'a>(&self, data: &'a WhatsNewData<'_>) -> Vec<Line<'a>> {
        match &data.view {
            NotesView::Ready(published) => lines(&published.notes, &self.expanded, data.language),
            _ => Vec::new(),
        }
    }

    fn command(
        &mut self,
        shell: &mut Shell,
        command: Command,
        data: &WhatsNewData<'_>,
        actions: &mut Vec<WhatsNewAction>,
    ) {
        match command {
            Command::Back => {
                shell.play(Cue::Back);
                self.close();
                actions.push(WhatsNewAction::Close);
            }
            Command::Select => self.activate(shell, data, actions),
            Command::Up | Command::Down => {
                let list = self.current_lines(data);
                let step: isize = if command == Command::Down { 1 } else { -1 };
                let mut at = self.cursor as isize + step;
                while (0..list.len() as isize).contains(&at) && !list[at as usize].selectable() {
                    at += step;
                }
                if (0..list.len() as isize).contains(&at) {
                    self.cursor = at as usize;
                    self.follow = true;
                    shell.play(Cue::Move);
                }
            }
            _ => {}
        }
    }

    /// Select on the line under the cursor: unfold an area, or open the pull request.
    fn activate(
        &mut self,
        shell: &mut Shell,
        data: &WhatsNewData<'_>,
        actions: &mut Vec<WhatsNewAction>,
    ) {
        let list = self.current_lines(data);
        match list.get(self.cursor) {
            Some(Line::Group { key, open, .. }) => {
                if *open {
                    self.expanded.remove(key);
                } else {
                    self.expanded.insert(key.clone());
                }
                shell.play(Cue::Select);
            }
            Some(Line::Change { change, .. }) => {
                if let Some(url) = change.url() {
                    shell.play(Cue::Select);
                    actions.push(WhatsNewAction::OpenUrl(url));
                }
            }
            _ => {}
        }
    }

    fn draw(&mut self, ui: &mut Ui, data: &WhatsNewData<'_>, clicked: &mut Vec<Pick>) {
        let language = data.language;
        let painter = ui.painter().clone();
        let area = ui.max_rect().shrink2(vec2(pad::LOG, pad::PANEL_TOP));
        // Header: title on the left, the two buttons on the right.
        let header = Rect::from_min_size(area.min, vec2(area.width(), 54.0));
        text::paint(
            &painter,
            pos2(header.min.x, header.center().y),
            Align2::LEFT_CENTER,
            ty::LOG_TITLE,
            Extra::WhatsNew.text(language),
            Color32::WHITE,
        );
        let mut buttons = ui.new_child(
            UiBuilder::new()
                .max_rect(header)
                .layout(Layout::right_to_left(Align::Center)),
        );
        buttons.spacing_mut().item_spacing.x = 12.0;
        if ConsoleButton::new(language.text(crate::ui::i18n::Key::Close))
            .icon(Icon::X)
            .show(&mut buttons)
            .clicked()
        {
            clicked.push(Pick::Close);
        }
        if ConsoleButton::new(Extra::NotesOpenRelease.text(language))
            .icon(Icon::GITHUB_LOGO)
            .show(&mut buttons)
            .clicked()
        {
            clicked.push(Pick::Release);
        }

        // The release line under the title.
        let mut y = header.max.y + 4.0;
        let subtitle = match &data.view {
            NotesView::Ready(published) => {
                let count = published.notes.changes.len();
                let how_many = if count == 1 {
                    Extra::ChangeOne.text(language).to_owned()
                } else {
                    Extra::ChangesCount.format(language, &[Arg::Str(&thousands(count))])
                };
                let when = if published.date.is_empty() {
                    format!("v{}", published.tag)
                } else {
                    Extra::NotesHeader.format(
                        language,
                        &[
                            Arg::Str(&format!("v{}", published.tag)),
                            Arg::Str(&published.date),
                        ],
                    )
                };
                format!("{when}  ·  {how_many}")
            }
            _ => format!("v{}", data.tag),
        };
        text::paint(
            &painter,
            pos2(area.min.x, y),
            Align2::LEFT_TOP,
            ty::DETAIL_LARGE,
            &subtitle,
            color::text_detail(),
        );
        y += 36.0;

        let body = Rect::from_min_max(pos2(area.min.x, y), area.max);
        match &data.view {
            NotesView::Loading | NotesView::Unavailable => {
                let message = if matches!(data.view, NotesView::Loading) {
                    Extra::NotesLoading
                } else {
                    Extra::NotesUnavailable
                };
                text::paint(
                    &painter,
                    body.center(),
                    Align2::CENTER_CENTER,
                    ty::ROW_TITLE,
                    message.text(language),
                    color::text_detail(),
                );
            }
            NotesView::Ready(_) => {
                let list = self.current_lines(data);
                self.cursor = self.cursor.min(list.len().saturating_sub(1));
                let mut child = ui.new_child(UiBuilder::new().max_rect(body));
                self.draw_list(&mut child, &list, clicked);
            }
        }
    }

    fn draw_list(&mut self, ui: &mut Ui, list: &[Line<'_>], clicked: &mut Vec<Pick>) {
        let step = ROW_HEIGHT + ROW_GAP;
        let follow = std::mem::take(&mut self.follow);
        let cursor = self.cursor;
        ScrollArea::vertical()
            .id_salt("whats-new-list")
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, viewport| {
                let width = ui.available_width();
                ui.set_height(list.len() as f32 * step);
                let origin = ui.min_rect().min;
                let first = (viewport.min.y / step).floor().max(0.0) as usize;
                let last = ((viewport.max.y / step).ceil() as usize + 1).min(list.len());
                if follow {
                    let rect = Rect::from_min_size(
                        pos2(origin.x, origin.y + cursor as f32 * step),
                        vec2(width, ROW_HEIGHT),
                    );
                    ui.scroll_to_rect(rect.expand2(vec2(0.0, step * 2.0)), None);
                }
                for (index, line) in list.iter().enumerate().take(last).skip(first) {
                    let rect = Rect::from_min_size(
                        pos2(origin.x, origin.y + index as f32 * step),
                        vec2(width, ROW_HEIGHT),
                    );
                    if draw_line(ui, rect, line, index == cursor, index) {
                        clicked.push(Pick::Line(index));
                    }
                }
            });
    }
}

#[derive(Clone, Copy, Debug)]
enum Pick {
    Close,
    Release,
    Line(usize),
}

/// `text` cut at the end with an ellipsis so that it fits `max_width`.
fn end_truncate(painter: &egui::Painter, style: TextStyle, text: &str, max_width: f32) -> String {
    if text::width(painter, style, text) <= max_width {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut low, mut high) = (0, chars.len());
    while low < high {
        let mid = (low + high).div_ceil(2);
        let candidate: String = chars[..mid].iter().collect::<String>() + "…";
        if text::width(painter, style, &candidate) <= max_width {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    chars[..low]
        .iter()
        .collect::<String>()
        .trim_end()
        .to_owned()
        + "…"
}

/// `1795` as `1,795`.
fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Paints one line and returns whether it was clicked (or activated from the keyboard).
fn draw_line(ui: &mut Ui, rect: Rect, line: &Line<'_>, focused: bool, index: usize) -> bool {
    let painter = ui.painter().clone();
    if let Line::Section(title) = line {
        text::paint(
            &painter,
            pos2(rect.min.x, rect.max.y - 10.0),
            Align2::LEFT_BOTTOM,
            ty::TROPHIES_OVERLINE,
            &title.to_uppercase(),
            color::text_caption(),
        );
        return false;
    }
    let response = ui.interact(
        rect,
        ui.make_persistent_id(("notes-line", index)),
        Sense::click(),
    );
    crate::ui::a11y::follow(ui.ctx(), response.id, focused);
    if input::hovered(ui.ctx(), &response) {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let ctx = ui.ctx().clone();
    let hover_t = ctx.animate_to(
        ("notes-line-hover", index),
        f32::from(input::hovered(&ctx, &response)),
        Motion::Hover,
    );
    let focus_t = ctx.focus_t(("notes-line-focus", index), focused);
    components::paint_surface(
        &painter,
        rect,
        radius::ROW,
        Color32::TRANSPARENT,
        hover_t,
        focus_t,
        0.0,
        size::FOCUS_RING,
    );
    let ink = Color32::WHITE;
    let quiet = color::text_caption();
    match line {
        Line::Group { key, count, open } => {
            let glyph = if *open {
                Icon::CARET_DOWN
            } else {
                Icon::CARET_RIGHT
            };
            painter.text(
                pos2(rect.min.x + 22.0, rect.center().y),
                Align2::CENTER_CENTER,
                glyph.glyph(),
                glyph.font(16.0, false),
                quiet,
            );
            text::paint(
                &painter,
                pos2(rect.min.x + 44.0, rect.center().y),
                Align2::LEFT_CENTER,
                ty::ROW_TITLE,
                key,
                ink,
            );
            text::paint(
                &painter,
                pos2(rect.max.x - 18.0, rect.center().y),
                Align2::RIGHT_CENTER,
                ty::ROW_INFO,
                &count.to_string(),
                quiet,
            );
        }
        Line::Change { change, with_scope } => {
            let mut x = rect.min.x + 18.0;
            if *with_scope {
                let scope = change.scope.as_deref().unwrap_or("");
                let width = 150.0;
                let shown = text::middle_truncate(&painter, ty::ROW_INFO, scope, width);
                text::paint(
                    &painter,
                    pos2(x, rect.center().y),
                    Align2::LEFT_CENTER,
                    ty::ROW_INFO,
                    &shown,
                    quiet,
                );
                x += width + 14.0;
            }
            let number = change.pull.map(|n| format!("#{n}")).unwrap_or_default();
            let right = rect.max.x - 18.0;
            let number_w = if number.is_empty() { 0.0 } else { 90.0 };
            if !number.is_empty() {
                text::paint(
                    &painter,
                    pos2(right, rect.center().y),
                    Align2::RIGHT_CENTER,
                    ty::ROW_INFO,
                    &number,
                    quiet,
                );
            }
            let shown = end_truncate(
                &painter,
                ty::CHOICE_VALUE,
                &change.subject,
                (right - number_w - x).max(60.0),
            );
            text::paint(
                &painter,
                pos2(x, rect.center().y),
                Align2::LEFT_CENTER,
                ty::CHOICE_VALUE,
                &shown,
                ink,
            );
        }
        Line::Section(_) => {}
    }
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTES: &str = "## What's Changed\n\
* feat(a): one by @x in https://github.com/boykopovar/AnyPS5/pull/1\n\
* feat(a): two by @x in https://github.com/boykopovar/AnyPS5/pull/2\n\
* perf(b): three by @x in https://github.com/boykopovar/AnyPS5/pull/3\n\
* fix(c): four by @x in https://github.com/boykopovar/AnyPS5/pull/4\n";

    #[test]
    fn the_list_is_highlights_then_everything() {
        let notes = ReleaseNotes::parse(NOTES);
        let list = lines(&notes, &HashSet::new(), Language::English);
        let kinds: Vec<&str> = list
            .iter()
            .map(|l| match l {
                Line::Section(_) => "section",
                Line::Group { .. } => "group",
                Line::Change { .. } => "change",
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "section", "group", "group", "section", "change", "change", "change", "change"
            ]
        );
        let open: HashSet<String> = ["a".to_owned()].into();
        assert_eq!(
            lines(&notes, &open, Language::English).len(),
            list.len() + 2
        );
    }

    #[test]
    fn areas_beyond_the_busiest_are_gathered() {
        let text: String = std::iter::once("## What's Changed\n".to_owned())
            .chain((0..12).map(|i| {
                format!(
                    "* feat(area{i:02}): x by @a in https://github.com/boykopovar/AnyPS5/pull/{i}\n"
                )
            }))
            .collect();
        let notes = ReleaseNotes::parse(&text);
        let groups = highlight_groups(&notes, "Other", Language::English);
        assert_eq!(groups.len(), AREAS + 1);
        let (name, changes, mixed) = groups.last().unwrap();
        assert_eq!(
            (name.as_str(), changes.len(), *mixed),
            ("Other", 12 - AREAS, true)
        );
    }

    #[test]
    fn highlights_speak_of_what_players_know() {
        let text = [
            "## What's Changed",
            "* feat(agc): a by @a in https://github.com/boykopovar/AnyPS5/pull/1",
            "* feat(shader): b by @a in https://github.com/boykopovar/AnyPS5/pull/2",
            "* feat(libSceNpTrophy): c by @a in https://github.com/boykopovar/AnyPS5/pull/3",
            "* feat(relinker): d by @a in https://github.com/boykopovar/AnyPS5/pull/4",
            "* feat(gizmo): e by @a in https://github.com/boykopovar/AnyPS5/pull/5",
        ]
        .join("\n");
        let notes = ReleaseNotes::parse(&text);
        let names: Vec<String> = highlight_groups(&notes, "Other", Language::English)
            .into_iter()
            .map(|(name, ..)| name)
            .collect();
        // agc and shader are both graphics, shown once; an unknown area keeps its name.
        assert_eq!(
            names.iter().filter(|n| n.as_str() == "Graphics").count(),
            1,
            "{names:?}"
        );
        for expected in ["Trophies", "Starting games", "gizmo"] {
            assert!(
                names.iter().any(|n| n == expected),
                "{expected} in {names:?}"
            );
        }
        assert_eq!(area_title("libSceSaveData", Language::Turkish), "Kayıtlar");
    }

    #[test]
    fn thousands_get_a_comma() {
        assert_eq!(thousands(7), "7");
        assert_eq!(thousands(1795), "1,795");
        assert_eq!(thousands(1_000_000), "1,000,000");
    }
}
