//! What the profile page and the feed take from this PC: the games the player owns (their names
//! and icons), the names of trophies (read from the game's own trophy files), and the avatars of
//! other players (fetched once by `app::avatars`).
//!
//! The cloud knows none of it: a title and a trophy are numbers there (network ADR 0022). The host
//! builds this each frame the page is open and hands it to the panel, so the panel never touches
//! the library, the disk or the network.

use std::collections::BTreeMap;

use egui::TextureId;

/// A game in the player's library.
#[derive(Clone, Debug, PartialEq)]
pub struct OwnedGame {
    pub name: String,
    pub icon: Option<TextureId>,
}

/// An avatar that is ready to draw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Avatar {
    pub texture: TextureId,
    /// The colour of the banner derived from the picture, `None` for a picture without colour.
    pub banner: Option<[u8; 3]>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Local {
    /// By title ID.
    pub games: BTreeMap<String, OwnedGame>,
    /// By avatar hash.
    pub avatars: BTreeMap<String, Avatar>,
    /// The name of a trophy the player may see, by title ID and trophy number.
    pub trophy_names: BTreeMap<(String, u32), String>,
}

impl Local {
    pub fn game_name(&self, title_id: &str) -> Option<&str> {
        self.games.get(title_id).map(|g| g.name.as_str())
    }

    pub fn trophy_name(&self, title_id: &str, number: u16) -> Option<&str> {
        self.trophy_names
            .get(&(title_id.to_owned(), u32::from(number)))
            .map(String::as_str)
    }

    pub fn avatar(&self, hash: Option<&str>) -> Option<Avatar> {
        hash.and_then(|h| self.avatars.get(h)).copied()
    }
}
