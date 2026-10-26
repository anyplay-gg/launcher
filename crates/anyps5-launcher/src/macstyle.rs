//! Mac style (docs/DESIGN.md 19): the launcher on macOS is the same launcher, dressed for the Mac.
//!
//! * a see-through window: the system's blur (`NSVisualEffectView`, on macOS 26 and newer
//!   Liquid Glass, `NSGlassEffectView`) behind a window that is transparent where the launcher
//!   leaves it so, in place of the animated background;
//! * the Mac's window chrome: traffic lights over a transparent title bar, content under it;
//! * an app menu with About, Settings (Cmd+,), Hide and Quit, and a Window menu.
//!
//! Everything that touches AppKit sits in `imp`, compiled on macOS only. Elsewhere [`MacStyle`]
//! does nothing and [`glass_active`] says no, so Windows and Linux behave exactly as before.
//!
//! The interface is three calls: [`MacStyle::viewport`] before the window exists,
//! [`MacStyle::frame`] once per frame, and [`glass_active`] for the rule that decides when the
//! window is see-through (the same rule everywhere, so the tests run on every platform).

use egui::ViewportBuilder;
use raw_window_handle::HasWindowHandle;

/// Whether this platform has the Mac style at all (the setting is only shown where it is true).
pub const AVAILABLE: bool = cfg!(target_os = "macos");

/// The name in the app menu and the menu bar.
#[cfg(target_os = "macos")]
const APP_NAME: &str = "AnyPlay Launcher";

/// What the launcher wants from the window this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Want {
    /// The player's setting "Glass Window".
    pub glass_setting: bool,
    /// The window fills its screen (native full screen): there is no desktop behind it.
    pub fullscreen: bool,
    /// A game runs: the launcher is behind it and must cost nothing.
    pub game_running: bool,
}

/// What the window did this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// The window is see-through: paint the glass backdrop in place of the animated one.
    pub glass: bool,
    /// The app menu's Settings item (Cmd+,) was chosen.
    pub open_settings: bool,
}

/// The rule: the window is see-through when the player wants it, there is a desktop behind it,
/// no game runs, and the system does not ask for less transparency.
pub fn glass_active(want: Want, reduce_transparency: bool) -> bool {
    AVAILABLE
        && want.glass_setting
        && !want.fullscreen
        && !want.game_running
        && !reduce_transparency
}

/// The window's Mac dress. Hold one per window and call [`frame`](Self::frame) every frame.
#[derive(Default)]
pub struct MacStyle {
    #[cfg(target_os = "macos")]
    imp: imp::Imp,
}

impl MacStyle {
    /// Adds what the Mac window needs to the viewport (identity elsewhere): transparent, with the
    /// title bar transparent and the content under it.
    pub fn viewport(builder: ViewportBuilder) -> ViewportBuilder {
        #[cfg(target_os = "macos")]
        {
            builder
                .with_transparent(true)
                .with_decorations(true)
                .with_fullsize_content_view(true)
                .with_title_shown(false)
                .with_titlebar_shown(false)
        }
        #[cfg(not(target_os = "macos"))]
        {
            builder
        }
    }

    /// Dresses the window (the first time), applies the glass state and reports. Elsewhere it
    /// reports nothing.
    #[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
    pub fn frame(
        &mut self,
        ctx: &egui::Context,
        window: &impl HasWindowHandle,
        want: Want,
    ) -> Outcome {
        #[cfg(target_os = "macos")]
        {
            self.imp.frame(ctx, window, want)
        }
        #[cfg(not(target_os = "macos"))]
        {
            // Never see-through here: the rule says so.
            Outcome {
                glass: glass_active(want, false),
                open_settings: false,
            }
        }
    }
}

/// The draggable strip of a Mac window, a stand-in for the title bar's own: the traffic lights
/// are the system's, the strip only makes sure the window can be moved from the top edge. Sized
/// in points of the screen, and never reaching into the home screen's top bar.
pub fn drag_strip(ctx: &egui::Context) {
    use egui::{Area, Id, Order, Rect, Sense, ViewportCommand, pos2};
    let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
    if !AVAILABLE || fullscreen {
        return;
    }
    let screen = ctx.viewport_rect();
    let unit = 1.0 / ctx.zoom_factor().max(1.0);
    let height = (TITLE_BAR * unit).min(crate::ui::theme::pad::HOME_TOP - 2.0);
    let strip = Rect::from_min_max(
        screen.left_top(),
        pos2(screen.right(), screen.top() + height),
    );
    let area = Area::new(Id::unique("mac-drag-strip"))
        .order(Order::Foreground)
        .fixed_pos(strip.min)
        .movable(false)
        .constrain(false)
        .show(ctx, |ui| ui.allocate_rect(strip, Sense::drag()));
    ctx.move_to_top(area.response.layer_id);
    let response = area.inner;
    if response.drag_started_by(egui::PointerButton::Primary)
        && ctx.input(|i| i.pointer.primary_down())
    {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
}

/// The height of a Mac title bar in points (the traffic lights sit in it).
pub const TITLE_BAR: f32 = 28.0;
/// The lowest and the rightmost point of the traffic lights in points, with room to spare
/// (macOS 26 draws them a little larger and lower than earlier versions).
#[cfg_attr(not(test), allow(dead_code))]
pub const LIGHTS_EXTENT: (f32, f32) = (84.0, 34.0);

#[cfg(target_os = "macos")]
mod imp {
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Sel};
    use objc2::{ClassType, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
    use objc2_app_kit::{
        NSAppearance, NSAppearanceCustomization, NSAppearanceNameDarkAqua, NSApplication, NSColor,
        NSEventModifierFlags, NSGlassEffectView, NSGlassEffectViewStyle, NSMenu, NSMenuItem,
        NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState,
        NSVisualEffectView, NSWindow, NSWindowOrderingMode, NSWorkspace,
    };
    use objc2_foundation::{NSObject, NSObjectProtocol, NSString};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    use super::{APP_NAME, Outcome, Want, glass_active};

    /// Set by the menu item, read once per frame.
    static OPEN_SETTINGS: AtomicBool = AtomicBool::new(false);
    /// Wakes the UI when a menu item is chosen (the window may be idle).
    static CONTEXT: OnceLock<egui::Context> = OnceLock::new();

    define_class!(
        // SAFETY: NSObject has no subclassing requirements; this class has no state and no Drop.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "AnyPlayMenuTarget"]
        struct MenuTarget;

        impl MenuTarget {
            #[unsafe(method(openSettings:))]
            fn open_settings(&self, _sender: Option<&AnyObject>) {
                OPEN_SETTINGS.store(true, Ordering::Release);
                if let Some(ctx) = CONTEXT.get() {
                    ctx.request_repaint();
                }
            }
        }

        unsafe impl NSObjectProtocol for MenuTarget {}
    );

    /// How often the system's Reduce Transparency setting is read.
    const POLL: Duration = Duration::from_secs(1);
    /// Frames to try for the window before giving up (it exists from the first frame on).
    const ATTEMPTS: u32 = 30;

    #[derive(Default)]
    pub struct Imp {
        dress: Option<Dress>,
        attempts: u32,
        menu: Option<Retained<MenuTarget>>,
        reduce_transparency: bool,
        polled: Option<Instant>,
        /// What the log last said: window dressed, system asks for less transparency, glass on.
        logged: Option<(bool, bool, bool)>,
    }

    /// The window and the blur view behind its content.
    struct Dress {
        window: Retained<NSWindow>,
        effect: Retained<NSView>,
        glass: Option<bool>,
    }

    impl Imp {
        pub fn frame(
            &mut self,
            ctx: &egui::Context,
            window: &impl HasWindowHandle,
            want: Want,
        ) -> Outcome {
            let Some(mtm) = MainThreadMarker::new() else {
                return Outcome::default();
            };
            CONTEXT.get_or_init(|| ctx.clone());
            if self.dress.is_none() && self.attempts < ATTEMPTS {
                self.attempts += 1;
                self.dress = Dress::new(mtm, window);
                if self.dress.is_some() {
                    self.menu = Some(install_menu(mtm));
                }
            }
            if self.polled.is_none_or(|at| at.elapsed() >= POLL) {
                self.polled = Some(Instant::now());
                self.reduce_transparency =
                    NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceTransparency();
            }
            let glass = self.dress.is_some() && glass_active(want, self.reduce_transparency);
            if let Some(dress) = self.dress.as_mut() {
                dress.apply(glass);
            }
            let state = (self.dress.is_some(), self.reduce_transparency, glass);
            if self.logged != Some(state) {
                self.logged = Some(state);
                tracing::info!(
                    "mac style: dressed {}, reduce transparency {}, glass {} (wanted {:?})",
                    state.0,
                    state.1,
                    state.2,
                    want
                );
            }
            // The setting is read again soon, so a change in System Settings shows without input.
            if want.glass_setting && !want.game_running && !want.fullscreen {
                ctx.request_repaint_after(POLL);
            }
            Outcome {
                glass,
                open_settings: OPEN_SETTINGS.swap(false, Ordering::AcqRel),
            }
        }
    }

    impl Dress {
        fn new(mtm: MainThreadMarker, window: &impl HasWindowHandle) -> Option<Self> {
            let RawWindowHandle::AppKit(handle) = window.window_handle().ok()?.as_raw() else {
                return None;
            };
            // SAFETY: the handle is the live NSView of this process's window; this is the main thread.
            let view: &NSView = unsafe { handle.ns_view.cast().as_ref() };
            let ns_window = view.window()?;
            // The launcher is dark only: dark traffic lights, dark material.
            let dark = NSAppearance::appearanceNamed(unsafe { NSAppearanceNameDarkAqua });
            ns_window.setAppearance(dark.as_deref());

            let bounds = view.bounds();
            // Liquid Glass where the system has it (macOS 26), the older blur otherwise.
            let use_glass = AnyClass::get(c"NSGlassEffectView").is_some()
                && std::env::var_os("ANYPLAY_GLASS_MATERIAL").is_none_or(|v| v != "blur");
            let effect: Retained<NSView> = if use_glass {
                // SAFETY: a plain init of an AppKit view on the main thread.
                let view: Retained<NSGlassEffectView> =
                    unsafe { msg_send![mtm.alloc::<NSGlassEffectView>(), initWithFrame: bounds] };
                view.setStyle(NSGlassEffectViewStyle::Regular);
                Retained::into_super(view)
            } else {
                // SAFETY: as above.
                let view: Retained<NSVisualEffectView> =
                    unsafe { msg_send![mtm.alloc::<NSVisualEffectView>(), initWithFrame: bounds] };
                view.setMaterial(NSVisualEffectMaterial::Sidebar);
                view.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
                view.setState(NSVisualEffectState::Active);
                Retained::into_super(view)
            };
            effect.setAutoresizingMask(
                objc2_app_kit::NSAutoresizingMaskOptions::ViewWidthSizable
                    | objc2_app_kit::NSAutoresizingMaskOptions::ViewHeightSizable,
            );
            // Behind the content view, in the frame that holds it: the rendering layer of the
            // content stays on top of the blur.
            // SAFETY: reading the superview of a live view.
            match unsafe { view.superview() } {
                Some(frame) => {
                    frame.addSubview_positioned_relativeTo(
                        &effect,
                        NSWindowOrderingMode::Below,
                        Some(view),
                    );
                }
                None => view.addSubview_positioned_relativeTo(
                    &effect,
                    NSWindowOrderingMode::Below,
                    None,
                ),
            }
            tracing::debug!(
                "mac style: {}",
                if use_glass { "liquid glass" } else { "blur" }
            );
            Some(Self {
                window: ns_window,
                effect,
                glass: None,
            })
        }

        /// See-through: no opaque window, the blur shown. Solid: a black opaque window, no blur.
        fn apply(&mut self, glass: bool) {
            if self.glass == Some(glass) {
                return;
            }
            self.glass = Some(glass);
            self.effect.setHidden(!glass);
            self.window.setOpaque(!glass);
            let background = if glass {
                NSColor::clearColor()
            } else {
                NSColor::blackColor()
            };
            self.window.setBackgroundColor(Some(&background));
            self.window.invalidateShadow();
        }
    }

    fn item(
        mtm: MainThreadMarker,
        title: &str,
        action: Option<Sel>,
        key: &str,
    ) -> Retained<NSMenuItem> {
        // SAFETY: the selector is one AppKit or MenuTarget answers.
        unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                mtm.alloc(),
                &NSString::from_str(title),
                action,
                &NSString::from_str(key),
            )
        }
    }

    fn menu(mtm: MainThreadMarker, title: &str) -> Retained<NSMenu> {
        NSMenu::initWithTitle(mtm.alloc(), &NSString::from_str(title))
    }

    /// The menu bar: the app menu (About, Settings, Hide, Quit) and a Window menu. It replaces the
    /// default one, whose items name the executable.
    fn install_menu(mtm: MainThreadMarker) -> Retained<MenuTarget> {
        // SAFETY: the class is defined above and needs no setup.
        let target: Retained<MenuTarget> = unsafe { msg_send![MenuTarget::class(), new] };
        let target_object: &AnyObject = &target;
        let app = NSApplication::sharedApplication(mtm);

        let app_menu = menu(mtm, APP_NAME);
        app_menu.addItem(&item(
            mtm,
            &format!("About {APP_NAME}"),
            Some(sel!(orderFrontStandardAboutPanel:)),
            "",
        ));
        app_menu.addItem(&NSMenuItem::separatorItem(mtm));
        let settings = item(mtm, "Settings\u{2026}", Some(sel!(openSettings:)), ",");
        // SAFETY: the target outlives the menu: `Imp` keeps it.
        unsafe { settings.setTarget(Some(target_object)) };
        app_menu.addItem(&settings);
        app_menu.addItem(&NSMenuItem::separatorItem(mtm));
        app_menu.addItem(&item(
            mtm,
            &format!("Hide {APP_NAME}"),
            Some(sel!(hide:)),
            "h",
        ));
        let others = item(mtm, "Hide Others", Some(sel!(hideOtherApplications:)), "h");
        others.setKeyEquivalentModifierMask(
            NSEventModifierFlags::Command | NSEventModifierFlags::Option,
        );
        app_menu.addItem(&others);
        app_menu.addItem(&item(
            mtm,
            "Show All",
            Some(sel!(unhideAllApplications:)),
            "",
        ));
        app_menu.addItem(&NSMenuItem::separatorItem(mtm));
        app_menu.addItem(&item(
            mtm,
            &format!("Quit {APP_NAME}"),
            Some(sel!(terminate:)),
            "q",
        ));

        let window_menu = menu(mtm, "Window");
        window_menu.addItem(&item(mtm, "Minimize", Some(sel!(performMiniaturize:)), "m"));
        window_menu.addItem(&item(mtm, "Zoom", Some(sel!(performZoom:)), ""));
        window_menu.addItem(&NSMenuItem::separatorItem(mtm));
        let full_screen = item(mtm, "Enter Full Screen", Some(sel!(toggleFullScreen:)), "f");
        full_screen.setKeyEquivalentModifierMask(
            NSEventModifierFlags::Command | NSEventModifierFlags::Control,
        );
        window_menu.addItem(&full_screen);

        let bar = NSMenu::new(mtm);
        for submenu in [&app_menu, &window_menu] {
            let holder = NSMenuItem::new(mtm);
            holder.setSubmenu(Some(submenu));
            bar.addItem(&holder);
        }
        app.setWindowsMenu(Some(&window_menu));
        app.setMainMenu(Some(&bar));
        target
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WANTS: Want = Want {
        glass_setting: true,
        fullscreen: false,
        game_running: false,
    };

    #[test]
    fn glass_needs_the_setting_a_desktop_no_game_and_no_reduced_transparency() {
        // Where there is no Mac style, nothing is ever see-through.
        let expected = AVAILABLE;
        assert_eq!(glass_active(WANTS, false), expected);
        assert!(!glass_active(
            Want {
                glass_setting: false,
                ..WANTS
            },
            false
        ));
        assert!(!glass_active(
            Want {
                fullscreen: true,
                ..WANTS
            },
            false
        ));
        assert!(!glass_active(
            Want {
                game_running: true,
                ..WANTS
            },
            false
        ));
        assert!(!glass_active(WANTS, true));
    }

    #[test]
    fn the_traffic_lights_stay_clear_of_the_home_top_bar() {
        use crate::ui::theme::{MIN_WINDOW, pad, size};
        // At the smallest window (scale 0.68) the top bar's centre line is the lowest point the
        // title can reach; it must sit below the lights in points.
        let scale = (MIN_WINDOW.0 / 1600.0).min(MIN_WINDOW.1 / 900.0).max(0.68);
        let centre = (pad::HOME_TOP + size::ICON_BUTTON * 0.5) * scale;
        assert!(
            centre - 12.0 * scale >= LIGHTS_EXTENT.1 - 4.0,
            "the title would touch the traffic lights"
        );
    }
}
