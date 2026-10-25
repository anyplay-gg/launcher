//! The Feed page (docs/DESIGN.md 17, "Feed"): what the player and their friends did, newest first,
//! each item a card with the four reactions under it. The cards are drawn here rather than as
//! rows, because a card has two levels of focus: the card as a unit, and the reactions in it,
//! reached with left and right.
//!
//! The states without items (loading, offline, empty, failed) are ordinary rows.

use anyps5_core::online::api::{AccountId, FeedEvent, FeedItem, Grade, Reaction};
use anyps5_core::online::{self, Ago, Rarity};
use egui::containers::scroll_area::ScrollBarVisibility;
use egui::{Align2, Color32, Context, Margin, Rect, ScrollArea, Sense, Ui, pos2, vec2};

use super::local::{Avatar, Local};
use super::{OnlineAction, OnlineData, OnlinePanel, Page, Row};
use crate::screens::format;
use crate::screens::online_feed_strings::FeedText as F;
use crate::screens::online_strings::OnlineText as T;
use crate::screens::scroll::Follow;
use crate::screens::side::{self, SideDrawn, SideView};
use crate::ui::Shell;
use crate::ui::anim::{AnimExt, Motion};
use crate::ui::chrome::{PanelRow, RowControl};
use crate::ui::components::{self, corner, paint_capsule};
use crate::ui::i18n::{Arg, Language};
use crate::ui::icons::Icon;
use crate::ui::input::{self, Command};
use crate::ui::sound::Cue;
use crate::ui::text;
use crate::ui::theme::{self, TextStyle, Weight, black, color, fade, mix, radius, size, ty, white};

/// Items from the end at which the next older page is fetched.
const PREFETCH: usize = 3;
/// Card padding and the avatar.
const PAD_X: f32 = 16.0;
const PAD_Y: f32 = 14.0;
const AVATAR: f32 = 40.0;
const AVATAR_GAP: f32 = 14.0;
/// The reaction buttons.
const REACTION_HEIGHT: f32 = size::CONTROL_S;
const REACTION_GAP: f32 = 12.0;
const REACTION_PAD_X: f32 = 10.0;
/// Room for the check in front of the label of the player's own reaction.
const CHECK_SLOT: f32 = 15.0;

const HANDLE: TextStyle = ty::ROW_TITLE;
const SENTENCE: TextStyle = TextStyle::new(14.0, Weight::Regular);
const WHEN: TextStyle = ty::DETAIL;
const FACT: TextStyle = TextStyle::new(14.0, Weight::Medium);
const REACTION: TextStyle = TextStyle::new(14.0, Weight::Medium);

/// Where the focus stands in the list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FeedFocus {
    /// The item; one past the last is the "Show Older Activity" row.
    pub item: usize,
    /// `None` is the card as a unit, `Some(n)` its n-th reaction.
    pub slot: Option<usize>,
}

/// What a command does to the focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// The focus moved (or stayed: the sound is `None` then).
    Moved(FeedFocus, bool),
    /// The focus is on this and the command activates it.
    Press(FeedFocus),
    Back,
}

/// Applies `command` to `focus` in a list of `items` cards, with a "Show Older" row after them
/// when `older`. Up and down keep the reaction column, so a run of GGs is down, select, down,
/// select.
pub fn step(focus: FeedFocus, command: Command, items: usize, older: bool) -> Step {
    let last = if older {
        items
    } else {
        items.saturating_sub(1)
    };
    let focus = FeedFocus {
        item: focus.item.min(last),
        slot: focus.slot.filter(|_| focus.item < items),
    };
    let moved = |next: FeedFocus| Step::Moved(next, next != focus);
    match command {
        Command::Up => moved(FeedFocus {
            item: focus.item.saturating_sub(1),
            ..focus
        }),
        Command::Down => {
            let item = (focus.item + 1).min(last);
            moved(FeedFocus {
                item,
                slot: focus.slot.filter(|_| item < items),
            })
        }
        Command::Left => moved(FeedFocus {
            slot: match focus.slot {
                Some(0) | None => None,
                Some(n) => Some(n - 1),
            },
            ..focus
        }),
        Command::Right => moved(FeedFocus {
            slot: if focus.item >= items {
                None
            } else {
                Some(
                    focus
                        .slot
                        .map_or(0, |n| (n + 1).min(Reaction::ALL.len() - 1)),
                )
            },
            ..focus
        }),
        Command::Select => Step::Press(focus),
        Command::Back | Command::Options => Step::Back,
    }
}

/// The feed item's sentence without its subject: "earned a gold trophy in Nebula Drift".
pub fn sentence(
    item: &FeedItem,
    me: Option<AccountId>,
    language: Language,
    local: &Local,
) -> String {
    let own = me == Some(item.account.id);
    let pick = |others: F, yours: F| if own { yours } else { others };
    let turkish = language == Language::Turkish;
    match &item.event {
        FeedEvent::Trophy {
            title_id,
            number,
            grade,
            ..
        } => {
            let grade = grade_word(*grade).text(language);
            let game = local.game_name(title_id);
            let name = local.trophy_name(title_id, *number);
            match (game, name) {
                (Some(game), Some(name)) => {
                    let args = if turkish {
                        [Arg::Str(game), Arg::Str(grade), Arg::Str(name)]
                    } else {
                        [Arg::Str(grade), Arg::Str(name), Arg::Str(game)]
                    };
                    pick(F::TrophyNamed, F::TrophyNamedOwn).format(language, &args)
                }
                (Some(game), None) => {
                    let args = if turkish {
                        [Arg::Str(game), Arg::Str(grade)]
                    } else {
                        [Arg::Str(grade), Arg::Str(game)]
                    };
                    pick(F::Trophy, F::TrophyOwn).format(language, &args)
                }
                (None, _) => pick(F::TrophyUnknownGame, F::TrophyUnknownGameOwn)
                    .format(language, &[Arg::Str(grade)]),
            }
        }
        FeedEvent::Friend { with } => {
            if me == Some(with.id) {
                F::FriendWithYou.text(language).to_owned()
            } else {
                pick(F::Friend, F::FriendOwn).format(language, &[Arg::Str(&with.handle)])
            }
        }
        FeedEvent::FirstPlayed { title_id } => match local.game_name(title_id) {
            Some(game) => {
                pick(F::FirstPlayed, F::FirstPlayedOwn).format(language, &[Arg::Str(game)])
            }
            None => pick(F::FirstPlayedUnknown, F::FirstPlayedUnknownOwn)
                .text(language)
                .to_owned(),
        },
        FeedEvent::Party { members } => {
            let others = online::party_others(&item.account, members);
            if others.is_empty() {
                pick(F::PartyAlone, F::PartyAloneOwn)
                    .text(language)
                    .to_owned()
            } else {
                let handles: Vec<&str> = others.iter().map(|a| a.handle.as_str()).collect();
                pick(F::Party, F::PartyOwn)
                    .format(language, &[Arg::Str(&names(&handles, language))])
            }
        }
    }
}

/// "Nox", "Nox and Rook", "Nox, Rook and Kit", "Nox, Rook and 3 more".
pub fn names(handles: &[&str], language: Language) -> String {
    match handles {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [a, b] => F::ListTwo.format(language, &[Arg::Str(a), Arg::Str(b)]),
        [a, b, c] => F::ListThree.format(language, &[Arg::Str(a), Arg::Str(b), Arg::Str(c)]),
        [a, b, rest @ ..] => F::ListMore.format(
            language,
            &[Arg::Str(a), Arg::Str(b), Arg::Str(&rest.len().to_string())],
        ),
    }
}

/// The launcher's colour of a grade the cloud names.
pub fn theme_grade(grade: Grade) -> theme::Grade {
    match grade {
        Grade::Platinum => theme::Grade::Platinum,
        Grade::Gold => theme::Grade::Gold,
        Grade::Silver => theme::Grade::Silver,
        Grade::Bronze => theme::Grade::Bronze,
    }
}

fn grade_word(grade: Grade) -> F {
    match grade {
        Grade::Platinum => F::GradePlatinum,
        Grade::Gold => F::GradeGold,
        Grade::Silver => F::GradeSilver,
        Grade::Bronze => F::GradeBronze,
    }
}

/// "Rare, 2.3%".
pub fn rarity_text(percent: f32, language: Language) -> String {
    let word = match Rarity::of(percent) {
        Rarity::Extreme => F::RarityExtreme,
        Rarity::Rare => F::RarityRare,
        Rarity::Uncommon => F::RarityUncommon,
        Rarity::Common => F::RarityCommon,
    };
    let mut number = format!("{percent:.1}");
    if language == Language::Turkish {
        number = number.replace('.', ",");
    }
    F::RarityFact.format(
        language,
        &[Arg::Str(word.text(language)), Arg::Str(&number)],
    )
}

/// "Just now", "5 min ago", "Yesterday", "Oct 21".
pub fn ago_text(now: i64, at: u64, utc_offset: i32, language: Language) -> String {
    let count = |n: u64| Arg::Int(i64::try_from(n).unwrap_or(i64::MAX));
    match Ago::of(now, at) {
        Ago::Now => F::AgoNow.text(language).to_owned(),
        Ago::Minutes(n) => F::AgoMinutes.format(language, &[count(n)]),
        Ago::Hours(n) => F::AgoHours.format(language, &[count(n)]),
        Ago::Yesterday => F::AgoYesterday.text(language).to_owned(),
        Ago::Days(n) => F::AgoDays.format(language, &[count(n)]),
        Ago::Older => format::short_date(i64::try_from(at).unwrap_or(0), utc_offset, language),
    }
}

fn reaction_label(reaction: Reaction) -> F {
    match reaction {
        Reaction::Nice => F::ReactNice,
        Reaction::Gg => F::ReactGg,
        Reaction::Wow => F::ReactWow,
        Reaction::Same => F::ReactSame,
    }
}

/// A calm colour for an avatar without a picture, from the account number.
pub fn monogram_colour(account: AccountId) -> Color32 {
    // Each has at least 5:1 with white.
    const COLOURS: [[u8; 3]; 6] = [
        [70, 90, 160],
        [120, 70, 140],
        [40, 120, 110],
        [150, 80, 60],
        [60, 110, 150],
        [110, 110, 50],
    ];
    let [r, g, b] = COLOURS[(account.0 % COLOURS.len() as u64) as usize];
    Color32::from_rgb(r, g, b)
}

/// One card, ready to draw.
struct Card {
    account: AccountId,
    handle: String,
    avatar: Option<Avatar>,
    sentence: String,
    when: String,
    /// "Rare, 2.3%" and the trophy's grade.
    fact: Option<(String, theme::Grade)>,
    counts: [u32; 4],
    mine: Option<Reaction>,
}

fn prepare(item: &FeedItem, data: &OnlineData<'_>, local: &Local) -> Card {
    let language = data.language;
    let me = data.online.account.as_ref().map(|a| a.id);
    let own = me == Some(item.account.id);
    let utc_offset = data.utc_offset.unwrap_or_else(|| {
        format::local_offset_seconds(
            i64::try_from(item.at)
                .unwrap_or(0)
                .saturating_mul(1_000_000),
        )
    });
    let fact = match &item.event {
        FeedEvent::Trophy { grade, rarity, .. } => {
            rarity.map(|percent| (rarity_text(percent, language), theme_grade(*grade)))
        }
        _ => None,
    };
    let hash = data
        .online
        .avatars
        .get(&item.account.id)
        .and_then(|h| h.as_deref());
    Card {
        account: item.account.id,
        handle: if own {
            T::You.text(language).to_owned()
        } else {
            item.account.handle.clone()
        },
        avatar: local.avatar(hash),
        sentence: sentence(item, me, language, local),
        when: ago_text(data.now, item.at, utc_offset, language),
        fact,
        counts: Reaction::ALL.map(|r| item.reactions.of(r)),
        mine: item.mine,
    }
}

/// Whether the next older page should be asked for now: the focus is near the end of the cards,
/// there is an older page, none is on its way, and the last try did not fail (after a refusal the
/// player asks again by hand; asking at every frame would never stop).
pub fn wants_older(
    focus: usize,
    items: usize,
    has_next: bool,
    failed: bool,
    loading: bool,
) -> bool {
    has_next && !failed && !loading && focus + PREFETCH >= items
}

/// What the mouse did in the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mouse {
    Press(FeedFocus),
}

impl OnlinePanel {
    /// The cards show: there are items. Otherwise the page is rows.
    pub(super) fn feed_is_list(&self, data: &OnlineData<'_>) -> bool {
        self.page == Page::Feed && !data.online.feed.items.is_empty()
    }

    /// What the Feed page says while it has no cards: the headline, with an icon and a note,
    /// and nothing at all when the page is just loading.
    pub(super) fn feed_state(&self, data: &OnlineData<'_>) -> (Icon, F, Option<F>) {
        let online = data.online;
        if !online.feed.loaded && online.feed.error.is_none() {
            if online.is_online() {
                (Icon::ARROWS_CLOCKWISE, F::Loading, None)
            } else {
                (Icon::CLOUD, F::Offline, Some(F::OfflineNote))
            }
        } else if online.feed.error.is_some() && !online.feed.loaded {
            (Icon::WARNING, F::LoadFailedTitle, Some(F::LoadFailed))
        } else {
            (Icon::LIST, F::Empty, Some(F::EmptyNote))
        }
    }

    /// The actions under the state block: Try Again after a failure, Add a Friend for a player
    /// who has none.
    pub(super) fn feed_rows(&self, data: &OnlineData<'_>, rows: &mut Vec<Row>) {
        let language = data.language;
        let online = data.online;
        if online.feed.error.is_some() && !online.feed.loaded {
            rows.push(
                Row::command("feedRetry", F::TryAgain.text(language)).icon(Icon::ARROWS_CLOCKWISE),
            );
        } else if online.feed.loaded && online.friends.friends.is_empty() {
            rows.push(Row::command("addFriend", T::AddFriend.text(language)).icon(Icon::USER_PLUS));
        }
    }

    /// Draws the Feed page without cards: the state block over its action rows.
    pub(super) fn feed_state_draw(
        &self,
        ctx: &Context,
        view: &SideView<'_>,
        rows: &mut [Row],
        data: &OnlineData<'_>,
    ) -> Option<SideDrawn> {
        let (icon, title, note) = self.feed_state(data);
        let language = data.language;
        side::draw_with_header(ctx, self.side.is_open(), view, rows, |ui| {
            paint_state(
                ui,
                icon,
                title.text(language),
                note.map(|n| n.text(language)),
            );
        })
    }

    /// A command on the card list. `true` leaves the page.
    pub(super) fn feed_command(
        &mut self,
        shell: &mut Shell,
        command: Command,
        data: &OnlineData<'_>,
        actions: &mut Vec<OnlineAction>,
    ) -> bool {
        let feed = &data.online.feed;
        self.feed_sync(data);
        match step(
            self.feed_focus,
            command,
            feed.items.len(),
            feed.next.is_some(),
        ) {
            Step::Moved(focus, moved) => {
                if moved {
                    shell.play(Cue::Move);
                }
                self.feed_focus = focus;
                self.feed_note_focus(data);
                self.feed_prefetch(data, actions);
                false
            }
            Step::Press(focus) => {
                shell.play(Cue::Select);
                self.feed_press(focus, data, actions);
                false
            }
            Step::Back => true,
        }
    }

    /// What activating the focused thing does.
    pub(super) fn feed_press(
        &mut self,
        focus: FeedFocus,
        data: &OnlineData<'_>,
        actions: &mut Vec<OnlineAction>,
    ) {
        let feed = &data.online.feed;
        let Some(item) = feed.items.get(focus.item) else {
            actions.push(OnlineAction::FeedOlder);
            return;
        };
        match focus.slot.and_then(|n| Reaction::ALL.get(n)) {
            Some(reaction) => {
                actions.push(OnlineAction::React(
                    item.id,
                    online::toggled(item.mine, *reaction),
                ));
            }
            None => self.open_profile(item.account.id),
        }
    }

    /// The list is fetched further when the focus comes near its end.
    fn feed_prefetch(&self, data: &OnlineData<'_>, actions: &mut Vec<OnlineAction>) {
        let feed = &data.online.feed;
        if wants_older(
            self.feed_focus.item,
            feed.items.len(),
            feed.next.is_some(),
            feed.error.is_some(),
            data.online.feed_loading(),
        ) {
            actions.push(OnlineAction::FeedOlder);
        }
    }

    /// Keeps the focus on the same item when items come in above it.
    pub(super) fn feed_sync(&mut self, data: &OnlineData<'_>) {
        let items = &data.online.feed.items;
        if let Some(id) = self.feed_anchor
            && items.get(self.feed_focus.item).map(|i| i.id) != Some(id)
            && let Some(at) = items.iter().position(|i| i.id == id)
        {
            self.feed_focus.item = at;
        }
        let last = items.len() + usize::from(data.online.feed.next.is_some());
        self.feed_focus.item = self.feed_focus.item.min(last.saturating_sub(1));
        self.feed_note_focus(data);
    }

    /// The focus is anchored to its item, except on the newest one: a player resting at the top
    /// sees what arrives above them.
    fn feed_note_focus(&mut self, data: &OnlineData<'_>) {
        self.feed_anchor = data
            .online
            .feed
            .items
            .get(self.feed_focus.item)
            .filter(|_| self.feed_focus.item > 0)
            .map(|i| i.id);
    }

    /// Draws the cards in the side panel; the actions come out of the mouse.
    pub(super) fn feed_draw(
        &mut self,
        ctx: &Context,
        title: &str,
        subtitle: Option<&str>,
        data: &OnlineData<'_>,
        actions: &mut Vec<OnlineAction>,
    ) -> Option<bool> {
        self.feed_sync(data);
        let local = &self.local;
        let feed = &data.online.feed;
        let cards: Vec<Card> = feed
            .items
            .iter()
            .map(|item| prepare(item, data, local))
            .collect();
        let focus = self.side.is_open().then_some(self.feed_focus);
        let loading = data.online.feed_loading();
        let older = feed.next.is_some().then_some(if loading {
            Older::Loading
        } else if feed.error.is_some() {
            Older::Failed
        } else {
            Older::Idle
        });
        let ended = feed.next.is_none() && !cards.is_empty();
        let language = data.language;
        let (scrim, events) =
            crate::screens::side::frame(ctx, self.side.is_open(), title, subtitle, |ui| {
                list(ui, &cards, focus, older, ended, language)
            })?;
        for event in events {
            match event {
                Mouse::Press(focus) => {
                    self.feed_focus = focus;
                    self.feed_note_focus(data);
                    self.feed_press(focus, data, actions);
                }
            }
        }
        self.feed_prefetch(data, actions);
        Some(scrim)
    }
}

/// What the "Show Older Activity" row after the cards says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Older {
    Idle,
    Loading,
    Failed,
}

/// The scrolling list of cards. `older` is `Some` while a "Show Older Activity" row follows the
/// cards.
fn list(
    ui: &mut Ui,
    cards: &[Card],
    focus: Option<FeedFocus>,
    older: Option<Older>,
    ended: bool,
    language: Language,
) -> Vec<Mouse> {
    let ctx = ui.ctx().clone();
    let id = ui.make_persistent_id("feed-cards");
    let (follow, scroll_to) = Follow::begin(&ctx, id, focus.map(|f| f.item));
    let mut events = Vec::new();
    let mut geometry: Vec<(f32, f32)> = Vec::with_capacity(cards.len() + 1);
    let mut area = ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink([false, false])
        .scroll_bar_visibility(ScrollBarVisibility::AlwaysHidden);
    if let Some(offset) = scroll_to {
        area = area.vertical_scroll_offset(offset);
    }
    let overflow_id = id.with("overflows");
    let overflows: bool = ctx.data(|d| d.get_temp(overflow_id)).unwrap_or(false);
    let clip = ui.clip_rect().intersect(ui.max_rect());
    let output = area.show(ui, |ui| {
        egui::Frame::NONE
            .inner_margin(Margin::same(size::LIST_INSET as i8))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = size::ROW_GAP;
                let origin = ui.cursor().min.y;
                for (index, card) in cards.iter().enumerate() {
                    let focused_here = focus.filter(|f| f.item == index);
                    let top = ui.cursor().min.y;
                    let fade_amount = edge_fade(overflows, top, clip);
                    let shown = ui
                        .scope(|ui| {
                            ui.multiply_opacity(fade_amount);
                            show_card(ui, card, index, focused_here, language, &mut events)
                        })
                        .inner;
                    geometry.push((shown.min.y - origin, shown.height()));
                }
                if let Some(older) = older {
                    let index = cards.len();
                    let focused = focus.is_some_and(|f| f.item == index);
                    let detail = match older {
                        Older::Idle => None,
                        Older::Loading => Some(F::Loading.text(language)),
                        Older::Failed => Some(F::OlderFailed.text(language)),
                    };
                    let mut row = PanelRow::new(F::ShowOlder.text(language), RowControl::Command)
                        .language(language)
                        .icon(Icon::CLOCK_COUNTER_CLOCKWISE)
                        .focused(focused);
                    if let Some(detail) = detail {
                        row = row.detail(detail);
                    }
                    let out = ui.push_id("feed-older", |ui| row.show(ui)).inner;
                    if out.response.clicked() {
                        events.push(Mouse::Press(FeedFocus {
                            item: index,
                            slot: None,
                        }));
                    }
                    geometry.push((out.response.rect.min.y - origin, out.response.rect.height()));
                } else if ended {
                    PanelRow::new(
                        F::EndOfFeed.text(language),
                        RowControl::Info { value: None },
                    )
                    .language(language)
                    .show(ui);
                }
            });
    });
    let overflowing = output.content_size.y > output.inner_rect.height() + 1.0;
    ctx.data_mut(|d| d.insert_temp(overflow_id, overflowing));
    follow.end(
        &ctx,
        geometry,
        output.content_size.y,
        output.inner_rect.height(),
        output.state.clamped_offset().y,
    );
    events
}

/// Cards fade out over their first 28 dp at a clipped edge of a list that scrolls, as rows do.
fn edge_fade(overflows: bool, top: f32, clip: Rect) -> f32 {
    if !overflows {
        return 1.0;
    }
    let centre = top + size::ROW_MIN_HEIGHT * 0.5;
    ((centre - clip.min.y).min(clip.max.y - centre) / 28.0).clamp(0.0, 1.0)
}

/// Draws one card and returns its rectangle.
fn show_card(
    ui: &mut Ui,
    card: &Card,
    index: usize,
    focus: Option<FeedFocus>,
    language: Language,
    events: &mut Vec<Mouse>,
) -> Rect {
    let ctx = ui.ctx().clone();
    let painter = ui.painter().clone();
    let width = ui.available_width();
    let text_x = PAD_X + AVATAR + AVATAR_GAP;
    let text_width = width - text_x - PAD_X;

    // Measure.
    let when = text::layout(
        &painter,
        WHEN,
        &card.when,
        Color32::PLACEHOLDER,
        f32::INFINITY,
    );
    let handle_width = (text_width - when.size().x - 12.0).max(40.0);
    let handle = {
        let mut job = egui::text::LayoutJob::simple(
            card.handle.clone(),
            HANDLE.font(),
            Color32::PLACEHOLDER,
            handle_width,
        );
        job.wrap.max_rows = 1;
        job.sections[0].format.line_height = Some(HANDLE.line_height());
        painter.layout_job(job)
    };
    let sentence = {
        let mut job = egui::text::LayoutJob::simple(
            card.sentence.clone(),
            SENTENCE.font(),
            Color32::PLACEHOLDER,
            text_width,
        );
        job.wrap.max_rows = 3;
        // Text of up to three lines needs more air than a single line of a row.
        job.sections[0].format.line_height = Some(1.4 * SENTENCE.size);
        painter.layout_job(job)
    };
    let fact_height = if card.fact.is_some() { 6.0 + 20.0 } else { 0.0 };
    let top_block = handle.size().y + 3.0 + sentence.size().y + fact_height;
    let height = PAD_Y + top_block.max(AVATAR) + 12.0 + REACTION_HEIGHT + PAD_Y;

    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let card_focused = focus.is_some();
    crate::ui::a11y::follow(&ctx, response.id, focus.is_some_and(|f| f.slot.is_none()));
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Button,
            true,
            format!(
                "{} {}, {}{}",
                card.handle,
                card.sentence,
                card.when,
                card.fact
                    .as_ref()
                    .map_or(String::new(), |(fact, _)| format!(", {fact}")),
            ),
        )
    });
    if response.clicked() {
        events.push(Mouse::Press(FeedFocus {
            item: index,
            slot: None,
        }));
    }
    let hovered = input::hovered(&ctx, &response);
    if hovered {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let t = ctx.focus_t((response.id, "card"), card_focused);
    if !ui.is_rect_visible(rect) {
        return rect;
    }

    // The card is a row: lighter under the pointer, lighter with the focus, and the ring when the
    // card itself (not one of its reactions) has it.
    let hover_t = ctx.animate_to((response.id, "hover"), f32::from(hovered), Motion::Hover);
    let bg = rect;
    components::paint_surface(
        &painter,
        bg,
        radius::ROW,
        color::surface_row(),
        hover_t,
        t,
        0.0,
        if focus.is_some_and(|f| f.slot.is_some()) {
            0.0
        } else {
            size::FOCUS_RING
        },
    );
    let fg = Color32::WHITE;
    let dim = |factor: f32| fade(fg, factor);

    // The avatar.
    let avatar = Rect::from_min_size(rect.min + vec2(PAD_X, PAD_Y), vec2(AVATAR, AVATAR));
    paint_avatar(&painter, avatar, card.avatar, card.account, &card.handle);

    // The words.
    let x = rect.min.x + text_x;
    let mut y = rect.min.y + PAD_Y;
    let handle_height = handle.size().y;
    painter.galley(pos2(x, y), handle, fg);
    painter.galley(
        pos2(
            rect.max.x - PAD_X - when.size().x,
            y + (handle_height - when.size().y) * 0.5,
        ),
        when,
        dim(0.55),
    );
    y += handle_height + 3.0;
    let sentence_height = sentence.size().y;
    painter.galley(pos2(x, y), sentence, dim(0.85));
    y += sentence_height;
    if let Some((fact, grade)) = &card.fact {
        y += 6.0;
        let ink = mix(grade.color(), fg, t * 0.8);
        painter.text(
            pos2(x + 8.0, y + 10.0),
            Align2::CENTER_CENTER,
            Icon::TROPHY.glyph(),
            Icon::TROPHY.font(14.0, true),
            ink,
        );
        painter.text(
            pos2(x + 24.0, y + 10.0),
            Align2::LEFT_CENTER,
            fact,
            FACT.font(),
            dim(0.72),
        );
    }

    // The reactions, left-aligned under the words.
    let row_top = rect.max.y - PAD_Y - REACTION_HEIGHT;
    let mut left = x;
    // One width for all four, the same on every card, so the columns line up and the focus can
    // keep its column; never wider than the room.
    let widest = Reaction::ALL
        .iter()
        .map(|r| {
            text::width(
                &painter,
                REACTION,
                &format!("{} 99", reaction_label(*r).text(language)),
            )
        })
        .fold(0.0, f32::max);
    let uniform =
        (widest + 2.0 * REACTION_PAD_X + CHECK_SLOT).min((text_width - 3.0 * REACTION_GAP) / 4.0);
    for (n, reaction) in Reaction::ALL.into_iter().enumerate() {
        let label = reaction_label(reaction).text(language);
        let count = card.counts[n];
        let own = card.mine == Some(reaction);
        let caption = if count > 99 {
            format!("{label} 99+")
        } else if count > 0 {
            format!("{label} {count}")
        } else {
            label.to_owned()
        };
        let galley = text::layout_font(
            &painter,
            REACTION.font(),
            REACTION.line_height(),
            0.0,
            &caption,
            Color32::PLACEHOLDER,
            f32::INFINITY,
        );
        let check = if own { CHECK_SLOT } else { 0.0 };
        let pill_width = uniform.max(galley.size().x + 2.0 * REACTION_PAD_X + check);
        let pill = Rect::from_min_size(pos2(left, row_top), vec2(pill_width, REACTION_HEIGHT));
        left += pill_width + REACTION_GAP;
        let slot_focused = focus.is_some_and(|f| f.slot == Some(n));
        let pill_response = ui.interact(pill, response.id.with(("reaction", n)), Sense::click());
        pill_response.widget_info(|| {
            let name = if own {
                F::ReactionMine.format(language, &[Arg::Str(label), Arg::Int(i64::from(count))])
            } else {
                F::ReactionCount.format(language, &[Arg::Str(label), Arg::Int(i64::from(count))])
            };
            egui::WidgetInfo::selected(egui::Role::Button, true, own, name)
        });
        if pill_response.clicked() {
            events.push(Mouse::Press(FeedFocus {
                item: index,
                slot: Some(n),
            }));
        }
        crate::ui::a11y::follow(&ctx, pill_response.id, slot_focused);
        let pt = ctx.focus_t((response.id, "pill", n), slot_focused);
        // Like every control: the pointer lights a pill only while it is the device in use.
        let pill_hover = input::hovered(&ctx, &pill_response);
        let pill_down = pill_hover && pill_response.is_pointer_button_down_on();
        if pill_hover {
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        // At rest a pill is a quiet shape of the card's own ink; yours is stronger and carries a
        // check, so it never rests on colour alone; the focused one has the ring.
        let rest_fill = if pill_down {
            dim(0.34)
        } else if own {
            dim(0.24)
        } else if pill_hover {
            dim(0.18)
        } else {
            dim(0.10)
        };
        let fill = mix(rest_fill, dim(0.34), pt);
        paint_capsule(&painter, pill, fill);
        components::paint_focus_ring(&painter, pill, REACTION_HEIGHT * 0.5, pt);
        let ink = dim(if own { 1.0 } else { 0.88 });
        // The label and the check sit in the middle of the pill.
        let content = galley.size().x + check;
        let mut text_x = pill.center().x - content * 0.5;
        if own {
            painter.text(
                pos2(text_x + 6.0, pill.center().y),
                Align2::CENTER_CENTER,
                Icon::CHECK.glyph(),
                Icon::CHECK.font(13.0, false),
                ink,
            );
            text_x += check;
        }
        let galley_height = galley.size().y;
        painter.galley(
            pos2(text_x, pill.center().y - galley_height * 0.5),
            galley,
            ink,
        );
    }
    rect
}

/// The headline of a page without cards: an icon on a quiet disc, the headline and a note, in
/// the middle of the room the rows below leave.
fn paint_state(ui: &mut Ui, icon: Icon, title: &str, note: Option<&str>) {
    let painter = ui.painter().clone();
    let width = ui.available_width();
    let wrap = (width - 2.0 * 24.0).min(380.0);
    let title_galley = text::layout(&painter, ty::INFO_VALUE, title, Color32::PLACEHOLDER, wrap);
    let note_galley = note.map(|note| {
        let mut job = egui::text::LayoutJob::simple(
            note.to_owned(),
            ty::NOTE.font(),
            Color32::PLACEHOLDER,
            wrap,
        );
        job.halign = egui::Align::Center;
        job.sections[0].format.line_height = Some(1.4 * ty::NOTE.size);
        painter.layout_job(job)
    });
    let disc = 56.0;
    let height = disc
        + 16.0
        + title_galley.size().y
        + note_galley.as_ref().map_or(0.0, |n| 6.0 + n.size().y);
    let top_space = (ui.available_height() * 0.5 - height * 0.5 - 40.0).clamp(24.0, 160.0);
    ui.add_space(top_space);
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, title));
    if !ui.is_rect_visible(rect) {
        return;
    }
    let centre = pos2(rect.center().x, rect.min.y + disc * 0.5);
    painter.circle_filled(centre, disc * 0.5, color::surface_row());
    painter.text(
        centre,
        Align2::CENTER_CENTER,
        icon.glyph(),
        icon.font(24.0, false),
        white(0.75),
    );
    let mut y = rect.min.y + disc + 16.0;
    let title_height = title_galley.size().y;
    painter.galley(
        pos2(rect.center().x - title_galley.size().x * 0.5, y),
        title_galley,
        Color32::WHITE,
    );
    y += title_height + 6.0;
    if let Some(note) = note_galley {
        painter.galley(pos2(rect.center().x, y), note, color::text_detail());
    }
    ui.add_space(12.0);
}

/// A round picture, or the first letter of the handle on a calm colour.
pub fn paint_avatar(
    painter: &egui::Painter,
    rect: Rect,
    avatar: Option<Avatar>,
    account: AccountId,
    handle: &str,
) {
    let r = rect.width() * 0.5;
    match avatar {
        Some(avatar) => {
            // A radius of exactly half draws egui's doubled-vertex seam on a snapped square (see
            // `paint_capsule`); one dp less keeps a straight sliver and the same round look.
            painter.add(
                egui::epaint::RectShape::filled(rect, corner(r - 1.0), Color32::WHITE)
                    .with_texture(
                        avatar.texture,
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    ),
            );
            // A faint edge keeps a dark picture apart from a dark card.
            painter.circle_stroke(rect.center(), r - 0.5, egui::Stroke::new(1.0, black(0.18)));
        }
        None => {
            painter.circle_filled(rect.center(), r, monogram_colour(account));
            let initial = handle.chars().next().map(|c| c.to_uppercase().to_string());
            if let Some(initial) = initial {
                painter.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    initial,
                    ty::BUTTON.font(),
                    crate::ui::theme::readable_ink(monogram_colour(account)),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use anyps5_core::online::api::{Account, ReactionCounts};

    use super::*;

    fn who(id: u64, handle: &str) -> Account {
        Account {
            id: AccountId(id),
            handle: handle.into(),
        }
    }

    fn item(event: FeedEvent) -> FeedItem {
        FeedItem {
            id: 1,
            account: who(2, "Lumen"),
            at: 0,
            event,
            reactions: ReactionCounts::default(),
            mine: None,
        }
    }

    fn local() -> Local {
        let mut local = Local::default();
        local.games.insert(
            "GAME00001".into(),
            super::super::local::OwnedGame {
                name: "Nebula Drift".into(),
                icon: None,
            },
        );
        local
            .trophy_names
            .insert(("GAME00001".into(), 4), "First Light".into());
        local
    }

    fn trophy(number: u16, title_id: &str) -> FeedEvent {
        FeedEvent::Trophy {
            title_id: title_id.into(),
            number,
            grade: Grade::Gold,
            rarity: Some(2.3),
        }
    }

    const EN: Language = Language::English;

    #[test]
    fn a_trophy_is_named_only_when_the_player_can_read_it() {
        let me = Some(AccountId(1));
        assert_eq!(
            sentence(&item(trophy(4, "GAME00001")), me, EN, &local()),
            "earned the gold trophy “First Light” in Nebula Drift"
        );
        // The game is owned but the trophy is not readable (a hidden one the player has not got).
        assert_eq!(
            sentence(&item(trophy(5, "GAME00001")), me, EN, &local()),
            "earned a gold trophy in Nebula Drift"
        );
        // Not owned: no game, so no name either.
        assert_eq!(
            sentence(&item(trophy(4, "GAME00002")), me, EN, &local()),
            "earned a gold trophy in a game"
        );
    }

    #[test]
    fn each_kind_has_a_sentence() {
        let me = Some(AccountId(1));
        let friend = item(FeedEvent::Friend {
            with: who(3, "Nox"),
        });
        assert_eq!(
            sentence(&friend, me, EN, &local()),
            "became friends with Nox"
        );
        let with_me = item(FeedEvent::Friend {
            with: who(1, "Mira"),
        });
        assert_eq!(
            sentence(&with_me, me, EN, &local()),
            "became friends with you"
        );
        assert_eq!(
            sentence(
                &item(FeedEvent::FirstPlayed {
                    title_id: "GAME00001".into()
                }),
                me,
                EN,
                &local()
            ),
            "played Nebula Drift for the first time"
        );
        assert_eq!(
            sentence(
                &item(FeedEvent::FirstPlayed {
                    title_id: "GAME00009".into()
                }),
                me,
                EN,
                &local()
            ),
            "played a game for the first time"
        );
        let party = item(FeedEvent::Party {
            members: vec![who(2, "Lumen"), who(3, "Nox"), who(4, "Rook")],
        });
        assert_eq!(
            sentence(&party, me, EN, &local()),
            "started a party with Nox and Rook"
        );
        let alone = item(FeedEvent::Party {
            members: vec![who(2, "Lumen")],
        });
        assert_eq!(sentence(&alone, me, EN, &local()), "started a party");
    }

    #[test]
    fn turkish_puts_the_game_first_and_knows_who_you_are() {
        let tr = Language::Turkish;
        let mut other = item(trophy(5, "GAME00001"));
        assert_eq!(
            sentence(&other, Some(AccountId(1)), tr, &local()),
            "Nebula Drift oyununda altın kupa kazandı"
        );
        other.account = who(1, "Mira");
        assert_eq!(
            sentence(&other, Some(AccountId(1)), tr, &local()),
            "Nebula Drift oyununda altın kupa kazandın"
        );
        assert_eq!(
            sentence(&item(trophy(4, "GAME00001")), None, tr, &local()),
            "Nebula Drift oyununda altın kupa kazandı: “First Light”"
        );
    }

    #[test]
    fn names_are_listed_in_words() {
        assert_eq!(names(&[], EN), "");
        assert_eq!(names(&["Nox"], EN), "Nox");
        assert_eq!(names(&["Nox", "Rook"], EN), "Nox and Rook");
        assert_eq!(names(&["Nox", "Rook", "Kit"], EN), "Nox, Rook and Kit");
        assert_eq!(
            names(&["Nox", "Rook", "Kit", "Ash", "Vale"], EN),
            "Nox, Rook and 3 more"
        );
    }

    #[test]
    fn rarity_reads_like_the_example() {
        assert_eq!(rarity_text(2.3, EN), "Rare, 2.3%");
        assert_eq!(rarity_text(0.4, EN), "Extremely rare, 0.4%");
        assert_eq!(rarity_text(61.0, EN), "Common, 61.0%");
        assert_eq!(rarity_text(2.3, Language::Turkish), "Nadir, %2,3");
    }

    #[test]
    fn time_is_relative_up_to_a_week() {
        let now = 1_792_670_400;
        let at = |seconds: i64| ago_text(now, (now - seconds) as u64, 0, EN);
        assert_eq!(at(10), "Just now");
        assert_eq!(at(5 * 60), "5 min ago");
        assert_eq!(at(3 * 3_600), "3 h ago");
        assert_eq!(at(30 * 3_600), "Yesterday");
        assert_eq!(at(3 * 86_400), "3 d ago");
        assert_eq!(at(10 * 86_400), "Oct 12");
    }

    #[test]
    fn the_focus_walks_the_cards_and_their_reactions() {
        let f = |item, slot| FeedFocus { item, slot };
        let go = |focus, command| match step(focus, command, 3, true) {
            Step::Moved(focus, _) => focus,
            other => panic!("{other:?}"),
        };
        assert_eq!(go(f(0, None), Command::Right), f(0, Some(0)));
        assert_eq!(go(f(0, Some(0)), Command::Right), f(0, Some(1)));
        assert_eq!(go(f(0, Some(3)), Command::Right), f(0, Some(3)), "no wrap");
        assert_eq!(go(f(0, Some(1)), Command::Left), f(0, Some(0)));
        assert_eq!(go(f(0, Some(0)), Command::Left), f(0, None));
        assert_eq!(go(f(0, None), Command::Left), f(0, None));
        // Down keeps the reaction; up does too.
        assert_eq!(go(f(0, Some(2)), Command::Down), f(1, Some(2)));
        assert_eq!(go(f(1, Some(2)), Command::Up), f(0, Some(2)));
        assert_eq!(go(f(0, None), Command::Up), f(0, None));
        // The "Show Older" row has no reactions.
        assert_eq!(go(f(2, Some(1)), Command::Down), f(3, None));
        assert_eq!(go(f(3, None), Command::Right), f(3, None));
        assert_eq!(go(f(3, None), Command::Down), f(3, None));
    }

    #[test]
    fn without_older_items_the_list_ends_at_the_last_card() {
        let f = |item, slot| FeedFocus { item, slot };
        assert_eq!(
            step(f(2, None), Command::Down, 3, false),
            Step::Moved(f(2, None), false)
        );
    }

    #[test]
    fn older_items_are_asked_for_near_the_end_and_never_in_a_loop() {
        assert!(wants_older(8, 10, true, false, false));
        assert!(!wants_older(2, 10, true, false, false), "far from the end");
        assert!(!wants_older(9, 10, false, false, false), "nothing older");
        assert!(!wants_older(9, 10, true, false, true), "one is on its way");
        assert!(
            !wants_older(9, 10, true, true, false),
            "the last try failed"
        );
    }

    #[test]
    fn select_presses_and_back_leaves() {
        let f = FeedFocus {
            item: 1,
            slot: Some(2),
        };
        assert_eq!(step(f, Command::Select, 3, false), Step::Press(f));
        assert_eq!(step(f, Command::Back, 3, false), Step::Back);
        assert_eq!(step(f, Command::Options, 3, false), Step::Back);
    }
}
