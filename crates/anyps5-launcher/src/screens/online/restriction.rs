//! What limits the account, in the panel: a row that says so where it matters (Home, Account, the
//! blocked action itself), and the page with what it means, the moderator's words and how to
//! appeal. The state is `anyps5_core::online::Restrictions`; this only shows it.

use anyps5_core::online::control::Restriction;
use anyps5_core::online::{Blocked, Purpose, plain};

use super::{OnlineAction, OnlineData, OnlinePanel, Page};
use crate::screens::online_restriction_strings::{self as words, RestrictionText as R, can_appeal};
use crate::screens::online_strings::OnlineText as T;
use crate::screens::rows::Row;
use crate::ui::i18n::Arg;
use crate::ui::icons::Icon;

/// The longest appeal the field takes, in characters. The cloud allows 1,000; the field keeps one
/// more so that the Send row can say it is too long.
const MAX_APPEAL: usize = 1000;

fn now(data: &OnlineData<'_>) -> u64 {
    u64::try_from(data.now).unwrap_or(0)
}

/// The row that tells of a restriction and opens its page. Its detail says until when.
pub(super) fn summary_row(
    id: &'static str,
    restriction: &Restriction,
    data: &OnlineData<'_>,
) -> Row {
    Row::action(id, words::title(restriction, data.language))
        .icon(Icon::WARNING)
        .detail(words::until_line(
            restriction.until,
            data.utc_offset,
            data.language,
        ))
}

/// The row that stands where `what` would be, while a restriction stops it.
pub(super) fn paused_row(
    id: &'static str,
    what: Blocked,
    restriction: &Restriction,
    data: &OnlineData<'_>,
) -> Row {
    Row::action(id, words::paused_title(what, data.language))
        .icon(Icon::WARNING)
        .detail(words::until_line(
            restriction.until,
            data.utc_offset,
            data.language,
        ))
}

impl OnlinePanel {
    /// The restrictions in force, as rows at the top of a page: none when nothing limits the
    /// account.
    pub(super) fn restriction_rows_for_page(&self, data: &OnlineData<'_>, rows: &mut Vec<Row>) {
        let active = data.online.restrictions.active(now(data));
        if active.is_empty() {
            return;
        }
        rows.push(Row::caption(
            "restrictionsCaption",
            R::Restrictions.text(data.language),
        ));
        for restriction in active {
            rows.push(summary_row("restriction", restriction, data));
        }
    }

    /// The Start page tells of a ban, which signed the account out.
    pub(super) fn ban_rows(&self, data: &OnlineData<'_>, rows: &mut Vec<Row>) {
        if let Some(ban) = data.online.restrictions.banned() {
            rows.push(summary_row("restriction", ban, data));
        }
    }

    /// The restriction a page is about, if it is still in force.
    fn restriction_of<'a>(&self, data: &'a OnlineData<'_>) -> Option<&'a Restriction> {
        let Page::Restriction(kind) = &self.page else {
            return None;
        };
        data.online.restrictions.of_kind(kind, now(data))
    }

    /// The title, subtitle and rows of the restriction page; `None` for any other page.
    pub(super) fn restriction_page_rows(
        &self,
        data: &OnlineData<'_>,
        rows: &mut Vec<Row>,
    ) -> Option<(String, Option<String>)> {
        let Page::Restriction(_) = &self.page else {
            return None;
        };
        let language = data.language;
        let Some(restriction) = self.restriction_of(data) else {
            return Some((R::Restrictions.text(language).to_owned(), None));
        };
        rows.push(Row::caption(
            "meansCaption",
            R::WhatThisMeans.text(language),
        ));
        rows.push(Row::info(
            "means",
            words::explanation(restriction, data.utc_offset, language),
            None,
        ));
        if restriction.rule.is_some() || restriction.reason.is_some() {
            rows.push(Row::caption("reasonCaption", R::Reason.text(language)).group_start());
            if let Some(rule) = &restriction.rule {
                rows.push(Row::info(
                    "rule",
                    R::Rule.text(language),
                    Some(words::rule_name(rule, language)),
                ));
            }
            if let Some(reason) = restriction.reason.as_deref().filter(|r| !r.is_empty()) {
                rows.push(Row::caption("noteCaption", R::ModeratorNote.text(language)));
                rows.push(Row::info("reason", plain(reason), None));
            }
        }
        self.appeal_rows(data, restriction, rows);
        Some((
            words::title(restriction, language).to_owned(),
            Some(words::until_line(
                restriction.until,
                data.utc_offset,
                language,
            )),
        ))
    }

    fn appeal_rows(&self, data: &OnlineData<'_>, restriction: &Restriction, rows: &mut Vec<Row>) {
        let language = data.language;
        let banned = restriction.kind == Restriction::BANNED;
        let Some(appeal) = restriction
            .appeal
            .as_ref()
            .filter(|_| restriction.kind == Restriction::MUTED || banned)
        else {
            if banned {
                // No statement was given: the browser flow is where the reason and the appeal are.
                self.browser_appeal_rows(data, rows);
            }
            return;
        };
        rows.push(Row::caption("appealCaption", R::Appeal.text(language)).group_start());
        let deadline = || words::when(appeal.deadline, data.utc_offset, language);
        match appeal.status.as_str() {
            "available" if can_appeal(restriction, now(data)) => {
                if banned {
                    self.browser_appeal_rows(data, rows);
                    return;
                }
                rows.push(Row::info(
                    "appealNote",
                    R::AppealAvailable.format(language, &[Arg::Str(&deadline())]),
                    None,
                ));
                rows.push(
                    Row::text(
                        "appeal",
                        R::AppealField.text(language),
                        self.appeal_text.clone(),
                    )
                    .wide(),
                );
                let mut send = Row::command("sendAppeal", R::SendAppeal.text(language))
                    .icon(Icon::PAPER_PLANE_RIGHT);
                if data.online.is_waiting(|p| matches!(p, Purpose::Appeal(_))) {
                    send = send.detail(T::Sending.text(language));
                } else if self.appeal_text.chars().count() > MAX_APPEAL {
                    send = send.detail(R::AppealTooLong.text(language));
                }
                rows.push(send);
            }
            "open" => rows.push(Row::info("appealNote", R::AppealOpen.text(language), None)),
            "upheld" => rows.push(Row::info(
                "appealNote",
                R::AppealUpheld.text(language),
                None,
            )),
            _ => rows.push(Row::info(
                "appealNote",
                R::AppealExpired.format(language, &[Arg::Str(&deadline())]),
                None,
            )),
        }
    }

    /// A ban is appealed on the account web: signing in there ends on the page with the reason
    /// and the form.
    fn browser_appeal_rows(&self, data: &OnlineData<'_>, rows: &mut Vec<Row>) {
        let language = data.language;
        if rows.last().is_none_or(|r| r.id != "appealCaption") {
            rows.push(Row::caption("appealCaption", R::Appeal.text(language)).group_start());
        }
        let mut open =
            Row::command("appealInBrowser", R::AppealInBrowser.text(language)).icon(Icon::GLOBE);
        if data
            .online
            .is_waiting(|p| matches!(p, Purpose::BrowserSignIn { .. }))
        {
            open = open.detail(T::Sending.text(language));
        }
        rows.push(open);
        rows.push(Row::info(
            "appealBrowserNote",
            R::AppealInBrowserNote.text(language),
            None,
        ));
    }

    /// What the restriction rows and the page do. True when the row was one of ours.
    pub(super) fn restriction_run(
        &mut self,
        row: &str,
        data: &OnlineData<'_>,
        actions: &mut Vec<OnlineAction>,
    ) -> bool {
        let at = now(data);
        match (row, self.page.clone()) {
            ("restriction", Page::Start) => {
                if let Some(ban) = data.online.restrictions.banned() {
                    self.go(Page::Restriction(ban.kind.clone()), data);
                }
            }
            ("restriction", _) => {
                let index = self.nth(data, "restriction");
                let kind = data
                    .online
                    .restrictions
                    .active(at)
                    .get(index)
                    .map(|r| r.kind.clone());
                if let Some(kind) = kind {
                    self.go(Page::Restriction(kind), data);
                }
            }
            ("pausedFriend", _) => self.open_blocking(Blocked::FriendRequest, data),
            ("pausedSend", Page::Conversation(id)) => {
                let what = match data.online.social.get(&id) {
                    Some(c) if data.online.is_stranger(c) => Blocked::Stranger,
                    _ => Blocked::Message,
                };
                self.open_blocking(what, data);
            }
            ("sendAppeal", Page::Restriction(_)) => {
                let text = self.appeal_text.trim().to_owned();
                let action = self.restriction_of(data).and_then(|r| r.action);
                if let Some(action) = action
                    && !text.is_empty()
                    && text.chars().count() <= MAX_APPEAL
                {
                    actions.push(OnlineAction::Appeal { action, text });
                }
            }
            ("appealInBrowser", Page::Restriction(_)) => {
                actions.push(OnlineAction::BrowserSignIn { create: false });
            }
            _ => return false,
        }
        true
    }

    /// Opens the page of the restriction that stops `what`.
    fn open_blocking(&mut self, what: Blocked, data: &OnlineData<'_>) {
        let kind = data
            .online
            .restrictions
            .blocking(what, now(data))
            .map(|r| r.kind.clone());
        if let Some(kind) = kind {
            self.go(Page::Restriction(kind), data);
        }
    }

    /// The page to show instead when a restriction page's restriction is over, or `None`.
    pub(super) fn restriction_follow(&self, data: &OnlineData<'_>) -> Option<Page> {
        let Page::Restriction(kind) = &self.page else {
            return None;
        };
        if data.online.restrictions.of_kind(kind, now(data)).is_some() {
            return None;
        }
        Some(if data.online.is_signed_in() {
            match &self.previous {
                Some(page @ (Page::Home | Page::Account | Page::AddFriend)) => page.clone(),
                _ => Page::Account,
            }
        } else {
            Page::Start
        })
    }

    /// The page Back leads to from a restriction page.
    pub(super) fn restriction_back(&self, data: &OnlineData<'_>) -> Page {
        match &self.previous {
            Some(page) if !matches!(page, Page::Restriction(_)) && data.online.is_signed_in() => {
                page.clone()
            }
            _ if data.online.is_signed_in() => Page::Account,
            _ => Page::Start,
        }
    }

    /// Nothing typed for an appeal survives a visit to another restriction.
    pub(super) fn clear_appeal(&mut self) {
        self.appeal_text.clear();
    }
}

#[cfg(test)]
mod tests {
    use anyps5_core::online::api::{
        Account, AccountId, Conversation, ConversationKind, Friend, Presence,
    };
    use anyps5_core::online::control::Appeal;
    use anyps5_core::online::{Cloud, Online, SignedIn, Status};
    use egui::Color32;

    use super::*;
    use crate::ui::i18n::Language;

    // 2026-10-22 12:00 UTC; the mute below ends three days later.
    const NOW: i64 = 1_792_670_400;
    const END: u64 = 1_792_929_600;

    fn account(id: u64, handle: &str) -> Account {
        Account {
            id: AccountId(id),
            handle: handle.into(),
        }
    }

    fn signed_in() -> Online {
        let mut online = Online::default();
        online.status = Status::Running;
        online.cloud = Some(Cloud::Online);
        online.account = Some(SignedIn {
            id: AccountId(1),
            handle: "Mira".into(),
            email: "mira@example.org".into(),
        });
        online.friends.friends = vec![Friend {
            account: account(2, "Lumen"),
            since: 0,
            presence: Presence {
                online: true,
                activity: None,
                revision: 0,
            },
        }];
        online
    }

    fn data(online: &Online) -> OnlineData<'_> {
        data_at(online, NOW)
    }

    fn data_at(online: &Online, now: i64) -> OnlineData<'_> {
        OnlineData {
            online,
            cloud: Some("https://cloud.example.org"),
            custom_cloud: false,
            official_cloud: true,
            relay_only: false,
            language: Language::English,
            accent: Color32::WHITE,
            now,
            utc_offset: Some(0),
        }
    }

    fn mute() -> Restriction {
        Restriction {
            rule: Some("spam".into()),
            reason: Some("Links in every message.".into()),
            action: Some(41),
            appeal: Some(Appeal {
                status: "available".into(),
                deadline: END + 86_400,
            }),
            ..Restriction::new(Restriction::MUTED, Some(END))
        }
    }

    fn muted() -> Online {
        let mut online = signed_in();
        online.restrictions.list(vec![mute()]);
        online
    }

    fn ids(rows: &[Row]) -> Vec<&'static str> {
        rows.iter().map(|r| r.id).collect()
    }

    fn chat(with: (u64, &str)) -> Conversation {
        Conversation {
            id: "c".into(),
            kind: ConversationKind::Direct,
            name: None,
            members: vec![account(1, "Mira"), account(with.0, with.1)],
            last: 0,
            read: 0,
            marks: Vec::new(),
            updated: 0,
            request: false,
            party: None,
        }
    }

    #[test]
    fn a_mute_shows_first_on_home_and_on_the_account_page() {
        let online = muted();
        let mut panel = OnlinePanel::default();
        panel.go(Page::Home, &data(&online));
        let (_, _, rows) = panel.rows(&data(&online));
        assert_eq!(ids(&rows)[..2], ["restrictionsCaption", "restriction"]);
        assert_eq!(rows[1].title, "Messages and Friend Requests Paused");
        assert_eq!(
            rows[1].detail.as_deref(),
            Some("Until Oct 25, 2026 at 12:00 PM")
        );

        panel.go(Page::Account, &data(&online));
        let rows = panel.rows(&data(&online)).2;
        assert_eq!(
            ids(&rows)[..3],
            ["restrictionsCaption", "restriction", "manageAccount"]
        );
        assert!(
            rows[2].group_start,
            "the restrictions are a group of their own"
        );
    }

    #[test]
    fn when_nothing_limits_the_account_nothing_shows() {
        let online = signed_in();
        let mut panel = OnlinePanel::default();
        panel.go(Page::Account, &data(&online));
        assert_eq!(panel.rows(&data(&online)).2[0].id, "manageAccount");
        // A mute that has run out is gone without waiting for the service.
        let muted = muted();
        let later = data_at(&muted, END as i64);
        assert_eq!(panel.rows(&later).2[0].id, "manageAccount");
    }

    #[test]
    fn the_friend_request_row_is_replaced_by_what_stops_it() {
        let online = muted();
        let mut panel = OnlinePanel::default();
        panel.go(Page::AddFriend, &data(&online));
        let rows = panel.rows(&data(&online)).2;
        assert_eq!(ids(&rows), ["pausedFriend", "hint"]);
        assert_eq!(rows[0].title, "Friend Requests Paused");
        assert_eq!(
            rows[0].detail.as_deref(),
            Some("Until Oct 25, 2026 at 12:00 PM")
        );

        let mut actions = Vec::new();
        panel.run("pausedFriend", &data(&online), &mut actions);
        assert_eq!(panel.page, Page::Restriction("muted".into()));
        assert!(actions.is_empty(), "opening the page sends nothing");

        let free = signed_in();
        panel.go(Page::AddFriend, &data(&free));
        assert_eq!(
            ids(&panel.rows(&data(&free)).2),
            ["friend", "sendRequest", "hint"]
        );
    }

    #[test]
    fn a_new_accounts_cap_on_friend_requests_shows_where_it_was_met() {
        let mut online = signed_in();
        online.restrictions.learn(
            Some(Blocked::FriendRequest),
            &Restriction::new(Restriction::NEW_ACCOUNT, Some(END)),
        );
        let mut panel = OnlinePanel::default();
        panel.go(Page::AddFriend, &data(&online));
        assert_eq!(ids(&panel.rows(&data(&online)).2), ["pausedFriend", "hint"]);
        panel.run("pausedFriend", &data(&online), &mut Vec::new());
        assert_eq!(panel.page, Page::Restriction("new_account".into()));
        let rows = panel.rows(&data(&online)).2;
        assert_eq!(rows[1].id, "means");
        assert!(
            rows[1]
                .title
                .contains("This opens up on Oct 25, 2026 at 12:00 PM.")
        );
        assert!(rows[1].title.contains("Messages to friends always work."));
        assert!(
            !ids(&rows).contains(&"appeal"),
            "a new account's limit is nothing to appeal"
        );
    }

    #[test]
    fn a_mute_replaces_the_message_field_but_a_new_account_may_write_to_friends() {
        let mut online = muted();
        online.social.upsert(chat((2, "Lumen")));
        let mut panel = OnlinePanel::default();
        panel.go(Page::Conversation("c".into()), &data(&online));
        let rows = panel.rows(&data(&online)).2;
        assert_eq!(ids(&rows), ["pausedSend"]);
        assert_eq!(rows[0].title, "Messages Paused");
        panel.run("pausedSend", &data(&online), &mut Vec::new());
        assert_eq!(panel.page, Page::Restriction("muted".into()));

        // A new account's limit is for strangers: a friend's chat keeps its field.
        let mut new = signed_in();
        new.restrictions.learn(
            Some(Blocked::Stranger),
            &Restriction::new(Restriction::NEW_ACCOUNT, Some(END)),
        );
        new.social.upsert(chat((2, "Lumen")));
        let mut panel = OnlinePanel::default();
        panel.go(Page::Conversation("c".into()), &data(&new));
        assert_eq!(ids(&panel.rows(&data(&new)).2), ["compose", "send"]);
        // With someone who is no friend it is replaced.
        new.social.upsert(chat((9, "Stranger")));
        panel.go(Page::Conversation("c".into()), &data(&new));
        assert_eq!(ids(&panel.rows(&data(&new)).2), ["pausedSend"]);
    }

    #[test]
    fn the_page_says_what_why_until_when_and_how_to_appeal() {
        let online = muted();
        let mut panel = OnlinePanel::default();
        panel.go(Page::Restriction("muted".into()), &data(&online));
        let (title, subtitle, rows) = panel.rows(&data(&online));
        assert_eq!(title, "Messages and Friend Requests Paused");
        assert_eq!(subtitle.as_deref(), Some("Until Oct 25, 2026 at 12:00 PM"));
        assert_eq!(
            ids(&rows),
            [
                "meansCaption",
                "means",
                "reasonCaption",
                "rule",
                "noteCaption",
                "reason",
                "appealCaption",
                "appealNote",
                "appeal",
                "sendAppeal"
            ]
        );
        assert_eq!(
            rows[1].title,
            "You cannot send messages or friend requests while this lasts. Everything else works as before."
        );
        assert!(
            matches!(&rows[3].control, crate::screens::rows::Control::Info(Some(v)) if v == "Spam")
        );
        assert_eq!(rows[5].title, "Links in every message.");
        assert!(
            rows[7]
                .title
                .contains("You can appeal until Oct 26, 2026 at 12:00 PM.")
        );
    }

    #[test]
    fn an_appeal_goes_out_with_its_text_and_not_empty() {
        let online = muted();
        let mut panel = OnlinePanel::default();
        panel.go(Page::Restriction("muted".into()), &data(&online));
        let mut actions = Vec::new();
        panel.run("sendAppeal", &data(&online), &mut actions);
        assert!(actions.is_empty(), "nothing written, nothing sent");
        panel.typed("appeal", "  I shared a link once.  ".into(), &mut actions);
        assert!(actions.is_empty(), "typing sends nothing");
        panel.run("sendAppeal", &data(&online), &mut actions);
        assert_eq!(
            actions,
            [OnlineAction::Appeal {
                action: 41,
                text: "I shared a link once.".into()
            }]
        );
        // Leaving and coming back starts a new appeal.
        panel.go(Page::Account, &data(&online));
        panel.go(Page::Restriction("muted".into()), &data(&online));
        assert!(panel.appeal_text.is_empty());
    }

    #[test]
    fn the_appeal_row_follows_where_the_appeal_stands() {
        for (status, wanted) in [
            ("open", "Your appeal was sent."),
            (
                "upheld",
                "A moderator read your appeal and the restriction stands.",
            ),
            (
                "expired",
                "The time to appeal ended on Oct 26, 2026 at 12:00 PM.",
            ),
        ] {
            let mut online = signed_in();
            let mut restriction = mute();
            restriction.appeal.as_mut().unwrap().status = status.into();
            online.restrictions.list(vec![restriction]);
            let mut panel = OnlinePanel::default();
            panel.go(Page::Restriction("muted".into()), &data(&online));
            let rows = panel.rows(&data(&online)).2;
            let note = rows.iter().find(|r| r.id == "appealNote").expect(status);
            assert!(note.title.starts_with(wanted), "{status}: {}", note.title);
            assert!(!ids(&rows).contains(&"sendAppeal"), "{status}");
        }
    }

    #[test]
    fn the_page_ends_with_the_restriction() {
        let online = muted();
        let mut panel = OnlinePanel::default();
        panel.go(Page::Account, &data(&online));
        panel.go(Page::Restriction("muted".into()), &data(&online));
        let later = data_at(&online, END as i64);
        assert_eq!(panel.restriction_follow(&later), Some(Page::Account));
        assert_eq!(panel.restriction_follow(&data(&online)), None);
        // Back leads where the player came from.
        assert_eq!(panel.restriction_back(&data(&online)), Page::Account);
    }

    fn banned(until: Option<u64>) -> Online {
        let mut online = Online::default();
        online.status = Status::Running;
        online.cloud = Some(Cloud::Online);
        online.restrictions.ban(Restriction {
            rule: Some("cheating".into()),
            reason: Some("Cheats in a game.".into()),
            action: Some(12),
            appeal: Some(Appeal {
                status: "available".into(),
                deadline: END,
            }),
            ..Restriction::new(Restriction::BANNED, until)
        });
        online
    }

    #[test]
    fn a_ban_shows_on_the_start_page_and_is_appealed_in_the_browser() {
        let online = banned(Some(END));
        let mut panel = OnlinePanel::default();
        panel.go(Page::Start, &data(&online));
        let rows = panel.rows(&data(&online)).2;
        assert_eq!(ids(&rows)[..2], ["restriction", "signIn"]);
        assert_eq!(rows[0].title, "Account Banned");
        assert_eq!(
            rows[0].detail.as_deref(),
            Some("Until Oct 25, 2026 at 12:00 PM")
        );
        assert!(rows[1].group_start);

        let mut actions = Vec::new();
        panel.run("restriction", &data(&online), &mut actions);
        assert_eq!(panel.page, Page::Restriction("banned".into()));
        // Signed out, the page stays: the ban is why.
        assert_eq!(panel.restriction_follow(&data(&online)), None);
        let (title, _, rows) = panel.rows(&data(&online));
        assert_eq!(title, "Account Banned");
        assert!(ids(&rows).contains(&"appealInBrowser"));
        assert!(
            !ids(&rows).contains(&"appeal"),
            "no text field: the browser has the form"
        );
        panel.run("appealInBrowser", &data(&online), &mut actions);
        assert_eq!(actions, [OnlineAction::BrowserSignIn { create: false }]);
        assert_eq!(panel.restriction_back(&data(&online)), Page::Start);
    }

    #[test]
    fn a_ban_for_good_has_no_end_date() {
        let online = banned(None);
        let mut panel = OnlinePanel::default();
        panel.go(Page::Restriction("banned".into()), &data(&online));
        let (_, subtitle, rows) = panel.rows(&data(&online));
        assert_eq!(subtitle.as_deref(), Some("Until further notice"));
        assert_eq!(
            rows[1].title,
            "This account is signed out and cannot sign in."
        );
        assert!(ids(&rows).contains(&"appealInBrowser"));
    }
}
