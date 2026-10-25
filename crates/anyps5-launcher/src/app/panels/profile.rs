//! The profile screen in the panel host: reads what the screen needs from this PC (the library,
//! the trophy files, the avatars) and from the net service, hands it over, and carries out what a
//! press asks for. The screen itself never touches the model.

use anyps5_core::online::Status;
use anyps5_core::online::api::AccountId;
use anyps5_core::trophies::Grade as CoreGrade;
use anyps5_core::{online as net, paths::GameId};

use super::PanelHost;
use super::online::online_action;
use crate::app::model::Effect;
use crate::app::overlay::{Overlay, OverlayCx};
use crate::app::profile::Target;
use crate::screens::convert;
use crate::screens::online::{OnlineAction, Page};
use crate::screens::profile::{
    Inputs, OwnGame, OwnLibrary, OwnTrophy, Press, ProfileAction, parse_stamp,
};
use crate::ui::chrome::TileImage;

/// The size game art is decoded to for the profile (shown at most 52 dp).
const ART: u32 = 128;
/// The trophies kept for the list (it shows the newest few).
const TROPHIES_KEPT: usize = 12;

impl PanelHost {
    pub(super) fn profile_ui(&mut self, cx: &mut OverlayCx<'_>, wanted: bool) {
        let events = self.take_events(wanted);
        let target = cx.model.profile;
        let language = cx.model.language();
        let accent = cx.shell.accent_color();
        let Some(Target { account, .. }) = target else {
            if wanted {
                cx.model.dismiss_overlay();
            }
            if self.profile.is_active() {
                self.profile.close();
            }
            return;
        };
        // Signing out leaves the profile screen: there is no one to show it for.
        if wanted && !cx.model.online.is_signed_in() {
            cx.model.dismiss_overlay();
            return;
        }
        let cloud = net::cloud(cx.model.prefs.online_cloud.as_deref()).map(str::to_owned);
        if wanted && !self.profile.is_open() {
            self.profile.open(account);
            ask(&mut cx.model.online, account);
        } else if !wanted && self.profile.is_open() {
            self.profile.close();
        }
        if !self.profile.is_active() {
            return;
        }
        if wanted
            && cloud.is_some()
            && !matches!(cx.model.online.status, Status::Running | Status::Missing)
        {
            cx.model.effects.push(Effect::StartOnline);
        }
        let own = cx.model.online.account.as_ref().map(|a| a.id) == Some(account);
        let local = crate::app::online_local::build(cx, cloud.as_deref());
        let library = own.then(|| own_library(cx));
        let unread = cx.model.online.social.unread_total();
        let actions = {
            let inputs = Inputs {
                online: &cx.model.online,
                local: &local,
                library: library.as_ref(),
                account,
                language,
                now: crate::app::clock::unix_seconds(),
                utc_offset: None,
                unread,
            };
            self.profile
                .show(cx.shell, cx.ctx, &events, &inputs, accent)
        };
        for action in actions {
            match action {
                ProfileAction::Close => cx.model.close_profile(),
                ProfileAction::Press(press) => self.profile_press(cx, account, press),
            }
        }
    }

    fn profile_press(&mut self, cx: &mut OverlayCx<'_>, account: AccountId, press: Press) {
        let model = &mut *cx.model;
        match press {
            Press::None => {}
            Press::EditProfile => online_action(model, OnlineAction::ManageAccount),
            Press::Retry => ask(&mut model.online, account),
            Press::AddFriend(handle) => online_action(model, OnlineAction::AddFriend(handle)),
            Press::FriendsOnline => {
                model.online_resume = false;
                self.online_start = None;
                model.switch_overlay(Overlay::Online);
            }
            Press::FriendOptions(id) => {
                self.online_start = Some((Page::Friend(id), None));
                model.switch_overlay(Overlay::Online);
            }
            Press::AnswerRequest(id, _) => {
                self.online_start = Some((Page::Request(id), None));
                model.switch_overlay(Overlay::Online);
            }
            Press::OpenFeed(item) => {
                self.online_start = Some((Page::Feed, Some(item)));
                model.switch_overlay(Overlay::Online);
            }
            Press::OpenGame(title_id) => {
                if let Some(index) = model.games.iter().position(|g| g.title_id == title_id) {
                    model.selected = index;
                    model.focus = crate::ui::focus::HomeFocus::Tiles;
                    model.measure_selected();
                    model.dismiss_overlay();
                }
            }
        }
    }
}

/// Asks the net service for the profile and, once, for the feed (the activity of a friend is
/// read from it).
fn ask(online: &mut net::Online, account: AccountId) {
    online.request_profile(account);
    if !online.feed.loaded && online.is_online() {
        online.request_feed();
    }
}

/// The player's own games and trophies, from this PC: playtime and the last start from the
/// preferences, trophies from the games' own trophy files (read once, off the UI thread).
fn own_library(cx: &mut OverlayCx<'_>) -> OwnLibrary {
    let ids: Vec<GameId> = cx.model.games.iter().map(|g| g.id.clone()).collect();
    cx.model.read_trophies_of(&ids);
    let mut library = OwnLibrary::default();
    for game in &cx.model.games {
        let art = game
            .icon
            .as_deref()
            .and_then(|path| cx.textures.get(path, ART))
            .map(|loaded| TileImage {
                texture: loaded.id,
                aspect: loaded.aspect,
            });
        let played = cx.model.prefs.playtime(&game.id);
        let last_played = cx
            .model
            .prefs
            .last_played
            .get(game.id.as_str())
            .and_then(|stamp| parse_stamp(stamp));
        let mut counts = None;
        if let Some(set) = cx.model.trophies.get(&game.id) {
            // The first set is the one the network records; others are add-ons.
            let main = || set.trophies.iter().filter(|t| t.service_label == 0);
            let earned = main().filter(|t| t.is_unlocked()).count();
            counts = Some((earned as u32, main().count() as u32));
            for trophy in main().filter(|t| t.is_unlocked()) {
                library.grades[grade_slot(trophy.grade)] += 1;
                library.trophies.push(OwnTrophy {
                    title_id: game.title_id.clone(),
                    game: game.name.clone(),
                    name: trophy.name.clone(),
                    grade: convert::grade(trophy.grade),
                    at: trophy.unlocked_secs().unwrap_or(0),
                });
            }
        }
        library.games.push(OwnGame {
            title_id: game.title_id.clone(),
            name: game.name.clone(),
            art,
            seconds: played.seconds,
            last_played,
            trophies: counts,
        });
    }
    library.trophies.sort_by_key(|t| std::cmp::Reverse(t.at));
    library.trophies.truncate(TROPHIES_KEPT);
    library
}

/// The place of a grade in the counts: platinum first.
fn grade_slot(grade: CoreGrade) -> usize {
    match grade {
        CoreGrade::Platinum => 0,
        CoreGrade::Gold => 1,
        CoreGrade::Silver => 2,
        CoreGrade::Bronze => 3,
    }
}
