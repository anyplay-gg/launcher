//! The Home button (the PS button of a controller, the Guide button of an Xbox pad, the PS button
//! of the phone controller) while a game runs (docs/DESIGN.md 6.6): it brings the launcher in
//! front of the game, and back to the game when the launcher is in front. While a game is in front
//! the launcher listens to the pad for this button alone.
//!
//! The gamepad thread, the XInput thread and the phone server act on the button themselves
//! instead of handing it to the UI: a minimised or hidden launcher draws no frames, so a message
//! queued for the UI would wait for a frame that the press is meant to cause. The UI learns of
//! the switch through two flags it takes once per frame ([`HomeButton::take_summoned`],
//! [`HomeButton::take_returned`]) and opens or closes the game's quick panel.
//!
//! The same press can arrive from two sources (gilrs and XInput both see an Xbox pad's Guide
//! button where Windows lets them): presses within [`DEBOUNCE`] count once.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// A second press this soon after the first is the same press seen by a second source.
const DEBOUNCE: Duration = Duration::from_millis(350);

/// What a pad button does, given where the game and the launcher are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// An ordinary button: the launcher acts on it as usual.
    Pass,
    /// Dropped: the game is in front (it has the pad), or the Home button has nothing to switch.
    Ignore,
    /// The Home button with the game in front.
    ShowLauncher,
    /// The Home button with the launcher in front.
    ShowGame,
}

/// The state machine. `home`: the button is the Home button. `enabled`: the preference.
pub fn route(game_running: bool, launcher_in_front: bool, home: bool, enabled: bool) -> Route {
    match (game_running, home) {
        (false, false) => Route::Pass,
        (false, true) => Route::Ignore,
        (true, false) if launcher_in_front => Route::Pass,
        (true, false) => Route::Ignore,
        (true, true) if !enabled => Route::Ignore,
        (true, true) if launcher_in_front => Route::ShowGame,
        (true, true) => Route::ShowLauncher,
    }
}

/// What the input threads need to know about the game and the launcher window. The UI thread
/// fills it once per frame; everything is an atomic, nothing blocks.
#[derive(Default)]
pub struct HomeButton {
    running: AtomicBool,
    #[cfg_attr(not(windows), allow(dead_code))]
    window: AtomicIsize,
    /// The preference is off.
    disabled: AtomicBool,
    /// The launcher window has focus (read where the system cannot be asked: not on Windows).
    #[cfg_attr(windows, allow(dead_code))]
    focused: AtomicBool,
    /// A test run fixed the game; the UI no longer sets `running` and `window`.
    pinned: AtomicBool,
    /// The launcher was brought in front by the button and the UI has not shown the panel yet.
    summoned: AtomicBool,
    /// The game was brought back by the button and the UI has not closed the panel yet.
    returned: AtomicBool,
    last_press: Mutex<Option<Instant>>,
    ctx: OnceLock<egui::Context>,
}

impl HomeButton {
    /// `ctx` raises the window where only egui can (not on Windows) and wakes the UI.
    pub fn new(ctx: &egui::Context) -> Self {
        let button = Self::default();
        button.focused.store(true, Ordering::Relaxed);
        let _ = button.ctx.set(ctx.clone());
        button
    }

    /// A game runs and shows (or not yet) a window; called every frame.
    pub fn set_game(&self, running: bool, window: Option<isize>) {
        if !self.pinned.load(Ordering::Relaxed) {
            self.running.store(running, Ordering::Relaxed);
            self.window.store(window.unwrap_or(0), Ordering::Relaxed);
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.disabled.store(!enabled, Ordering::Relaxed);
    }

    pub fn set_focused(&self, focused: bool) {
        self.focused.store(focused, Ordering::Relaxed);
    }

    /// Whether a game runs, as the UI last said.
    pub fn game_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Whether the preference is on.
    pub fn enabled(&self) -> bool {
        !self.disabled.load(Ordering::Relaxed)
    }

    /// Test hook: a stand-in window is the running game and stays so.
    pub fn pin_game(&self, window: isize) {
        self.pinned.store(true, Ordering::Relaxed);
        self.running.store(true, Ordering::Relaxed);
        self.window.store(window, Ordering::Relaxed);
    }

    /// The button brought the launcher in front since the last call.
    pub fn take_summoned(&self) -> bool {
        self.summoned.swap(false, Ordering::SeqCst)
    }

    /// The button brought the game back since the last call.
    pub fn take_returned(&self) -> bool {
        self.returned.swap(false, Ordering::SeqCst)
    }

    /// Whether other buttons and directions reach the launcher right now.
    pub fn accepts_pad(&self) -> bool {
        self.decide(self.launcher_in_front(), false) == Route::Pass
    }

    /// The Home button went down: switches and says what it did. `Route::Ignore` means nothing
    /// happened (no game, switched off, or the same press again).
    pub fn press(&self) -> Route {
        {
            let mut last = self.last_press.lock().expect("press lock");
            let now = Instant::now();
            if last.is_some_and(|at| now.duration_since(at) < DEBOUNCE) {
                return Route::Ignore;
            }
            // A press that did nothing does not start a debounce window.
            if self.running.load(Ordering::Relaxed) && self.enabled() {
                *last = Some(now);
            }
        }
        let front = self.launcher_in_front();
        let route = self.decide(front, true);
        match route {
            Route::ShowLauncher => {
                self.summoned.store(true, Ordering::SeqCst);
                self.raise_launcher();
            }
            Route::ShowGame => {
                self.returned.store(true, Ordering::SeqCst);
                self.raise_game();
            }
            Route::Pass | Route::Ignore => {}
        }
        if route != Route::Ignore {
            tracing::info!("Home button: launcher in front = {front}, {route:?}");
            if let Some(ctx) = self.ctx.get() {
                ctx.request_repaint();
            }
        }
        route
    }

    fn decide(&self, launcher_in_front: bool, home: bool) -> Route {
        route(
            self.running.load(Ordering::Relaxed),
            launcher_in_front,
            home,
            self.enabled(),
        )
    }

    #[cfg(windows)]
    fn launcher_in_front(&self) -> bool {
        anyps5_core::win::foreground().1 == std::process::id()
    }

    #[cfg(not(windows))]
    fn launcher_in_front(&self) -> bool {
        self.focused.load(Ordering::Relaxed)
    }

    #[cfg(windows)]
    fn raise_launcher(&self) {
        crate::instance::bring_to_front();
    }

    /// X11 may honour the request (window managers can refuse to steal focus); a Wayland
    /// compositor ignores it without an activation token the launcher does not have.
    #[cfg(not(windows))]
    fn raise_launcher(&self) {
        if let Some(ctx) = self.ctx.get() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
    }

    #[cfg(windows)]
    fn raise_game(&self) {
        let window = self.window.load(Ordering::Relaxed);
        if window != 0 && !anyps5_core::win::activate_window(window) {
            tracing::warn!("Home button: the game window did not take the foreground");
        }
    }

    /// Other platforms do not find the game's window, so the launcher steps aside instead and
    /// leaves the game what is behind it.
    #[cfg(not(windows))]
    fn raise_game(&self) {
        if let Some(ctx) = self.ctx.get() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// XInput: the Guide button of an Xbox pad
// ---------------------------------------------------------------------------------------------

/// Reads the Guide button of XInput pads (and of everything that shows itself as one: Steam
/// Input, DS4Windows). Windows.Gaming.Input, which gilrs uses, hands a pad to the focused window
/// only and never shows the Guide button; `XInputGetStateEx` (ordinal 100) shows both, and from
/// the background.
#[cfg(windows)]
pub fn spawn_xinput(home: Arc<HomeButton>) {
    let spawned = std::thread::Builder::new()
        .name("xinput-guide".into())
        .spawn(move || xinput::poll(&home));
    if let Err(error) = spawned {
        tracing::warn!("XInput thread not started: {error}");
    }
}

#[cfg(not(windows))]
pub fn spawn_xinput(_home: Arc<HomeButton>) {}

#[cfg(windows)]
mod xinput {
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    use super::HomeButton;

    /// The Guide button in `XINPUT_GAMEPAD::wButtons` of `XInputGetStateEx`.
    pub const GUIDE: u16 = 0x0400;

    #[repr(C)]
    #[derive(Default)]
    struct State {
        packet: u32,
        buttons: u16,
        triggers: [u8; 2],
        sticks: [i16; 4],
    }

    type GetStateEx = unsafe extern "system" fn(u32, *mut State) -> u32;

    fn load() -> Option<GetStateEx> {
        for name in ["xinput1_4.dll", "xinput1_3.dll", "xinput9_1_0.dll"] {
            let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
            // SAFETY: a NUL-terminated name; the library stays loaded for the process.
            let library = unsafe { LoadLibraryW(wide.as_ptr()) };
            if library.is_null() {
                continue;
            }
            // SAFETY: ordinal 100 is `XInputGetStateEx`, undocumented but stable since XInput 1.3;
            // the ordinal is passed as a pointer-sized integer as the API wants.
            let function = unsafe { GetProcAddress(library, 100usize as *const u8) };
            if let Some(function) = function {
                // SAFETY: the signature of `XInputGetStateEx`.
                return Some(unsafe {
                    std::mem::transmute::<unsafe extern "system" fn() -> isize, GetStateEx>(
                        function,
                    )
                });
            }
        }
        None
    }

    /// The edge of the Guide bit: pressed now, not before.
    pub fn pressed(before: u16, now: u16) -> bool {
        before & GUIDE == 0 && now & GUIDE != 0
    }

    pub fn poll(home: &HomeButton) {
        let Some(get_state) = load() else {
            tracing::info!("XInput is not available: the Guide button is read through gilrs only");
            return;
        };
        let mut before = [0u16; 4];
        loop {
            // Nothing to switch without a game: look a few times a second at most.
            if !home.running.load(Ordering::Relaxed) {
                before = [0; 4];
                std::thread::sleep(Duration::from_millis(250));
                continue;
            }
            let mut any = false;
            for (user, before) in before.iter_mut().enumerate() {
                let mut state = State::default();
                // SAFETY: a valid out pointer for one `XINPUT_STATE`.
                let status = unsafe { get_state(user as u32, &mut state) };
                if status != 0 {
                    *before = 0;
                    continue;
                }
                any = true;
                if pressed(*before, state.buttons) {
                    home.press();
                }
                *before = state.buttons;
            }
            std::thread::sleep(Duration::from_millis(if any { 16 } else { 500 }));
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn only_the_edge_of_the_guide_bit_counts() {
            assert!(pressed(0, GUIDE));
            assert!(pressed(0x1000, 0x1000 | GUIDE));
            assert!(!pressed(GUIDE, GUIDE), "held");
            assert!(!pressed(GUIDE, 0), "released");
            assert!(!pressed(0, 0x1000), "another button");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Test hook of the demo
// ---------------------------------------------------------------------------------------------

/// `ANYPS5_PS_TEST_WINDOW=<hwnd>` makes that window the running game, and each line on stdin
/// (`ps`) is a press of the Home button, through the same `press` the input threads call. The
/// foreground window is logged before and after, so a run shows the switch.
pub fn test_game_from_env(home: &Arc<HomeButton>) {
    let Some(window) = std::env::var("ANYPS5_PS_TEST_WINDOW")
        .ok()
        .and_then(|v| v.trim().parse::<isize>().ok())
    else {
        return;
    };
    home.pin_game(window);
    let home = home.clone();
    let spawned = std::thread::Builder::new()
        .name("ps-test".into())
        .spawn(move || {
            use std::io::BufRead;
            for line in std::io::stdin().lock().lines().map_while(Result::ok) {
                if line.trim() != "ps" {
                    continue;
                }
                let before = foreground_report();
                let route = home.press();
                std::thread::sleep(Duration::from_millis(400));
                tracing::info!(
                    "ps-test: {route:?}; foreground before {before}, after {}; game window {window}",
                    foreground_report()
                );
            }
        });
    if let Err(error) = spawned {
        tracing::warn!("ps-test thread not started: {error}");
    }
}

#[cfg(windows)]
fn foreground_report() -> String {
    let (window, pid) = anyps5_core::win::foreground();
    format!("{window} (pid {pid}, own {})", std::process::id())
}

#[cfg(not(windows))]
fn foreground_report() -> String {
    "unknown".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_with_the_game_in_front_brings_the_launcher() {
        assert_eq!(route(true, false, true, true), Route::ShowLauncher);
    }

    #[test]
    fn home_with_the_launcher_in_front_returns_to_the_game() {
        assert_eq!(route(true, true, true, true), Route::ShowGame);
    }

    #[test]
    fn home_without_a_game_does_nothing() {
        assert_eq!(route(false, true, true, true), Route::Ignore);
        assert_eq!(route(false, false, true, true), Route::Ignore);
    }

    #[test]
    fn other_buttons_are_ignored_while_the_game_is_in_front() {
        assert_eq!(route(true, false, false, true), Route::Ignore);
    }

    #[test]
    fn other_buttons_work_in_the_launcher_with_or_without_a_game() {
        assert_eq!(route(true, true, false, true), Route::Pass);
        assert_eq!(route(false, true, false, true), Route::Pass);
        assert_eq!(route(false, false, false, true), Route::Pass);
    }

    #[test]
    fn the_preference_turns_the_switch_off_but_not_the_silence() {
        assert_eq!(route(true, false, true, false), Route::Ignore);
        assert_eq!(route(true, true, true, false), Route::Ignore);
        assert_eq!(route(true, false, false, false), Route::Ignore);
    }

    #[test]
    fn the_shared_state_follows_the_game() {
        let button = HomeButton::default();
        assert_eq!(button.decide(false, true), Route::Ignore);
        button.set_game(true, Some(7));
        assert_eq!(button.decide(false, true), Route::ShowLauncher);
        button.set_enabled(false);
        assert_eq!(button.decide(false, true), Route::Ignore);
        button.set_enabled(true);
        button.set_game(false, None);
        assert_eq!(button.decide(false, true), Route::Ignore);
    }

    #[test]
    fn a_pinned_game_ignores_the_ui() {
        let button = HomeButton::default();
        button.pin_game(7);
        button.set_game(false, None);
        assert_eq!(button.decide(true, true), Route::ShowGame);
    }

    #[test]
    fn a_press_seen_by_two_sources_counts_once() {
        let button = HomeButton::default();
        button.set_game(true, Some(7));
        button.focused.store(false, Ordering::Relaxed);
        // `launcher_in_front` reads the system on Windows, so the first route varies there; what
        // matters is that the second press of the same moment does nothing.
        let first = button.press();
        assert_ne!(first, Route::Ignore);
        assert_eq!(button.press(), Route::Ignore);
    }

    #[test]
    fn a_press_without_a_game_is_not_remembered() {
        let button = HomeButton::default();
        assert_eq!(button.press(), Route::Ignore);
        button.set_game(true, Some(7));
        assert_ne!(button.press(), Route::Ignore);
    }

    #[test]
    fn the_switch_is_told_to_the_ui_once() {
        let button = HomeButton::default();
        button.set_game(true, Some(7));
        button.focused.store(false, Ordering::Relaxed);
        let route = button.press();
        match route {
            Route::ShowLauncher => {
                assert!(button.take_summoned());
                assert!(!button.take_summoned());
            }
            Route::ShowGame => {
                assert!(button.take_returned());
                assert!(!button.take_returned());
            }
            other => panic!("{other:?}"),
        }
    }
}
