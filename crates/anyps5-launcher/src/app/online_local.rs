//! What the online panel takes from this PC for the profile page and the feed (see
//! `screens::online::Local`): the player's games by title ID, the names of trophies from the games'
//! own trophy files, and the avatars of the players on screen.
//!
//! Called every frame the page is open. It only looks at memory: trophy files are read by the
//! trophy service the home screen already uses (`Effect::LoadTrophies`, off the UI thread), and
//! avatars are fetched by `avatars`.

use std::collections::BTreeSet;

use anyps5_core::online::api::FeedEvent;
use anyps5_core::paths::GameId;

use crate::app::avatars;
use crate::app::overlay::OverlayCx;
use crate::screens::online::{Avatar, Local, OwnedGame};

/// The size games are decoded to for the profile page and the feed (shown at most 64 dp).
const ICON: u32 = 192;
/// The size avatars are decoded to (shown at most 72 dp).
const AVATAR: u32 = 192;

pub fn build(cx: &mut OverlayCx<'_>, cloud: Option<&str>) -> Local {
    let mut local = Local::default();
    let online = &cx.model.online;

    // The titles and the avatars on screen.
    let mut titles: BTreeSet<String> = BTreeSet::new();
    let mut hashes: BTreeSet<String> = BTreeSet::new();
    let mut trophy_titles: BTreeSet<String> = BTreeSet::new();
    for item in &online.feed.items {
        match &item.event {
            FeedEvent::Trophy { title_id, .. } => {
                titles.insert(title_id.clone());
                trophy_titles.insert(title_id.clone());
            }
            FeedEvent::FirstPlayed { title_id } => {
                titles.insert(title_id.clone());
            }
            _ => {}
        }
        if let Some(Some(hash)) = online.avatars.get(&item.account.id) {
            hashes.insert(hash.clone());
        }
    }
    for page in online.profiles.values().filter_map(|e| e.page.as_ref()) {
        titles.extend(page.profile.favourites.iter().flatten().cloned());
        titles.extend(
            page.profile
                .playtime
                .iter()
                .flatten()
                .map(|p| p.title_id.clone()),
        );
        hashes.extend(page.profile.avatar.clone());
    }

    // Games and the names of the trophies in the feed.
    let mut to_read: Vec<GameId> = Vec::new();
    for title in &titles {
        let Some(game) = cx.model.games.iter().find(|g| &g.title_id == title) else {
            continue;
        };
        let icon = game
            .icon
            .as_deref()
            .and_then(|path| cx.textures.get(path, ICON))
            .map(|loaded| loaded.id);
        local.games.insert(
            title.clone(),
            OwnedGame {
                name: game.name.clone(),
                icon,
            },
        );
        if !trophy_titles.contains(title) {
            continue;
        }
        match cx.model.trophies.get(&game.id) {
            Some(set) => {
                // A trophy that is hidden and not earned by the player stays unnamed: its name is
                // a spoiler.
                for trophy in set.trophies.iter().filter(|t| t.service_label == 0) {
                    if !trophy.hidden || trophy.is_unlocked() {
                        local
                            .trophy_names
                            .insert((title.clone(), trophy.number), trophy.name.clone());
                    }
                }
            }
            None => to_read.push(game.id.clone()),
        }
    }
    cx.model.read_trophies_of(&to_read);

    // Avatars.
    let dir = cx.model.paths.network_dir().join("avatars");
    for hash in hashes {
        if let Some(ready) = avatars::get(cx.ctx, cloud, &dir, &hash)
            && let Some(loaded) = cx.textures.get(&ready.path, AVATAR)
        {
            local.avatars.insert(
                hash,
                Avatar {
                    texture: loaded.id,
                    banner: ready.banner,
                },
            );
        }
    }
    local
}
