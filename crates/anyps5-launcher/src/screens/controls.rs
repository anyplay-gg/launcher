//! The controls editor (docs/DESIGN.md 5.9, "Controls panel"): one row per pad action with its
//! key and mouse sources, key capture, reset. Bindings are the core's [`Controls`] changes inside
//! the game's [`GameSettings`]; the panel writes them and reports `changed` so the host saves.
//!
//! Capture: choosing a row sets `shell.input.capture_keys`; the next key press (physical position,
//! like SDL scancodes) replaces the row's keyboard keys, keeps its mouse sources and is taken away
//! from every other action. Esc cancels. Modifier keys count when pressed.
//!
//! Layout (docs/DESIGN.md 5.17): a choice above the keys swaps the whole layout at once, the
//! launcher's, a mouse-and-keyboard one, the PC keys the game's own files name, or one the
//! community shares for the title (docs/DESIGN.md 5.18). Rows of a game whose files name its
//! actions show them under the button ("Jump"). Export and import move a layout as a file in the
//! form of upstream's `INPUT_MAPPING.md`.

use anyps5_core::community::CommunityLayout;
use anyps5_core::controls::{Changes, Controls, PadAction, Preset};
use anyps5_core::gamefiles::{GameAction, GameInput, game_layout};
use anyps5_core::settings::GameSettings;
use egui::{Color32, Context, Key as EKey};

use crate::screens::rows::{self, Change, Row};
use crate::screens::share_strings::Share;
use crate::screens::side::{self, Output, SideState, SideView};
use crate::screens::strings::Extra;
use crate::screens::{ToastRequest, dialogs};
use crate::ui::Shell;
use crate::ui::components::PadGlyph;
use crate::ui::i18n::{Arg, Key, Language};
use crate::ui::icons::Icon;
use crate::ui::input::InputEvent;
use crate::ui::sound::Cue;

/// What the host has to do after a frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControlsAction {
    /// Back or options: the panel is gone; open the game settings on the `controls` row.
    Back,
    /// The scrim was clicked: everything closes.
    Close,
    /// Something to tell the player (a layout was saved, imported or refused).
    Toast(ToastRequest),
}

/// The game, borrowed for one frame.
pub struct ControlsData<'a> {
    pub title: &'a str,
    pub title_id: &'a str,
    pub settings: &'a mut GameSettings,
    pub language: Language,
    pub accent: Color32,
    /// What the game's files say about its controls; `None` while they are being read.
    pub game_input: Option<&'a GameInput>,
    /// The layouts the community shares for the title; none offline.
    pub community: &'a [CommunityLayout],
}

/// State of the controls panel.
#[derive(Default)]
pub struct ControlsPanel {
    side: SideState,
    capturing: Option<PadAction>,
}

impl ControlsPanel {
    /// Opens on the layout row (the hint above it is not focusable).
    pub fn open(&mut self) {
        self.side.open_at(1);
        self.capturing = None;
    }

    /// Opens on a row (previews).
    pub fn open_row(&mut self, row: usize) {
        self.side.open_at(row);
        self.capturing = None;
    }

    pub fn close(&mut self, shell: &mut Shell) {
        self.end_capture(shell);
        self.side.close();
    }

    pub fn dismiss(&mut self, shell: &mut Shell) {
        self.end_capture(shell);
        self.side.dismiss();
    }

    pub fn is_active(&self) -> bool {
        self.side.is_active()
    }

    /// The action waiting for a key, if any.
    pub fn capturing(&self) -> Option<PadAction> {
        self.capturing
    }

    /// Starts waiting for a key for `action` (the `controls-capture` state of the preview).
    pub fn begin_capture(&mut self, shell: &mut Shell, action: PadAction) {
        self.capturing = Some(action);
        shell.input.capture_keys = true;
    }

    fn end_capture(&mut self, shell: &mut Shell) {
        self.capturing = None;
        shell.input.capture_keys = false;
    }

    pub fn show(
        &mut self,
        shell: &mut Shell,
        ctx: &Context,
        events: &[InputEvent],
        data: &mut ControlsData<'_>,
    ) -> Output<ControlsAction> {
        let mut out = Output::default();
        if !self.side.is_active() {
            return out;
        }
        if self.side.is_open() {
            self.handle_events(shell, events, data, &mut out);
        }
        let mut rows = build_rows(
            data.settings,
            data.language,
            self.capturing,
            data.game_input,
            data.community,
        );
        let view = SideView {
            title: data.title,
            subtitle: Some(data.language.text(Key::Controls)),
            question: false,
            list_id: "controls-rows",
            nav: &self.side.nav,
            accent: data.accent,
            language: data.language,
            force: None,
        };
        match side::draw(ctx, self.side.is_open(), &view, &mut rows) {
            Some(drawn) if self.side.is_open() => {
                let (changes, _) =
                    rows::apply_mouse(shell, &mut self.side.nav, &rows, drawn.events);
                for change in &changes {
                    self.apply(shell, data, change, &mut out);
                }
                if drawn.back_clicked {
                    shell.play(Cue::Close);
                    self.close(shell);
                    out.actions.push(ControlsAction::Close);
                }
            }
            Some(_) => {}
            None => self.side.finished(),
        }
        out
    }

    fn handle_events(
        &mut self,
        shell: &mut Shell,
        events: &[InputEvent],
        data: &mut ControlsData<'_>,
        out: &mut Output<ControlsAction>,
    ) {
        for event in events {
            match (event, self.capturing) {
                // Waiting for a key: the key press binds, Esc and a pad's back cancel.
                (
                    InputEvent::CapturedKey {
                        key, physical_key, ..
                    },
                    Some(action),
                ) => {
                    if *key == EKey::Escape {
                        shell.play(Cue::Back);
                        self.end_capture(shell);
                    } else if let Some(name) = key_name(physical_key.unwrap_or(*key)) {
                        let current = data.settings.controls.clone().unwrap_or_default();
                        let next = Controls::assign(&format!("KEY:{name}"), action, &current);
                        data.settings.controls = (!next.is_empty()).then_some(next);
                        out.changed = true;
                        shell.play(Cue::Move);
                        self.end_capture(shell);
                    }
                    // A key that cannot be bound is ignored; the capture keeps waiting.
                }
                (InputEvent::Command(crate::ui::input::Command::Back), Some(_)) => {
                    shell.play(Cue::Back);
                    self.end_capture(shell);
                }
                (InputEvent::Command(_), Some(_)) => {}
                (InputEvent::Command(command), None) => {
                    let rows = build_rows(
                        data.settings,
                        data.language,
                        None,
                        data.game_input,
                        data.community,
                    );
                    let driven = rows::drive(shell, &mut self.side.nav, &rows, &[*command]);
                    for change in &driven.changes {
                        self.apply(shell, data, change, out);
                    }
                    if driven.close {
                        // Back and options return to the game settings: no slide.
                        self.dismiss(shell);
                        out.actions.push(ControlsAction::Back);
                        return;
                    }
                }
                _ => {}
            }
        }
    }

    fn apply(
        &mut self,
        shell: &mut Shell,
        data: &mut ControlsData<'_>,
        change: &Change,
        out: &mut Output<ControlsAction>,
    ) {
        match change {
            Change::Capture { row } => {
                if let Some(action) = PadAction::from_name(row) {
                    self.begin_capture(shell, action);
                }
            }
            Change::Run { row: "reset" } if data.settings.controls.take().is_some() => {
                out.changed = true;
            }
            Change::Choice {
                row: "layout",
                index,
            } => {
                let next = match layouts(data.game_input, data.community).get(*index) {
                    Some(Layout::Preset(preset)) => preset.changes(),
                    Some(Layout::Game(actions)) => game_layout(actions, &Changes::new()).0,
                    Some(Layout::Community(layout)) => {
                        // A layout with tuning lines brings its sticks and mouse along.
                        if let Some(tuning) = layout.tuning {
                            data.settings.tuning = Some(tuning);
                        }
                        layout.changes.clone()
                    }
                    None => return,
                };
                data.settings.controls = (!next.is_empty()).then_some(next);
                out.changed = true;
            }
            Change::Run { row: "export" } => self.export(shell, data, out),
            Change::Run { row: "import" } => self.import(shell, data, out),
            _ => {}
        }
    }
}

impl ControlsPanel {
    /// Saves the layout and the tuning as a file in the form of `INPUT_MAPPING.md`.
    fn export(
        &mut self,
        shell: &mut Shell,
        data: &ControlsData<'_>,
        out: &mut Output<ControlsAction>,
    ) {
        let language = data.language;
        let changes = data.settings.controls.clone().unwrap_or_default();
        let tuning = data.settings.tuning.unwrap_or_default();
        let label = if data.title_id.is_empty() {
            data.title.to_owned()
        } else {
            format!("{} ({})", data.title, data.title_id)
        };
        let Some(path) = dialogs::save_file(
            shell,
            Share::LayoutExport.text(language),
            &format!("{} layout.ini", data.title),
            ("Layout", &["ini"]),
            None,
        ) else {
            return;
        };
        let toast = match std::fs::write(&path, Controls::export(&changes, &tuning, &label)) {
            Ok(()) => ToastRequest::new(Icon::FILE_TEXT, Share::LayoutExported.text(language)),
            Err(_) => ToastRequest::warning(Share::LayoutNotSaved.text(language)),
        };
        out.actions.push(ControlsAction::Toast(toast));
    }

    /// Reads a layout file and makes it the game's layout. A file the engine would refuse is
    /// refused with the line.
    fn import(
        &mut self,
        shell: &mut Shell,
        data: &mut ControlsData<'_>,
        out: &mut Output<ControlsAction>,
    ) {
        let language = data.language;
        let Some(path) = dialogs::pick_file(
            shell,
            Share::LayoutImport.text(language),
            ("Layout", &["ini", "txt"]),
            None,
        ) else {
            return;
        };
        let toast = match read_layout(&path) {
            Ok(parsed) => {
                let partly = parsed
                    .changes
                    .keys()
                    .any(|name| anyps5_core::controls::EXTRA_ACTIONS.contains(&name.as_str()));
                if let Some(tuning) = parsed.tuning {
                    data.settings.tuning = Some(tuning);
                }
                data.settings.controls = (!parsed.changes.is_empty()).then_some(parsed.changes);
                out.changed = true;
                let text = if partly {
                    Share::LayoutImportedPartly
                } else {
                    Share::LayoutImported
                };
                ToastRequest::new(Icon::FILE_TEXT, text.text(language))
            }
            Err(reason) => ToastRequest::warning(
                Share::LayoutImportFailed.format(language, &[Arg::Str(&reason)]),
            ),
        };
        out.actions.push(ControlsAction::Toast(toast));
    }
}

/// Layout files are a few hundred bytes; a larger one is not a layout.
const LAYOUT_FILE_LIMIT: u64 = 64 * 1024;

/// The layout in a file, or why it cannot be used (English, as the line it names).
fn read_layout(path: &std::path::Path) -> Result<anyps5_core::controls::ParsedLayout, String> {
    let unreadable = || Share::LayoutUnreadable.english().to_owned();
    let size = std::fs::metadata(path).map_err(|_| unreadable())?.len();
    if size > LAYOUT_FILE_LIMIT {
        return Err(unreadable());
    }
    let bytes = std::fs::read(path).map_err(|_| unreadable())?;
    let text = String::from_utf8(bytes).map_err(|_| unreadable())?;
    Controls::parse(&text).map_err(|error| {
        let mut reason = error.to_string();
        reason[..1].make_ascii_uppercase();
        format!("{reason}.")
    })
}

/// A layout the layout row offers.
enum Layout<'a> {
    Preset(Preset),
    /// The PC keys the game's files name.
    Game(&'a [GameAction]),
    /// One the community shares for the title.
    Community(&'a CommunityLayout),
}

/// The layouts in the order of the choice: the presets, the game's own when its files name PC
/// keys for at least one button, then the community's.
fn layouts<'a>(game: Option<&'a GameInput>, community: &'a [CommunityLayout]) -> Vec<Layout<'a>> {
    let mut layouts: Vec<Layout> = Preset::ALL.into_iter().map(Layout::Preset).collect();
    if let Some(actions) = game_actions(game)
        && game_layout(actions, &Changes::new()).1 > 0
    {
        layouts.push(Layout::Game(actions));
    }
    layouts.extend(community.iter().map(Layout::Community));
    layouts
}

fn game_actions(game: Option<&GameInput>) -> Option<&[GameAction]> {
    match game {
        Some(GameInput::Bindings { actions, .. }) => Some(actions),
        _ => None,
    }
}

/// The layout row: the layout the changes are, or "Custom" (an extra, last option) after the
/// user moved keys.
fn layout_row(
    changes: &Changes,
    language: Language,
    game: Option<&GameInput>,
    community: &[CommunityLayout],
) -> Row {
    let layouts = layouts(game, community);
    let mut options: Vec<String> = layouts
        .iter()
        .map(|layout| match layout {
            Layout::Preset(Preset::Classic) => Extra::LayoutLauncher.text(language).to_string(),
            Layout::Preset(Preset::MouseAndKeyboard) => {
                Extra::LayoutMouse.text(language).to_string()
            }
            Layout::Game(_) => Extra::LayoutGame.text(language).to_string(),
            Layout::Community(layout) => {
                Share::LayoutCommunity.format(language, &[Arg::Str(&layout.name)])
            }
        })
        .collect();
    let selected = layouts.iter().position(|layout| match layout {
        Layout::Preset(preset) => preset.changes() == *changes,
        Layout::Game(actions) => game_layout(actions, &Changes::new()).0 == *changes,
        Layout::Community(layout) => layout.changes == *changes,
    });
    let selected = selected.unwrap_or_else(|| {
        options.push(Extra::LayoutCustom.text(language).to_string());
        options.len() - 1
    });
    let mut row =
        Row::choice("layout", Extra::Layout.text(language), options, selected).icon(Icon::KEYBOARD);
    if let Some(actions) = game_actions(game) {
        let count = game_layout(actions, &Changes::new()).1;
        if count > 0 {
            row = row.detail(Extra::LayoutGameDetail.format(language, &[Arg::Int(count as i64)]));
        }
    }
    row
}

/// What the game calls a button: the names of its actions on it, words split ("MoveForward" →
/// "Move Forward"), the minus half of an axis marked "−". `None` when the files say nothing.
fn game_names(actions: &[GameAction], button: PadAction) -> Option<String> {
    let mut names: Vec<String> = Vec::new();
    for action in actions.iter().filter(|a| a.pad.contains(&button)) {
        let (base, sign) = match action.name.rsplit_once(' ') {
            Some((base, "+")) => (base, ""),
            Some((base, "-")) => (base, " −"),
            _ => (action.name.as_str(), ""),
        };
        let name = format!("{}{sign}", words(base));
        if !names.contains(&name) {
            names.push(name);
        }
    }
    (!names.is_empty()).then(|| names.into_iter().take(3).collect::<Vec<_>>().join(", "))
}

/// `MoveForward` → `Move Forward`, `Use_Item` → `Use Item`, `Artifact1` → `Artifact 1`.
fn words(name: &str) -> String {
    let mut out = String::new();
    let mut previous: Option<char> = None;
    for c in name.chars() {
        if c == '_' {
            out.push(' ');
        } else {
            let starts_word = (c.is_ascii_uppercase()
                && previous.is_some_and(|p| p.is_ascii_lowercase() || p.is_ascii_digit()))
                || (c.is_ascii_digit() && previous.is_some_and(|p| p.is_ascii_alphabetic()));
            if starts_word {
                out.push(' ');
            }
            out.push(c);
        }
        previous = Some(c);
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The rows: the hint, the layout, one per pad action, import, export, reset.
pub fn build_rows(
    settings: &GameSettings,
    language: Language,
    capturing: Option<PadAction>,
    game: Option<&GameInput>,
    community: &[CommunityLayout],
) -> Vec<Row> {
    let changes = settings.controls.clone().unwrap_or_default();
    let mut rows = vec![
        Row::info("hint", language.text(Key::ControlsHint), None),
        layout_row(&changes, language, game, community),
    ];
    for (index, action) in PadAction::ALL.into_iter().enumerate() {
        let mut row = Row::keys(
            action.name(),
            action_title(action, language),
            Controls::sources(action, &changes),
            capturing == Some(action),
        );
        // A face button shows its own glyph; the other rows a glyph that is not a button shape.
        row = match face_glyph(action) {
            Some(glyph) => row.pad_glyph(glyph),
            None => row.icon(action_icon(action)),
        };
        if let Some(names) = game_actions(game).and_then(|a| game_names(a, action)) {
            row = row.detail(names);
        }
        if index == 0 {
            row = row.group_start();
        }
        rows.push(row);
    }
    rows.push(
        Row::command("import", Share::LayoutImport.text(language))
            .icon(Icon::DOWNLOAD_SIMPLE)
            .group_start(),
    );
    rows.push(Row::command("export", Share::LayoutExport.text(language)).icon(Icon::EXPORT));
    rows.push(
        Row::command("reset", language.text(Key::ResetControls))
            .group_start()
            .icon(Icon::ARROW_COUNTER_CLOCKWISE),
    );
    rows
}

/// The controller face glyph in front of Cross, Circle, Square and Triangle.
fn face_glyph(action: PadAction) -> Option<PadGlyph> {
    match action {
        PadAction::Cross => Some(PadGlyph::Cross),
        PadAction::Circle => Some(PadGlyph::Circle),
        PadAction::Square => Some(PadGlyph::Square),
        PadAction::Triangle => Some(PadGlyph::Triangle),
        _ => None,
    }
}

/// The row title of an action.
pub fn action_title(action: PadAction, language: Language) -> String {
    let t = |key: Key| language.text(key);
    let dir = |stick: &str, key: Key| format!("{stick} · {}", t(key));
    let dpad = t(Key::Dpad);
    let left = t(Key::LeftStick);
    let right = t(Key::RightStick);
    match action {
        PadAction::Cross => "Cross".into(),
        PadAction::Circle => "Circle".into(),
        PadAction::Square => "Square".into(),
        PadAction::Triangle => "Triangle".into(),
        PadAction::L1 => "L1".into(),
        PadAction::R1 => "R1".into(),
        PadAction::L2 => "L2".into(),
        PadAction::R2 => "R2".into(),
        PadAction::L3 => "L3".into(),
        PadAction::R3 => "R3".into(),
        PadAction::Options => "Options".into(),
        PadAction::TouchLeft => t(Key::TouchpadLeft).into(),
        PadAction::TouchRight => t(Key::TouchpadRight).into(),
        PadAction::Up => dir(dpad, Key::DirectionUp),
        PadAction::Down => dir(dpad, Key::DirectionDown),
        PadAction::Left => dir(dpad, Key::DirectionLeft),
        PadAction::Right => dir(dpad, Key::DirectionRight),
        PadAction::LeftStickUp => dir(left, Key::DirectionUp),
        PadAction::LeftStickDown => dir(left, Key::DirectionDown),
        PadAction::LeftStickLeft => dir(left, Key::DirectionLeft),
        PadAction::LeftStickRight => dir(left, Key::DirectionRight),
        PadAction::RightStickUp => dir(right, Key::DirectionUp),
        PadAction::RightStickDown => dir(right, Key::DirectionDown),
        PadAction::RightStickLeft => dir(right, Key::DirectionLeft),
        PadAction::RightStickRight => dir(right, Key::DirectionRight),
        PadAction::StickModifier => t(Key::StickModifier).into(),
    }
}

/// The glyph of an action row.
pub fn action_icon(action: PadAction) -> Icon {
    match action {
        PadAction::Cross | PadAction::Circle | PadAction::Square | PadAction::Triangle => {
            Icon::GAME_CONTROLLER
        }
        // Not RECTANGLE or CIRCLE: those read as the Square and Circle buttons.
        PadAction::L1 | PadAction::R1 | PadAction::L2 | PadAction::R2 => Icon::GAME_CONTROLLER,
        PadAction::L3 | PadAction::R3 => Icon::GAME_CONTROLLER,
        PadAction::Options => Icon::LIST,
        PadAction::TouchLeft | PadAction::TouchRight => Icon::HAND_POINTING,
        PadAction::Up | PadAction::Down | PadAction::Left | PadAction::Right => {
            Icon::ARROWS_OUT_CARDINAL
        }
        PadAction::LeftStickUp
        | PadAction::LeftStickDown
        | PadAction::LeftStickLeft
        | PadAction::LeftStickRight
        | PadAction::RightStickUp
        | PadAction::RightStickDown
        | PadAction::RightStickLeft
        | PadAction::RightStickRight => Icon::GAME_CONTROLLER,
        PadAction::StickModifier => Icon::PERSON_SIMPLE_WALK,
    }
}

/// The SDL scancode name of the physical key, as the input file names it. `None` for keys a
/// binding cannot use (the capture ignores them). Keypad keys arrive as their top-row twins.
pub fn key_name(key: EKey) -> Option<&'static str> {
    use EKey::*;
    Some(match key {
        A => "A",
        B => "B",
        C => "C",
        D => "D",
        E => "E",
        F => "F",
        G => "G",
        H => "H",
        I => "I",
        J => "J",
        K => "K",
        L => "L",
        M => "M",
        N => "N",
        O => "O",
        P => "P",
        Q => "Q",
        R => "R",
        S => "S",
        T => "T",
        U => "U",
        V => "V",
        W => "W",
        X => "X",
        Y => "Y",
        Z => "Z",
        Num0 => "0",
        Num1 => "1",
        Num2 => "2",
        Num3 => "3",
        Num4 => "4",
        Num5 => "5",
        Num6 => "6",
        Num7 => "7",
        Num8 => "8",
        Num9 => "9",
        Minus => "-",
        Equals | Plus => "=",
        OpenBracket => "[",
        CloseBracket => "]",
        Backslash | IntlBackslash => "\\",
        Semicolon | Colon => ";",
        Quote => "'",
        Backtick => "`",
        Comma => ",",
        Period => ".",
        Slash => "/",
        Enter => "Return",
        Space => "Space",
        Tab => "Tab",
        Backspace => "Backspace",
        Escape => "Escape",
        Insert => "Insert",
        Delete => "Delete",
        Home => "Home",
        End => "End",
        PageUp => "PageUp",
        PageDown => "PageDown",
        ArrowLeft => "Left",
        ArrowRight => "Right",
        ArrowUp => "Up",
        ArrowDown => "Down",
        F1 => "F1",
        F2 => "F2",
        F3 => "F3",
        F4 => "F4",
        F5 => "F5",
        F6 => "F6",
        F7 => "F7",
        F8 => "F8",
        F9 => "F9",
        F10 => "F10",
        F11 => "F11",
        F12 => "F12",
        ShiftLeft => "Left Shift",
        ShiftRight => "Right Shift",
        ControlLeft => "Left Ctrl",
        ControlRight => "Right Ctrl",
        AltLeft => "Left Alt",
        AltRight => "Right Alt",
        SuperLeft => "Left GUI",
        SuperRight => "Right GUI",
        CapsLock => "CapsLock",
        NumLock => "Numlock",
        ScrollLock => "ScrollLock",
        PrintScreen => "PrintScreen",
        Menu => "Application",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyps5_core::controls::scancode_of;

    #[test]
    fn every_capturable_key_is_a_name_the_core_knows() {
        use EKey::*;
        let keys = [
            A,
            Z,
            Num0,
            Num9,
            Minus,
            Equals,
            OpenBracket,
            CloseBracket,
            Backslash,
            Semicolon,
            Quote,
            Backtick,
            Comma,
            Period,
            Slash,
            Enter,
            Space,
            Tab,
            Backspace,
            Escape,
            Insert,
            Delete,
            Home,
            End,
            PageUp,
            PageDown,
            ArrowLeft,
            ArrowRight,
            ArrowUp,
            ArrowDown,
            F1,
            F12,
            ShiftLeft,
            ShiftRight,
            ControlLeft,
            ControlRight,
            AltLeft,
            AltRight,
            SuperLeft,
            SuperRight,
            CapsLock,
            NumLock,
            ScrollLock,
            PrintScreen,
            Menu,
        ];
        for key in keys {
            let name = key_name(key).unwrap_or_else(|| panic!("{key:?} has no name"));
            assert!(
                scancode_of(name).is_some(),
                "{key:?} -> {name} is not a scancode name"
            );
        }
    }

    #[test]
    fn unbindable_keys_are_ignored() {
        assert_eq!(key_name(EKey::F24), None, "F24 is the unbound marker");
        assert_eq!(key_name(EKey::Copy), None);
        assert_eq!(key_name(EKey::BrowserBack), None);
    }

    #[test]
    fn the_default_layout_shows_in_the_rows() {
        let settings = GameSettings::default();
        let rows = build_rows(&settings, Language::English, None, None, &[]);
        assert_eq!(
            rows.len(),
            1 + 1 + 26 + 3,
            "hint, layout, 26 actions, import, export, reset"
        );
        assert_eq!(rows[0].id, "hint");
        assert_eq!(rows[1].id, "layout");
        let cross = &rows[2];
        assert_eq!(cross.id, "Cross");
        let rows::Control::Keys { sources, capturing } = &cross.control else {
            panic!("keys row");
        };
        assert_eq!(sources, &["KEY:K", "KEY:Space", "KEY:Return"]);
        assert!(!capturing);
        assert_eq!(rows.last().unwrap().id, "reset");
    }

    #[test]
    fn a_captured_key_replaces_keys_keeps_mouse_and_leaves_other_actions() {
        let mut settings = GameSettings::default();
        // Square has J and the left mouse button; give it K (Cross's key).
        let next = Controls::assign("KEY:K", PadAction::Square, &Default::default());
        settings.controls = Some(next);
        let rows = build_rows(
            &settings,
            Language::English,
            Some(PadAction::Square),
            None,
            &[],
        );
        let square = rows.iter().find(|r| r.id == "Square").unwrap();
        let rows::Control::Keys { sources, capturing } = &square.control else {
            panic!("keys row");
        };
        assert_eq!(sources, &["KEY:K", "MOUSE:Left"]);
        assert!(capturing);
        let cross = rows.iter().find(|r| r.id == "Cross").unwrap();
        let rows::Control::Keys { sources, .. } = &cross.control else {
            panic!("keys row");
        };
        assert_eq!(
            sources,
            &["KEY:Space", "KEY:Return"],
            "K was taken from Cross"
        );
    }

    #[test]
    fn the_game_names_its_buttons() {
        use anyps5_core::gamefiles::parse_input_ini;
        assert_eq!(words("MoveForward"), "Move Forward");
        assert_eq!(words("Track_Quest"), "Track Quest");
        assert_eq!(words("Artifact1"), "Artifact 1");
        assert_eq!(words("OpenUI"), "Open UI");
        let ini = "+ActionMappings=(ActionName=\"Jump\",Key=SpaceBar)
                   +ActionMappings=(ActionName=\"Jump\",Key=Gamepad_FaceButton_Bottom)
                   +AxisMappings=(AxisName=\"MoveForward\",Scale=1.0,Key=Gamepad_LeftY)
";
        let input = GameInput::Bindings {
            actions: parse_input_ini(ini),
            source: Default::default(),
        };
        let rows = build_rows(
            &GameSettings::default(),
            Language::English,
            None,
            Some(&input),
            &[],
        );
        let detail = |id: &str| rows.iter().find(|r| r.id == id).unwrap().detail.clone();
        assert_eq!(detail("Cross").as_deref(), Some("Jump"));
        assert_eq!(detail("LeftStickUp").as_deref(), Some("Move Forward"));
        assert_eq!(detail("LeftStickDown").as_deref(), Some("Move Forward −"));
        assert_eq!(detail("Circle"), None);
        let rows::Control::Choice { options, selected } = &rows[1].control else {
            panic!("layout row");
        };
        assert_eq!(
            options,
            &["Launcher", "Mouse and Keyboard", "From the Game"]
        );
        assert_eq!(*selected, 0);
        // The game's layout picked: the row says so; one key moved by hand: Custom.
        let mut settings = GameSettings {
            controls: Some(game_layout(game_actions(Some(&input)).unwrap(), &Changes::new()).0),
            ..Default::default()
        };
        let rows = build_rows(&settings, Language::English, None, Some(&input), &[]);
        let rows::Control::Choice { selected, .. } = &rows[1].control else {
            panic!("layout row");
        };
        assert_eq!(*selected, 2);
        let moved = Controls::assign(
            "KEY:Z",
            PadAction::Triangle,
            settings.controls.as_ref().unwrap(),
        );
        settings.controls = Some(moved);
        let rows = build_rows(&settings, Language::English, None, Some(&input), &[]);
        let rows::Control::Choice { options, selected } = &rows[1].control else {
            panic!("layout row");
        };
        assert_eq!(options[*selected], "Custom");
    }

    #[test]
    fn community_layouts_follow_the_presets_and_show_when_chosen() {
        let shared = CommunityLayout {
            name: "Mouse look".into(),
            author: None,
            description: None,
            changes: Controls::assign("KEY:Z", PadAction::Circle, &Changes::new()),
            tuning: None,
        };
        let community = [shared.clone()];
        let mut settings = GameSettings::default();
        let rows = build_rows(&settings, Language::English, None, None, &community);
        let rows::Control::Choice { options, selected } = &rows[1].control else {
            panic!("layout row");
        };
        assert_eq!(
            options,
            &["Launcher", "Mouse and Keyboard", "Community: Mouse look"]
        );
        assert_eq!(*selected, 0);
        settings.controls = Some(shared.changes);
        let rows = build_rows(&settings, Language::English, None, None, &community);
        let rows::Control::Choice { selected, .. } = &rows[1].control else {
            panic!("layout row");
        };
        assert_eq!(*selected, 2);
        // Offline: nothing extra.
        let rows = build_rows(&GameSettings::default(), Language::English, None, None, &[]);
        let rows::Control::Choice { options, .. } = &rows[1].control else {
            panic!("layout row");
        };
        assert_eq!(options.len(), 2);
    }

    #[test]
    fn layout_files_are_read_with_the_line_of_the_first_mistake() {
        let dir = std::env::temp_dir().join(format!("anyps5-layout-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.ini");
        std::fs::write(
            &file,
            "Cross = KEY:F
Jump = KEY:Space
",
        )
        .unwrap();
        assert_eq!(
            read_layout(&file).unwrap_err(),
            "Line 2: unknown action or setting \"Jump\"."
        );
        std::fs::write(
            &file,
            "Cross = KEY:F
",
        )
        .unwrap();
        assert_eq!(read_layout(&file).unwrap().changes["Cross"], ["KEY:F"]);
        std::fs::write(&file, vec![0xff, 0xfe, 0x41]).unwrap();
        assert_eq!(read_layout(&file).unwrap_err(), "The file cannot be read.");
        std::fs::write(&file, vec![b'#'; 70_000]).unwrap();
        assert_eq!(read_layout(&file).unwrap_err(), "The file cannot be read.");
        assert!(read_layout(&dir.join("missing.ini")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn titles_localise_the_directions() {
        assert_eq!(action_title(PadAction::Up, Language::English), "D-pad · Up");
        assert_eq!(action_title(PadAction::Cross, Language::Turkish), "Cross");
        assert!(
            action_title(PadAction::LeftStickLeft, Language::English).starts_with("Left Stick")
        );
    }
}
