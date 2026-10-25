//! AnyPlay Launcher: native front end for AnyPS5 on Windows, Linux and macOS.

// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod fonts;
mod instance;
mod screens;
mod startup;
mod ui;

use instance::Join;

fn main() -> eframe::Result {
    startup::begin();
    tracing_subscriber::fmt().init();
    startup::mark("main");
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    instance::wait_after_update(&mut args);
    // A first argument that is no flag is a link or a folder (the URL scheme and the folder menu
    // pass one): it is never a preview flag, whatever follows it.
    let opened = args.first().is_some_and(|a| !a.starts_with('-'));
    if !opened {
        // Hidden modes for screenshots and trying the flows: `--gallery=<scene>` of the home screen
        // and the onboarding, `--demo`, and the foundation's own `--gallery[=components|panels|...]`.
        if let Some(options) = app::preview::parse_args(args.iter().cloned()) {
            // Previews take no part in the single-instance handoff. The demo may, under the
            // instance name a test run chose, to show a link reaching a running launcher.
            let inbox = if options.scene.is_none() && instance::is_named() {
                match instance::join(&instance::name(), &[]) {
                    Join::Delivered => return Ok(()),
                    Join::Primary(inbox) => Some(inbox),
                    Join::Alone => None,
                }
            } else {
                None
            };
            return app::preview::run(options, inbox);
        }
        if let Some(options) = ui::gallery::parse_args(args.iter().cloned()) {
            return ui::gallery::run(options);
        }
    }
    let arguments: Vec<String> = args.into_iter().filter(|a| !a.starts_with('-')).collect();
    let inbox = match instance::join(&instance::name(), &arguments) {
        Join::Delivered => return Ok(()),
        Join::Primary(inbox) => Some(inbox),
        Join::Alone => None,
    };
    startup::mark("instance joined");
    app::run(inbox, arguments)
}
