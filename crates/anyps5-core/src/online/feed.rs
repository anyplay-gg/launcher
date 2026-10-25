//! The feed behind the Feed page: what the player and their friends did, newest first, held as
//! pages came in, with the player's reactions applied at once and taken back when the cloud
//! refuses them. Plain data; the panel only reads it. The words are the panel's: this module
//! only decides which of them apply (how rare a trophy is, how long ago something happened).

use anyplay_control::Failure;

use super::api::{Account, AccountId, Feed, FeedItem, Reaction, ReactionCounts, Reactions};

/// The feed as the launcher holds it.
#[derive(Debug, Default)]
pub struct FeedState {
    /// Newest first.
    pub items: Vec<FeedItem>,
    /// Where the next older page starts; `None` at the end of the feed.
    pub next: Option<u64>,
    /// The first page arrived since the sign-in. Until then the page shows "loading".
    pub loaded: bool,
    /// Why the last fetch failed, until a fetch succeeds.
    pub error: Option<Failure>,
}

impl FeedState {
    /// Takes the newest page, which a first look, a refresh and the `feed` event all fetch.
    ///
    /// Items of the page replace the ones held with the same number. Items held that the page
    /// should contain but does not were deleted (a block, a retention sweep) and go. Older items
    /// stay, unless the page does not reach them: then more than a page of news arrived and the
    /// held items would leave a gap, so the page alone is the feed.
    pub fn merge_newest(&mut self, page: Feed) {
        self.loaded = true;
        self.error = None;
        let Some(oldest) = page.items.iter().map(|i| i.id).min() else {
            // An empty first page is an empty feed.
            self.items.clear();
            self.next = None;
            return;
        };
        let reaches = page.next.is_none() || self.items.iter().any(|i| i.id >= oldest);
        if !reaches {
            self.items = page.items;
            self.next = page.next;
            sort(&mut self.items);
            return;
        }
        let older: Vec<FeedItem> = self
            .items
            .drain(..)
            .filter(|held| held.id < oldest)
            .collect();
        let keeps_older = !older.is_empty();
        self.items = page.items;
        self.items.extend(older);
        sort(&mut self.items);
        // The held older items carry their own continuation; a page that ends the feed ends it.
        match (page.next, keeps_older) {
            (None, _) => self.next = None,
            (Some(next), false) => self.next = Some(next),
            (Some(_), true) => {}
        }
    }

    /// Takes a page older than the items held: `before` was the `next` it was fetched with.
    pub fn merge_older(&mut self, before: u64, page: Feed) {
        // The feed was replaced while this page was on its way: it fits no longer.
        if self.next != Some(before) {
            return;
        }
        self.error = None;
        for item in page.items {
            if item.id < before && !self.items.iter().any(|held| held.id == item.id) {
                self.items.push(item);
            }
        }
        sort(&mut self.items);
        self.next = page.next.filter(|next| *next < before);
    }

    pub fn item(&self, id: u64) -> Option<&FeedItem> {
        self.items.iter().find(|item| item.id == id)
    }

    /// Sets the player's reaction to `item` at once. Returns what the item showed before, to put
    /// it back when the cloud refuses; `None` when the item is not held.
    pub fn react(&mut self, item: u64, reaction: Option<Reaction>) -> Option<Reactions> {
        let item = self.items.iter_mut().find(|held| held.id == item)?;
        let before = Reactions {
            reactions: item.reactions,
            mine: item.mine,
        };
        item.reactions = adjusted(item.reactions, item.mine, reaction);
        item.mine = reaction;
        Some(before)
    }

    /// Shows what the cloud says an item's reactions are (the answer to a reaction).
    pub fn set_reactions(&mut self, item: u64, now: Reactions) {
        if let Some(item) = self.items.iter_mut().find(|held| held.id == item) {
            item.reactions = now.reactions;
            item.mine = now.mine;
        }
    }

    /// The accounts the items are about, each once, newest first.
    pub fn accounts(&self) -> Vec<AccountId> {
        let mut seen = Vec::new();
        for item in &self.items {
            if !seen.contains(&item.account.id) {
                seen.push(item.account.id);
            }
        }
        seen
    }
}

fn sort(items: &mut [FeedItem]) {
    items.sort_by_key(|item| std::cmp::Reverse(item.id));
}

/// `counts` after the player's reaction went from `from` to `to`.
pub fn adjusted(
    mut counts: ReactionCounts,
    from: Option<Reaction>,
    to: Option<Reaction>,
) -> ReactionCounts {
    if from == to {
        return counts;
    }
    if let Some(from) = from {
        let slot = slot(&mut counts, from);
        *slot = slot.saturating_sub(1);
    }
    if let Some(to) = to {
        counts.add(to);
    }
    counts
}

fn slot(counts: &mut ReactionCounts, reaction: Reaction) -> &mut u32 {
    match reaction {
        Reaction::Nice => &mut counts.nice,
        Reaction::Gg => &mut counts.gg,
        Reaction::Wow => &mut counts.wow,
        Reaction::Same => &mut counts.same,
    }
}

/// What pressing `pressed` does to the player's reaction: their own again takes it back.
pub fn toggled(mine: Option<Reaction>, pressed: Reaction) -> Option<Reaction> {
    if mine == Some(pressed) {
        None
    } else {
        Some(pressed)
    }
}

/// How rare a trophy is, from the percent of a title's players who earned it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rarity {
    /// Under 1 %.
    Extreme,
    /// Under 5 %.
    Rare,
    /// Under 20 %.
    Uncommon,
    Common,
}

impl Rarity {
    pub fn of(percent: f32) -> Rarity {
        if percent < 1.0 {
            Rarity::Extreme
        } else if percent < 5.0 {
            Rarity::Rare
        } else if percent < 20.0 {
            Rarity::Uncommon
        } else {
            Rarity::Common
        }
    }
}

/// How long ago something happened, in the steps the feed tells it in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ago {
    /// Under a minute (or a clock a little ahead of the cloud's).
    Now,
    Minutes(u64),
    Hours(u64),
    Yesterday,
    Days(u64),
    /// A week or more: the day is told instead.
    Older,
}

impl Ago {
    /// `now` and `at` in Unix seconds.
    pub fn of(now: i64, at: u64) -> Ago {
        let seconds = u64::try_from(now).unwrap_or(0).saturating_sub(at);
        match seconds {
            0..=59 => Ago::Now,
            60..=3_599 => Ago::Minutes(seconds / 60),
            3_600..=86_399 => Ago::Hours(seconds / 3_600),
            86_400..=172_799 => Ago::Yesterday,
            172_800..=604_799 => Ago::Days(seconds / 86_400),
            _ => Ago::Older,
        }
    }
}

/// The members of a party item besides the account it is about, leader first as the cloud sends
/// them.
pub fn party_others<'a>(account: &Account, members: &'a [Account]) -> Vec<&'a Account> {
    members.iter().filter(|m| m.id != account.id).collect()
}

#[cfg(test)]
mod tests {
    use super::super::api::{FeedEvent, Grade};
    use super::*;

    fn who(id: u64, handle: &str) -> Account {
        Account {
            id: AccountId(id),
            handle: handle.into(),
        }
    }

    fn item(id: u64) -> FeedItem {
        FeedItem {
            id,
            account: who(2, "Lumen"),
            at: 1_000 + id,
            event: FeedEvent::Trophy {
                title_id: "GAME00001".into(),
                number: 1,
                grade: Grade::Gold,
                rarity: Some(2.3),
            },
            reactions: ReactionCounts::default(),
            mine: None,
        }
    }

    fn page(ids: &[u64], next: Option<u64>) -> Feed {
        Feed {
            items: ids.iter().map(|id| item(*id)).collect(),
            next,
        }
    }

    fn ids(feed: &FeedState) -> Vec<u64> {
        feed.items.iter().map(|i| i.id).collect()
    }

    #[test]
    fn a_first_page_is_the_feed() {
        let mut feed = FeedState::default();
        assert!(!feed.loaded);
        feed.merge_newest(page(&[9, 8, 7], Some(7)));
        assert!(feed.loaded);
        assert_eq!(ids(&feed), [9, 8, 7]);
        assert_eq!(feed.next, Some(7));
    }

    #[test]
    fn a_refresh_puts_new_items_on_top_and_keeps_the_older_pages() {
        let mut feed = FeedState::default();
        feed.merge_newest(page(&[9, 8, 7], Some(7)));
        feed.merge_older(7, page(&[6, 5], None));
        assert_eq!(ids(&feed), [9, 8, 7, 6, 5]);
        assert_eq!(feed.next, None);
        // Two new items; the page ends at 8, which the player holds already.
        feed.merge_newest(page(&[11, 10, 9, 8], Some(8)));
        assert_eq!(ids(&feed), [11, 10, 9, 8, 7, 6, 5]);
        assert_eq!(feed.next, None, "the older pages keep their end");
    }

    #[test]
    fn a_refresh_drops_what_the_cloud_no_longer_lists() {
        let mut feed = FeedState::default();
        feed.merge_newest(page(&[9, 8, 7], Some(7)));
        feed.merge_newest(page(&[9, 7], Some(7)));
        assert_eq!(ids(&feed), [9, 7]);
    }

    #[test]
    fn a_refresh_that_does_not_reach_the_held_items_replaces_them() {
        let mut feed = FeedState::default();
        feed.merge_newest(page(&[9, 8, 7], Some(7)));
        // More than a page of news: 40 to 38 do not touch 9, so the two would leave a gap.
        feed.merge_newest(page(&[40, 39, 38], Some(38)));
        assert_eq!(ids(&feed), [40, 39, 38]);
        assert_eq!(feed.next, Some(38));
    }

    #[test]
    fn a_last_page_that_ends_the_feed_ends_it() {
        let mut feed = FeedState::default();
        feed.merge_newest(page(&[9, 8, 7], Some(7)));
        feed.merge_older(7, page(&[6], Some(6)));
        feed.merge_newest(page(&[9, 8, 7, 6], None));
        assert_eq!(feed.next, None);
        assert_eq!(ids(&feed), [9, 8, 7, 6]);
    }

    #[test]
    fn an_empty_first_page_is_an_empty_feed() {
        let mut feed = FeedState::default();
        feed.merge_newest(page(&[3], None));
        feed.merge_newest(page(&[], None));
        assert!(feed.loaded && feed.items.is_empty());
    }

    #[test]
    fn an_older_page_adds_only_what_is_older_and_new() {
        let mut feed = FeedState::default();
        feed.merge_newest(page(&[9, 8, 7], Some(7)));
        feed.merge_older(7, page(&[8, 6, 5], Some(5)));
        assert_eq!(ids(&feed), [9, 8, 7, 6, 5]);
        assert_eq!(feed.next, Some(5));
        // A page fetched for a continuation the feed has left is ignored.
        feed.merge_older(7, page(&[1], None));
        assert_eq!(ids(&feed), [9, 8, 7, 6, 5]);
    }

    #[test]
    fn a_reaction_counts_at_once_and_the_old_one_goes() {
        let mut feed = FeedState::default();
        feed.merge_newest(page(&[9], None));
        let before = feed.react(9, Some(Reaction::Gg)).unwrap();
        assert_eq!(before.mine, None);
        assert_eq!(feed.items[0].reactions.gg, 1);
        assert_eq!(feed.items[0].mine, Some(Reaction::Gg));
        let before = feed.react(9, Some(Reaction::Wow)).unwrap();
        assert_eq!(before.mine, Some(Reaction::Gg));
        assert_eq!(feed.items[0].reactions.gg, 0);
        assert_eq!(feed.items[0].reactions.wow, 1);
        feed.react(9, None);
        assert_eq!(feed.items[0].reactions, ReactionCounts::default());
        assert!(feed.react(404, Some(Reaction::Nice)).is_none());
    }

    #[test]
    fn a_refused_reaction_goes_back_and_an_answer_wins() {
        let mut feed = FeedState::default();
        feed.merge_newest(page(&[9], None));
        feed.items[0].reactions.nice = 2;
        feed.items[0].mine = Some(Reaction::Nice);
        let before = feed.react(9, Some(Reaction::Same)).unwrap();
        feed.set_reactions(9, before);
        assert_eq!(feed.items[0].mine, Some(Reaction::Nice));
        assert_eq!(feed.items[0].reactions.nice, 2);
        assert_eq!(feed.items[0].reactions.same, 0);
        feed.set_reactions(
            9,
            Reactions {
                reactions: ReactionCounts {
                    same: 5,
                    ..Default::default()
                },
                mine: Some(Reaction::Same),
            },
        );
        assert_eq!(feed.items[0].reactions.same, 5);
    }

    #[test]
    fn a_count_never_goes_below_nothing() {
        let counts = adjusted(
            ReactionCounts::default(),
            Some(Reaction::Nice),
            Some(Reaction::Gg),
        );
        assert_eq!((counts.nice, counts.gg), (0, 1));
    }

    #[test]
    fn pressing_the_own_reaction_again_takes_it_back() {
        assert_eq!(toggled(None, Reaction::Gg), Some(Reaction::Gg));
        assert_eq!(toggled(Some(Reaction::Gg), Reaction::Gg), None);
        assert_eq!(
            toggled(Some(Reaction::Gg), Reaction::Wow),
            Some(Reaction::Wow)
        );
    }

    #[test]
    fn rarity_has_four_steps() {
        assert_eq!(Rarity::of(0.4), Rarity::Extreme);
        assert_eq!(Rarity::of(2.3), Rarity::Rare);
        assert_eq!(Rarity::of(5.0), Rarity::Uncommon);
        assert_eq!(Rarity::of(19.9), Rarity::Uncommon);
        assert_eq!(Rarity::of(50.0), Rarity::Common);
    }

    #[test]
    fn time_is_told_in_steps() {
        let now = 1_000_000;
        let ago = |seconds: i64| Ago::of(now, (now - seconds) as u64);
        assert_eq!(ago(5), Ago::Now);
        assert_eq!(ago(-30), Ago::Now, "a clock a little ahead");
        assert_eq!(ago(60), Ago::Minutes(1));
        assert_eq!(ago(59 * 60), Ago::Minutes(59));
        assert_eq!(ago(3_600), Ago::Hours(1));
        assert_eq!(ago(23 * 3_600), Ago::Hours(23));
        assert_eq!(ago(30 * 3_600), Ago::Yesterday);
        assert_eq!(ago(3 * 86_400), Ago::Days(3));
        assert_eq!(ago(7 * 86_400), Ago::Older);
    }

    #[test]
    fn a_party_names_the_others_only() {
        let members = [who(2, "Lumen"), who(3, "Nox"), who(4, "Rook")];
        let others = party_others(&members[0], &members);
        assert_eq!(
            others.iter().map(|a| a.handle.as_str()).collect::<Vec<_>>(),
            ["Nox", "Rook"]
        );
    }

    #[test]
    fn the_accounts_of_the_items_are_listed_once() {
        let mut feed = FeedState::default();
        let mut other = item(8);
        other.account = who(3, "Nox");
        feed.merge_newest(Feed {
            items: vec![item(9), other, item(7)],
            next: None,
        });
        assert_eq!(feed.accounts(), [AccountId(2), AccountId(3)]);
    }
}
