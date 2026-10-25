//! What the profile screen shows, as plain data: [`view`] turns what the launcher knows (the cloud's
//! profile page, the feed, the friends, and this PC's library) into a [`ProfileView`] that the
//! painter draws without deciding anything. Every rule of "what is shown when" lives here, so it
//! is tested without a window.
//!
//! The rules (docs/DESIGN.md 17, "Profile screen"):
//!
//! - A part the cloud leaves out (`None`) leaves no row and no placeholder for another player.
//!   The player's own profile may say what would fill a section.
//! - Names and art of games come only from the player's own library. A game the player does not
//!   have is a neutral tile with its title ID.
//! - The player's own games and trophies come from this PC (playtime, trophy files); another
//!   player's from the cloud (playtime, favourites) and from their items in the feed.
//! - Nothing is invented: no points, no ranks. The trophies are counted by grade.

use std::collections::BTreeMap;

use anyps5_core::online::api::{AccountId, Count, FeedEvent, FeedItem, Friend, Reaction};
use anyps5_core::online::{self, Online};

use crate::screens::format;
use crate::screens::online::{
    Avatar, Local, OwnedGame, ago_text, monogram_colour, presence_text, rarity_text,
};
use crate::screens::online::{sentence as feed_sentence, theme_grade};
use crate::screens::online_feed_strings::FeedText as F;
use crate::screens::online_strings::OnlineText as T;
use crate::screens::profile_strings::ProfileText as P;
use crate::ui::chrome::TileImage;
use crate::ui::i18n::{Arg, Language};
use crate::ui::theme::Grade;

/// The most rows a section of another player's profile shows.
pub const GAMES_SHOWN: usize = 8;
pub const TROPHIES_SHOWN: usize = 5;
pub const ACTIVITY_SHOWN: usize = 5;

// ---------------------------------------------------------------------------------------------
// The player's own library, as the host reads it from this PC
// ---------------------------------------------------------------------------------------------

/// A game of the player's library with what this PC knows of it.
#[derive(Clone, Debug, Default)]
pub struct OwnGame {
    pub title_id: String,
    pub name: String,
    pub art: Option<TileImage>,
    pub seconds: u64,
    /// Unix seconds of the last start.
    pub last_played: Option<i64>,
    /// `(earned, all)` once the game's trophy file is read.
    pub trophies: Option<(u32, u32)>,
}

/// A trophy the player earned, from the game's own trophy file.
#[derive(Clone, Debug)]
pub struct OwnTrophy {
    pub title_id: String,
    pub game: String,
    pub name: String,
    pub grade: Grade,
    /// Unix seconds.
    pub at: i64,
}

#[derive(Clone, Debug, Default)]
pub struct OwnLibrary {
    pub games: Vec<OwnGame>,
    /// Newest first.
    pub trophies: Vec<OwnTrophy>,
    /// Earned trophies by grade: platinum, gold, silver, bronze.
    pub grades: [u32; 4],
}

// ---------------------------------------------------------------------------------------------
// The view
// ---------------------------------------------------------------------------------------------

/// What pressing a row or button does; the host carries it out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Press {
    /// Nothing: the row is a fact.
    None,
    EditProfile,
    FriendsOnline,
    FriendOptions(AccountId),
    AnswerRequest(AccountId, bool),
    AddFriend(String),
    /// Ask the cloud for the profile again.
    Retry,
    /// Leave the profile for this game on Home.
    OpenGame(String),
    /// Open the feed on this item (where the reactions are).
    OpenFeed(u64),
}

#[derive(Clone, Debug)]
pub struct Action {
    pub press: Press,
    pub label: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    Own,
    Friend,
    /// The player sent a request and it waits.
    Requested,
    /// The other player sent one.
    Invited,
    Stranger,
}

#[derive(Clone, Debug)]
pub struct Stat {
    pub figure: String,
    pub label: String,
    /// The trophies by grade (platinum first), drawn under the label.
    pub grades: Option<[u32; 4]>,
    /// The words a screen reader says.
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct GameRow {
    pub title_id: String,
    pub name: String,
    pub art: Option<TileImage>,
    pub owned: bool,
    pub detail: String,
    /// A fact at the end of the row ("9/40", "42 h").
    pub trailing: Option<String>,
    /// A thin bar under the trailing fact, 0 to 1.
    pub progress: Option<f32>,
    pub press: Press,
    pub name_a11y: String,
}

#[derive(Clone, Debug)]
pub struct TrophyRow {
    pub grade: Grade,
    pub title: String,
    pub detail: String,
    pub trailing: Option<String>,
    pub press: Press,
}

#[derive(Clone, Debug)]
pub struct ActivityRow {
    pub account: AccountId,
    pub handle: String,
    pub avatar: Option<Avatar>,
    pub text: String,
    pub when: String,
    /// "GG 3 · Nice 1", only the reactions that were given.
    pub reactions: String,
    pub press: Press,
}

#[derive(Clone, Debug)]
pub struct Section<T> {
    pub rows: Vec<T>,
    /// How many there are in all, when more than the rows.
    pub total: usize,
    /// What the player's own empty section says; `None` for another player's (it is left out).
    pub empty: Option<String>,
}

impl<T> Section<T> {
    fn new() -> Self {
        Section {
            rows: Vec::new(),
            total: 0,
            empty: None,
        }
    }

    /// The section is drawn: it has rows, or the player's own words for having none.
    pub fn shown(&self) -> bool {
        !self.rows.is_empty() || self.empty.is_some()
    }
}

#[derive(Clone, Debug)]
pub struct Status {
    pub text: String,
    /// The account is online: a dot in the accent colour in front of the text.
    pub online: bool,
}

#[derive(Clone, Debug)]
pub struct Ready {
    pub relation: Relation,
    /// "Friend", "Request sent": a chip beside the status.
    pub chip: Option<String>,
    pub status: Option<Status>,
    pub about: Option<String>,
    /// A line under the header of the player's own profile while the cloud is out of reach.
    pub note: Option<String>,
    pub stats: Vec<Stat>,
    pub actions: Vec<Action>,
    pub games: Section<GameRow>,
    pub trophies: Section<TrophyRow>,
    pub activity: Section<ActivityRow>,
}

#[derive(Clone, Debug)]
pub enum Body {
    Loading,
    Offline,
    Failed,
    /// Another player shows nothing to someone who is not their friend.
    Private,
    /// A friend with nothing to show yet.
    Empty,
    Ready(Box<Ready>),
}

#[derive(Clone, Debug)]
pub struct ProfileView {
    pub account: AccountId,
    pub own: bool,
    pub handle: String,
    pub avatar: Option<Avatar>,
    /// The colour of the soft wash behind the header: the avatar's, or the monogram's.
    pub tint: [u8; 3],
    /// What a screen reader says for the whole screen.
    pub name: String,
    pub body: Body,
}

/// Everything [`view`] reads.
pub struct Inputs<'a> {
    pub online: &'a Online,
    pub local: &'a Local,
    /// The player's own library; only read for their own profile.
    pub library: Option<&'a OwnLibrary>,
    pub account: AccountId,
    pub language: Language,
    /// Unix seconds.
    pub now: i64,
    /// The local UTC offset; `None` asks the system for each time.
    pub utc_offset: Option<i32>,
    /// Messages that wait.
    pub unread: u64,
}

/// Playtime in words: "35 min", "4 h 20 min", "12 h". From ten hours on, the minutes are noise.
pub fn playtime_text(minutes: u64, language: Language) -> String {
    let (hours, rest) = online::hours_minutes(minutes);
    let number = |n: u64| Arg::Int(i64::try_from(n).unwrap_or(i64::MAX));
    if hours == 0 {
        F::Minutes.format(language, &[number(rest)])
    } else if hours >= 10 || rest == 0 {
        F::Hours.format(language, &[number(hours)])
    } else {
        F::HoursMinutes.format(language, &[number(hours), number(rest)])
    }
}

/// Unix seconds of a stamp `YYYY-MM-DDTHH:MM:SSZ`, as the launcher writes `last_played`.
pub fn parse_stamp(stamp: &str) -> Option<i64> {
    let day = online::api::day_number(stamp.get(..10)?)?;
    let part = |range: std::ops::Range<usize>| -> Option<i64> { stamp.get(range)?.parse().ok() };
    if stamp.get(10..11)? != "T" {
        return None;
    }
    Some(day * 86_400 + part(11..13)? * 3600 + part(14..16)? * 60 + part(17..19)?)
}

fn offset_for(inputs: &Inputs<'_>, at: i64) -> i32 {
    inputs
        .utc_offset
        .unwrap_or_else(|| format::local_offset_seconds(at.saturating_mul(1_000_000)))
}

fn when(inputs: &Inputs<'_>, at: i64) -> String {
    let at_u = u64::try_from(at).unwrap_or(0);
    ago_text(inputs.now, at_u, offset_for(inputs, at), inputs.language)
}

fn number(n: u64) -> String {
    n.to_string()
}

/// The view of the profile of `inputs.account`.
pub fn view(inputs: &Inputs<'_>) -> ProfileView {
    let online = inputs.online;
    let language = inputs.language;
    let id = inputs.account;
    let me = online.account.as_ref().map(|a| a.id);
    let own = me == Some(id);
    let entry = online.profiles.get(&id);
    let page = entry.and_then(|e| e.page.as_ref());
    let handle = page
        .map(|p| p.profile.account.handle.clone())
        .or_else(|| online.known(id).map(|a| a.handle.clone()))
        .or_else(|| {
            own.then(|| online.account.as_ref().map(|a| a.handle.clone()))
                .flatten()
        })
        .unwrap_or_default();
    let hash = page
        .and_then(|p| p.profile.avatar.as_deref())
        .or_else(|| online.avatars.get(&id).and_then(|h| h.as_deref()));
    let avatar = inputs.local.avatar(hash);
    let tint = vivid(avatar.and_then(|a| a.banner).unwrap_or_else(|| {
        let c = monogram_colour(id);
        [c.r(), c.g(), c.b()]
    }));
    let name = P::ScreenName.format(language, &[Arg::Str(&handle)]);
    let shell = |body| ProfileView {
        account: id,
        own,
        handle: handle.clone(),
        avatar,
        tint,
        name: name.clone(),
        body,
    };

    let relation = if own {
        Relation::Own
    } else if online.friend(id).is_some() {
        Relation::Friend
    } else if online.friends.outgoing.iter().any(|r| r.account.id == id) {
        Relation::Requested
    } else if online.friends.incoming.iter().any(|r| r.account.id == id) {
        Relation::Invited
    } else {
        Relation::Stranger
    };

    if !own && page.is_none() {
        return shell(if entry.is_some_and(|e| e.failed) {
            Body::Failed
        } else if !online.is_online() {
            Body::Offline
        } else {
            Body::Loading
        });
    }

    let mut ready = Ready {
        relation,
        chip: None,
        status: None,
        about: page.and_then(|p| p.profile.about.clone()),
        note: None,
        stats: Vec::new(),
        actions: Vec::new(),
        games: Section::new(),
        trophies: Section::new(),
        activity: Section::new(),
    };
    header(inputs, &handle, &mut ready);
    if own {
        own_parts(inputs, &mut ready);
    } else if let Some(page) = page {
        other_parts(inputs, page, &mut ready);
    }
    activity(inputs, &mut ready);
    // One figure alone only repeats a caption: the strip needs two to be a strip.
    if ready.stats.len() < 2 {
        ready.stats.clear();
    }

    // Another player's profile with nothing to see.
    if !own && nothing_to_show(&ready) {
        return shell(if relation == Relation::Stranger {
            Body::Private
        } else {
            Body::Empty
        });
    }
    shell(Body::Ready(Box::new(ready)))
}

/// The colour brought up to a brightness a wash can show: the banner colour is darkened until white
/// text reads on it, which is too dark for a faint wash. Hue and saturation stay.
fn vivid(rgb: [u8; 3]) -> [u8; 3] {
    let top = rgb.into_iter().max().unwrap_or(0);
    if top == 0 {
        return [120, 120, 120];
    }
    let scale = 210.0 / f32::from(top);
    rgb.map(|c| (f32::from(c) * scale).round().min(255.0) as u8)
}

fn nothing_to_show(ready: &Ready) -> bool {
    ready.about.is_none()
        && ready.stats.is_empty()
        && ready.games.rows.is_empty()
        && ready.trophies.rows.is_empty()
        && ready.activity.rows.is_empty()
}

/// The chip, the status line and the buttons under the name.
fn header(inputs: &Inputs<'_>, handle: &str, ready: &mut Ready) {
    let online = inputs.online;
    let language = inputs.language;
    let id = inputs.account;
    let action = |press, label: &str| Action {
        press,
        label: label.to_owned(),
    };
    match ready.relation {
        Relation::Own => {
            ready.status = Some(Status {
                text: F::OwnSubtitle.text(language).to_owned(),
                online: false,
            });
            ready
                .actions
                .push(action(Press::EditProfile, F::EditProfile.text(language)));
            let label = if inputs.unread > 0 {
                P::FriendsOnlineUnread.format(language, &[Arg::Int(inputs.unread as i64)])
            } else {
                P::FriendsOnline.text(language).to_owned()
            };
            ready.actions.push(Action {
                press: Press::FriendsOnline,
                label,
            });
            if !online.is_online() {
                ready.note = Some(P::OwnOfflineNote.text(language).to_owned());
            }
        }
        Relation::Friend => {
            ready.chip = Some(P::Friend.text(language).to_owned());
            if let Some(friend) = online.friend(id) {
                ready.status = Some(status_of(friend, language));
            }
            ready.actions.push(action(
                Press::FriendOptions(id),
                P::FriendOptions.text(language),
            ));
        }
        Relation::Requested => {
            ready.chip = Some(P::RequestSent.text(language).to_owned());
        }
        Relation::Invited => {
            ready.chip = Some(P::RequestReceived.text(language).to_owned());
            ready.actions.push(action(
                Press::AnswerRequest(id, true),
                P::AnswerRequest.text(language),
            ));
        }
        Relation::Stranger => {
            if online.is_online() && !handle.is_empty() {
                ready.actions.push(action(
                    Press::AddFriend(handle.to_owned()),
                    P::AddFriend.text(language),
                ));
            }
        }
    }
}

/// What they do now. The profile has no Join button (that is on their page in the panel), so it
/// does not say that the game can be joined.
fn status_of(friend: &Friend, language: Language) -> Status {
    let text = match &friend.presence.activity {
        Some(activity) => T::Playing.format(language, &[Arg::Str(&activity.title)]),
        None => presence_text(friend, language),
    };
    Status {
        text,
        online: friend.presence.online,
    }
}

fn stat(figure: String, singular: bool, one: P, many: P, language: Language) -> Stat {
    let label = if singular { one } else { many }.text(language).to_owned();
    let name = P::StatName.format(language, &[Arg::Str(&figure), Arg::Str(&label)]);
    Stat {
        figure,
        label,
        grades: None,
        name,
    }
}

fn games_stat(count: usize, language: Language) -> Stat {
    stat(
        number(count as u64),
        count == 1,
        P::StatGame,
        P::StatGames,
        language,
    )
}

/// Hours from ten minutes' worth up; below an hour, minutes. `None` for no time at all.
fn time_stat(minutes: u64, language: Language) -> Option<Stat> {
    if minutes == 0 {
        return None;
    }
    Some(if minutes >= 60 {
        let hours = minutes / 60;
        stat(
            number(hours),
            hours == 1,
            P::StatHour,
            P::StatHours,
            language,
        )
    } else {
        stat(
            number(minutes),
            minutes == 1,
            P::StatMinute,
            P::StatMinutes,
            language,
        )
    })
}

fn trophy_stat(grades: [u32; 4], language: Language) -> Option<Stat> {
    let total: u32 = grades.iter().sum();
    if total == 0 {
        return None;
    }
    let mut stat = stat(
        number(u64::from(total)),
        total == 1,
        P::StatTrophy,
        P::StatTrophies,
        language,
    );
    stat.grades = Some(grades);
    Some(stat)
}

fn grades_of(count: &Count) -> [u32; 4] {
    [count.platinum, count.gold, count.silver, count.bronze]
}

// ---------------------------------------------------------------------------------------------
// The player's own profile: this PC
// ---------------------------------------------------------------------------------------------

fn own_parts(inputs: &Inputs<'_>, ready: &mut Ready) {
    let language = inputs.language;
    let empty = OwnLibrary::default();
    let library = inputs.library.unwrap_or(&empty);

    if !library.games.is_empty() {
        ready.stats.push(games_stat(library.games.len(), language));
    }
    let minutes: u64 = library.games.iter().map(|g| g.seconds / 60).sum();
    ready.stats.extend(time_stat(minutes, language));
    ready.stats.extend(trophy_stat(library.grades, language));

    let mut games: Vec<&OwnGame> = library.games.iter().collect();
    games.sort_by(|a, b| {
        b.last_played
            .cmp(&a.last_played)
            .then_with(|| b.seconds.cmp(&a.seconds))
            .then_with(|| a.name.cmp(&b.name))
    });
    ready.games.total = games.len();
    ready.games.rows = games
        .into_iter()
        .map(|game| own_game_row(inputs, game))
        .collect();
    if ready.games.rows.is_empty() {
        ready.games.empty = Some(P::OwnGamesEmpty.text(language).to_owned());
    }

    ready.trophies.total = library.trophies.len();
    ready.trophies.rows = library
        .trophies
        .iter()
        .take(TROPHIES_SHOWN)
        .map(|trophy| TrophyRow {
            grade: trophy.grade,
            title: trophy.name.clone(),
            detail: format!(
                "{} · {} · {}",
                trophy.grade.title(language),
                trophy.game,
                when(inputs, trophy.at)
            ),
            trailing: None,
            press: Press::OpenGame(trophy.title_id.clone()),
        })
        .collect();
    if ready.trophies.rows.is_empty() {
        ready.trophies.empty = Some(P::OwnTrophiesEmpty.text(language).to_owned());
    }
}

fn own_game_row(inputs: &Inputs<'_>, game: &OwnGame) -> GameRow {
    let language = inputs.language;
    let played = if game.seconds >= 60 {
        Some(playtime_text(game.seconds / 60, language))
    } else {
        None
    };
    let detail = match (played, game.last_played) {
        (Some(time), Some(at)) => format!(
            "{time} · {}",
            P::LastPlayed.format(language, &[Arg::Str(&when(inputs, at))])
        ),
        (Some(time), None) => time,
        (None, Some(at)) => P::LastPlayed.format(language, &[Arg::Str(&when(inputs, at))]),
        (None, None) => P::NeverPlayed.text(language).to_owned(),
    };
    let (trailing, progress, a11y) = match game.trophies {
        Some((earned, all)) if all > 0 => (
            Some(format!("{earned}/{all}")),
            Some(earned as f32 / all as f32),
            P::TrophyProgress.format(
                language,
                &[Arg::Int(i64::from(earned)), Arg::Int(i64::from(all))],
            ),
        ),
        _ => (None, None, String::new()),
    };
    GameRow {
        title_id: game.title_id.clone(),
        name: game.name.clone(),
        art: game.art,
        owned: true,
        detail: detail.clone(),
        trailing,
        progress,
        press: Press::OpenGame(game.title_id.clone()),
        name_a11y: if a11y.is_empty() {
            format!("{}, {detail}", game.name)
        } else {
            format!("{}, {detail}, {a11y}", game.name)
        },
    }
}

// ---------------------------------------------------------------------------------------------
// Another player's profile: the cloud
// ---------------------------------------------------------------------------------------------

fn other_parts(inputs: &Inputs<'_>, page: &online::api::ProfilePage, ready: &mut Ready) {
    let language = inputs.language;
    let local = inputs.local;

    // The titles: favourites first, in their order, then the rest by time.
    let mut titles: BTreeMap<&str, (Option<u64>, Option<usize>)> = BTreeMap::new();
    for entry in page.profile.playtime.iter().flatten() {
        titles.entry(&entry.title_id).or_default().0 = Some(entry.minutes);
    }
    for (rank, id) in page.profile.favourites.iter().flatten().enumerate() {
        titles.entry(id).or_default().1 = Some(rank);
    }
    let mut ordered: Vec<(&str, Option<u64>, Option<usize>)> = titles
        .into_iter()
        .map(|(id, (minutes, rank))| (id, minutes, rank))
        .collect();
    ordered.sort_by(|a, b| {
        let key = |x: &(&str, Option<u64>, Option<usize>)| (x.2.is_none(), x.2);
        key(a)
            .cmp(&key(b))
            .then_with(|| b.1.unwrap_or(0).cmp(&a.1.unwrap_or(0)))
            .then_with(|| a.0.cmp(b.0))
    });

    if page.profile.playtime.is_some() || !ordered.is_empty() {
        ready.stats.push(games_stat(ordered.len(), language));
    }
    if let Some(playtime) = &page.profile.playtime {
        let minutes: u64 = playtime.iter().map(|p| p.minutes).sum();
        ready.stats.extend(time_stat(minutes, language));
    }
    if let Some(count) = &page.trophies {
        ready.stats.extend(trophy_stat(grades_of(count), language));
    }

    // What only a friend's page can say: how much of their library is yours.
    let shared = ordered
        .iter()
        .filter(|(id, _, _)| local.games.contains_key(*id))
        .count();
    if shared > 0 {
        ready.note = Some(P::SharedGames.format(language, &[Arg::Int(shared as i64)]));
    }
    ready.games.total = ordered.len();
    ready.games.rows = ordered
        .into_iter()
        .take(GAMES_SHOWN)
        .map(|(title_id, minutes, rank)| {
            let owned = local.games.get(title_id);
            other_game_row(title_id, minutes, rank.is_some(), owned, language)
        })
        .collect();

    // Trophies: what their feed items say.
    let items = items_of(inputs);
    let earned: Vec<&FeedItem> = items
        .iter()
        .copied()
        .filter(|i| matches!(i.event, FeedEvent::Trophy { .. }))
        .collect();
    ready.trophies.total = earned.len();
    ready.trophies.rows = earned
        .into_iter()
        .take(TROPHIES_SHOWN)
        .filter_map(|item| other_trophy_row(inputs, item))
        .collect();
}

fn other_game_row(
    title_id: &str,
    minutes: Option<u64>,
    favourite: bool,
    owned: Option<&OwnedGame>,
    language: Language,
) -> GameRow {
    let mut parts: Vec<String> = Vec::new();
    if favourite {
        parts.push(P::Favourite.text(language).to_owned());
    }
    if owned.is_none() {
        parts.push(P::NotInLibraryDetail.text(language).to_owned());
    }
    let trailing = minutes
        .filter(|m| *m > 0)
        .map(|m| playtime_text(m, language));
    let name = owned.map_or_else(|| title_id.to_owned(), |g| g.name.clone());
    let detail = parts.join(" · ");
    GameRow {
        title_id: title_id.to_owned(),
        name_a11y: [
            Some(name.clone()),
            Some(detail.clone()).filter(|d| !d.is_empty()),
            trailing.clone(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", "),
        name,
        art: owned.and_then(|g| g.icon).map(|texture| TileImage {
            texture,
            aspect: 1.0,
        }),
        owned: owned.is_some(),
        detail,
        trailing,
        progress: None,
        press: if owned.is_some() {
            Press::OpenGame(title_id.to_owned())
        } else {
            Press::None
        },
    }
}

fn other_trophy_row(inputs: &Inputs<'_>, item: &FeedItem) -> Option<TrophyRow> {
    let FeedEvent::Trophy {
        title_id,
        number,
        grade,
        rarity,
    } = &item.event
    else {
        return None;
    };
    let language = inputs.language;
    let grade = theme_grade(*grade);
    // A trophy is named only from the player's own copy of the game.
    let named = inputs.local.trophy_name(title_id, *number);
    let title = match named {
        Some(name) => name.to_owned(),
        None => P::GradeTrophy.format(language, &[Arg::Str(grade.title(language))]),
    };
    let game = inputs
        .local
        .game_name(title_id)
        .unwrap_or_else(|| P::InAGame.text(language));
    // The grade is in the words too, never only in the colour of the tile: in the title when the
    // trophy is unnamed, in the line under it when it has a name.
    let mut parts: Vec<String> = Vec::new();
    if named.is_some() {
        parts.push(grade.title(language).to_owned());
    }
    parts.push(game.to_owned());
    parts.push(when(inputs, i64::try_from(item.at).unwrap_or(0)));
    parts.extend(rarity.map(|percent| rarity_text(percent, language)));
    Some(TrophyRow {
        grade,
        title,
        detail: parts.join(" · "),
        trailing: None,
        press: Press::OpenFeed(item.id),
    })
}

/// The feed's items of the profile's account, newest first.
fn items_of<'a>(inputs: &Inputs<'a>) -> Vec<&'a FeedItem> {
    inputs
        .online
        .feed
        .items
        .iter()
        .filter(|item| item.account.id == inputs.account)
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Activity: the same for both
// ---------------------------------------------------------------------------------------------

/// "GG 3 · Nice 1": the reactions that were given, in the order of the buttons.
pub fn reactions_text(item: &FeedItem, language: Language) -> String {
    Reaction::ALL
        .iter()
        .filter(|r| item.reactions.of(**r) > 0)
        .map(|r| {
            let word = match r {
                Reaction::Nice => F::ReactNice,
                Reaction::Gg => F::ReactGg,
                Reaction::Wow => F::ReactWow,
                Reaction::Same => F::ReactSame,
            };
            format!("{} {}", word.text(language), item.reactions.of(*r))
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The activity of an account: its items, but not the trophies, which have their own section.
fn activity(inputs: &Inputs<'_>, ready: &mut Ready) {
    let language = inputs.language;
    let own = ready.relation == Relation::Own;
    let me = inputs.online.account.as_ref().map(|a| a.id);
    let all: Vec<&FeedItem> = items_of(inputs)
        .into_iter()
        .filter(|i| !matches!(i.event, FeedEvent::Trophy { .. }))
        .collect();
    ready.activity.total = all.len();
    ready.activity.rows = all
        .into_iter()
        .take(ACTIVITY_SHOWN)
        .map(|item| {
            let handle = if own {
                T::You.text(language).to_owned()
            } else {
                item.account.handle.clone()
            };
            let hash = inputs
                .online
                .avatars
                .get(&item.account.id)
                .and_then(|h| h.as_deref());
            let sentence = feed_sentence(item, me, language, inputs.local);
            ActivityRow {
                account: item.account.id,
                text: P::ActivityLine.format(language, &[Arg::Str(&handle), Arg::Str(&sentence)]),
                handle,
                avatar: inputs.local.avatar(hash),
                when: when(inputs, i64::try_from(item.at).unwrap_or(0)),
                reactions: reactions_text(item, language),
                press: Press::OpenFeed(item.id),
            }
        })
        .collect();
    if own && ready.activity.rows.is_empty() {
        ready.activity.empty = Some(P::OwnActivityEmpty.text(language).to_owned());
    }
}

#[cfg(test)]
mod tests;
