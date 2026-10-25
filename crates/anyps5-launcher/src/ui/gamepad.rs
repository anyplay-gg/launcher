//! Gamepad polling on its own thread (docs/DESIGN.md 6.3). The thread only translates `gilrs`
//! events into [`PadMessage`]s; repeat timing lives in `ui::input` so it uses the UI clock.
//!
//! D-pad and left stick are directions (stick axis above 0.55, otherwise that axis counts as 0;
//! the dominant axis wins). A / Cross = select, B / Circle = back, Y / Triangle = options,
//! Menu / Start = options. The Home / Guide button (`Button::Mode`) never becomes a command: it
//! switches between the game and the launcher (`ui::home_button`), and while a game is in front
//! it and Create (which saves a clip) are the only buttons that count.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use gilrs::{Axis, Button, EventType, Gilrs};

use crate::ui::home_button::HomeButton;
use crate::ui::input::{Command, PadMessage};

/// Stick deflection that counts as a direction.
pub const STICK_THRESHOLD: f32 = 0.55;

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

/// Held directional input from the D-pad and the left stick.
#[derive(Debug, Default)]
struct Directions {
    dpad: (f32, f32),
    stick: (f32, f32),
}

impl Directions {
    /// The D-pad wins when it is pressed, else the thresholded stick.
    fn current(&self) -> Option<Command> {
        if self.dpad != (0.0, 0.0) {
            direction(self.dpad.0, self.dpad.1)
        } else {
            direction(stick_axis(self.stick.0), stick_axis(self.stick.1))
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
                // Create on a DualSense, Share on a DualShock, View or Back on an Xbox pad: it
                // saves a clip, which is only ever wanted while a game is in front.
                EventType::ButtonPressed(Button::Select, _) => send(PadMessage::Create),
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
        // 8 ms keeps a pad responsive; with none connected there is nothing to poll for, and a
        // plug-in is noticed within a tenth of a second.
        let idle = gilrs.gamepads().next().is_none();
        thread::sleep(Duration::from_millis(if idle { 100 } else { 8 }));
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
        let dirs = Directions {
            dpad: (0.0, 0.0),
            stick: (0.5, 0.0),
        };
        assert_eq!(dirs.current(), None);
        let dirs = Directions {
            dpad: (0.0, 0.0),
            stick: (0.6, 0.5),
        };
        assert_eq!(
            dirs.current(),
            Some(Command::Right),
            "the weak axis counts as 0"
        );
    }

    #[test]
    fn dpad_beats_stick() {
        let dirs = Directions {
            dpad: (0.0, -1.0),
            stick: (1.0, 0.0),
        };
        assert_eq!(dirs.current(), Some(Command::Down));
    }
}
