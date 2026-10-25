//! The row list every panel shares (docs/DESIGN.md 5.8): an owned row model, the rule for what a
//! command or click does to a row, and the scrolling list that keeps the focused row centred.
//!
//! A panel builds its rows from its model each frame, feeds the frame's commands through
//! [`drive`], applies the returned [`Change`]s to the model, rebuilds the rows and draws them with
//! [`list`]; mouse input comes back as [`ListEvent`]s and is resolved with the same rules.

use egui::containers::scroll_area::ScrollBarVisibility;
use egui::{Color32, Margin, ScrollArea, Ui};

use crate::screens::scroll::Follow;
use crate::ui::Shell;
use crate::ui::chrome::{KeyCapSpec, PanelRow, RowAvatar, RowControl};
use crate::ui::components::{Force, PadGlyph};
use crate::ui::focus::{PanelEffect, PanelNav, RowKind};
use crate::ui::i18n::{Key, Language};
use crate::ui::icons::Icon;
use crate::ui::input::Command;
use crate::ui::sound::Cue;
use crate::ui::theme::size;

use anyps5_core::controls::Controls;

/// Stable name of a row; panels match on it to apply a [`Change`].
pub type RowId = &'static str;

/// The value control of a row.
#[derive(Clone, Debug, PartialEq)]
pub enum Control {
    Choice {
        options: Vec<String>,
        selected: usize,
    },
    Toggle(bool),
    Stepper {
        value: i32,
        min: i32,
        max: i32,
        step: i32,
        /// The value as shown (`75%`, `1.0×`).
        text: String,
    },
    /// Opens a picker or a panel: a trailing chevron.
    Action,
    /// Runs at once: no chevron.
    Command,
    /// Static text; without a value only the title shows (section headers, hints).
    Info(Option<String>),
    /// A section caption over the rows that follow.
    Caption,
    Text(String),
    /// A pad action's key and mouse sources (`KEY:K`, `MOUSE:Left`).
    Keys {
        sources: Vec<String>,
        capturing: bool,
    },
    /// Small buttons side by side at the row's end; left and right pick one, select presses it.
    Buttons {
        labels: Vec<String>,
        selected: usize,
    },
}

/// One row of a panel.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: RowId,
    pub icon: Option<Icon>,
    pub pad_glyph: Option<PadGlyph>,
    pub title: String,
    pub detail: Option<String>,
    pub control: Control,
    /// Danger: the row removes, ends or blocks something.
    pub destructive: bool,
    /// Caution: an info row that says something may not work. Never danger.
    pub warning: bool,
    /// Starts a new group: the list leaves a double gap above it.
    pub group_start: bool,
    /// A note that belongs to the row above: no gap between them.
    pub attached: bool,
    /// A text row whose field takes the room the title leaves, written from the left.
    pub wide: bool,
    /// A person's picture in the place of the glyph.
    pub avatar: Option<RowAvatar>,
    /// The buttons of a buttons row come up with the focus or the pointer.
    pub reveal: bool,
}

impl Row {
    fn new(id: RowId, title: impl Into<String>, control: Control) -> Row {
        Row {
            id,
            icon: None,
            pad_glyph: None,
            title: title.into(),
            detail: None,
            control,
            destructive: false,
            warning: false,
            group_start: false,
            attached: false,
            wide: false,
            avatar: None,
            reveal: false,
        }
    }

    /// A person's picture instead of a glyph.
    pub fn avatar(mut self, avatar: RowAvatar) -> Row {
        self.avatar = Some(avatar);
        self
    }

    /// The buttons rest hidden and come up with the focus or the pointer.
    pub fn reveal(mut self) -> Row {
        self.reveal = true;
        self
    }

    pub fn choice(
        id: RowId,
        title: impl Into<String>,
        options: Vec<String>,
        selected: usize,
    ) -> Row {
        Row::new(id, title, Control::Choice { options, selected })
    }

    /// Starts a new group (a double gap above the row).
    pub fn group_start(mut self) -> Row {
        self.group_start = true;
        self
    }

    /// Sits right under the row above it, as its note.
    pub fn attached(mut self) -> Row {
        self.attached = true;
        self
    }

    /// A text row whose field takes the room the title leaves.
    pub fn wide(mut self) -> Row {
        self.wide = true;
        self
    }

    pub fn toggle(id: RowId, title: impl Into<String>, on: bool) -> Row {
        Row::new(id, title, Control::Toggle(on))
    }

    pub fn stepper(
        id: RowId,
        title: impl Into<String>,
        value: i32,
        range: (i32, i32),
        step: i32,
        text: impl Into<String>,
    ) -> Row {
        Row::new(
            id,
            title,
            Control::Stepper {
                value,
                min: range.0,
                max: range.1,
                step,
                text: text.into(),
            },
        )
    }

    pub fn action(id: RowId, title: impl Into<String>) -> Row {
        Row::new(id, title, Control::Action)
    }

    pub fn command(id: RowId, title: impl Into<String>) -> Row {
        Row::new(id, title, Control::Command)
    }

    pub fn buttons(
        id: RowId,
        title: impl Into<String>,
        labels: Vec<String>,
        selected: usize,
    ) -> Row {
        Row::new(id, title, Control::Buttons { labels, selected })
    }

    pub fn info(id: RowId, title: impl Into<String>, value: Option<String>) -> Row {
        Row::new(id, title, Control::Info(value))
    }

    pub fn caption(id: RowId, title: impl Into<String>) -> Row {
        Row::new(id, title, Control::Caption)
    }

    pub fn text(id: RowId, title: impl Into<String>, value: impl Into<String>) -> Row {
        Row::new(id, title, Control::Text(value.into()))
    }

    pub fn keys(id: RowId, title: impl Into<String>, sources: Vec<String>, capturing: bool) -> Row {
        Row::new(id, title, Control::Keys { sources, capturing })
    }

    pub fn icon(mut self, icon: Icon) -> Row {
        self.icon = Some(icon);
        self
    }

    pub fn pad_glyph(mut self, glyph: PadGlyph) -> Row {
        self.pad_glyph = Some(glyph);
        self
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Row {
        self.detail = Some(detail.into());
        self
    }

    pub fn destructive(mut self) -> Row {
        self.destructive = true;
        self
    }

    pub fn warning(mut self) -> Row {
        self.warning = true;
        self
    }

    pub fn kind(&self) -> RowKind {
        match self.control {
            Control::Choice { .. } | Control::Buttons { .. } => RowKind::Choice,
            Control::Toggle(_) => RowKind::Toggle,
            Control::Stepper { .. } => RowKind::Stepper,
            Control::Action | Control::Command => RowKind::Action,
            Control::Info(_) | Control::Caption => RowKind::Info,
            Control::Text(_) => RowKind::Text,
            Control::Keys { .. } => RowKind::Keys,
        }
    }
}

/// The kinds of `rows`, as the focus model wants them.
pub fn kinds(rows: &[Row]) -> Vec<RowKind> {
    rows.iter().map(Row::kind).collect()
}

/// What a command or click does to a row's value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    Choice {
        row: RowId,
        index: usize,
    },
    Toggle {
        row: RowId,
        on: bool,
    },
    Stepper {
        row: RowId,
        value: i32,
    },
    /// An action row ran.
    Run {
        row: RowId,
    },
    /// A button of a buttons row was pressed (an index past the last button: the row itself).
    Press {
        row: RowId,
        index: usize,
    },
    /// A keys row asks for a key.
    Capture {
        row: RowId,
    },
    /// A text row asks for the caret.
    Edit {
        row: RowId,
    },
    /// Enter ended the editing of a text row.
    Submit {
        row: RowId,
    },
}

impl Change {
    pub fn row(&self) -> RowId {
        match self {
            Change::Choice { row, .. }
            | Change::Toggle { row, .. }
            | Change::Stepper { row, .. }
            | Change::Run { row }
            | Change::Press { row, .. }
            | Change::Capture { row }
            | Change::Edit { row }
            | Change::Submit { row } => row,
        }
    }

    /// Value changes tick, actions and captures confirm (section 7.1).
    pub fn cue(&self) -> Cue {
        match self {
            Change::Run { .. }
            | Change::Press { .. }
            | Change::Capture { .. }
            | Change::Edit { .. }
            | Change::Submit { .. } => Cue::Select,
            _ => Cue::Move,
        }
    }
}

/// Applies a panel effect to the row it names. Left/right (`Adjust`) clamp; select (`Activate`)
/// advances a choice (wrapping), flips a toggle, steps a stepper up (wrapping to the minimum),
/// runs an action and starts a capture.
pub fn resolve(rows: &[Row], effect: PanelEffect) -> Option<Change> {
    match effect {
        PanelEffect::Adjust { row, direction } => {
            let r = rows.get(row)?;
            match &r.control {
                Control::Choice {
                    options: choices,
                    selected,
                }
                | Control::Buttons {
                    labels: choices,
                    selected,
                } => {
                    let next = (*selected as i64 + i64::from(direction))
                        .clamp(0, choices.len() as i64 - 1) as usize;
                    (next != *selected).then_some(Change::Choice {
                        row: r.id,
                        index: next,
                    })
                }
                Control::Toggle(on) => ((direction > 0) != *on).then_some(Change::Toggle {
                    row: r.id,
                    on: direction > 0,
                }),
                Control::Stepper {
                    value,
                    min,
                    max,
                    step,
                    ..
                } => {
                    let next = (value + i32::from(direction) * step).clamp(*min, *max);
                    (next != *value).then_some(Change::Stepper {
                        row: r.id,
                        value: next,
                    })
                }
                _ => None,
            }
        }
        PanelEffect::Activate { row } => {
            let r = rows.get(row)?;
            match &r.control {
                Control::Choice { options, selected } if !options.is_empty() => {
                    Some(Change::Choice {
                        row: r.id,
                        index: (selected + 1) % options.len(),
                    })
                }
                Control::Buttons { selected, .. } => Some(Change::Press {
                    row: r.id,
                    index: *selected,
                }),
                Control::Toggle(on) => Some(Change::Toggle { row: r.id, on: !on }),
                Control::Stepper {
                    value,
                    min,
                    max,
                    step,
                    ..
                } => Some(Change::Stepper {
                    row: r.id,
                    value: if value + step > *max {
                        *min
                    } else {
                        value + step
                    },
                }),
                Control::Action | Control::Command => Some(Change::Run { row: r.id }),
                Control::Keys { .. } => Some(Change::Capture { row: r.id }),
                Control::Text(_) => Some(Change::Edit { row: r.id }),
                _ => None,
            }
        }
        PanelEffect::None | PanelEffect::Close => None,
    }
}

/// What one frame of commands did to a panel.
#[derive(Debug, Default)]
pub struct Driven {
    pub changes: Vec<Change>,
    /// A command asked to close the panel (or to go back, for controls and advanced).
    pub close: bool,
}

/// Runs `commands` through the navigation model: moves focus, plays the cues and collects the
/// value changes. `nav.row` is kept inside `rows`.
pub fn drive(shell: &mut Shell, nav: &mut PanelNav, rows: &[Row], commands: &[Command]) -> Driven {
    let mut out = Driven::default();
    let kinds = kinds(rows);
    for &command in commands {
        if nav.row >= rows.len() {
            nav.row = rows.len().saturating_sub(1);
        }
        let step = nav.command(command, &kinds);
        if let Some(cue) = step.sound {
            shell.play(cue);
        }
        match step.effect {
            PanelEffect::Close => out.close = true,
            effect @ (PanelEffect::Adjust { .. } | PanelEffect::Activate { .. }) => {
                if let Some(change) = resolve(rows, effect) {
                    out.changes.push(change);
                }
            }
            PanelEffect::None => {}
        }
    }
    out
}

/// What the mouse did in the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListEvent {
    /// A row was clicked or its control used: it takes the focus.
    Focus(usize),
    Effect {
        row: usize,
        effect: PanelEffect,
    },
    /// The text field of a row changed.
    Text {
        row: usize,
        value: String,
    },
    /// Enter ended the editing of a text field.
    Submit(usize),
    /// A button of a buttons row was clicked; an index past the last one is the row's own body.
    Press {
        row: usize,
        index: usize,
    },
}

/// How [`list`] draws.
#[derive(Clone, Copy)]
pub struct ListOptions {
    pub accent: Color32,
    pub language: Language,
    /// The focused row (from the model); `None` while a category list has the focus.
    pub focus: Option<usize>,
    /// Shows a hover or press state without a pointer (galleries).
    pub force: Option<(usize, Force)>,
}

/// Name of a source in the launcher's language: Shift rather than Left Shift, arrows as arrows.
pub fn source_label(source: &str, language: Language) -> String {
    let Some((kind, name)) = source.split_once(':') else {
        return source.to_owned();
    };
    let t = |key: Key| language.text(key).to_owned();
    match (kind, name) {
        ("MOUSE", "Left") => t(Key::MouseLeft),
        ("MOUSE", "Right") => t(Key::MouseRight),
        ("MOUSE", "Middle") => t(Key::MouseMiddle),
        ("MOUSE", "X1") => t(Key::MouseBack),
        ("MOUSE", "X2") => t(Key::MouseForward),
        ("WHEEL", "Up") => t(Key::WheelUp),
        ("WHEEL", "Down") => t(Key::WheelDown),
        ("KEY", "Space") => t(Key::KeySpace),
        ("KEY", "Return") => "Enter".to_owned(),
        ("KEY", "Escape") => "Esc".to_owned(),
        ("KEY", "Backspace") => "⌫".to_owned(),
        ("KEY", "Up") => "↑".to_owned(),
        ("KEY", "Down") => "↓".to_owned(),
        ("KEY", "Left") => "←".to_owned(),
        ("KEY", "Right") => "→".to_owned(),
        ("KEY", "Left Shift" | "Right Shift") => "Shift".to_owned(),
        ("KEY", "Left Ctrl" | "Right Ctrl") => "Ctrl".to_owned(),
        // Windows: Alt and the Windows key, not Option and Cmd (docs/DESIGN.md 6.5).
        ("KEY", "Left Alt" | "Right Alt") => "Alt".to_owned(),
        ("KEY", "Left GUI" | "Right GUI") => "Win".to_owned(),
        (_, name) => name.to_owned(),
    }
}

fn key_caps(sources: &[String], language: Language) -> Vec<KeyCapSpec> {
    sources
        .iter()
        .filter(|s| s.as_str() != Controls::UNBOUND)
        .map(|s| KeyCapSpec {
            label: source_label(s, language),
            keyboard: s.starts_with("KEY:"),
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// The scrolling list
// ---------------------------------------------------------------------------------------------

/// Rows fade out over this distance at a clipped edge of a list that scrolls.
const LIST_FADE: f32 = 28.0;

/// Draws `rows` in a scroll area that fills the available space and follows the focused row with
/// the focus spring. Returns what the mouse did.
pub fn list(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    rows: &mut [Row],
    options: &ListOptions,
) -> Vec<ListEvent> {
    list_with_header(ui, id_salt, rows, options, |_| {})
}

/// [`list`] with something above the first row, scrolling with the rows (the profile page's
/// banner). The follower counts its height, so the focus still centres the right row.
pub fn list_with_header(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    rows: &mut [Row],
    options: &ListOptions,
    header: impl FnOnce(&mut Ui),
) -> Vec<ListEvent> {
    let ctx = ui.ctx().clone();
    let id = ui.make_persistent_id(id_salt);
    let (follow, scroll_to) = Follow::begin(&ctx, id, options.focus);

    let mut events = Vec::new();
    let mut geometry: Vec<(f32, f32)> = Vec::with_capacity(rows.len());
    // A row keeps its identity when others come or go above it: a message arriving does not
    // move the caret out of the field below.
    let mut seen: std::collections::HashMap<RowId, usize> = std::collections::HashMap::new();
    let mut area = ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink([false, false])
        .scroll_bar_visibility(ScrollBarVisibility::AlwaysHidden);
    if let Some(offset) = scroll_to {
        area = area.vertical_scroll_offset(offset);
    }
    // A list taller than its panel fades rows out over the last 28 dp at a clipped edge instead
    // of cutting them hard. Whether it overflows is known from the last frame.
    let overflow_id = id.with("overflows");
    let overflows: bool = ctx.data(|d| d.get_temp(overflow_id)).unwrap_or(false);
    let clip = ui.clip_rect().intersect(ui.max_rect());
    let output = area.show(ui, |ui| {
        egui::Frame::NONE
            .inner_margin(Margin::same(size::LIST_INSET as i8))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = size::ROW_GAP;
                let origin = ui.cursor().min.y;
                header(ui);
                for index in 0..rows.len() {
                    // The gap after a row is the next row's to give: none before an attached note.
                    let next_attached = rows.get(index + 1).is_some_and(|next| next.attached);
                    ui.spacing_mut().item_spacing.y =
                        if next_attached { 0.0 } else { size::ROW_GAP };
                    let occurrence = {
                        let count = seen.entry(rows[index].id).or_default();
                        *count += 1;
                        *count - 1
                    };
                    let row = &mut rows[index];
                    if row.group_start && index > 0 {
                        ui.add_space(size::ROW_GAP);
                    }
                    let focused = options.focus == Some(index);
                    let force = options.force.and_then(|(i, f)| (i == index).then_some(f));
                    let top = ui.cursor().min.y;
                    let centre = top + size::ROW_MIN_HEIGHT * 0.5;
                    let fade = if overflows {
                        let edge = (centre - clip.min.y).min(clip.max.y - centre);
                        (edge / LIST_FADE).clamp(0.0, 1.0)
                    } else {
                        1.0
                    };
                    let out = ui
                        .scope(|ui| {
                            ui.multiply_opacity(fade);
                            show_row(
                                ui,
                                row,
                                (index, occurrence),
                                focused,
                                force,
                                options,
                                &mut events,
                            )
                        })
                        .inner;
                    geometry.push((out.rect.min.y - origin, out.rect.height()));
                }
            });
    });
    let overflowing = output.content_size.y > output.inner_rect.height() + 1.0;
    ctx.data_mut(|d| d.insert_temp(overflow_id, overflowing));
    follow.end(
        &ctx,
        geometry,
        output.content_size.y,
        output.inner_rect.height(),
        output.state.clamped_offset().y,
    );
    events
}

struct Shown {
    rect: egui::Rect,
}

fn show_row(
    ui: &mut Ui,
    row: &mut Row,
    (index, occurrence): (usize, usize),
    focused: bool,
    force: Option<Force>,
    options: &ListOptions,
    events: &mut Vec<ListEvent>,
) -> Shown {
    let language = options.language;
    let Row {
        id,
        icon,
        pad_glyph,
        title,
        detail,
        control,
        destructive,
        warning,
        group_start: _,
        attached,
        wide,
        avatar,
        reveal,
    } = row;
    let caps;
    let none_label = language.text(Key::NoKey);
    let press_label = language.text(Key::PressKey);
    let rc = match control {
        Control::Choice {
            options: choices,
            selected,
        } => RowControl::Choice {
            value: choices.get(*selected).map_or("", String::as_str),
            can_decrement: *selected > 0,
            can_increment: *selected + 1 < choices.len(),
        },
        Control::Toggle(on) => RowControl::Toggle {
            on: *on,
            accent: options.accent,
        },
        Control::Stepper {
            value,
            min,
            max,
            text,
            ..
        } => RowControl::Stepper {
            value: text.as_str(),
            can_decrement: *value > *min,
            can_increment: *value < *max,
        },
        Control::Buttons { labels, selected } => RowControl::Buttons {
            labels,
            selected: *selected,
        },
        Control::Action => RowControl::Action,
        Control::Command => RowControl::Command,
        Control::Caption => RowControl::Caption,
        Control::Info(value) => RowControl::Info {
            value: value.as_deref(),
        },
        Control::Text(value) => RowControl::Text { value },
        Control::Keys { sources, capturing } => {
            caps = key_caps(sources, language);
            RowControl::Keys {
                caps: &caps,
                capturing: *capturing,
                none_label,
                press_label,
            }
        }
    };
    let is_text = matches!(rc, RowControl::Text { .. });
    let focusable = !matches!(rc, RowControl::Info { .. } | RowControl::Caption);
    let mut panel_row = PanelRow::new(title, rc)
        .language(language)
        .focused(focused)
        .destructive(*destructive)
        .warning(*warning)
        .attached(*attached)
        .wide(*wide)
        .avatar(*avatar)
        .reveal(*reveal)
        .force(force);
    if let Some(icon) = icon {
        panel_row = panel_row.icon(*icon);
    }
    if let Some(detail) = detail.as_deref() {
        panel_row = panel_row.detail(detail);
    }
    if let Some(glyph) = pad_glyph {
        panel_row = panel_row.pad_glyph(*glyph);
    }
    let out = ui.push_id((*id, occurrence), |ui| panel_row.show(ui)).inner;
    let rect = out.response.rect;
    let buttons = match &*control {
        Control::Buttons { labels, .. } => Some(labels.len()),
        _ => None,
    };
    if let Some(pressed) = out.pressed {
        events.push(ListEvent::Focus(index));
        events.push(ListEvent::Press {
            row: index,
            index: pressed,
        });
    } else if let Some(direction) = out.step {
        events.push(ListEvent::Focus(index));
        events.push(ListEvent::Effect {
            row: index,
            effect: PanelEffect::Adjust {
                row: index,
                direction,
            },
        });
    } else if let (Some(count), true) = (buttons, out.response.clicked()) {
        // The body of a buttons row never presses one of them: a stray click must not answer.
        events.push(ListEvent::Focus(index));
        events.push(ListEvent::Press {
            row: index,
            index: count,
        });
    } else if out.toggled || (out.response.clicked() && focusable && !is_text) {
        events.push(ListEvent::Focus(index));
        events.push(ListEvent::Effect {
            row: index,
            effect: PanelEffect::Activate { row: index },
        });
    }
    if is_text {
        if out.response.hovered() && ui.input(|i| i.pointer.primary_pressed()) {
            events.push(ListEvent::Focus(index));
        }
        if out.text_changed
            && let Control::Text(value) = control
        {
            events.push(ListEvent::Text {
                row: index,
                value: value.clone(),
            });
        }
        if out.submitted {
            events.push(ListEvent::Submit(index));
        }
    }
    Shown { rect }
}

/// Resolves the mouse events of a frame: moves the focus, plays the cues and collects the value
/// changes and text edits.
pub fn apply_mouse(
    shell: &mut Shell,
    nav: &mut PanelNav,
    rows: &[Row],
    events: Vec<ListEvent>,
) -> (Vec<Change>, Vec<(RowId, String)>) {
    let mut changes = Vec::new();
    let mut texts = Vec::new();
    for event in events {
        match event {
            ListEvent::Focus(row) => {
                nav.row = row;
                nav.in_rows = true;
            }
            ListEvent::Effect { effect, .. } => {
                if let Some(change) = resolve(rows, effect) {
                    shell.play(change.cue());
                    changes.push(change);
                }
            }
            ListEvent::Text { row, value } => {
                if let Some(r) = rows.get(row) {
                    texts.push((r.id, value));
                }
            }
            ListEvent::Submit(row) => {
                if let Some(r) = rows.get(row) {
                    changes.push(Change::Submit { row: r.id });
                }
            }
            ListEvent::Press { row, index } => {
                if let Some(r) = rows.get(row) {
                    shell.play(Cue::Select);
                    changes.push(Change::Press { row: r.id, index });
                }
            }
        }
    }
    (changes, texts)
}

/// Index of the row with this id.
pub fn position(rows: &[Row], id: RowId) -> Option<usize> {
    rows.iter().position(|r| r.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> Vec<Row> {
        vec![
            Row::caption("head", "Section"),
            Row::choice(
                "choice",
                "Choice",
                vec!["a".into(), "b".into(), "c".into()],
                1,
            ),
            Row::toggle("toggle", "Toggle", true),
            Row::stepper("step", "Step", 90, (40, 100), 5, "90%"),
            Row::action("act", "Action"),
            Row::keys("keys", "Keys", vec!["KEY:K".into()], false),
        ]
    }

    fn adjust(row: usize, direction: i8) -> PanelEffect {
        PanelEffect::Adjust { row, direction }
    }

    #[test]
    fn left_and_right_clamp_instead_of_wrapping() {
        let rows = rows();
        assert_eq!(
            resolve(&rows, adjust(1, -1)),
            Some(Change::Choice {
                row: "choice",
                index: 0
            })
        );
        assert_eq!(
            resolve(&rows, adjust(1, 1)),
            Some(Change::Choice {
                row: "choice",
                index: 2
            })
        );
        let mut at_end = rows.clone();
        at_end[1].control = Control::Choice {
            options: vec!["a".into(), "b".into()],
            selected: 1,
        };
        assert_eq!(resolve(&at_end, adjust(1, 1)), None);
        assert_eq!(resolve(&rows, adjust(2, 1)), None, "already on");
        assert_eq!(
            resolve(&rows, adjust(2, -1)),
            Some(Change::Toggle {
                row: "toggle",
                on: false
            })
        );
        assert_eq!(
            resolve(&rows, adjust(3, 1)),
            Some(Change::Stepper {
                row: "step",
                value: 95
            })
        );
        assert_eq!(resolve(&rows, adjust(4, 1)), None, "actions do not adjust");
    }

    #[test]
    fn select_cycles_wraps_and_runs() {
        let rows = rows();
        let activate = |row| PanelEffect::Activate { row };
        assert_eq!(
            resolve(&rows, activate(1)),
            Some(Change::Choice {
                row: "choice",
                index: 2
            })
        );
        let mut last = rows.clone();
        last[1].control = Control::Choice {
            options: vec!["a".into(), "b".into()],
            selected: 1,
        };
        assert_eq!(
            resolve(&last, activate(1)),
            Some(Change::Choice {
                row: "choice",
                index: 0
            }),
            "select wraps"
        );
        assert_eq!(
            resolve(&rows, activate(2)),
            Some(Change::Toggle {
                row: "toggle",
                on: false
            })
        );
        assert_eq!(
            resolve(&rows, activate(3)),
            Some(Change::Stepper {
                row: "step",
                value: 95
            })
        );
        let mut top = rows.clone();
        top[3].control = Control::Stepper {
            value: 100,
            min: 40,
            max: 100,
            step: 5,
            text: "100%".into(),
        };
        assert_eq!(
            resolve(&top, activate(3)),
            Some(Change::Stepper {
                row: "step",
                value: 40
            }),
            "a stepper wraps to its minimum"
        );
        assert_eq!(
            resolve(&rows, activate(4)),
            Some(Change::Run { row: "act" })
        );
        assert_eq!(
            resolve(&rows, activate(5)),
            Some(Change::Capture { row: "keys" })
        );
        assert_eq!(resolve(&rows, activate(0)), None, "info rows do nothing");
    }

    #[test]
    fn cues_follow_section_7() {
        assert_eq!(Change::Run { row: "x" }.cue(), Cue::Select);
        assert_eq!(Change::Capture { row: "x" }.cue(), Cue::Select);
        assert_eq!(Change::Toggle { row: "x", on: true }.cue(), Cue::Move);
    }

    #[test]
    fn source_labels_read_like_keys_on_a_pc() {
        let en = Language::English;
        assert_eq!(source_label("KEY:Left Shift", en), "Shift");
        assert_eq!(source_label("KEY:Right Alt", en), "Alt");
        assert_eq!(source_label("KEY:Left GUI", en), "Win");
        assert_eq!(source_label("KEY:Up", en), "↑");
        assert_eq!(source_label("KEY:Escape", en), "Esc");
        assert_eq!(source_label("KEY:K", en), "K");
        assert_eq!(source_label("MOUSE:Left", en), "Left Click");
        assert_eq!(source_label("MOUSE:X1", en), "Mouse 4");
        assert_eq!(source_label("WHEEL:Up", en), "Wheel ↑");
    }

    #[test]
    fn unbound_has_no_key_cap() {
        let caps = key_caps(&["KEY:F24".to_owned()], Language::English);
        assert!(caps.is_empty());
    }
}
