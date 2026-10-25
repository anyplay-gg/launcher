//! Gamepad polling on its own thread (docs/DESIGN.md 6.3). The thread only translates `gilrs`
//! events into [`PadMessage`]s; repeat timing lives in `ui::input` so it uses the UI clock.
//!
//! D-pad and left stick are directions (stick axis above 0.55, otherwise that axis counts as 0;
//! the dominant axis wins). A / Cross = select, B / Circle = back, Y / Triangle = options,
//! Menu / Start = options. The Home / Guide button (`Button::Mode`) never becomes a command: it
//! switches between the game and the launcher (`ui::home_button`), and while a game is in front
//! it is the only button that counts.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use gilrs::{Axis, Button, EventType, Gilrs};

use crate::ui::home_button::HomeButton;
use crate::ui::input::{Command, PadMessage};

/// Stick deflection that makes a direction.
pub const STICK_THRESHOLD: f32 = 0.55;
/// Deflection below which a held stick direction lets go. Between the two nothing changes, so a
/// thumb resting near the threshold does not flutter between "right" and "nothing".
pub const STICK_RELEASE: f32 = 0.38;
/// A held direction gives way to the other axis only when that one is this much stronger, so a
/// thumb near the diagonal does not jump between "right" and "up".
pub const STICK_SWITCH: f32 = 1.25;

/// Direction for an (x, y) pair with y up: dominant axis wins, ties and zero give `None`.
pub fn direction(x: f32, y: f32) -> Option<Command> {
    if x.abs() > y.abs() {
        Some(if x > 0.0 {
            Command::Right
        } else {
            Command::Left
        })
    } else if y != 0.0 {
        Some(if y > 0.0 { Command::Up } else { Command::Down })
    } else {
        None
    }
}

fn stick_axis(v: f32) -> f32 {
    if v.abs() > STICK_THRESHOLD { v } else { 0.0 }
}

/// The left stick as a direction, with hysteresis: it engages at [`STICK_THRESHOLD`], lets go at
/// [`STICK_RELEASE`] and changes axis only for a clearly stronger other axis.
#[derive(Debug, Default, Clone, Copy)]
pub struct Stick {
    held: Option<Command>,
}

impl Stick {
    /// The direction for a stick at (x, y), y up.
    pub fn update(&mut self, x: f32, y: f32) -> Option<Command> {
        let fresh = || direction(stick_axis(x), stick_axis(y));
        self.held = match self.held {
            None => fresh(),
            Some(held) => {
                // How far the stick still leans the held way, and how far across it.
                let (along, across) = match held {
                    Command::Right => (x, y),
                    Command::Left => (-x, y),
                    Command::Up => (y, x),
                    _ => (-y, x),
                };
                if along >= STICK_RELEASE {
                    if across.abs() >= STICK_THRESHOLD && across.abs() > along * STICK_SWITCH {
                        fresh()
                    } else {
                        Some(held)
                    }
                } else {
                    fresh()
                }
            }
        };
        self.held
    }
}

/// Held directional input from the D-pad and the left stick.
#[derive(Debug, Default)]
struct Directions {
    dpad: (f32, f32),
    stick: (f32, f32),
    stick_state: Stick,
}

impl Directions {
    /// The D-pad wins when it is pressed, else the stick. Call after every axis event.
    fn current(&mut self) -> Option<Command> {
        let stick = self.stick_state.update(self.stick.0, self.stick.1);
        if self.dpad != (0.0, 0.0) {
            direction(self.dpad.0, self.dpad.1)
        } else {
            stick
        }
    }
}

/// Starts the poll loop and returns the receiving end. A missing gamepad backend is logged once
/// and is not fatal (the receiver then simply stays silent).
pub fn spawn(ctx: egui::Context, home: Arc<HomeButton>) -> Receiver<PadMessage> {
    let (tx, rx) = mpsc::channel();
    let spawned = thread::Builder::new()
        .name("gamepad".into())
        .spawn(move || poll(&ctx, &tx, &home));
    if let Err(err) = spawned {
        tracing::warn!("gamepad thread not started: {err}");
    }
    rx
}

fn poll(ctx: &egui::Context, tx: &Sender<PadMessage>, home: &HomeButton) {
    let mut gilrs = match Gilrs::new() {
        Ok(gilrs) => gilrs,
        Err(err) => {
            tracing::warn!("gamepad backend unavailable: {err}");
            return;
        }
    };
    let mut dirs = Directions::default();
    let mut last = None;
    loop {
        while let Some(event) = gilrs.next_event() {
            let send = |message| {
                crate::ui::latency::pad_event_read();
                if tx.send(message).is_ok() {
                    ctx.request_repaint();
                }
            };
            // With a game in front the launcher hears the Home button and nothing else; the held
            // directions are kept up to date but not passed on.
            let mine = home.accepts_pad();
            match event.event {
                EventType::ButtonPressed(Button::Mode, _) => {
                    home.press();
                }
                EventType::ButtonPressed(..) if !mine => {}
                EventType::ButtonPressed(button, _) => match button {
                    Button::South => send(PadMessage::Button(Command::Select)),
                    Button::East => send(PadMessage::Button(Command::Back)),
                    Button::North | Button::Start => send(PadMessage::Button(Command::Options)),
                    Button::DPadUp => dirs.dpad.1 = 1.0,
                    Button::DPadDown => dirs.dpad.1 = -1.0,
                    Button::DPadLeft => dirs.dpad.0 = -1.0,
                    Button::DPadRight => dirs.dpad.0 = 1.0,
                    _ => {}
                },
                EventType::ButtonReleased(button, _) => match button {
                    Button::DPadUp | Button::DPadDown => dirs.dpad.1 = 0.0,
                    Button::DPadLeft | Button::DPadRight => dirs.dpad.0 = 0.0,
                    _ => {}
                },
                EventType::AxisChanged(axis, value, _) => match axis {
                    Axis::LeftStickX => dirs.stick.0 = value,
                    Axis::LeftStickY => dirs.stick.1 = value,
                    Axis::DPadX => dirs.dpad.0 = value,
                    Axis::DPadY => dirs.dpad.1 = value,
                    _ => {}
                },
                EventType::Disconnected => {
                    dirs = Directions::default();
                }
                _ => {}
            }
            let now = if mine { dirs.current() } else { None };
            if now != last {
                last = now;
                send(PadMessage::Direction(now));
            }
        }
        // 4 ms (a pad reports at 125 to 250 Hz) puts the wait for a press at 2 ms on average and
        // costs no measurable CPU; with none connected there is nothing to poll for, and a plug-in
        // is noticed within a tenth of a second.
        let idle = gilrs.gamepads().next().is_none();
        thread::sleep(Duration::from_millis(if idle { 100 } else { 4 }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dominant_axis_wins() {
        assert_eq!(direction(1.0, 0.2), Some(Command::Right));
        assert_eq!(direction(-0.8, 0.3), Some(Command::Left));
        assert_eq!(direction(0.1, 0.9), Some(Command::Up));
        assert_eq!(direction(0.0, -1.0), Some(Command::Down));
        assert_eq!(direction(0.0, 0.0), None);
    }

    #[test]
    fn stick_below_the_threshold_is_ignored() {
        let mut dirs = Directions {
            stick: (0.5, 0.0),
            ..Directions::default()
        };
        assert_eq!(dirs.current(), None);
        let mut dirs = Directions {
            stick: (0.6, 0.5),
            ..Directions::default()
        };
        assert_eq!(
            dirs.current(),
            Some(Command::Right),
            "the weak axis counts as 0"
        );
    }

    #[test]
    fn dpad_beats_stick() {
        let mut dirs = Directions {
            dpad: (0.0, -1.0),
            stick: (1.0, 0.0),
            ..Directions::default()
        };
        assert_eq!(dirs.current(), Some(Command::Down));
    }

    #[test]
    fn a_stick_near_the_threshold_does_not_flutter() {
        let mut stick = Stick::default();
        assert_eq!(stick.update(0.50, 0.0), None);
        assert_eq!(stick.update(0.56, 0.0), Some(Command::Right));
        // Wobbling between the two thresholds keeps the direction ...
        for x in [0.50, 0.45, 0.52, 0.41, 0.54] {
            assert_eq!(stick.update(x, 0.0), Some(Command::Right), "{x}");
        }
        // ... and it lets go only below the release level.
        assert_eq!(stick.update(0.37, 0.0), None);
        // From rest the old threshold applies again.
        assert_eq!(stick.update(0.50, 0.0), None);
    }

    #[test]
    fn a_stick_near_the_diagonal_does_not_jump_between_axes() {
        let mut stick = Stick::default();
        assert_eq!(stick.update(0.9, 0.2), Some(Command::Right));
        // Up is a little stronger than right now, but not by a quarter.
        assert_eq!(stick.update(0.70, 0.75), Some(Command::Right));
        assert_eq!(stick.update(0.65, 0.80), Some(Command::Right));
        // Clearly up: it switches.
        assert_eq!(stick.update(0.40, 0.90), Some(Command::Up));
        // And back, again only for a clear lead.
        assert_eq!(stick.update(0.75, 0.70), Some(Command::Up));
        assert_eq!(stick.update(0.95, 0.30), Some(Command::Right));
    }

    #[test]
    fn a_flick_through_the_middle_changes_direction_at_once() {
        let mut stick = Stick::default();
        assert_eq!(stick.update(0.9, 0.0), Some(Command::Right));
        assert_eq!(stick.update(-0.9, 0.0), Some(Command::Left));
        assert_eq!(stick.update(0.0, 0.9), Some(Command::Up));
        assert_eq!(stick.update(0.0, -0.9), Some(Command::Down));
    }
}
