//! The overlays that open over the home screen and the hook the panel modules plug into.
//!
//! The home model only knows *which* overlay is open ([`Overlay`], DESIGN.md 5) and routes the
//! commands that arrive while one is open to an [`OverlayHost`]. The panels (settings, library,
//! game settings, controls, advanced, log, trophies, confirm) implement the host in their own
//! modules; until then [`NoPanels`] closes whatever opens on back/options.

use egui::{Context, Ui};

use crate::app::model::Model;
use crate::app::textures::Textures;
use crate::ui::Shell;
use crate::ui::input::Command;

/// The overlays of the original (`Overlay` in `AppModel.swift`). Settings, library, log, trophies,
/// the profile and the friends are full-screen panels; the others slide in as the side panel
/// family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Overlay {
    Settings,
    GameSettings,
    Library,
    Log,
    ConfirmUninstall,
    /// The backups of a game's saves (a side panel).
    Saves,
    /// The question before a backup is put back (a side panel).
    ConfirmRestore,
    Controls,
    Trophies,
    Advanced,
    /// What the catalogue recommends for the game (a side panel).
    Recommended,
    /// The notes of a toolkit release (a full-screen panel).
    WhatsNew,
    /// Friends, party, messages, activity and the online settings of the AnyPlay network, and
    /// signing in to it (a full-screen panel).
    Friends,
    /// A player's profile, the player's own or another's (a full-screen panel).
    Profile,
    /// The player's trophies in a game next to another player's (a full-screen panel).
    Compare,
    /// The release a game stays on is gone: download it again or follow the global toolkit
    /// (a side panel).
    ToolkitMissing,
    /// The quick panel of the running game, opened by the Home button (a side panel).
    InGame,
    /// The question before the running game is closed (a side panel).
    ConfirmCloseGame,
    /// "Update required": this version is retired, so the update comes before the online area.
    /// Offline play stays open (a full-screen panel, docs/DESIGN.md 9.5.1).
    Required,
}

impl Overlay {
    /// Rendered as the side panel (slides in from the right, the home screen stays visible).
    pub fn is_side_panel(self) -> bool {
        matches!(
            self,
            Overlay::GameSettings
                | Overlay::ConfirmUninstall
                | Overlay::Saves
                | Overlay::ConfirmRestore
                | Overlay::Controls
                | Overlay::Advanced
                | Overlay::Recommended
                | Overlay::ToolkitMissing
                | Overlay::InGame
                | Overlay::ConfirmCloseGame
        )
    }

    /// The overlays of the online area: closed while the launcher's update is required.
    pub fn is_online(self) -> bool {
        matches!(self, Overlay::Friends | Overlay::Profile | Overlay::Compare)
    }

    /// Number of categories of a two-level panel (only settings has more than one).
    pub fn categories(self) -> usize {
        if self == Overlay::Settings { 6 } else { 1 }
    }
}

/// Everything a panel may touch while it draws or handles a command.
pub struct OverlayCx<'a> {
    pub model: &'a mut Model,
    pub shell: &'a mut Shell,
    pub textures: &'a mut Textures,
    pub ctx: &'a Context,
}

/// Implemented by the module that draws the overlays.
pub trait OverlayHost {
    /// A command arrived while `overlay` is open (the model did not consume it).
    fn command(&mut self, cx: &mut OverlayCx<'_>, overlay: Overlay, command: Command);

    /// Draws `overlay`. Called every frame while it is open and while it animates out.
    /// `openness` goes 0..=1 (spring `Motion::Overlay`); a panel that is not open is not drawn.
    fn ui(&mut self, ui: &mut Ui, cx: &mut OverlayCx<'_>, overlay: Overlay, openness: f32);

    /// A key arrived while `Shell.input.capture_keys` is on (the controls panel binds with it).
    fn captured_key(
        &mut self,
        _cx: &mut OverlayCx<'_>,
        _key: egui::Key,
        _physical_key: Option<egui::Key>,
        _modifiers: egui::Modifiers,
    ) {
    }

    /// The launcher is closing, after the preferences were saved: the self-update swaps in here.
    fn exit(&mut self) {}

    /// Once per frame after the overlays, open or not: background work and the trophy banner.
    fn frame(&mut self, _cx: &mut OverlayCx<'_>) {}
}

/// The placeholder host: draws nothing and closes on back and options.
#[derive(Default)]
pub struct NoPanels;

impl OverlayHost for NoPanels {
    fn command(&mut self, cx: &mut OverlayCx<'_>, _overlay: Overlay, command: Command) {
        if matches!(command, Command::Back | Command::Options) {
            cx.model.close_overlay();
        }
    }

    fn ui(&mut self, _ui: &mut Ui, _cx: &mut OverlayCx<'_>, _overlay: Overlay, _openness: f32) {}
}
