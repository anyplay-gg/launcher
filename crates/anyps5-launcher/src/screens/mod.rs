//! The panels and overlays of the launcher (docs/DESIGN.md section 5), one module each. Every
//! module owns a state struct and a `show(...)` that takes the [`Shell`](crate::ui::Shell), the
//! egui context and the frame's input events and returns the actions the host has to perform; the
//! host (the home screen and its state machine) decides which panel is open and keeps the data.
//!
//! | Module | Screen |
//! |---|---|
//! | [`settings`] | app settings: appearance, library, language, behaviour, toolkit, advanced, about |
//! | [`game_settings`] | the "..." side panel and the uninstall confirmation |
//! | [`controls`] | key bindings of a game with key capture |
//! | [`advanced`] | sticks, mouse, keyboard, shader cache, sharing |
//! | [`trophies`] | the trophy list |
//! | [`banner`] | the trophy banner card |
//! | [`start`] | the launch splash with its timer |
//! | [`unlock`] | the unlocking progress (steps, overall fraction, hero block) |
//! | [`log`] | the log view, the diagnosis line and the report |
//! | [`library`] | the library grid |
//!
//! [`rows`] and [`side`] are the shared row list and side-panel frame, [`convert`] maps the core's
//! preference enums to the UI's tokens, [`strings`] holds the Windows-only texts and [`dialogs`]
//! the native file dialogs. [`gallery`] renders every panel and state for screenshots.

// The host (the home screen) consumes these modules in a later change; until then most of the
// API is unused outside the gallery and the tests.
#![allow(dead_code)]

pub mod about_strings;
pub mod advanced;
pub mod banner;
pub mod calls;
pub mod controls;
pub mod convert;
pub mod dialogs;
pub mod discord_strings;
pub mod failure_strings;
pub mod format;
pub mod gallery;
pub mod gallery_feed;
pub mod game_settings;
pub mod glibc_strings;
pub mod home;
pub mod ingame;
pub mod library;
pub mod log;
pub mod onboarding;
pub mod onboarding_strings;
pub mod online;
pub mod online_call_strings;
pub mod online_feed_strings;
pub mod online_restriction_strings;
pub mod online_screen_strings;
pub mod online_social_strings;
pub mod online_strings;
pub mod online_voice_strings;
pub mod phone;
pub mod phone_strings;
pub mod profile;
pub mod profile_strings;
pub mod recommended;
pub mod rows;
pub mod run_strings;
pub mod saves;
pub mod scroll;
pub mod settings;
pub mod share_strings;
mod shared;
pub mod side;
pub mod start;
pub mod strings;
pub mod toolkit_pin;
pub mod trophies;
pub mod unlock;
pub mod update_strings;
pub mod whatsnew;

pub use shared::*;

use std::sync::Arc;

use egui::epaint::text::LayoutJob;
use egui::{Color32, Galley, Painter};

use crate::ui::icons::Icon;
use crate::ui::theme::TextStyle;

/// Text clamped to `max_rows` rows of `width`, with an ellipsis where it is cut.
pub fn clamped(
    painter: &Painter,
    style: TextStyle,
    text: &str,
    color: Color32,
    width: f32,
    max_rows: usize,
) -> Arc<Galley> {
    let mut job = LayoutJob::simple(text.to_owned(), style.font(), color, width);
    job.wrap.max_rows = max_rows;
    job.sections[0].format.line_height = Some(style.line_height());
    painter.layout_job(job)
}

/// Draws the hint bar (Move, Select, Back) right-aligned inside `rect`, vertically centred.
pub fn right_aligned_hints(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    language: crate::ui::i18n::Language,
) {
    use egui::{Align, Layout, UiBuilder, pos2};
    let mut probe = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center))
            .invisible(),
    );
    crate::ui::components::hint_bar(&mut probe, language, false);
    let width = probe.min_rect().width();
    let mut bar = ui.new_child(
        UiBuilder::new()
            .max_rect(egui::Rect::from_min_max(
                pos2(rect.max.x - width, rect.min.y),
                rect.max,
            ))
            .layout(Layout::left_to_right(Align::Center)),
    );
    crate::ui::components::hint_bar(&mut bar, language, false);
}

/// A message for the host's toast (section 5.6): a glyph and a sentence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToastRequest {
    pub icon: Icon,
    pub text: String,
}

impl ToastRequest {
    pub fn new(icon: Icon, text: impl Into<String>) -> Self {
        Self {
            icon,
            text: text.into(),
        }
    }

    pub fn warning(text: impl Into<String>) -> Self {
        Self::new(Icon::WARNING, text)
    }
}
