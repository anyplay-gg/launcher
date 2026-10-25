//! "How did it run?" (docs/DESIGN.md 5.3): one quiet question on the game page after a session.
//!
//! It is asked once per game version and toolkit version, never signed out, and never again after
//! an answer or a dismiss. The answer is a run report (CONTEXT.md); what it holds is the title ID,
//! the versions, the verdict and the system, which the net service adds.

use anyps5_core::online::api::Verdict;
use anyps5_core::paths::GameId;
use anyps5_core::settings::run_question_key;

use crate::app::model::{GameState, Model};

/// The four answers, worst first. The question line shows them in this order.
pub const ANSWERS: [Verdict; 4] = [
    Verdict::NoStart,
    Verdict::Menus,
    Verdict::InGame,
    Verdict::Playable,
];

/// A session that ended and has not been asked about yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunPrompt {
    pub game: GameId,
    pub title_id: String,
    pub version: String,
    pub toolkit: String,
}

impl RunPrompt {
    fn key(&self) -> String {
        run_question_key(&self.title_id, &self.version, &self.toolkit)
    }
}

impl Model {
    /// A session of the game ended while the player was signed in: remember to ask, unless this
    /// game version with this toolkit was answered or dismissed before.
    pub fn session_ended(&mut self, game: &GameId) {
        self.run_prompt = None;
        if !self.online.is_signed_in() {
            return;
        }
        let Some(game) = self.game(game) else {
            return;
        };
        let Some(toolkit) = game.unlocked_with.clone().or_else(|| self.toolkit.clone()) else {
            return;
        };
        let prompt = RunPrompt {
            game: game.id.clone(),
            title_id: game.title_id.clone(),
            version: game.version.clone(),
            toolkit,
        };
        if !self.prefs.run_asked.contains(&prompt.key()) {
            self.run_prompt = Some(prompt);
        }
    }

    /// The question to show now: on the game page of the game that was played, signed in, with
    /// nothing in front of the page.
    pub fn run_question(&self) -> Option<&RunPrompt> {
        let prompt = self.run_prompt.as_ref()?;
        let game = self.selected_game()?;
        let shown = game.id == prompt.game
            && self.overlay.is_none()
            && self.online.is_signed_in()
            && matches!(
                self.state_of(game),
                GameState::Installed { .. } | GameState::Failed(_)
            );
        shown.then_some(prompt)
    }

    /// A press on the question line: an answer (0 to 3) or the dismiss (4). Either way the
    /// question is done for this game version and toolkit.
    pub fn answer_run(&mut self, choice: usize) {
        let Some(prompt) = self.run_question().cloned() else {
            return;
        };
        self.run_prompt = None;
        self.focus = crate::ui::focus::HomeFocus::Actions(0);
        let key = prompt.key();
        self.edit_prefs(|p| {
            p.run_asked.insert(key);
        });
        let Some(&verdict) = ANSWERS.get(choice) else {
            return;
        };
        if self
            .online
            .send_run_report(&prompt.title_id, &prompt.version, &prompt.toolkit, verdict)
        {
            // The new answer counts in what the game page shows.
            self.online
                .request_runs(&prompt.title_id, &prompt.version, true);
        }
    }

    /// Asks the cloud what players found for the selected game, once per sign-in.
    pub fn request_selected_runs(&mut self) {
        let Some((title_id, version)) = self
            .selected_game()
            .map(|g| (g.title_id.clone(), g.version.clone()))
        else {
            return;
        };
        self.online.request_runs(&title_id, &version, false);
    }
}
