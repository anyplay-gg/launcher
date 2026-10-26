//! Input (docs/DESIGN.md section 6): one pipeline from keyboard, mouse wheel and gamepad to the
//! seven abstract [`Command`]s. [`Input::poll`] is the only consumer of raw events; screens act on
//! the returned [`InputEvent`]s and draw focus from their own model (see `ui::focus`).
//!
//! * Keyboard: arrows, Enter/Space/NumpadEnter = select, Esc/Backspace = back, `O` = options.
//!   Keys with Ctrl or Alt held are ignored, except `Ctrl+,` (settings; `Cmd+,` on a Mac) and `F11`
//!   (fullscreen).
//! * Wheel: +-40 dp of accumulated scroll make one step (one classic wheel notch = one step).
//! * Gamepad: D-pad and left stick (> 0.55) are directions; A/B/Y/Start are select/back/options/options.
//! * A held direction, from the pad or from an arrow key, fires at once, repeats after 0.36 s every
//!   0.10 s and, after six repeats, every 0.06 s (one [`Repeater`] logic for both devices, so a
//!   list moves the same under a thumb and under a finger).

use std::sync::mpsc::Receiver;
use std::time::Duration;

use egui::{Context, Event, Key, Modifiers, MouseWheelUnit, PointerButton};

/// The seven abstract commands every screen understands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    Left,
    Right,
    Up,
    Down,
    Select,
    Back,
    Options,
}

impl Command {
    pub fn is_direction(self) -> bool {
        matches!(
            self,
            Command::Left | Command::Right | Command::Up | Command::Down
        )
    }
}

/// What [`Input::poll`] hands to the screens.
#[derive(Clone, Debug, PartialEq)]
pub enum InputEvent {
    /// A command from keyboard or gamepad.
    Command(Command),
    /// A wheel step: `Left` = previous, `Right` = next. Only the home screen without an overlay uses it.
    Scroll(Command),
    /// `Tab` (`backwards` with Shift): the next or previous stop in reading order. Each screen
    /// decides what that is; text fields and a call on screen keep Tab for egui.
    Tab { backwards: bool },
    /// `Ctrl+,`, `Cmd+,` on a Mac
    OpenSettings,
    /// `F11`
    ToggleFullscreen,
    /// The controller's Create (Share) button: save a clip.
    Create,
    /// A raw key press while [`Input::capture_keys`] is on (the controls panel binds keys with it).
    CapturedKey {
        key: Key,
        physical_key: Option<Key>,
        modifiers: Modifiers,
    },
}

/// Gamepad messages from the polling thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PadMessage {
    /// The held direction changed (`None` = released).
    Direction(Option<Command>),
    /// A button went down.
    Button(Command),
    /// The Create (Share, View) button went down.
    Create,
}

// ---------------------------------------------------------------------------------------------
// Keyboard
// ---------------------------------------------------------------------------------------------

/// The command a key press stands for. Ignored with Ctrl, Alt or Command held.
pub fn command_for_key(key: Key, modifiers: Modifiers) -> Option<Command> {
    if modifiers.ctrl || modifiers.alt || modifiers.command || modifiers.mac_cmd {
        return None;
    }
    match key {
        Key::ArrowLeft => Some(Command::Left),
        Key::ArrowRight => Some(Command::Right),
        Key::ArrowUp => Some(Command::Up),
        Key::ArrowDown => Some(Command::Down),
        Key::Enter | Key::Space => Some(Command::Select),
        Key::Escape | Key::Backspace => Some(Command::Back),
        Key::O => Some(Command::Options),
        _ => None,
    }
}

/// Tab or Shift+Tab; Ctrl, Alt and Command with Tab belong to the system.
pub fn is_tab_press(modifiers: Modifiers) -> bool {
    !(modifiers.ctrl || modifiers.alt || modifiers.command || modifiers.mac_cmd)
}

// ---------------------------------------------------------------------------------------------
// Wheel
// ---------------------------------------------------------------------------------------------

/// Accumulated scroll that makes one step, in dp.
pub const WHEEL_STEP: f32 = 40.0;
/// A pause this long ends a scroll gesture and clears the accumulator.
const WHEEL_IDLE: f64 = 0.25;

/// Turns scroll deltas into steps. `next` positive = toward the next game (wheel down, scroll right).
#[derive(Debug, Default)]
pub struct WheelAccumulator {
    accumulated: f32,
    last_event: f64,
}

impl WheelAccumulator {
    pub fn feed(&mut self, next: f32, time: f64) -> Option<Command> {
        if time - self.last_event > WHEEL_IDLE {
            self.accumulated = 0.0;
        }
        self.last_event = time;
        self.accumulated += next;
        if self.accumulated.abs() < WHEEL_STEP {
            return None;
        }
        let command = if self.accumulated > 0.0 {
            Command::Right
        } else {
            Command::Left
        };
        self.accumulated = 0.0;
        Some(command)
    }
}

// ---------------------------------------------------------------------------------------------
// Direction repeat
// ---------------------------------------------------------------------------------------------

/// Delay before a held direction starts to repeat.
pub const REPEAT_DELAY: f64 = 0.36;
/// Interval between the first repeats.
pub const REPEAT_INTERVAL: f64 = 0.10;
/// Repeats at [`REPEAT_INTERVAL`] before a long list speeds up.
pub const REPEAT_SLOW_COUNT: u32 = 6;
/// Interval once the hold has gone on for a while.
pub const REPEAT_FAST_INTERVAL: f64 = 0.06;

/// Console-menu style repeat for held directions: a pause, a steady pace, then a faster one for
/// long lists and the library grid.
#[derive(Debug, Default)]
pub struct Repeater {
    held: Option<Command>,
    next_fire: f64,
    repeats: u32,
}

impl Repeater {
    /// A direction was pressed, changed or released. Returns the command to fire now: a new
    /// direction fires at once, an identical one is ignored, a release cancels the timers.
    pub fn set_direction(&mut self, direction: Option<Command>, now: f64) -> Option<Command> {
        if direction == self.held {
            return None;
        }
        self.held = direction;
        self.next_fire = now + REPEAT_DELAY;
        self.repeats = 0;
        direction
    }

    fn interval(&self) -> f64 {
        if self.repeats >= REPEAT_SLOW_COUNT {
            REPEAT_FAST_INTERVAL
        } else {
            REPEAT_INTERVAL
        }
    }

    /// Call every frame. Returns a repeat if one is due.
    pub fn tick(&mut self, now: f64) -> Option<Command> {
        let held = self.held?;
        if now < self.next_fire {
            return None;
        }
        // Keep the rate when frames are slow, but never fire a burst.
        let interval = self.interval();
        self.next_fire = (self.next_fire + interval).max(now + interval * 0.5);
        self.repeats += 1;
        Some(held)
    }

    /// Seconds until the next repeat, while a direction is held.
    pub fn next_due_in(&self, now: f64) -> Option<f64> {
        self.held.map(|_| (self.next_fire - now).max(0.0))
    }

    pub fn held(&self) -> Option<Command> {
        self.held
    }

    /// The hold has produced a repeat: what fires now is not a fresh press.
    pub fn is_repeating(&self) -> bool {
        self.repeats > 0
    }
}

// ---------------------------------------------------------------------------------------------
// The pipeline
// ---------------------------------------------------------------------------------------------

/// The device the player used last; hint bars show its keys.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Device {
    #[default]
    Keyboard,
    Gamepad,
}

/// The device [`Input::poll`] saw last (keyboard until a gamepad is used).
pub fn device(ctx: &Context) -> Device {
    ctx.data(|d| d.get_temp(device_id())).unwrap_or_default()
}

fn device_id() -> egui::Id {
    egui::Id::unique("input-device")
}

/// Whether the pointer may light things up: not while a gamepad is the device in use, so a resting
/// mouse never leaves a second control lit beside the one the pad has focused, and not after a key
/// until the pointer moves again, so a list that scrolls under a resting pointer does not light
/// (or reveal the quick actions of) whatever slides beneath it.
pub fn pointer_in_use(ctx: &Context) -> bool {
    device(ctx) != Device::Gamepad && !keys_steer(ctx)
}

/// A key was pressed and the pointer has not moved, clicked or scrolled since: the keys steer.
fn keys_steer(ctx: &Context) -> bool {
    ctx.data(|d| d.get_temp(steer_id())).unwrap_or(false)
}

fn set_keys_steer(ctx: &Context, steer: bool) {
    ctx.data_mut(|d| d.insert_temp(steer_id(), steer));
}

fn steer_id() -> egui::Id {
    egui::Id::unique("input-keys-steer")
}

/// Whether the focus ring shows: like `:focus-visible`, only while keyboard or gamepad steer. A click
/// or tap anywhere hides it (the control answers with its press, nothing lingers), and the next
/// key, stick or d-pad press brings it back on the control that holds the focus. It shows at
/// start, where nothing has been clicked yet.
pub fn focus_visible(ctx: &Context) -> bool {
    ctx.data(|d| d.get_temp(focus_visible_id())).unwrap_or(true)
}

/// Sets [`focus_visible`] (the input pipeline does this; previews force it).
pub fn set_focus_visible(ctx: &Context, visible: bool) {
    ctx.data_mut(|d| d.insert_temp(focus_visible_id(), visible));
    ctx.request_repaint();
}

fn focus_visible_id() -> egui::Id {
    egui::Id::unique("input-focus-visible")
}

/// Keys or the pad were used: shows the ring. True when it was hidden, so the press that brings it
/// back only shows where the focus is and does not also move it.
fn reveal_focus(ctx: &Context) -> bool {
    let hidden = !focus_visible(ctx);
    if hidden {
        set_focus_visible(ctx, true);
    }
    hidden
}

/// `response.hovered()`, but only while the pointer is the device in use. The element it says
/// yes for is what the hover tick follows (see [`take_hovered`]).
pub fn hovered(ctx: &Context, response: &egui::Response) -> bool {
    let hovered = response.hovered() && pointer_in_use(ctx);
    if hovered {
        ctx.data_mut(|d| d.insert_temp(hovered_id(), response.id.value()));
    }
    hovered
}

fn hovered_id() -> egui::Id {
    egui::Id::unique("input-hovered-element")
}

/// The element [`hovered`] last reported in this frame (the one drawn last wins), cleared by
/// the call. Call once at the end of a frame.
pub fn take_hovered(ctx: &Context) -> Option<u64> {
    ctx.data_mut(|d| d.remove_temp::<u64>(hovered_id()))
}

/// Sets the device the hint bars follow (the input pipeline does this; previews force it).
pub fn set_device(ctx: &Context, device: Device) {
    ctx.data_mut(|d| d.insert_temp(device_id(), device));
}

/// Owns the input state. Create once, call [`poll`](Self::poll) once per frame.
#[derive(Default)]
pub struct Input {
    wheel: WheelAccumulator,
    repeater: Repeater,
    /// The repeat of a held arrow key (the OS's own repeat is ignored, so the keyboard paces a
    /// list like the pad does).
    key_repeater: Repeater,
    held_key: Option<Key>,
    pad: Option<Receiver<PadMessage>>,
    /// The last direction a command went: the way an edge bump leans.
    pub last_direction: Option<Command>,
    /// What fired last was a repeat of a held direction, not a fresh press: an edge is bumped once
    /// per press, not on every repeat.
    pub repeating: bool,
    /// The pad is the device in use: only then does it vibrate.
    pub pad_in_use: bool,
    /// True while a native dialog is open: keyboard and wheel are ignored.
    pub blocked: bool,
    /// True while a binding waits for a key: every key press is returned as [`InputEvent::CapturedKey`].
    pub capture_keys: bool,
    /// True while a call is ringing: the card's buttons are reached with Tab through egui, so
    /// Tab is not taken for the focus ring.
    pub tab_for_egui: bool,
}

impl Input {
    pub fn new(pad: Option<Receiver<PadMessage>>) -> Self {
        Self {
            pad,
            ..Self::default()
        }
    }

    /// Collects this frame's events. Text fields keep their keys; Enter and Esc leave them.
    pub fn poll(&mut self, ctx: &Context) -> Vec<InputEvent> {
        let (mut fresh, mut repeated) = (false, false);
        let out = self.collect(ctx, &mut fresh, &mut repeated);
        // A frame with any fresh press is a press; only a frame of repeats alone is a repeat.
        if fresh {
            self.repeating = false;
        } else if repeated {
            self.repeating = true;
        }
        out
    }

    fn collect(&mut self, ctx: &Context, fresh: &mut bool, repeated: &mut bool) -> Vec<InputEvent> {
        let mut out = Vec::new();
        let now = ctx.input(|i| i.time);

        // Gamepad first: it is never blocked by text fields or panels.
        if let Some(pad) = &self.pad {
            while let Ok(message) = pad.try_recv() {
                if !matches!(message, PadMessage::Direction(None)) {
                    set_device(ctx, Device::Gamepad);
                    self.pad_in_use = true;
                }
                match message {
                    PadMessage::Direction(direction) => {
                        if direction.is_some() {
                            crate::ui::latency::pad_event_seen();
                        }
                        let revealed = direction.is_some() && reveal_focus(ctx);
                        if let Some(command) = self.repeater.set_direction(direction, now) {
                            *fresh = true;
                            self.last_direction = Some(command);
                            if !revealed {
                                out.push(InputEvent::Command(command));
                            }
                        }
                    }
                    PadMessage::Button(command) => {
                        crate::ui::latency::pad_event_seen();
                        let revealed = reveal_focus(ctx);
                        if !(revealed && command.is_direction()) {
                            out.push(InputEvent::Command(command));
                        }
                    }
                    PadMessage::Create => {
                        reveal_focus(ctx);
                        out.push(InputEvent::Create);
                    }
                }
            }
        }
        if let Some(command) = self.repeater.tick(now) {
            *repeated = true;
            self.last_direction = Some(command);
            out.push(InputEvent::Command(command));
        }
        if let Some(due) = self.repeater.next_due_in(now) {
            ctx.request_repaint_after(Duration::from_secs_f64(due.max(0.001)));
        }

        // A held arrow key repeats on our clock. It ends when the key is up (or the keyboard is
        // taken away: a dialog, a binding waiting for a key), whatever the key events said.
        if let Some(key) = self.held_key
            && (self.blocked
                || self.capture_keys
                || crate::ui::a11y::foreign_focus(ctx)
                || ctx.input(|i| !i.key_down(key) || i.modifiers.command || i.modifiers.alt))
        {
            self.held_key = None;
            self.key_repeater.set_direction(None, now);
        }
        if let Some(command) = self.key_repeater.tick(now) {
            *repeated = true;
            self.last_direction = Some(command);
            out.push(InputEvent::Command(command));
        }
        if let Some(due) = self.key_repeater.next_due_in(now) {
            ctx.request_repaint_after(Duration::from_secs_f64(due.max(0.001)));
        }

        if self.blocked {
            // The model ignores the keys now; egui must not click the followed widget with them.
            crate::ui::a11y::consume_activation(ctx);
            return out;
        }
        let in_text_field = crate::ui::a11y::foreign_focus(ctx);
        let events = ctx.input(|i| i.events.clone());
        for event in events {
            match event {
                Event::PointerButton {
                    button, pressed, ..
                } => {
                    set_keys_steer(ctx, false);
                    if pressed {
                        set_focus_visible(ctx, false);
                    }
                    self.pad_in_use = false;
                    // The mouse's back button (button 4) is the Back command, like Escape.
                    if button == PointerButton::Extra1
                        && pressed
                        && !self.capture_keys
                        && !in_text_field
                    {
                        out.push(InputEvent::Command(Command::Back));
                    }
                }
                Event::PointerMoved(_) => {
                    set_device(ctx, Device::Keyboard);
                    set_keys_steer(ctx, false);
                    self.pad_in_use = false;
                }
                Event::Key {
                    key,
                    physical_key,
                    pressed: true,
                    repeat,
                    modifiers,
                } => {
                    set_device(ctx, Device::Keyboard);
                    // Only a key that steers the launcher hands the lighting over, not a
                    // modifier or a letter that means nothing here.
                    let mut revealed = false;
                    if !in_text_field
                        && (command_for_key(key, modifiers).is_some()
                            || (key == Key::Tab && is_tab_press(modifiers)))
                    {
                        set_keys_steer(ctx, true);
                        revealed = reveal_focus(ctx);
                    }
                    self.pad_in_use = false;
                    if self.capture_keys {
                        crate::ui::a11y::consume_activation(ctx);
                        out.push(InputEvent::CapturedKey {
                            key,
                            physical_key,
                            modifiers,
                        });
                        continue;
                    }
                    if in_text_field {
                        if matches!(key, Key::Enter | Key::Escape)
                            && let Some(id) = ctx.memory(|m| m.focused())
                        {
                            ctx.memory_mut(|m| m.surrender_focus(id));
                        }
                        continue;
                    }
                    if key == Key::Tab && is_tab_press(modifiers) && !self.tab_for_egui {
                        out.push(InputEvent::Tab {
                            backwards: modifiers.shift,
                        });
                        crate::ui::a11y::consume_tab(ctx);
                    } else if key == Key::F11 {
                        out.push(InputEvent::ToggleFullscreen);
                    } else if key == Key::Comma && modifiers.command && !modifiers.alt {
                        out.push(InputEvent::OpenSettings);
                    } else if let Some(command) = command_for_key(key, modifiers) {
                        // The OS repeat is ignored: a held arrow repeats on our clock, held action
                        // keys do not re-fire.
                        if !repeat {
                            if command.is_direction() {
                                self.held_key = Some(key);
                                self.key_repeater.set_direction(Some(command), now);
                                *fresh = true;
                                self.last_direction = Some(command);
                            }
                            // The press that brings a hidden ring back only shows it.
                            if !(revealed && command.is_direction()) {
                                out.push(InputEvent::Command(command));
                            }
                        }
                        crate::ui::a11y::consume_activation(ctx);
                    }
                }
                Event::MouseWheel { unit, delta, .. } => {
                    // The wheel is the pointer's: what scrolls under it may light up again.
                    set_keys_steer(ctx, false);
                    self.pad_in_use = false;
                    let dominant = if delta.x.abs() > delta.y.abs() {
                        delta.x
                    } else {
                        delta.y
                    };
                    let dp = match unit {
                        MouseWheelUnit::Point => dominant,
                        // One notch of a classic wheel is one line: exactly one step.
                        MouseWheelUnit::Line => dominant * WHEEL_STEP,
                        MouseWheelUnit::Page => dominant * WHEEL_STEP * 4.0,
                    };
                    // Wheel down / scroll right move the content left/up (negative): that is "next".
                    if let Some(command) = self.wheel.feed(-dp, now) {
                        out.push(InputEvent::Scroll(command));
                    }
                }
                _ => {}
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pointer_lights_nothing_while_a_pad_is_in_use() {
        let ctx = Context::default();
        assert!(pointer_in_use(&ctx));
        set_device(&ctx, Device::Gamepad);
        assert!(!pointer_in_use(&ctx));
        set_device(&ctx, Device::Keyboard);
        assert!(pointer_in_use(&ctx));
    }

    #[test]
    fn a_key_hands_the_lighting_to_the_keys_until_the_pointer_moves() {
        let ctx = Context::default();
        assert!(pointer_in_use(&ctx));
        set_keys_steer(&ctx, true);
        assert!(
            !pointer_in_use(&ctx),
            "a resting pointer lights nothing after a key"
        );
        set_keys_steer(&ctx, false);
        assert!(
            pointer_in_use(&ctx),
            "moving, clicking or the wheel gives it back"
        );
    }

    #[test]
    fn the_ring_hides_on_a_click_and_a_key_brings_it_back_without_moving() {
        let ctx = Context::default();
        assert!(focus_visible(&ctx), "the ring shows at start");
        set_focus_visible(&ctx, false);
        assert!(!focus_visible(&ctx));
        assert!(reveal_focus(&ctx), "the first press only reveals");
        assert!(focus_visible(&ctx));
        assert!(!reveal_focus(&ctx), "the next press moves");
    }

    #[test]
    fn keys_map_to_commands() {
        let none = Modifiers::NONE;
        assert_eq!(command_for_key(Key::ArrowLeft, none), Some(Command::Left));
        assert_eq!(command_for_key(Key::ArrowRight, none), Some(Command::Right));
        assert_eq!(command_for_key(Key::ArrowUp, none), Some(Command::Up));
        assert_eq!(command_for_key(Key::ArrowDown, none), Some(Command::Down));
        for key in [Key::Enter, Key::Space] {
            assert_eq!(command_for_key(key, none), Some(Command::Select));
        }
        for key in [Key::Escape, Key::Backspace] {
            assert_eq!(command_for_key(key, none), Some(Command::Back));
        }
        assert_eq!(command_for_key(Key::O, none), Some(Command::Options));
        assert_eq!(
            command_for_key(Key::O, Modifiers::SHIFT),
            Some(Command::Options)
        );
        assert_eq!(command_for_key(Key::A, none), None);
    }

    #[test]
    fn control_alt_and_command_disable_commands() {
        assert_eq!(command_for_key(Key::ArrowLeft, Modifiers::CTRL), None);
        assert_eq!(command_for_key(Key::O, Modifiers::ALT), None);
        assert_eq!(command_for_key(Key::Enter, Modifiers::COMMAND), None);
    }

    #[test]
    fn a_wheel_notch_is_one_step() {
        let mut wheel = WheelAccumulator::default();
        assert_eq!(wheel.feed(40.0, 1.0), Some(Command::Right));
        assert_eq!(wheel.feed(-40.0, 2.0), Some(Command::Left));
    }

    #[test]
    fn trackpad_deltas_accumulate_and_reset_on_pause() {
        let mut wheel = WheelAccumulator::default();
        assert_eq!(wheel.feed(15.0, 1.00), None);
        assert_eq!(wheel.feed(15.0, 1.05), None);
        assert_eq!(wheel.feed(15.0, 1.10), Some(Command::Right)); // 45 >= 40
        assert_eq!(
            wheel.feed(30.0, 1.15),
            None,
            "the accumulator restarted after a step"
        );
        // A pause clears leftovers, so 30 + 30 after a pause is not a step.
        assert_eq!(wheel.feed(30.0, 3.0), None);
        assert_eq!(wheel.feed(-30.0, 3.05), None);
    }

    #[test]
    fn a_direction_fires_at_once_then_repeats_after_the_delay() {
        let mut r = Repeater::default();
        assert_eq!(
            r.set_direction(Some(Command::Right), 10.0),
            Some(Command::Right)
        );
        assert!(!r.is_repeating());
        assert_eq!(r.tick(10.2), None);
        assert_eq!(r.tick(10.35), None);
        assert_eq!(r.tick(10.361), Some(Command::Right));
        assert!(r.is_repeating());
        assert_eq!(r.tick(10.40), None);
        assert_eq!(r.tick(10.462), Some(Command::Right)); // 0.10 s later
        assert_eq!(r.tick(10.563), Some(Command::Right));
    }

    #[test]
    fn a_long_hold_speeds_up_after_six_repeats() {
        let mut r = Repeater::default();
        r.set_direction(Some(Command::Down), 0.0);
        let mut fired = Vec::new();
        let mut t = 0.0;
        while t < 1.6 {
            t += 0.001;
            if r.tick(t).is_some() {
                fired.push(t);
            }
        }
        let gaps: Vec<f64> = fired.windows(2).map(|w| w[1] - w[0]).collect();
        // The first repeat comes after the delay; the next five at the steady pace.
        assert!((fired[0] - REPEAT_DELAY).abs() < 0.003);
        for gap in &gaps[..5] {
            assert!((gap - REPEAT_INTERVAL).abs() < 0.003, "{gap}");
        }
        for gap in &gaps[6..] {
            assert!((gap - REPEAT_FAST_INTERVAL).abs() < 0.003, "{gap}");
        }
        // 0.36 s, six steps of 0.10 s, then 0.06 s: a hold of 1.6 s moves a list 17 rows.
        assert!((16..=18).contains(&fired.len()), "{}", fired.len());
    }

    #[test]
    fn identical_directions_are_not_refired_and_changes_restart_the_timers() {
        let mut r = Repeater::default();
        assert_eq!(
            r.set_direction(Some(Command::Down), 0.0),
            Some(Command::Down)
        );
        assert_eq!(r.set_direction(Some(Command::Down), 0.1), None);
        assert_eq!(r.set_direction(Some(Command::Up), 0.2), Some(Command::Up));
        assert_eq!(r.tick(0.5), None, "0.36 s count from the change at 0.2");
        assert_eq!(r.tick(0.561), Some(Command::Up));
        assert_eq!(r.set_direction(None, 0.6), None);
        assert_eq!(r.tick(5.0), None, "released: nothing repeats");
        assert!(!r.is_repeating(), "a new press starts fresh");
    }

    #[test]
    fn a_slow_frame_never_bursts() {
        let mut r = Repeater::default();
        r.set_direction(Some(Command::Left), 0.0);
        assert_eq!(r.tick(2.0), Some(Command::Left));
        assert_eq!(r.tick(2.0), None);
        assert_eq!(r.tick(2.01), None);
    }

    /// Runs one frame of `input` at `time` with the given key events.
    fn frame(ctx: &Context, input: &mut Input, time: f64, events: Vec<Event>) -> Vec<InputEvent> {
        let raw = egui::RawInput {
            time: Some(time),
            events,
            ..Default::default()
        };
        let mut out = Vec::new();
        let mut output = ctx.run_ui(raw, |ui| out = input.poll(ui.ctx()));
        output.textures_delta.clear();
        out
    }

    fn arrow(pressed: bool, repeat: bool) -> Event {
        Event::Key {
            key: Key::ArrowRight,
            physical_key: Some(Key::ArrowRight),
            pressed,
            repeat,
            modifiers: Modifiers::NONE,
        }
    }

    #[test]
    fn a_held_arrow_key_repeats_on_our_clock_not_the_systems() {
        let ctx = Context::default();
        let mut input = Input::new(None);
        let right = || InputEvent::Command(Command::Right);
        assert_eq!(
            frame(&ctx, &mut input, 1.0, vec![arrow(true, false)]),
            [right()]
        );
        assert!(!input.repeating);
        // The OS repeat events are dropped.
        assert!(frame(&ctx, &mut input, 1.1, vec![arrow(true, true)]).is_empty());
        assert!(frame(&ctx, &mut input, 1.2, vec![arrow(true, true)]).is_empty());
        assert_eq!(frame(&ctx, &mut input, 1.37, vec![]), [right()]);
        assert!(input.repeating);
        assert_eq!(input.last_direction, Some(Command::Right));
        // Released: nothing more, and the next press is fresh.
        assert!(frame(&ctx, &mut input, 1.4, vec![arrow(false, false)]).is_empty());
        assert!(frame(&ctx, &mut input, 3.0, vec![]).is_empty());
        assert_eq!(
            frame(&ctx, &mut input, 4.0, vec![arrow(true, false)]),
            [right()]
        );
        assert!(!input.repeating);
    }

    #[test]
    fn a_hold_ends_when_the_keyboard_is_taken_away() {
        let ctx = Context::default();
        let mut input = Input::new(None);
        frame(&ctx, &mut input, 1.0, vec![arrow(true, false)]);
        input.blocked = true;
        assert!(frame(&ctx, &mut input, 1.5, vec![]).is_empty());
        input.blocked = false;
        assert!(frame(&ctx, &mut input, 2.5, vec![]).is_empty());
    }
}
