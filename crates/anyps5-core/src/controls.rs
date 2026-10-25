//! Ported from utkuhalis/AnyPS5-ARM (Controls.swift, GPL-2.0-only) and changed since; see README "Credits".
//! Keyboard and mouse bindings, the tuning values and the text of the per-game `anyps5-input.ini`
//! (PORT.md 6.7, Controls.swift; file format: upstream `docs/user/INPUT_MAPPING.md`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A pad input a key or mouse button can press, named as the engine's input file names it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub enum PadAction {
    Cross,
    Circle,
    Square,
    Triangle,
    L1,
    R1,
    L2,
    R2,
    L3,
    R3,
    Options,
    TouchLeft,
    TouchRight,
    Up,
    Down,
    Left,
    Right,
    LeftStickUp,
    LeftStickDown,
    LeftStickLeft,
    LeftStickRight,
    RightStickUp,
    RightStickDown,
    RightStickLeft,
    RightStickRight,
    /// Held, the keyboard sticks tilt only by the tuning's stick modifier scale (walking).
    StickModifier,
}

impl PadAction {
    /// Every action, in the order of the ini lines.
    pub const ALL: [PadAction; 26] = [
        PadAction::Cross,
        PadAction::Circle,
        PadAction::Square,
        PadAction::Triangle,
        PadAction::L1,
        PadAction::R1,
        PadAction::L2,
        PadAction::R2,
        PadAction::L3,
        PadAction::R3,
        PadAction::Options,
        PadAction::TouchLeft,
        PadAction::TouchRight,
        PadAction::Up,
        PadAction::Down,
        PadAction::Left,
        PadAction::Right,
        PadAction::LeftStickUp,
        PadAction::LeftStickDown,
        PadAction::LeftStickLeft,
        PadAction::LeftStickRight,
        PadAction::RightStickUp,
        PadAction::RightStickDown,
        PadAction::RightStickLeft,
        PadAction::RightStickRight,
        PadAction::StickModifier,
    ];

    /// The name used in the ini file and as the key of the settings JSON.
    pub fn name(self) -> &'static str {
        match self {
            PadAction::Cross => "Cross",
            PadAction::Circle => "Circle",
            PadAction::Square => "Square",
            PadAction::Triangle => "Triangle",
            PadAction::L1 => "L1",
            PadAction::R1 => "R1",
            PadAction::L2 => "L2",
            PadAction::R2 => "R2",
            PadAction::L3 => "L3",
            PadAction::R3 => "R3",
            PadAction::Options => "Options",
            PadAction::TouchLeft => "TouchLeft",
            PadAction::TouchRight => "TouchRight",
            PadAction::Up => "Up",
            PadAction::Down => "Down",
            PadAction::Left => "Left",
            PadAction::Right => "Right",
            PadAction::LeftStickUp => "LeftStickUp",
            PadAction::LeftStickDown => "LeftStickDown",
            PadAction::LeftStickLeft => "LeftStickLeft",
            PadAction::LeftStickRight => "LeftStickRight",
            PadAction::RightStickUp => "RightStickUp",
            PadAction::RightStickDown => "RightStickDown",
            PadAction::RightStickLeft => "RightStickLeft",
            PadAction::RightStickRight => "RightStickRight",
            PadAction::StickModifier => "StickModifier",
        }
    }

    pub fn from_name(name: &str) -> Option<PadAction> {
        PadAction::ALL.into_iter().find(|a| a.name() == name)
    }
}

/// Settings for the controller sticks, the mouse as the right stick and the keyboard sticks, in
/// the steps the launcher shows: whole percentages, tenths and milliseconds. The defaults are the
/// engine's; only changed values are written. Missing keys in a saved file keep their default.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InputTuning {
    pub left_deadzone: i32,
    pub right_deadzone: i32,
    pub mouse_sensitivity: i32,
    pub mouse_vertical_scale: i32,
    pub mouse_curve: i32,
    pub mouse_deadzone_offset: i32,
    pub mouse_max_speed: i32,
    pub mouse_smoothing_ms: i32,
    pub mouse_decay_ms: i32,
    pub stick_modifier_scale: i32,
    pub keyboard_ramp_ms: i32,
    pub trigger_ramp_ms: i32,
}

impl Default for InputTuning {
    fn default() -> Self {
        InputTuning {
            left_deadzone: 0,
            right_deadzone: 0,
            mouse_sensitivity: 10,
            mouse_vertical_scale: 10,
            mouse_curve: 10,
            mouse_deadzone_offset: 20,
            mouse_max_speed: 10,
            mouse_smoothing_ms: 10,
            mouse_decay_ms: 30,
            stick_modifier_scale: 50,
            keyboard_ramp_ms: 0,
            trigger_ramp_ms: 0,
        }
    }
}

impl InputTuning {
    /// `Setting = Value` lines for the values that differ from the defaults after clamping to the
    /// ranges the engine accepts, in the file's order.
    pub fn lines(&self) -> Vec<String> {
        let defaults = InputTuning::default();
        // (name, clamped value, default, divisor: 1 = integer)
        let settings: [(&str, i32, i32, i32); 12] = [
            (
                "LeftStickDeadzone",
                self.left_deadzone.clamp(0, 90),
                defaults.left_deadzone,
                1,
            ),
            (
                "RightStickDeadzone",
                self.right_deadzone.clamp(0, 90),
                defaults.right_deadzone,
                1,
            ),
            (
                "MouseSensitivity",
                self.mouse_sensitivity.clamp(1, 100),
                defaults.mouse_sensitivity,
                10,
            ),
            (
                "MouseVerticalScale",
                self.mouse_vertical_scale.clamp(1, 100),
                defaults.mouse_vertical_scale,
                10,
            ),
            (
                "MouseCurve",
                self.mouse_curve.clamp(2, 50),
                defaults.mouse_curve,
                10,
            ),
            (
                "MouseDeadzoneOffset",
                self.mouse_deadzone_offset.clamp(0, 90),
                defaults.mouse_deadzone_offset,
                100,
            ),
            (
                "MouseMaxSpeed",
                self.mouse_max_speed.clamp(1, 100),
                defaults.mouse_max_speed,
                10,
            ),
            (
                "MouseSmoothingMs",
                self.mouse_smoothing_ms.clamp(0, 500),
                defaults.mouse_smoothing_ms,
                1,
            ),
            (
                "MouseDecayMs",
                self.mouse_decay_ms.clamp(1, 1000),
                defaults.mouse_decay_ms,
                1,
            ),
            (
                "StickModifierScale",
                self.stick_modifier_scale.clamp(5, 100),
                defaults.stick_modifier_scale,
                100,
            ),
            (
                "KeyboardRampMs",
                self.keyboard_ramp_ms.clamp(0, 2000),
                defaults.keyboard_ramp_ms,
                1,
            ),
            (
                "TriggerRampMs",
                self.trigger_ramp_ms.clamp(0, 2000),
                defaults.trigger_ramp_ms,
                1,
            ),
        ];
        settings
            .into_iter()
            .filter(|(_, value, default, _)| value != default)
            .map(|(name, value, _, divisor)| match divisor {
                1 => format!("{name} = {value}"),
                // Debug prints the shortest round-trip form with ".0" for whole numbers, like Swift's Double.
                d => format!("{name} = {:?}", f64::from(value) / f64::from(d)),
            })
            .collect()
    }
}

/// The user's moves away from the default layout: action name to its sources.
pub type Changes = BTreeMap<String, Vec<String>>;

/// The keyboard and mouse layout a game starts with: the launcher's own layout with the keys the
/// user moved. Every action is written to the input file, so none of the engine's scattered
/// built-in keys stay behind.
pub struct Controls;

impl Controls {
    /// Stands for "no key": the engine refuses an empty source and no offered key is F24.
    pub const UNBOUND: &'static str = "KEY:F24";

    /// First line of every input file.
    pub const HEADER: &'static str = "# Written by the AnyPlay Launcher at each start; change the keys in the game's settings there.";

    /// The launcher's default sources of an action (`KEY:<SDL name>`, `MOUSE:<button>`, `WHEEL:<Up|Down>`).
    pub fn default_sources(action: PadAction) -> &'static [&'static str] {
        use PadAction::*;
        match action {
            Cross => &["KEY:K", "KEY:Space", "KEY:Return"],
            Circle => &["KEY:L"],
            Square => &["KEY:J", "MOUSE:Left"],
            Triangle => &["KEY:I"],
            L1 => &["KEY:Q"],
            R1 => &["KEY:E"],
            L2 => &["KEY:U", "MOUSE:X1"],
            R2 => &["KEY:O", "MOUSE:Right"],
            L3 => &["KEY:Left Shift"],
            R3 => &["KEY:Left Ctrl"],
            Options => &["KEY:Escape"],
            TouchLeft => &["KEY:Backspace"],
            TouchRight => &["KEY:Tab"],
            Up => &["KEY:Up", "WHEEL:Up"],
            Down => &["KEY:Down", "WHEEL:Down"],
            Left => &["KEY:Left"],
            Right => &["KEY:Right"],
            LeftStickUp => &["KEY:W"],
            LeftStickDown => &["KEY:S"],
            LeftStickLeft => &["KEY:A"],
            LeftStickRight => &["KEY:D"],
            RightStickUp => &["KEY:T"],
            RightStickDown => &["KEY:G"],
            RightStickLeft => &["KEY:F"],
            RightStickRight => &["KEY:H"],
            StickModifier => &[],
        }
    }

    /// The sources an action has: the user's change if there is one (even an empty list), else
    /// the default layout.
    pub fn sources(action: PadAction, changes: &Changes) -> Vec<String> {
        match changes.get(action.name()) {
            Some(own) => own.clone(),
            None => Controls::default_sources(action)
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
        }
    }

    /// The changes after giving `key` (a `KEY:` source) to `action`: it replaces the action's keys,
    /// keeps its mouse and wheel sources, and is taken away from every other action that had it.
    /// Entries equal to the default layout are dropped.
    pub fn assign(key: &str, to: PadAction, changes: &Changes) -> Changes {
        let mut result = changes.clone();
        for other in PadAction::ALL.into_iter().filter(|a| *a != to) {
            let current = Controls::sources(other, changes);
            if current.iter().any(|s| s == key) {
                result.insert(
                    other.name().to_string(),
                    current.into_iter().filter(|s| s != key).collect(),
                );
            }
        }
        let mut own = vec![key.to_string()];
        own.extend(
            Controls::sources(to, changes)
                .into_iter()
                .filter(|s| !s.starts_with("KEY:")),
        );
        result.insert(to.name().to_string(), own);
        result.retain(|name, value| match PadAction::from_name(name) {
            Some(a) => Controls::default_sources(a) != value.as_slice(),
            None => EXTRA_ACTIONS.contains(&name.as_str()),
        });
        result
    }

    /// The text of `anyps5-input.ini` for the changes and the tuning. UTF-8, LF, final newline.
    pub fn file(changes: &Changes, tuning: &InputTuning) -> String {
        Controls::file_for(changes, tuning, None)
    }

    /// [`Controls::file`] for the toolkit that will read it (its version or tag; `None` when
    /// unknown): the lines only toolkits from v0.1.3 accept are left out for an older one.
    pub fn file_for(changes: &Changes, tuning: &InputTuning, toolkit: Option<&str>) -> String {
        let mut changes = changes.clone();
        let mut tuning = *tuning;
        if !writes_v013_lines(toolkit) {
            changes.remove(PadAction::StickModifier.name());
            tuning.keyboard_ramp_ms = InputTuning::default().keyboard_ramp_ms;
            tuning.trigger_ramp_ms = InputTuning::default().trigger_ramp_ms;
        }
        Controls::text(Controls::HEADER, &changes, &tuning)
    }

    /// A layout as a file to share: the same lines as the input file (the form of upstream's
    /// `INPUT_MAPPING.md`) under a header that names the layout.
    pub fn export(changes: &Changes, tuning: &InputTuning, title: &str) -> String {
        let header = format!(
            "# AnyPS5 controls layout: {}\n# Format: INPUT_MAPPING.md of AnyPS5. Actions left out have no key of their own.",
            title.replace(['\r', '\n'], " ")
        );
        Controls::text(&header, changes, tuning)
    }

    fn text(header: &str, changes: &Changes, tuning: &InputTuning) -> String {
        let mut lines = vec![header.to_string()];
        for action in PadAction::ALL {
            let sources = Controls::sources(action, changes);
            // Toolkits before v0.1.3 do not know StickModifier and stop at a line naming it; it
            // has no built-in key, so while it is unbound the line stays out.
            if action == PadAction::StickModifier && sources.is_empty() {
                continue;
            }
            push_sources(&mut lines, action.name(), &sources);
        }
        // Actions the launcher has no row for travel with the layout.
        for name in EXTRA_ACTIONS {
            if let Some(sources) = changes.get(name) {
                push_sources(&mut lines, name, sources);
            }
        }
        lines.extend(tuning.lines());
        let mut text = lines.join("\n");
        text.push('\n');
        text
    }

    /// Reads a layout file in the form of `INPUT_MAPPING.md`, as the engine does: names without
    /// regard to case, `#` or `;` at the start of a line for a comment, the first line of an
    /// action replacing its built-in keys and later ones adding to them, repeated entries
    /// ignored, actions the file leaves out keeping the engine's own keys. The first line the
    /// engine would refuse is the error.
    pub fn parse(text: &str) -> Result<ParsedLayout, ImportError> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut listed: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
        let mut tuning = InputTuning::default();
        let mut tuned: Vec<&'static str> = Vec::new();
        for (index, raw) in text.lines().enumerate() {
            let fail = |message: String| ImportError {
                line: index + 1,
                message,
            };
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            let Some((name, value)) = line.split_once('=') else {
                return Err(fail("expected \"Action = KEY:Name\"".into()));
            };
            let (name, value) = (name.trim(), value.trim());
            if let Some(setting) = TUNING_NAMES
                .into_iter()
                .find(|s| s.eq_ignore_ascii_case(name))
            {
                if tuned.contains(&setting) {
                    return Err(fail(format!("{setting} appears twice")));
                }
                set_tuning(&mut tuning, setting, value).map_err(fail)?;
                tuned.push(setting);
                continue;
            }
            let Some(action) = ACTION_NAMES
                .into_iter()
                .find(|a| a.eq_ignore_ascii_case(name))
            else {
                return Err(fail(format!("unknown action or setting \"{name}\"")));
            };
            let source = parse_source(action, value).map_err(fail)?;
            let sources = listed.entry(action).or_default();
            if !sources.contains(&source) {
                sources.push(source);
            }
        }
        let own = |name: &str| -> Option<Vec<String>> {
            listed.get(name).map(|sources| {
                sources
                    .iter()
                    .filter(|s| s.as_str() != Controls::UNBOUND)
                    .cloned()
                    .collect()
            })
        };
        let mut changes = Changes::new();
        for action in PadAction::ALL {
            let sources = own(action.name())
                .unwrap_or_else(|| strings(upstream_default_sources(action.name())));
            if sources != Controls::default_sources(action) {
                changes.insert(action.name().to_string(), sources);
            }
        }
        for name in EXTRA_ACTIONS {
            if let Some(sources) = own(name)
                && sources != upstream_default_sources(name)
            {
                changes.insert(name.to_string(), sources);
            }
        }
        Ok(ParsedLayout {
            changes,
            tuning: (!tuned.is_empty()).then_some(tuning),
        })
    }
}

/// Whether the toolkit reads `StickModifier`, `KeyboardRampMs` and `TriggerRampMs` (an older one
/// stops input at such a line); see [`crate::compat`]. An unknown or local toolkit gets them.
pub fn writes_v013_lines(toolkit: Option<&str>) -> bool {
    use crate::compat::{Feature, supports};
    toolkit.is_none_or(|t| supports(Feature::StickModifier, t) && supports(Feature::InputRamps, t))
}

fn strings(sources: &[&str]) -> Vec<String> {
    sources.iter().map(|s| (*s).to_string()).collect()
}

/// The lines of one action; an action without a source is written as the unbound marker.
fn push_sources(lines: &mut Vec<String>, name: &str, sources: &[String]) {
    if sources.is_empty() {
        lines.push(format!("{name} = {}", Controls::UNBOUND));
    }
    lines.extend(sources.iter().map(|source| format!("{name} = {source}")));
}

/// Actions of the engine's input file the launcher has no row for. They are kept in the changes
/// by name, written back and shared with the layout.
pub const EXTRA_ACTIONS: [&str; 5] = [
    "Shake",
    "ToggleMouse",
    "ToggleFullscreen",
    "TogglePerformanceOverlay",
    "Screenshot",
];

const ACTION_NAMES: [&str; 31] = [
    "Cross",
    "Circle",
    "Square",
    "Triangle",
    "L1",
    "R1",
    "L2",
    "R2",
    "L3",
    "R3",
    "Options",
    "TouchLeft",
    "TouchRight",
    "Up",
    "Down",
    "Left",
    "Right",
    "LeftStickUp",
    "LeftStickDown",
    "LeftStickLeft",
    "LeftStickRight",
    "RightStickUp",
    "RightStickDown",
    "RightStickLeft",
    "RightStickRight",
    "StickModifier",
    "Shake",
    "ToggleMouse",
    "ToggleFullscreen",
    "TogglePerformanceOverlay",
    "Screenshot",
];

const TUNING_NAMES: [&str; 12] = [
    "LeftStickDeadzone",
    "RightStickDeadzone",
    "MouseSensitivity",
    "MouseVerticalScale",
    "MouseCurve",
    "MouseDeadzoneOffset",
    "MouseMaxSpeed",
    "MouseSmoothingMs",
    "MouseDecayMs",
    "StickModifierScale",
    "KeyboardRampMs",
    "TriggerRampMs",
];

/// The engine's built-in sources of an action (INPUT_MAPPING.md, "Default bindings"): what a
/// file that does not mention the action leaves in force.
pub fn upstream_default_sources(action: &str) -> &'static [&'static str] {
    match action {
        "Cross" => &["KEY:Return", "KEY:Space"],
        "Circle" => &["KEY:C"],
        "Triangle" => &["KEY:I"],
        "Square" => &["MOUSE:Left"],
        "L1" => &["KEY:Q"],
        "R1" => &["KEY:E", "KEY:Left Alt", "KEY:Right Alt"],
        "L2" => &["KEY:R", "MOUSE:X1"],
        "R2" => &["MOUSE:Right"],
        "L3" => &["KEY:Left Shift", "KEY:Right Shift"],
        "R3" => &["KEY:Left Ctrl", "KEY:Right Ctrl"],
        "Options" => &["KEY:Escape"],
        "Up" => &["KEY:Up", "WHEEL:Up"],
        "Right" => &["KEY:Right"],
        "Down" => &["KEY:Down", "WHEEL:Down"],
        "Left" => &["KEY:Left"],
        "LeftStickLeft" => &["KEY:A"],
        "LeftStickRight" => &["KEY:D"],
        "LeftStickUp" => &["KEY:W"],
        "LeftStickDown" => &["KEY:S"],
        "RightStickLeft" => &["KEY:F"],
        "RightStickRight" => &["KEY:H"],
        "RightStickUp" => &["KEY:T"],
        "RightStickDown" => &["KEY:G"],
        "TouchLeft" => &["KEY:Backspace"],
        "TouchRight" => &["KEY:Tab"],
        "Shake" => &["KEY:X"],
        "ToggleMouse" => &["MOUSE:Middle", "KEY:F7"],
        "ToggleFullscreen" => &["KEY:F11"],
        "TogglePerformanceOverlay" => &["KEY:F10"],
        "Screenshot" => &["KEY:F12"],
        _ => &[],
    }
}

/// A layout file read by [`Controls::parse`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedLayout {
    /// Against the launcher's layout, the form games store.
    pub changes: Changes,
    /// `Some` when the file has at least one tuning line; the other values are the defaults.
    pub tuning: Option<InputTuning>,
}

/// Why a layout file cannot be used: the line the engine would stop at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportError {
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

/// `Type:Value` as a canonical source (`KEY:Left Shift`).
fn parse_source(action: &str, value: &str) -> Result<String, String> {
    let Some((kind, name)) = value.split_once(':') else {
        return Err(format!(
            "\"{value}\" is not Type:Value (KEY, MOUSE or WHEEL)"
        ));
    };
    let (kind, name) = (kind.trim(), name.trim());
    let pick =
        |names: &[&'static str]| names.iter().find(|n| n.eq_ignore_ascii_case(name)).copied();
    if kind.eq_ignore_ascii_case("KEY") {
        if let Some((_, known)) = SCANCODES.iter().find(|(_, n)| n.eq_ignore_ascii_case(name)) {
            return Ok(format!("KEY:{known}"));
        }
        // F13 to F24: SDL names them, no key of the launcher's list is one (F24 is "unbound").
        if let Some(number) = name
            .strip_prefix(['F', 'f'])
            .and_then(|n| n.parse::<u8>().ok())
            && (13..=24).contains(&number)
        {
            return Ok(format!("KEY:F{number}"));
        }
        return Err(format!("\"{name}\" is not a key name the launcher knows"));
    }
    if matches!(
        action,
        "ToggleFullscreen" | "TogglePerformanceOverlay" | "Screenshot"
    ) {
        return Err(format!("{action} takes keyboard keys only"));
    }
    if kind.eq_ignore_ascii_case("MOUSE") {
        return pick(&["Left", "Middle", "Right", "X1", "X2"])
            .map(|b| format!("MOUSE:{b}"))
            .ok_or_else(|| {
                format!("\"{name}\" is not a mouse button (Left, Middle, Right, X1 or X2)")
            });
    }
    if kind.eq_ignore_ascii_case("WHEEL") {
        return pick(&["Up", "Down"])
            .map(|b| format!("WHEEL:{b}"))
            .ok_or_else(|| format!("\"{name}\" is not a wheel direction (Up or Down)"));
    }
    Err(format!(
        "\"{kind}\" is not a source type (KEY, MOUSE or WHEEL)"
    ))
}

/// Writes one `Setting = Value` into the tuning, in the steps the launcher keeps, within the
/// ranges of INPUT_MAPPING.md.
fn set_tuning(tuning: &mut InputTuning, setting: &str, value: &str) -> Result<(), String> {
    let number: f64 = value
        .parse()
        .ok()
        .filter(|n: &f64| n.is_finite())
        .ok_or_else(|| format!("{setting} needs a number, not \"{value}\""))?;
    let check = |low: f64, high: f64, step: f64| -> Result<i32, String> {
        if number < low || number > high {
            return Err(format!("{setting} must be from {low} to {high}"));
        }
        Ok((number * step).round() as i32)
    };
    match setting {
        "LeftStickDeadzone" => tuning.left_deadzone = check(0.0, 90.0, 1.0)?,
        "RightStickDeadzone" => tuning.right_deadzone = check(0.0, 90.0, 1.0)?,
        "MouseSensitivity" => tuning.mouse_sensitivity = check(0.1, 10.0, 10.0)?,
        "MouseVerticalScale" => tuning.mouse_vertical_scale = check(0.1, 10.0, 10.0)?,
        "MouseCurve" => tuning.mouse_curve = check(0.2, 5.0, 10.0)?,
        "MouseDeadzoneOffset" => tuning.mouse_deadzone_offset = check(0.0, 0.9, 100.0)?,
        "MouseMaxSpeed" => tuning.mouse_max_speed = check(0.1, 10.0, 10.0)?,
        "MouseSmoothingMs" => tuning.mouse_smoothing_ms = check(0.0, 500.0, 1.0)?,
        "MouseDecayMs" => tuning.mouse_decay_ms = check(1.0, 1000.0, 1.0)?,
        "StickModifierScale" => tuning.stick_modifier_scale = check(0.05, 1.0, 100.0)?,
        "KeyboardRampMs" => tuning.keyboard_ramp_ms = check(0.0, 2000.0, 1.0)?,
        "TriggerRampMs" => tuning.trigger_ramp_ms = check(0.0, 2000.0, 1.0)?,
        _ => {}
    }
    Ok(())
}

/// Ready-made layouts the Controls panel offers (docs/DESIGN.md 5.17).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    /// The launcher's own layout: WASD moves, J K L I are the face buttons.
    Classic,
    /// The usual PC action-game layout: the mouse looks (it already drives the right stick), its
    /// buttons are the triggers, and the face buttons sit around WASD.
    MouseAndKeyboard,
}

impl Preset {
    pub const ALL: [Preset; 2] = [Preset::Classic, Preset::MouseAndKeyboard];

    /// The sources of every action in this layout.
    pub fn sources(self, action: PadAction) -> &'static [&'static str] {
        use PadAction::*;
        match self {
            Preset::Classic => Controls::default_sources(action),
            Preset::MouseAndKeyboard => match action {
                Cross => &["KEY:Space"],
                Circle => &["KEY:C"],
                Square => &["KEY:F"],
                Triangle => &["KEY:E"],
                L1 => &["KEY:Q"],
                R1 => &["KEY:R"],
                L2 => &["MOUSE:Right"],
                R2 => &["MOUSE:Left"],
                L3 => &["KEY:Left Shift"],
                R3 => &["KEY:V", "MOUSE:Middle"],
                Options => &["KEY:Escape"],
                TouchLeft => &["KEY:Tab"],
                TouchRight => &["KEY:M"],
                Up => &["KEY:1", "WHEEL:Up"],
                Right => &["KEY:2"],
                Down => &["KEY:3", "WHEEL:Down"],
                Left => &["KEY:4"],
                LeftStickUp => &["KEY:W"],
                LeftStickDown => &["KEY:S"],
                LeftStickLeft => &["KEY:A"],
                LeftStickRight => &["KEY:D"],
                RightStickUp => &["KEY:Up"],
                RightStickDown => &["KEY:Down"],
                RightStickLeft => &["KEY:Left"],
                RightStickRight => &["KEY:Right"],
                StickModifier => &["KEY:Left Alt"],
            },
        }
    }

    /// The layout as changes against the launcher's defaults (the form games store).
    pub fn changes(self) -> Changes {
        PadAction::ALL
            .into_iter()
            .filter(|a| self.sources(*a) != Controls::default_sources(*a))
            .map(|a| {
                (
                    a.name().to_string(),
                    self.sources(a).iter().map(|s| (*s).to_string()).collect(),
                )
            })
            .collect()
    }

    /// The preset these changes are exactly, if any.
    pub fn matching(changes: &Changes) -> Option<Preset> {
        Preset::ALL.into_iter().find(|p| p.changes() == *changes)
    }
}

/// The SDL scancode name of a Windows set-1 scan code (`MapVirtualKeyW(vk, MAPVK_VK_TO_VSC_EX)`:
/// extended keys as `0xE000 | code`), `None` for keys a binding cannot use. The key's position on a
/// US keyboard decides, as SDL reads scancodes. F24 is deliberately absent.
pub fn key_name(scancode: u16) -> Option<&'static str> {
    SCANCODES
        .iter()
        .find(|(code, _)| *code == scancode)
        .map(|(_, name)| *name)
}

/// The inverse of [`key_name`], for showing which physical key a binding means.
pub fn scancode_of(name: &str) -> Option<u16> {
    SCANCODES
        .iter()
        .find(|(_, n)| *n == name)
        .map(|(code, _)| *code)
}

#[rustfmt::skip]
const SCANCODES: &[(u16, &str)] = &[
    (0x01, "Escape"), (0x02, "1"), (0x03, "2"), (0x04, "3"), (0x05, "4"), (0x06, "5"), (0x07, "6"), (0x08, "7"),
    (0x09, "8"), (0x0A, "9"), (0x0B, "0"), (0x0C, "-"), (0x0D, "="), (0x0E, "Backspace"), (0x0F, "Tab"),
    (0x10, "Q"), (0x11, "W"), (0x12, "E"), (0x13, "R"), (0x14, "T"), (0x15, "Y"), (0x16, "U"), (0x17, "I"),
    (0x18, "O"), (0x19, "P"), (0x1A, "["), (0x1B, "]"), (0x1C, "Return"), (0x1D, "Left Ctrl"),
    (0x1E, "A"), (0x1F, "S"), (0x20, "D"), (0x21, "F"), (0x22, "G"), (0x23, "H"), (0x24, "J"), (0x25, "K"),
    (0x26, "L"), (0x27, ";"), (0x28, "'"), (0x29, "`"), (0x2A, "Left Shift"), (0x2B, "\\"),
    (0x2C, "Z"), (0x2D, "X"), (0x2E, "C"), (0x2F, "V"), (0x30, "B"), (0x31, "N"), (0x32, "M"), (0x33, ","),
    (0x34, "."), (0x35, "/"), (0x36, "Right Shift"), (0x37, "Keypad *"), (0x38, "Left Alt"),
    (0x39, "Space"), (0x3A, "CapsLock"),
    (0x3B, "F1"), (0x3C, "F2"), (0x3D, "F3"), (0x3E, "F4"), (0x3F, "F5"), (0x40, "F6"), (0x41, "F7"),
    (0x42, "F8"), (0x43, "F9"), (0x44, "F10"), (0x45, "Numlock"), (0x46, "ScrollLock"),
    (0x47, "Keypad 7"), (0x48, "Keypad 8"), (0x49, "Keypad 9"), (0x4A, "Keypad -"), (0x4B, "Keypad 4"),
    (0x4C, "Keypad 5"), (0x4D, "Keypad 6"), (0x4E, "Keypad +"), (0x4F, "Keypad 1"), (0x50, "Keypad 2"),
    (0x51, "Keypad 3"), (0x52, "Keypad 0"), (0x53, "Keypad ."), (0x57, "F11"), (0x58, "F12"),
    (0xE01C, "Keypad Enter"), (0xE01D, "Right Ctrl"), (0xE035, "Keypad /"), (0xE037, "PrintScreen"),
    (0xE038, "Right Alt"), (0xE047, "Home"), (0xE048, "Up"), (0xE049, "PageUp"), (0xE04B, "Left"),
    (0xE04D, "Right"), (0xE04F, "End"), (0xE050, "Down"), (0xE051, "PageDown"), (0xE052, "Insert"),
    (0xE053, "Delete"), (0xE05B, "Left GUI"), (0xE05C, "Right GUI"), (0xE05D, "Application"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const GOLDEN: &str = "# Written by the AnyPlay Launcher at each start; change the keys in the game's settings there.
Cross = KEY:K
Cross = KEY:Space
Cross = KEY:Return
Circle = KEY:L
Square = KEY:J
Square = MOUSE:Left
Triangle = KEY:I
L1 = KEY:Q
R1 = KEY:E
L2 = KEY:U
L2 = MOUSE:X1
R2 = KEY:O
R2 = MOUSE:Right
L3 = KEY:Left Shift
R3 = KEY:Left Ctrl
Options = KEY:Escape
TouchLeft = KEY:Backspace
TouchRight = KEY:Tab
Up = KEY:Up
Up = WHEEL:Up
Down = KEY:Down
Down = WHEEL:Down
Left = KEY:Left
Right = KEY:Right
LeftStickUp = KEY:W
LeftStickDown = KEY:S
LeftStickLeft = KEY:A
LeftStickRight = KEY:D
RightStickUp = KEY:T
RightStickDown = KEY:G
RightStickLeft = KEY:F
RightStickRight = KEY:H
";

    fn changes(entries: &[(&str, &[&str])]) -> Changes {
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.iter().map(|s| s.to_string()).collect()))
            .collect()
    }

    #[test]
    fn default_file_is_the_golden_text() {
        assert_eq!(
            Controls::file(&Changes::new(), &InputTuning::default()),
            GOLDEN
        );
    }

    #[test]
    fn all_26_actions_in_order_and_names_roundtrip() {
        assert_eq!(PadAction::ALL.len(), 26);
        assert_eq!(PadAction::ALL[0].name(), "Cross");
        assert_eq!(PadAction::ALL[25].name(), "StickModifier");
        for a in PadAction::ALL {
            assert_eq!(PadAction::from_name(a.name()), Some(a));
        }
        assert_eq!(PadAction::from_name("cross"), None);
    }

    #[test]
    fn changed_key_and_emptied_action() {
        let c = changes(&[("Circle", &["KEY:C"]), ("Triangle", &[])]);
        let text = Controls::file(&c, &InputTuning::default());
        assert!(text.contains("\nCircle = KEY:C\n"));
        assert!(!text.contains("Circle = KEY:L"));
        assert!(text.contains("\nTriangle = KEY:F24\n"));
    }

    #[test]
    fn tuning_lines_are_appended() {
        let tuning = InputTuning {
            left_deadzone: 15,
            mouse_sensitivity: 25,
            ..Default::default()
        };
        let text = Controls::file(&Changes::new(), &tuning);
        assert!(text.ends_with(
            "RightStickRight = KEY:H\nLeftStickDeadzone = 15\nMouseSensitivity = 2.5\n"
        ));
    }

    #[test]
    fn stick_modifier_is_written_only_when_it_has_a_key() {
        // Toolkits before v0.1.3 stop at a StickModifier line; no key is its built-in state.
        let text = Controls::file(&Changes::new(), &InputTuning::default());
        assert!(!text.contains("StickModifier"));
        // The ramps are tuning lines: written only when moved, so v0.1.2 files stay valid.
        assert!(!text.contains("KeyboardRampMs") && !text.contains("TriggerRampMs"));
        let on = changes(&[("StickModifier", &["KEY:Left Alt"])]);
        assert!(
            Controls::file(&on, &InputTuning::default())
                .contains("\nStickModifier = KEY:Left Alt\n")
        );
    }

    #[test]
    fn older_toolkits_get_no_v013_lines() {
        assert!(!writes_v013_lines(Some("0.1.2")));
        assert!(!writes_v013_lines(Some("v0.1.1")));
        assert!(!writes_v013_lines(Some("v0.0.9-abcdef")));
        assert!(writes_v013_lines(Some("0.1.3")));
        assert!(writes_v013_lines(Some("v0.1.3-abcdef123456")));
        assert!(writes_v013_lines(Some("v0.2.0")));
        assert!(writes_v013_lines(Some("1.0.0")));
        assert!(writes_v013_lines(None), "unknown toolkit");
        assert!(writes_v013_lines(Some("local-abcdef")), "adopted folder");
        let moved = changes(&[("StickModifier", &["KEY:Z"]), ("Circle", &["KEY:C"])]);
        let tuning = InputTuning {
            keyboard_ramp_ms: 100,
            trigger_ramp_ms: 50,
            left_deadzone: 5,
            ..Default::default()
        };
        let old = Controls::file_for(&moved, &tuning, Some("0.1.2"));
        assert!(!old.contains("StickModifier") && !old.contains("Ramp"));
        assert!(old.contains("Circle = KEY:C\n") && old.contains("LeftStickDeadzone = 5\n"));
        let new = Controls::file_for(&moved, &tuning, Some("0.1.3"));
        assert!(new.contains("StickModifier = KEY:Z\n") && new.contains("KeyboardRampMs = 100\n"));
        assert!(new.contains("TriggerRampMs = 50\n"));
        assert_eq!(new, Controls::file(&moved, &tuning));
    }

    #[test]
    fn defaults_give_no_tuning_lines() {
        assert!(InputTuning::default().lines().is_empty());
    }

    #[test]
    fn each_tuning_setting_alone() {
        let one = |f: fn(&mut InputTuning)| {
            let mut t = InputTuning::default();
            f(&mut t);
            t.lines()
        };
        assert_eq!(one(|t| t.left_deadzone = 30), ["LeftStickDeadzone = 30"]);
        assert_eq!(one(|t| t.right_deadzone = 90), ["RightStickDeadzone = 90"]);
        assert_eq!(one(|t| t.mouse_sensitivity = 1), ["MouseSensitivity = 0.1"]);
        assert_eq!(
            one(|t| t.mouse_vertical_scale = 20),
            ["MouseVerticalScale = 2.0"]
        );
        assert_eq!(one(|t| t.mouse_curve = 25), ["MouseCurve = 2.5"]);
        assert_eq!(
            one(|t| t.mouse_deadzone_offset = 5),
            ["MouseDeadzoneOffset = 0.05"]
        );
        assert_eq!(one(|t| t.mouse_max_speed = 90), ["MouseMaxSpeed = 9.0"]);
        assert_eq!(one(|t| t.mouse_smoothing_ms = 0), ["MouseSmoothingMs = 0"]);
        assert_eq!(one(|t| t.mouse_decay_ms = 500), ["MouseDecayMs = 500"]);
        assert_eq!(
            one(|t| t.stick_modifier_scale = 100),
            ["StickModifierScale = 1.0"]
        );
        assert_eq!(one(|t| t.keyboard_ramp_ms = 120), ["KeyboardRampMs = 120"]);
        assert_eq!(one(|t| t.trigger_ramp_ms = 2000), ["TriggerRampMs = 2000"]);
        assert_eq!(
            one(|t| t.mouse_deadzone_offset = 90),
            ["MouseDeadzoneOffset = 0.9"]
        );
    }

    #[test]
    fn clamp_edges() {
        let one = |f: fn(&mut InputTuning)| {
            let mut t = InputTuning::default();
            f(&mut t);
            t.lines()
        };
        assert_eq!(one(|t| t.left_deadzone = 91), ["LeftStickDeadzone = 90"]);
        assert_eq!(
            one(|t| t.left_deadzone = -5),
            Vec::<String>::new(),
            "clamps to 0 = default"
        );
        assert_eq!(one(|t| t.mouse_sensitivity = 0), ["MouseSensitivity = 0.1"]);
        assert_eq!(
            one(|t| t.mouse_sensitivity = 101),
            ["MouseSensitivity = 10.0"]
        );
        assert_eq!(one(|t| t.mouse_curve = 1), ["MouseCurve = 0.2"]);
        assert_eq!(one(|t| t.mouse_curve = 51), ["MouseCurve = 5.0"]);
        assert_eq!(
            one(|t| t.stick_modifier_scale = 4),
            ["StickModifierScale = 0.05"]
        );
        assert_eq!(one(|t| t.mouse_decay_ms = 0), ["MouseDecayMs = 1"]);
        assert_eq!(one(|t| t.mouse_decay_ms = 1001), ["MouseDecayMs = 1000"]);
        assert_eq!(
            one(|t| t.keyboard_ramp_ms = 2001),
            ["KeyboardRampMs = 2000"]
        );
        assert_eq!(
            one(|t| t.mouse_smoothing_ms = 501),
            ["MouseSmoothingMs = 500"]
        );
        assert_eq!(
            one(|t| t.mouse_deadzone_offset = 91),
            ["MouseDeadzoneOffset = 0.9"]
        );
        // a value that clamps onto the default writes nothing
        assert!(one(|t| t.mouse_curve = 10).is_empty());
    }

    #[test]
    fn line_order_follows_the_file_order() {
        let t = InputTuning {
            trigger_ramp_ms: 5,
            left_deadzone: 1,
            mouse_decay_ms: 40,
            ..Default::default()
        };
        assert_eq!(
            t.lines(),
            [
                "LeftStickDeadzone = 1",
                "MouseDecayMs = 40",
                "TriggerRampMs = 5"
            ]
        );
    }

    #[test]
    fn tuning_json_defaults_and_names() {
        let t: InputTuning = serde_json::from_str(r#"{"leftDeadzone": 12}"#).unwrap();
        assert_eq!(t.left_deadzone, 12);
        assert_eq!(t.mouse_decay_ms, 30);
        let json = serde_json::to_value(InputTuning::default()).unwrap();
        assert_eq!(json["mouseDeadzoneOffset"], 20);
        assert_eq!(json.as_object().unwrap().len(), 12);
    }

    #[test]
    fn presets_use_real_keys_once_each() {
        for preset in Preset::ALL {
            let mut seen = HashSet::new();
            for action in PadAction::ALL {
                for source in preset.sources(action) {
                    assert!(seen.insert(*source), "{preset:?}: {source} twice");
                    if let Some(name) = source.strip_prefix("KEY:") {
                        assert!(scancode_of(name).is_some(), "{preset:?}: {name}");
                    }
                }
            }
        }
        assert!(Preset::Classic.changes().is_empty());
        let mk = Preset::MouseAndKeyboard.changes();
        assert_eq!(mk["R2"], vec!["MOUSE:Left".to_string()]);
        assert!(
            !mk.contains_key("LeftStickUp"),
            "WASD is the default already"
        );
        assert_eq!(Preset::matching(&mk), Some(Preset::MouseAndKeyboard));
        assert_eq!(Preset::matching(&Changes::new()), Some(Preset::Classic));
    }

    #[test]
    fn sources_prefer_changes_even_when_empty() {
        let c = changes(&[("Cross", &[])]);
        assert!(Controls::sources(PadAction::Cross, &c).is_empty());
        assert_eq!(Controls::sources(PadAction::Circle, &c), ["KEY:L"]);
        assert!(Controls::sources(PadAction::StickModifier, &c).is_empty());
    }

    #[test]
    fn assigning_a_taken_key_moves_it() {
        let c = Controls::assign("KEY:W", PadAction::Cross, &Changes::new());
        assert_eq!(c["Cross"], ["KEY:W"]);
        assert_eq!(c["LeftStickUp"], Vec::<String>::new());
        assert_eq!(c.len(), 2);
        let text = Controls::file(&c, &InputTuning::default());
        assert!(text.contains("Cross = KEY:W\n") && text.contains("LeftStickUp = KEY:F24\n"));
        assert!(!text.contains("Cross = KEY:K"));
    }

    #[test]
    fn assign_keeps_mouse_and_wheel_sources() {
        let c = Controls::assign("KEY:Z", PadAction::Up, &Changes::new());
        assert_eq!(c["Up"], ["KEY:Z", "WHEEL:Up"]);
        let c = Controls::assign("KEY:Z", PadAction::Square, &Changes::new());
        assert_eq!(c["Square"], ["KEY:Z", "MOUSE:Left"]);
    }

    #[test]
    fn assigning_the_default_back_removes_the_entry_and_is_idempotent() {
        let c = Controls::assign("KEY:Z", PadAction::Circle, &Changes::new());
        assert_eq!(c.len(), 1);
        assert_eq!(Controls::assign("KEY:Z", PadAction::Circle, &c), c);
        let back = Controls::assign("KEY:L", PadAction::Circle, &c);
        assert!(back.is_empty());
    }

    #[test]
    fn assign_drops_unknown_action_names() {
        let c = changes(&[("Bogus", &["KEY:A"])]);
        assert!(!Controls::assign("KEY:Z", PadAction::Circle, &c).contains_key("Bogus"));
    }

    #[test]
    fn scancode_table_is_consistent() {
        let names: HashSet<_> = SCANCODES.iter().map(|(_, n)| *n).collect();
        let codes: HashSet<_> = SCANCODES.iter().map(|(c, _)| *c).collect();
        assert_eq!(names.len(), SCANCODES.len(), "names unique");
        assert_eq!(codes.len(), SCANCODES.len(), "codes unique");
        assert!(SCANCODES.len() > 100);
        assert!(!names.contains("F24"));
        assert_eq!(key_name(0x2A), Some("Left Shift"));
        assert_eq!(key_name(0x36), Some("Right Shift"));
        assert_eq!(key_name(0x1D), Some("Left Ctrl"));
        assert_eq!(key_name(0xE01D), Some("Right Ctrl"));
        assert_eq!(key_name(0x38), Some("Left Alt"));
        assert_eq!(key_name(0xE038), Some("Right Alt"));
        assert_eq!(key_name(0x56), None);
        assert_eq!(key_name(0x64), None);
        assert_eq!(scancode_of("Space"), Some(0x39));
    }

    #[test]
    fn the_input_file_reads_back_as_the_launcher_layout() {
        let parsed = Controls::parse(GOLDEN).unwrap();
        assert!(parsed.changes.is_empty());
        assert_eq!(parsed.tuning, None);
    }

    #[test]
    fn an_export_reads_back_exactly() {
        let mut c = Controls::assign("KEY:Z", PadAction::Circle, &Changes::new());
        c = Controls::assign("KEY:W", PadAction::Cross, &c);
        c.insert("ToggleMouse".into(), vec!["KEY:F8".into()]);
        c.insert("Shake".into(), vec![]);
        let tuning = InputTuning {
            left_deadzone: 12,
            mouse_sensitivity: 25,
            mouse_deadzone_offset: 12,
            ..Default::default()
        };
        let text = Controls::export(&c, &tuning, "Stray\nPPSA01");
        assert!(text.starts_with("# AnyPS5 controls layout: Stray PPSA01\n"));
        assert!(text.contains("\nShake = KEY:F24\n") && text.contains("\nToggleMouse = KEY:F8\n"));
        let parsed = Controls::parse(&text).unwrap();
        assert_eq!(parsed.changes, c);
        assert_eq!(parsed.tuning, Some(tuning));
    }

    #[test]
    fn a_partial_file_leaves_the_engines_own_keys_in_force() {
        // The example of INPUT_MAPPING.md.
        let text = "\u{feff}# comment\n; another\nCross = KEY:F\nCROSS = KEY:Space\nLeftStickLeft = KEY:J\n\
                    leftstickright = key:l\nSquare = MOUSE:Left\nR2 = mouse:right\nToggleMouse = MOUSE:Middle\n\
                    Cross = KEY:F\n";
        let c = Controls::parse(text).unwrap().changes;
        assert_eq!(
            c["Cross"],
            ["KEY:F", "KEY:Space"],
            "first replaces, later add, repeats go"
        );
        assert_eq!(c["LeftStickLeft"], ["KEY:J"]);
        assert_eq!(c["LeftStickRight"], ["KEY:L"]);
        // Left out: the engine's keys, which are not the launcher's.
        assert_eq!(c["Circle"], ["KEY:C"]);
        assert_eq!(c["L3"], ["KEY:Left Shift", "KEY:Right Shift"]);
        // Same as the launcher's layout: no entry.
        assert!(!c.contains_key("Options"));
        assert_eq!(
            c["ToggleMouse"],
            ["MOUSE:Middle"],
            "F7 is dropped: the file named only one"
        );
    }

    #[test]
    fn the_first_bad_line_is_named() {
        let err = |text: &str| Controls::parse(text).unwrap_err();
        assert_eq!(err("Cross = KEY:K\nJump = KEY:Space\n").line, 2);
        assert!(err("Cross KEY:K").message.contains("expected"));
        assert!(err("Cross = KEY:Banana").message.contains("Banana"));
        assert!(err("Cross = PAD:A").message.contains("source type"));
        assert!(err("Cross = KEY").message.contains("Type:Value"));
        assert!(err("Cross = MOUSE:Fourth").message.contains("mouse button"));
        assert!(err("Cross = WHEEL:Left").message.contains("wheel"));
        assert!(
            err("ToggleFullscreen = MOUSE:Left")
                .message
                .contains("keyboard")
        );
        assert!(err("MouseCurve = 6").message.contains("0.2 to 5"));
        assert!(err("MouseCurve = fast").message.contains("number"));
        assert!(
            err("LeftStickDeadzone = 1\nleftstickdeadzone = 2")
                .message
                .contains("twice")
        );
        assert_eq!(
            err("LeftStickDeadzone = 91").to_string(),
            "line 1: LeftStickDeadzone must be from 0 to 90"
        );
        assert!(Controls::parse("Cross = KEY:F24").is_ok());
        assert!(Controls::parse("Cross = KEY:F17").is_ok());
    }

    #[test]
    fn unbound_markers_and_empty_files() {
        let c = Controls::parse("Cross = KEY:F24\nCircle = KEY:F24\nCircle = KEY:Z\n")
            .unwrap()
            .changes;
        assert_eq!(c["Cross"], Vec::<String>::new());
        assert_eq!(c["Circle"], ["KEY:Z"]);
        // Nothing listed: every action is the engine's.
        let all = Controls::parse("").unwrap().changes;
        assert_eq!(all["Cross"], ["KEY:Return", "KEY:Space"]);
        assert_eq!(all["Square"], ["MOUSE:Left"]);
    }

    #[test]
    fn every_default_key_can_be_captured_again() {
        for action in PadAction::ALL {
            for source in Controls::default_sources(action) {
                if let Some(key) = source.strip_prefix("KEY:") {
                    assert!(scancode_of(key).is_some(), "{key} has no scan code");
                }
            }
        }
    }
}
