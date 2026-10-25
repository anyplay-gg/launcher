//! The trophy comparison in the panel host: asks the net service, reads the game's own trophy
//! file for names and pictures, hands the screen what it needs and carries out what it asks for.
//! The screen itself never touches the model.

use super::PanelHost;
use crate::app::compare::Target;
use crate::app::overlay::OverlayCx;
use crate::screens::compare::{CompareAction, Inputs, Look, view};
use anyps5_core::online::{self as net, Status};

impl PanelHost {
    pub(super) fn compare_ui(&mut self, cx: &mut OverlayCx<'_>, wanted: bool) {
        let events = self.take_events(wanted);
        let Some(Target { account, title_id }) = cx.model.compare.clone() else {
            if wanted {
                cx.model.dismiss_overlay();
            }
            if self.compare.is_active() {
                self.compare.close();
            }
            return;
        };
        // Signing out leaves the screen: there is no one to compare for.
        if wanted && !cx.model.online.is_signed_in() {
            cx.model.dismiss_overlay();
            return;
        }
        let language = cx.model.language();
        let accent = cx.shell.accent_color();
        let cloud = net::cloud(cx.model.prefs.online_cloud.as_deref()).map(str::to_owned);
        if wanted && !self.compare.is_open() {
            self.compare.open();
            ask(&mut cx.model.online, account, &title_id);
        } else if !wanted && self.compare.is_open() {
            self.compare.close();
        }
        if !self.compare.is_active() {
            return;
        }
        if wanted
            && cloud.is_some()
            && !matches!(cx.model.online.status, Status::Running | Status::Missing)
        {
            cx.model
                .effects
                .push(crate::app::model::Effect::StartOnline);
        }
        // The player's own copy of the game gives the names and pictures; its file is read once.
        let game = cx
            .model
            .games
            .iter()
            .find(|g| g.title_id == title_id)
            .map(|g| g.id.clone());
        if let Some(id) = &game {
            cx.model.read_trophies_of(std::slice::from_ref(id));
        }
        let local = crate::app::online_local::build(cx, cloud.as_deref());
        let show_hidden = cx.model.prefs.show_hidden_trophies;
        let revealed = self.compare.revealed().clone();
        let actions = {
            let set = game.as_ref().and_then(|id| cx.model.trophies.get(id));
            let inputs = Inputs {
                online: &cx.model.online,
                local: &local,
                set,
                account,
                title_id: &title_id,
                show_hidden,
                revealed: &revealed,
                language,
                utc_offset: None,
            };
            let model = view(&inputs);
            let look = Look { language, accent };
            self.compare
                .show(cx.shell, cx.ctx, &events, &model, set, look)
        };
        for action in actions {
            match action {
                CompareAction::Close => cx.model.close_compare(),
                CompareAction::Retry => ask(&mut cx.model.online, account, &title_id),
                // The player read a secret: everyone who may see their trophies may, too.
                CompareAction::Revealed(number) => {
                    cx.model.online.reveal_secret(&title_id, number);
                }
            }
        }
    }
}

/// Asks the net service for the comparison and for both profiles (the handles and avatars).
fn ask(online: &mut net::Online, account: net::api::AccountId, title_id: &str) {
    online.request_compare(account, title_id);
    online.request_profile(account);
    if let Some(me) = online.account.as_ref().map(|a| a.id) {
        online.request_profile(me);
    }
}
