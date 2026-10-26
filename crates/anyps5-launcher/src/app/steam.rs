//! Add to Steam, the launcher's side (docs/DESIGN.md 9.8): which entries the two settings rows
//! stand for. The writing is `anyps5_core::steam_shortcuts`; the pictures are `steam_art`.

use std::path::PathBuf;

use anyps5_core::library::Game;
use anyps5_core::steam_shortcuts::{self, Error, Outcome, Shortcut};

use super::steam_art;

/// What a row asks for.
#[derive(Clone, Debug)]
pub enum Request {
    /// "Add to Steam": the launcher itself.
    Launcher,
    /// "Add My Games to Steam": one entry per game.
    Games(Vec<Game>),
}

/// The name of the launcher's entry.
pub const LAUNCHER_NAME: &str = "AnyPlay Launcher";

/// The program Steam starts for the launcher. A Linux AppImage runs from a temporary mount, so
/// Steam gets the AppImage file instead.
pub fn launcher_exe() -> Option<PathBuf> {
    if cfg!(target_os = "linux")
        && let Some(appimage) = std::env::var_os("APPIMAGE").map(PathBuf::from)
        && appimage.is_file()
    {
        return Some(appimage);
    }
    std::env::current_exe().ok()
}

/// The entry of the launcher itself.
pub fn launcher_shortcut(exe: PathBuf) -> Shortcut {
    Shortcut {
        name: LAUNCHER_NAME.to_owned(),
        exe,
        arguments: String::new(),
        art: steam_art::launcher_art(),
    }
}

/// One entry per game: the launcher, opened on that game by its link. The game's name is the one
/// its own files give. The title ID only goes into the link when the strict parser takes it.
pub fn game_shortcuts(exe: &std::path::Path, games: &[Game]) -> Vec<Shortcut> {
    games
        .iter()
        .filter_map(|game| {
            let link = format!("{}://game/{}", anyps5_core::links::SCHEME, game.title_id);
            matches!(
                anyps5_core::links::parse(&link),
                Ok(anyps5_core::links::Link::Game { .. })
            )
            .then(|| Shortcut {
                name: game.name.clone(),
                exe: exe.to_path_buf(),
                arguments: link,
                art: steam_art::game_art(game),
            })
        })
        .collect()
}

/// Does the request. Slow (pictures are cut and written): run it off the UI thread.
pub fn run(request: Request) -> Result<Outcome, Error> {
    let exe = launcher_exe()
        .ok_or_else(|| Error::Io(std::io::Error::other("the launcher's own path is unknown")))?;
    let shortcuts = match request {
        Request::Launcher => vec![launcher_shortcut(exe)],
        Request::Games(games) => game_shortcuts(&exe, &games),
    };
    steam_shortcuts::install(&shortcuts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyps5_core::links::{self, Link};

    fn game(title_id: &str, name: &str) -> Game {
        Game {
            id: anyps5_core::paths::GameId::from_key(title_id),
            folder: PathBuf::from("nowhere"),
            title_id: title_id.to_owned(),
            name: name.to_owned(),
            names: Default::default(),
            version: String::new(),
            content_id: None,
            icon: None,
            background: None,
            music: None,
            executable: anyps5_core::library::ExecutableKind::Elf,
            size: None,
            unlocked_with: None,
        }
    }

    #[test]
    fn each_game_is_the_launcher_opened_on_that_game() {
        let exe = PathBuf::from("/opt/anyplay/anyplay-launcher");
        let games = [game("PPSA00001", "One"), game("PPSA00002", "Two")];
        let list = game_shortcuts(&exe, &games);
        assert_eq!(list.len(), 2);
        assert_eq!(list[1].name, "Two");
        assert_eq!(list[1].exe, exe);
        assert_eq!(
            links::parse(&list[1].arguments),
            Ok(Link::Game {
                title_id: "PPSA00002".into()
            })
        );
    }

    #[test]
    fn a_title_id_the_link_parser_refuses_makes_no_entry() {
        let exe = PathBuf::from("/opt/anyplay/anyplay-launcher");
        let games = [game("../evil\" --x", "Bad"), game("PPSA00001", "Fine")];
        let list = game_shortcuts(&exe, &games);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Fine");
    }

    #[test]
    fn the_launchers_entry_has_the_plain_name_and_no_arguments() {
        let entry = launcher_shortcut(PathBuf::from("x"));
        assert_eq!(entry.name, "AnyPlay Launcher");
        assert!(entry.arguments.is_empty());
        assert!(entry.art.portrait.is_some() && entry.art.icon.is_some());
    }
}
