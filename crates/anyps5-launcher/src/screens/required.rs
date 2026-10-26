//! "Update required" (docs/DESIGN.md 9.5.1): the full-screen stop a launcher meets when its version
//! is retired, either by the signed manifest (`min_supported_version`) or by the cloud
//! (`client_too_old`). A calm column in the middle: what happened in one line, where the update
//! stands, a few lines of what is new, one primary action and, below it, *Play Offline*, so a
//! player is never locked out of their own games. The update itself is the launcher's ordinary
//! one (download, verify, restart); this screen only shows it and asks for the restart.
//!
//! Which button is the primary one follows the state of the update ([`plan`]); the state machine
//! is plain data and tested without a window.

use egui::{Align, Align2, Color32, Context, Layout, Rect, Sense, Ui, UiBuilder, pos2, vec2};

use crate::screens::side::commands;
use crate::screens::update_strings::UpdateText as U;
use crate::screens::{clamped, settings::LauncherUpdate, settings::UpdateFailure};
use crate::ui::Shell;
use crate::ui::chrome::fullscreen_panel;
use crate::ui::components::{ConsoleButton, ControlSize, Hint, HintAction, ProgressLine};
use crate::ui::glass::GlassStyle;
use crate::ui::i18n::{Arg, Key, Language};
use crate::ui::icons::Icon;
use crate::ui::input::{Command, InputEvent};
use crate::ui::sound::Cue;
use crate::ui::text;
use crate::ui::theme::{self, color, space, ty};

/// Width of the column. Text wraps inside it; the buttons are as wide as the longest label needs.
const COLUMN: f32 = 440.0;
/// The most lines of release notes shown. They are a taste, not the notes: the whole text is the
/// "What's New" panel's.
const NOTE_LINES: usize = 3;
/// Between two bordered controls (docs/DESIGN.md 3.9).
const BUTTON_GAP: f32 = space::M;
/// The progress line: its width (the buttons'), its height, and the gap above it.
const BUTTON_WIDTH: f32 = 320.0;
const BAR_HEIGHT: f32 = 4.0;
const BAR_GAP: f32 = space::M;

/// Why the screen is up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// The launcher started below the supported version, or the cloud turned it away.
    Retired,
    /// The player asked for the online area while the update is required.
    OnlineClosed,
}

/// The primary action, which follows the state of the update.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Primary {
    /// Restart into the downloaded update. Not yet possible while it downloads or waits for a game.
    UpdateNow { enabled: bool },
    /// The check could not finish: ask again.
    TryAgain,
    /// This launcher cannot install it itself: open the releases page.
    Download,
}

impl Primary {
    pub fn enabled(self) -> bool {
        !matches!(self, Primary::UpdateNow { enabled: false })
    }
}

/// What the player did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequiredAction {
    /// Restart into the downloaded update (held while a game runs, as everywhere).
    UpdateNow,
    TryAgain,
    /// Open the releases page.
    Download,
    /// Leave the screen and play offline; the online area stays closed.
    PlayOffline,
}

/// What the screen shows, borrowed for one frame.
pub struct RequiredData<'a> {
    pub language: Language,
    pub update: &'a LauncherUpdate,
    pub reason: Reason,
    /// Lines of the release notes of the update (the subjects of its changes), when known.
    pub notes: &'a [String],
}

/// The primary action for the state of the update.
pub fn plan(update: &LauncherUpdate) -> Primary {
    match update {
        LauncherUpdate::Ready(_) => Primary::UpdateNow { enabled: true },
        LauncherUpdate::None
        | LauncherUpdate::Checking
        | LauncherUpdate::Available(_)
        | LauncherUpdate::Downloading { .. }
        | LauncherUpdate::Holding(_) => Primary::UpdateNow { enabled: false },
        LauncherUpdate::UpToDate | LauncherUpdate::Failed(UpdateFailure::Offline) => {
            Primary::TryAgain
        }
        LauncherUpdate::ReadOnly(_)
        | LauncherUpdate::Manual(_)
        | LauncherUpdate::Failed(UpdateFailure::Refused | UpdateFailure::Other) => {
            Primary::Download
        }
    }
}

/// The version the update brings, when the state knows it.
pub fn target(update: &LauncherUpdate) -> Option<&str> {
    match update {
        LauncherUpdate::Available(v)
        | LauncherUpdate::Ready(v)
        | LauncherUpdate::Holding(v)
        | LauncherUpdate::ReadOnly(v)
        | LauncherUpdate::Manual(v) => Some(v),
        LauncherUpdate::Downloading { version, .. } => Some(version),
        _ => None,
    }
}

/// How the update stands, as one line, and the share of the download when there is one.
pub fn status(update: &LauncherUpdate, language: Language) -> (String, Option<f32>) {
    let percent = |permille: u16| format!("{}%", permille / 10);
    match update {
        LauncherUpdate::None | LauncherUpdate::Checking => {
            (U::Checking.text(language).to_owned(), None)
        }
        LauncherUpdate::Available(_) => (
            U::Downloading.format(language, &[Arg::Str(&percent(0))]),
            Some(0.0),
        ),
        LauncherUpdate::Downloading { permille, .. } if *permille >= 1000 => {
            (U::Verifying.text(language).to_owned(), Some(1.0))
        }
        LauncherUpdate::Downloading { permille, .. } => (
            U::Downloading.format(language, &[Arg::Str(&percent(*permille))]),
            Some(f32::from(*permille) / 1000.0),
        ),
        LauncherUpdate::Ready(v) => (U::Ready.format(language, &[Arg::Str(v)]), None),
        LauncherUpdate::Holding(_) => (U::NoticeHolding.text(language).to_owned(), None),
        LauncherUpdate::UpToDate => (U::RequiredNoNewer.text(language).to_owned(), None),
        LauncherUpdate::ReadOnly(_) => (U::NoteReadOnly.text(language).to_owned(), None),
        LauncherUpdate::Manual(_) => (U::RequiredNoteManual.text(language).to_owned(), None),
        LauncherUpdate::Failed(UpdateFailure::Offline) => {
            (U::RequiredOffline.text(language).to_owned(), None)
        }
        LauncherUpdate::Failed(UpdateFailure::Refused) => {
            (U::NoteRefused.text(language).to_owned(), None)
        }
        LauncherUpdate::Failed(UpdateFailure::Other) => {
            (U::NoteOther.text(language).to_owned(), None)
        }
    }
}

/// The one line that says why the screen is there.
pub fn reason_line(reason: Reason, update: &LauncherUpdate, language: Language) -> String {
    match (reason, target(update)) {
        (Reason::OnlineClosed, _) => U::RequiredOnline.text(language).to_owned(),
        (Reason::Retired, Some(v)) => U::RequiredReason.format(language, &[Arg::Str(v)]),
        (Reason::Retired, None) => U::RequiredReasonUnknown.text(language).to_owned(),
    }
}

/// The lines of notes to show: the first few subjects, which come from unsigned release data and
/// are only ever drawn as text.
pub fn note_lines(notes: &anyps5_core::notes::ReleaseNotes) -> Vec<String> {
    notes
        .changes
        .iter()
        .map(|change| change.subject.trim().to_owned())
        .filter(|line| !line.is_empty())
        .take(NOTE_LINES)
        .collect()
}

/// The screen.
#[derive(Default)]
pub struct RequiredScreen {
    open: bool,
    closing: bool,
    /// 0 is the primary button, 1 is *Play Offline*.
    focus: usize,
    /// The player moved the focus themselves; it then stays where they put it.
    moved: bool,
}

impl RequiredScreen {
    pub fn open(&mut self) {
        self.open = true;
        self.closing = false;
        self.focus = 0;
        self.moved = false;
    }

    /// Opens with the focus on *Play Offline* (previews).
    pub fn focus_offline(&mut self) {
        self.focus = 1;
        self.moved = true;
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

    pub fn is_active(&self) -> bool {
        self.open || self.closing
    }

    /// The button that has the focus: the primary one, unless it cannot be pressed (the download
    /// runs), in which case the way out has it, so Enter always does something.
    fn focused(&self, primary: Primary) -> usize {
        if primary.enabled() { self.focus } else { 1 }
    }

    pub fn show(
        &mut self,
        shell: &mut Shell,
        ctx: &Context,
        events: &[InputEvent],
        data: &RequiredData<'_>,
    ) -> Vec<RequiredAction> {
        let mut actions = Vec::new();
        if !self.is_active() {
            return actions;
        }
        let primary = plan(data.update);
        // The update finished downloading while the player had not touched anything: the focus
        // goes to the action that is now possible. Once they moved it, it stays.
        if !self.moved {
            self.focus = 0;
        }
        if self.open {
            for command in commands(events) {
                self.command(shell, command, primary, &mut actions);
            }
        }
        let focused = self.focused(primary);
        let mut clicked: Vec<RequiredAction> = Vec::new();
        let drawn = fullscreen_panel(
            ctx,
            "update-required",
            self.open,
            GlassStyle::fullscreen_panel(),
            |ui, _| draw(ui, data, primary, focused, &mut clicked),
        );
        if drawn.is_none() {
            self.closing = false;
        }
        if self.open {
            for action in clicked {
                self.activate(shell, action, &mut actions);
            }
        }
        actions
    }

    fn command(
        &mut self,
        shell: &mut Shell,
        command: Command,
        primary: Primary,
        actions: &mut Vec<RequiredAction>,
    ) {
        match command {
            Command::Up | Command::Down | Command::Left | Command::Right => {
                // Two buttons: any direction goes to the other one, when there is another one.
                if primary.enabled() {
                    let up = matches!(command, Command::Up | Command::Left);
                    let next = if up { 0 } else { 1 };
                    if self.focus != next {
                        self.focus = next;
                        self.moved = true;
                        shell.play(Cue::Move);
                    }
                }
            }
            Command::Select => {
                let action = if self.focused(primary) == 0 {
                    match primary {
                        Primary::UpdateNow { .. } => RequiredAction::UpdateNow,
                        Primary::TryAgain => RequiredAction::TryAgain,
                        Primary::Download => RequiredAction::Download,
                    }
                } else {
                    RequiredAction::PlayOffline
                };
                self.activate(shell, action, actions);
            }
            // Back is the way out, as on every screen: here it is the offline play.
            Command::Back => self.activate(shell, RequiredAction::PlayOffline, actions),
            _ => {}
        }
    }

    fn activate(
        &mut self,
        shell: &mut Shell,
        action: RequiredAction,
        actions: &mut Vec<RequiredAction>,
    ) {
        shell.play(match action {
            RequiredAction::PlayOffline => Cue::Back,
            _ => Cue::Select,
        });
        actions.push(action);
    }
}

fn centred(
    painter: &egui::Painter,
    style: theme::TextStyle,
    text: &str,
    color: Color32,
    width: f32,
    max_rows: usize,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple(text.to_owned(), style.font(), color, width);
    job.halign = Align::Center;
    job.wrap.max_rows = max_rows;
    job.sections[0].format.line_height = Some(style.line_height());
    painter.layout_job(job)
}

/// Gives painted text to the screen reader.
fn announce(
    ui: &mut Ui,
    rect: Rect,
    id: impl std::hash::Hash + std::fmt::Debug,
    role: egui::Role,
    text: &str,
) {
    ui.interact(
        rect,
        egui::Id::unique(("update-required", id)),
        Sense::hover(),
    )
    .widget_info(|| egui::WidgetInfo::labeled(role, true, text));
}

fn draw(
    ui: &mut Ui,
    data: &RequiredData<'_>,
    primary: Primary,
    focused: usize,
    clicked: &mut Vec<RequiredAction>,
) {
    let painter = ui.painter().clone();
    let screen = ui.max_rect();
    let language = data.language;
    let cx = screen.center().x;
    let width = COLUMN.min(screen.width() - 64.0).max(240.0);

    let title = centred(
        &painter,
        ty::SPLASH_TITLE,
        U::RequiredTitle.text(language),
        Color32::WHITE,
        width,
        2,
    );
    let reason_text = reason_line(data.reason, data.update, language);
    let reason = centred(
        &painter,
        ty::ONBOARDING_SUBTITLE,
        &reason_text,
        color::text_subtitle(),
        width,
        3,
    );
    let (status_text, progress) = status(data.update, language);
    let status = centred(
        &painter,
        ty::CHOICE_VALUE,
        &status_text,
        color::text_detail(),
        width,
        2,
    );

    const ICON: f32 = 44.0;
    let button_h = ControlSize::Large.height();
    let buttons_h = button_h * 2.0 + BUTTON_GAP;
    // The bar's room is kept in every state, so the buttons do not move when the state changes.
    let status_h = status.size().y + BAR_GAP + BAR_HEIGHT;
    let head_h = ICON + space::XL + title.size().y + space::M + reason.size().y;
    let core_h = head_h + space::XXL + status_h + space::XXL + buttons_h;

    // The notes are a nice thing to have, not a reason to push the buttons off a short window:
    // they are left out when they do not fit.
    let note_style = ty::NOTE;
    // The notes share the buttons' edges.
    let notes_w = width.min(BUTTON_WIDTH);
    let note_galleys: Vec<_> = data
        .notes
        .iter()
        .map(|line| clamped(&painter, note_style, line, color::text_detail(), notes_w, 2))
        .collect();
    let caption = match crate::screens::required::target(data.update) {
        Some(version) => U::WhatsNewIn.format(language, &[Arg::Str(version)]),
        None => String::new(),
    };
    let notes_h = if note_galleys.is_empty() || caption.is_empty() {
        0.0
    } else {
        space::XL
            + space::XXL
            + note_galleys
                .iter()
                .map(|g| g.size().y + space::M)
                .sum::<f32>()
    };
    let hints_room = 40.0 + 24.0 + 16.0;
    // Room for the notes is kept in every state of a tall window, with or without notes, for the
    // same reason; a short window centres what it has.
    const NOTES_RESERVE: f32 = 170.0;
    let fits = core_h + NOTES_RESERVE + hints_room * 2.0 <= screen.height();
    let show_notes = notes_h > 0.0 && fits;
    let total = core_h + if fits { NOTES_RESERVE } else { 0.0 };
    let mut y = (screen.center().y - total * 0.5).max(screen.min.y + space::XL);

    // The glyph: ink, not accent.
    painter.text(
        pos2(cx, y + ICON * 0.5),
        Align2::CENTER_CENTER,
        Icon::DOWNLOAD_SIMPLE.glyph(),
        Icon::DOWNLOAD_SIMPLE.font(ICON, false),
        color::text_label(),
    );
    y += ICON + space::XL;

    announce(
        ui,
        Rect::from_min_size(pos2(cx - width * 0.5, y), title.size()),
        "title",
        egui::Role::Heading,
        U::RequiredTitle.text(language),
    );
    painter.galley(pos2(cx, y), title.clone(), Color32::WHITE);
    y += title.size().y + space::M;

    announce(
        ui,
        Rect::from_min_size(pos2(cx - width * 0.5, y), reason.size()),
        "reason",
        egui::Role::Label,
        &reason_text,
    );
    painter.galley(pos2(cx, y), reason.clone(), color::text_subtitle());
    y += reason.size().y + space::XXL;

    // Where the update stands.
    let status_rect = Rect::from_min_size(pos2(cx - width * 0.5, y), vec2(width, status.size().y));
    ui.interact(
        status_rect,
        egui::Id::unique(("update-required", "status")),
        Sense::hover(),
    )
    .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Status, true, &status_text));
    painter.galley(pos2(cx, y), status.clone(), color::text_detail());
    y += status.size().y;
    // The bar's room is taken in every state, with or without the bar, so the buttons stay where
    // they are while the update goes from downloading to ready.
    y += BAR_GAP;
    if let Some(fraction) = progress {
        let bar =
            Rect::from_center_size(pos2(cx, y + 2.0), vec2(width.min(BUTTON_WIDTH), BAR_HEIGHT));
        let mut child = ui.new_child(UiBuilder::new().max_rect(bar));
        ProgressLine::new(fraction, color::text_label())
            .width(bar.width())
            .label(status_text.clone())
            .show(&mut child);
    }
    y += BAR_HEIGHT;
    y += space::XXL;

    // The two buttons, one under the other, the primary above.
    let primary_label = match primary {
        Primary::UpdateNow { .. } => U::UpdateNow.text(language),
        Primary::TryAgain => U::TryAgain.text(language),
        Primary::Download => U::Download.text(language),
    };
    let primary_icon = match primary {
        Primary::UpdateNow { .. } => Icon::DOWNLOAD_SIMPLE,
        Primary::TryAgain => Icon::ARROW_CLOCKWISE,
        Primary::Download => Icon::DOWNLOAD_SIMPLE,
    };
    // One width for both, wide enough for the longest label ("Download from GitHub").
    let button_w = BUTTON_WIDTH.min(width);
    let column = Rect::from_min_size(pos2(cx - button_w * 0.5, y), vec2(button_w, buttons_h));
    let mut buttons = ui.new_child(
        UiBuilder::new()
            .max_rect(column)
            .layout(Layout::top_down(Align::Center)),
    );
    buttons.spacing_mut().item_spacing.y = BUTTON_GAP;
    if ConsoleButton::new(primary_label)
        .icon(primary_icon)
        .prominent(true)
        .enabled(primary.enabled())
        .focused(focused == 0)
        .min_width(button_w)
        .show(&mut buttons)
        .clicked()
    {
        clicked.push(match primary {
            Primary::UpdateNow { .. } => RequiredAction::UpdateNow,
            Primary::TryAgain => RequiredAction::TryAgain,
            Primary::Download => RequiredAction::Download,
        });
    }
    if ConsoleButton::new(U::PlayOffline.text(language))
        .focused(focused == 1)
        .min_width(button_w)
        .show(&mut buttons)
        .clicked()
    {
        clicked.push(RequiredAction::PlayOffline);
    }
    y += buttons_h;

    if show_notes {
        y += space::XL;
        let left = cx - notes_w * 0.5;
        text::paint(
            &painter,
            pos2(left, y),
            Align2::LEFT_TOP,
            ty::SECTION_CAPTION,
            &caption,
            color::text_overline(),
        );
        y += space::XXL;
        for (index, galley) in note_galleys.iter().enumerate() {
            announce(
                ui,
                Rect::from_min_size(pos2(left, y), galley.size()),
                ("note", index),
                egui::Role::Label,
                galley.text(),
            );
            let h = galley.size().y;
            painter.galley(pos2(left, y), galley.clone(), color::text_detail());
            y += h + space::M;
        }
    }

    // The hints, 40 above the bottom edge: the keys of the device in use.
    let hint_rect = Rect::from_center_size(
        pos2(cx, screen.max.y - 40.0 - 12.0),
        vec2(width.max(320.0), 24.0),
    );
    let mut hints = ui.new_child(
        UiBuilder::new()
            .max_rect(hint_rect)
            .layout(Layout::left_to_right(Align::Center).with_main_justify(false)),
    );
    let device = crate::ui::input::device(ui.ctx());
    let select = Hint {
        action: HintAction::Select,
        label: language.text(Key::HintSelect),
    };
    let back = Hint {
        action: HintAction::Back,
        label: U::PlayOffline.text(language),
    };
    let total = select.width(&painter, device) + 24.0 + back.width(&painter, device);
    hints.add_space(((hint_rect.width() - total) * 0.5).max(0.0));
    hints.spacing_mut().item_spacing.x = 24.0;
    select.show(&mut hints);
    back.show(&mut hints);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::settings::UpdateFailure::{Offline, Other, Refused};

    fn v(s: &str) -> String {
        s.to_owned()
    }

    #[test]
    fn the_primary_action_follows_the_state_of_the_update() {
        use LauncherUpdate as L;
        // Waiting: nothing to press yet, the way out has the focus.
        for state in [
            L::None,
            L::Checking,
            L::Available(v("0.3.0")),
            L::Downloading {
                version: v("0.3.0"),
                permille: 420,
            },
            L::Holding(v("0.3.0")),
        ] {
            assert_eq!(
                plan(&state),
                Primary::UpdateNow { enabled: false },
                "{state:?}"
            );
        }
        // Downloaded and verified: Update Now.
        assert_eq!(
            plan(&L::Ready(v("0.3.0"))),
            Primary::UpdateNow { enabled: true }
        );
        // The check could not finish: ask again.
        assert_eq!(plan(&L::Failed(Offline)), Primary::TryAgain);
        assert_eq!(plan(&L::UpToDate), Primary::TryAgain);
        // This launcher cannot install it itself: the releases page.
        for state in [
            L::ReadOnly(v("0.3.0")),
            L::Manual(v("0.3.0")),
            L::Failed(Refused),
            L::Failed(Other),
        ] {
            assert_eq!(plan(&state), Primary::Download, "{state:?}");
        }
    }

    #[test]
    fn offline_play_is_always_the_other_button() {
        // Whatever the primary is, a screen has one: the focus falls to it when the primary
        // cannot be pressed, so Enter and a pad's select always do something.
        let mut screen = RequiredScreen::default();
        screen.open();
        assert_eq!(screen.focused(Primary::UpdateNow { enabled: false }), 1);
        assert_eq!(screen.focused(Primary::UpdateNow { enabled: true }), 0);
        assert_eq!(screen.focused(Primary::TryAgain), 0);
    }

    #[test]
    fn back_and_select_on_the_second_button_play_offline_and_select_on_the_first_updates() {
        let shell_free = |command, focus, primary| {
            let mut screen = RequiredScreen::default();
            screen.open();
            screen.focus = focus;
            screen.moved = true;
            let mut actions = Vec::new();
            let mut shell =
                crate::ui::Shell::for_test(&Context::default(), crate::ui::theme::Theme::Cosmic);
            screen.command(&mut shell, command, primary, &mut actions);
            actions
        };
        let ready = Primary::UpdateNow { enabled: true };
        let waiting = Primary::UpdateNow { enabled: false };
        assert_eq!(
            shell_free(Command::Back, 0, ready),
            [RequiredAction::PlayOffline]
        );
        assert_eq!(
            shell_free(Command::Back, 0, waiting),
            [RequiredAction::PlayOffline]
        );
        assert_eq!(
            shell_free(Command::Select, 0, ready),
            [RequiredAction::UpdateNow]
        );
        assert_eq!(
            shell_free(Command::Select, 1, ready),
            [RequiredAction::PlayOffline]
        );
        // While the download runs, Select can only mean the way out, never a restart.
        assert_eq!(
            shell_free(Command::Select, 0, waiting),
            [RequiredAction::PlayOffline]
        );
        assert_eq!(
            shell_free(Command::Select, 0, Primary::TryAgain),
            [RequiredAction::TryAgain]
        );
        assert_eq!(
            shell_free(Command::Select, 0, Primary::Download),
            [RequiredAction::Download]
        );
    }

    #[test]
    fn the_reason_names_the_version_when_it_is_known() {
        let lang = Language::English;
        let ready = LauncherUpdate::Ready(v("0.3.0"));
        assert!(reason_line(Reason::Retired, &ready, lang).contains("0.3.0"));
        assert!(!reason_line(Reason::Retired, &LauncherUpdate::Checking, lang).contains("%@"));
        // The online area attempt says what is closed and that offline play is open.
        let online = reason_line(Reason::OnlineClosed, &ready, lang);
        assert!(online.contains("online") && online.contains("offline"));
    }

    #[test]
    fn the_status_line_shows_the_download() {
        let lang = Language::English;
        let (text, fraction) = status(
            &LauncherUpdate::Downloading {
                version: v("0.3.0"),
                permille: 425,
            },
            lang,
        );
        assert_eq!(text, "Downloading 42%");
        assert_eq!(fraction, Some(0.425));
        let (text, fraction) = status(
            &LauncherUpdate::Downloading {
                version: v("0.3.0"),
                permille: 1000,
            },
            lang,
        );
        assert_eq!((text.as_str(), fraction), ("Verifying", Some(1.0)));
        assert_eq!(status(&LauncherUpdate::Ready(v("0.3.0")), lang).1, None);
    }

    #[test]
    fn only_a_few_lines_of_notes_are_taken() {
        let notes = anyps5_core::notes::ReleaseNotes::parse_launcher(
            "### A\n- One\n- Two\n- Three\n- Four\n- Five\n",
        );
        assert_eq!(note_lines(&notes), ["One", "Two", "Three"]);
    }
}
