//! What the trophy comparison shows, as plain data: [`view`] turns what the launcher knows (the
//! cloud's comparison, the player's own trophy file for the game, the profiles) into a
//! [`CompareView`] that the painter draws without deciding anything.
//!
//! The rules (docs/DESIGN.md 17, "Trophy compare"):
//!
//! - The cloud sends numbers only: for each side the trophies earned (number, grade, time), the
//!   tally of the game with its points, and the level of all trophies. **Names, texts and pictures
//!   come only from the player's own copy of the game.** Without one the rows are numbers.
//! - A hidden trophy the player has not earned stays a secret until the player reveals it (or
//!   "Always Show Hidden Trophies" is on), in both columns.
//! - The other side is exactly what that player's profile shows the player: a trophy they keep
//!   secret is not in their column.
//! - Nothing is invented: the points, tallies and levels are the cloud's.

use std::collections::{BTreeMap, HashSet};

use anyps5_core::online::api::{AccountId, CompareSide, Trophy as Record, TrophyLevel};
use anyps5_core::online::{self, Online};
use anyps5_core::trophies::TrophySet;
use egui::TextureId;

use crate::screens::compare_strings::CompareText as C;
use crate::screens::convert;
use crate::screens::format;
use crate::screens::online::{Avatar, Local, theme_grade};
use crate::ui::i18n::{Arg, Key, Language};
use crate::ui::theme::Grade;

/// What pressing a row does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    /// Nothing: the row is a fact.
    None,
    /// Show the secret trophy with this number.
    Reveal(u32),
}

/// A player's state on one trophy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cell {
    /// Earned, with the day.
    Earned(String),
    NotYet,
}

/// One trophy of the game.
#[derive(Clone, Debug)]
pub struct Row {
    pub number: u32,
    /// The trophy's name, "Hidden Trophy" for a secret, "Trophy 4" without the game's file.
    pub name: String,
    /// The grade and what the trophy asks for; the grade alone without the game's file.
    pub detail: String,
    pub grade: Option<Grade>,
    /// A secret: the name and the picture stay hidden until it is shown.
    pub secret: bool,
    /// The position in the player's trophy set, for the picture; `None` without the game's file.
    pub index: Option<usize>,
    pub you: Cell,
    pub them: Cell,
    pub press: Press,
    /// The words a screen reader says.
    pub label: String,
}

/// One column's head: who, the level and the game's tally.
#[derive(Clone, Debug)]
pub struct Side {
    pub account: AccountId,
    /// "You", or the handle.
    pub name: String,
    pub handle: String,
    pub avatar: Option<Avatar>,
    pub level: u16,
    /// 0 to 1 on the way to the next level.
    pub fraction: f32,
    /// "Level 12".
    pub level_text: String,
    /// "1,240 points" in all.
    pub points_text: String,
    /// The game's trophies by grade, platinum first.
    pub grades: [u32; 4],
    /// "640 points in this game".
    pub game_points_text: String,
    pub label: String,
}

#[derive(Clone, Debug)]
pub struct Ready {
    pub you: Side,
    pub them: Side,
    pub rows: Vec<Row>,
    /// Said quietly under the header when the game is not in the player's library.
    pub note: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Body {
    Loading,
    Offline,
    Failed,
    /// The other player keeps their trophies from the player.
    Hidden,
    /// Neither has a trophy in this game.
    Empty,
    Ready(Box<Ready>),
}

#[derive(Clone, Debug)]
pub struct CompareView {
    pub account: AccountId,
    /// The other player's handle.
    pub handle: String,
    pub title_id: String,
    /// The game's name from the player's library, else its title ID.
    pub game: String,
    pub art: Option<TextureId>,
    /// What a screen reader says for the whole screen.
    pub name: String,
    pub body: Body,
}

/// Everything [`view`] reads.
pub struct Inputs<'a> {
    pub online: &'a Online,
    pub local: &'a Local,
    /// The player's trophy file of the game, once read; `None` for a game they do not have.
    pub set: Option<&'a TrophySet>,
    pub account: AccountId,
    pub title_id: &'a str,
    /// "Always Show Hidden Trophies".
    pub show_hidden: bool,
    /// The secrets the player showed on this screen.
    pub revealed: &'a HashSet<u32>,
    pub language: Language,
    /// The local UTC offset; `None` asks the system for each time.
    pub utc_offset: Option<i32>,
}

fn offset_for(inputs: &Inputs<'_>, at: i64) -> i32 {
    inputs
        .utc_offset
        .unwrap_or_else(|| format::local_offset_seconds(at.saturating_mul(1_000_000)))
}

fn day(inputs: &Inputs<'_>, at: i64) -> String {
    format::day_text(at, offset_for(inputs, at), inputs.language)
}

/// The view of the comparison with `inputs.account` in `inputs.title_id`.
pub fn view(inputs: &Inputs<'_>) -> CompareView {
    let online = inputs.online;
    let language = inputs.language;
    let id = inputs.account;
    let entry = online
        .compares
        .get(&(id, inputs.title_id.to_owned()))
        .filter(|_| online.is_signed_in());
    let handle = online
        .profiles
        .get(&id)
        .and_then(|e| e.page.as_ref())
        .map(|p| p.profile.account.handle.clone())
        .or_else(|| online.known(id).map(|a| a.handle.clone()))
        .unwrap_or_default();
    let owned = inputs.local.games.get(inputs.title_id);
    let game = owned.map_or_else(|| inputs.title_id.to_owned(), |g| g.name.clone());
    let shell = |body| CompareView {
        account: id,
        handle: handle.clone(),
        title_id: inputs.title_id.to_owned(),
        game: game.clone(),
        art: owned.and_then(|g| g.icon),
        name: C::ScreenName.format(language, &[Arg::Str(&game), Arg::Str(&handle)]),
        body,
    };
    let Some(entry) = entry else {
        return shell(if online.is_online() {
            Body::Loading
        } else {
            Body::Offline
        });
    };
    if entry.hidden {
        return shell(Body::Hidden);
    }
    let Some(data) = &entry.data else {
        return shell(if entry.failed {
            Body::Failed
        } else if online.is_online() {
            Body::Loading
        } else {
            Body::Offline
        });
    };

    let rows = rows(inputs, &data.you, &data.them);
    if rows.is_empty() {
        return shell(Body::Empty);
    }
    let ready = Ready {
        you: side(inputs, &data.you, true),
        them: side(inputs, &data.them, false),
        rows,
        note: inputs
            .set
            .is_none()
            .then(|| C::NotOwnedNote.text(language).to_owned()),
    };
    shell(Body::Ready(Box::new(ready)))
}

fn side(inputs: &Inputs<'_>, side: &CompareSide, own: bool) -> Side {
    let language = inputs.language;
    let online = inputs.online;
    let account = AccountId(side.account);
    let handle = if own {
        online
            .account
            .as_ref()
            .map(|a| a.handle.clone())
            .unwrap_or_default()
    } else {
        online
            .profiles
            .get(&account)
            .and_then(|e| e.page.as_ref())
            .map(|p| p.profile.account.handle.clone())
            .or_else(|| online.known(account).map(|a| a.handle.clone()))
            .unwrap_or_default()
    };
    let name = if own {
        C::You.text(language).to_owned()
    } else {
        handle.clone()
    };
    let hash = online
        .profiles
        .get(&account)
        .and_then(|e| e.page.as_ref())
        .and_then(|p| p.profile.avatar.as_deref())
        .or_else(|| online.avatars.get(&account).and_then(|h| h.as_deref()));
    let level: &TrophyLevel = &side.level;
    let level_text = C::Level.format(language, &[Arg::Str(&level.level.to_string())]);
    let points = format::grouped(u64::from(level.points), language);
    let points_text = C::Points.format(language, &[Arg::Str(&points)]);
    let game_points = format::grouped(u64::from(side.title.points), language);
    let game_points_text = C::GamePoints.format(language, &[Arg::Str(&game_points)]);
    let counts = side.title.counts;
    let grades = [counts.platinum, counts.gold, counts.silver, counts.bronze];
    // The grades in words, for a screen reader: the glyphs alone say them by colour.
    let by_grade: Vec<String> = Grade::ALL
        .into_iter()
        .zip(grades)
        .filter(|(_, n)| *n > 0)
        .map(|(grade, n)| format!("{n} {}", grade.title(language)))
        .collect();
    let by_grade = if by_grade.is_empty() {
        C::NoneInGame.text(language).to_owned()
    } else {
        by_grade.join(", ")
    };
    Side {
        account,
        label: C::SideName.format(
            language,
            &[
                Arg::Str(&name),
                Arg::Str(&level.level.to_string()),
                Arg::Str(&points),
                Arg::Str(&by_grade),
                Arg::Str(&game_points),
            ],
        ),
        name,
        avatar: inputs.local.avatar(hash),
        handle,
        level: level.level,
        fraction: online::level_progress(level),
        level_text,
        points_text,
        grades,
        game_points_text,
    }
}

fn state(cell: &Cell, language: Language) -> String {
    match cell {
        Cell::Earned(day) => C::EarnedOn.format(language, &[Arg::Str(day)]),
        Cell::NotYet => C::NotYet.text(language).to_owned(),
    }
}

fn rows(inputs: &Inputs<'_>, you: &CompareSide, them: &CompareSide) -> Vec<Row> {
    let language = inputs.language;
    let by = |side: &CompareSide| -> BTreeMap<u32, Record> {
        side.trophies
            .iter()
            .map(|t| (u32::from(t.number), *t))
            .collect()
    };
    let (yours, theirs) = (by(you), by(them));
    let cell = |record: Option<&Record>, local: Option<i64>| -> Cell {
        match record.map(|r| r.earned as i64).or(local) {
            Some(at) => Cell::Earned(day(inputs, at)),
            None => Cell::NotYet,
        }
    };
    let their_name = online_handle(inputs);
    let you_name = C::You.text(language);
    let finish = |mut row: Row| {
        row.label = C::RowName.format(
            language,
            &[
                Arg::Str(&row.name),
                Arg::Str(you_name),
                Arg::Str(&state(&row.you, language)),
                Arg::Str(&their_name),
                Arg::Str(&state(&row.them, language)),
            ],
        );
        row
    };

    let mut out = Vec::new();
    let mut seen: HashSet<u32> = HashSet::new();
    // The trophies of the player's own copy, in the game's order. Only the first set is the
    // network's.
    if let Some(set) = inputs.set {
        for (index, trophy) in set.trophies.iter().enumerate() {
            if trophy.service_label != 0 {
                continue;
            }
            seen.insert(trophy.number);
            let mine = yours.get(&trophy.number);
            let local = trophy.unlocked_secs();
            let secret = trophy.hidden
                && local.is_none()
                && mine.is_none()
                && !inputs.show_hidden
                && !inputs.revealed.contains(&trophy.number);
            let grade = convert::grade(trophy.grade);
            let (name, detail) = if secret {
                (
                    language.text(Key::TrophyHidden).to_owned(),
                    language.text(Key::TrophyHiddenDetail).to_owned(),
                )
            } else if trophy.detail.is_empty() {
                (trophy.name.clone(), grade.title(language).to_owned())
            } else {
                (
                    trophy.name.clone(),
                    format!("{} · {}", grade.title(language), trophy.detail),
                )
            };
            out.push(finish(Row {
                number: trophy.number,
                name,
                detail,
                grade: Some(grade),
                secret,
                index: Some(index),
                you: cell(mine, local),
                them: cell(theirs.get(&trophy.number), None),
                press: if secret {
                    Press::Reveal(trophy.number)
                } else {
                    Press::None
                },
                label: String::new(),
            }));
        }
    }
    // Numbers only the cloud knows: all of them without the game's file, the rest otherwise.
    let mut numbers: Vec<u32> = yours
        .keys()
        .chain(theirs.keys())
        .copied()
        .filter(|n| !seen.contains(n))
        .collect();
    numbers.sort_unstable();
    numbers.dedup();
    for number in numbers {
        let record = yours.get(&number).or_else(|| theirs.get(&number));
        let grade = record.map(|r| theme_grade(r.grade));
        out.push(finish(Row {
            number,
            name: C::NumberedTrophy.format(language, &[Arg::Str(&number.to_string())]),
            detail: grade.map_or_else(String::new, |g| g.title(language).to_owned()),
            grade,
            secret: false,
            index: None,
            you: cell(yours.get(&number), None),
            them: cell(theirs.get(&number), None),
            press: Press::None,
            label: String::new(),
        }));
    }
    out
}

fn online_handle(inputs: &Inputs<'_>) -> String {
    let online = inputs.online;
    online
        .profiles
        .get(&inputs.account)
        .and_then(|e| e.page.as_ref())
        .map(|p| p.profile.account.handle.clone())
        .or_else(|| online.known(inputs.account).map(|a| a.handle.clone()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
