//! The two rows of Home that lead to the player's own profile and to the feed, and what the
//! pages of the feed do with a profile: they open the profile screen (`screens::profile`), a full
//! screen of its own, and the panel stays on the page it was on, so Back from the profile lands
//! where the player came from.

use anyps5_core::online::api::AccountId;

use super::{OnlineAction, OnlineData, OnlinePanel, Page, Row};
use crate::screens::online_feed_strings::FeedText as F;
use crate::screens::rows;
use crate::ui::focus::RowKind;
use crate::ui::icons::Icon;

impl OnlinePanel {
    /// Asks the host to open the profile screen on `account`.
    pub(super) fn open_profile(&mut self, account: AccountId) {
        self.queued.push(OnlineAction::OpenProfile(account));
    }

    /// The rows of Home that lead to the player's own profile and to the feed.
    pub(super) fn home_profile_rows(&self, data: &OnlineData<'_>, rows: &mut Vec<Row>) {
        let language = data.language;
        let after_others = !rows.is_empty();
        let mut profile = Row::action("profile", F::YourProfile.text(language))
            .icon(Icon::USER_CIRCLE)
            .detail(F::YourProfileDetail.text(language));
        profile.group_start = after_others;
        rows.push(profile);
        rows.push(
            Row::action("feed", F::Feed.text(language))
                .icon(Icon::LIST)
                .detail(F::FeedDetail.text(language)),
        );
    }

    /// The rows of the Feed page without cards arrive after the page opened (the retry row once
    /// the fetch failed): the focus moves to the first row that takes it when it stands on one
    /// that does not.
    pub(super) fn settle_focus(&mut self, data: &OnlineData<'_>) {
        if self.page != Page::Feed || self.feed_is_list(data) {
            return;
        }
        let rows = self.rows(data).2;
        let kinds = rows::kinds(&rows);
        let on_note = kinds
            .get(self.side.nav.row)
            .is_none_or(|kind| *kind == RowKind::Info);
        if on_note && let Some(first) = kinds.iter().position(|kind| *kind != RowKind::Info) {
            self.side.nav.row = first;
        }
    }

    /// A row of these pages ran. `false` for any other row.
    pub(super) fn profile_feed_run(
        &mut self,
        row: &str,
        data: &OnlineData<'_>,
        actions: &mut Vec<OnlineAction>,
    ) -> bool {
        let me = data.online.account.as_ref().map(|a| a.id);
        match (row, &self.page) {
            ("profile", _) => {
                if let Some(me) = me {
                    self.open_profile(me);
                }
            }
            ("feed", _) => self.go(Page::Feed, data),
            ("viewProfile", Page::Friend(id)) => {
                let id = *id;
                self.open_profile(id);
            }
            ("feedRetry", Page::Feed) => actions.push(OnlineAction::FeedRefresh),
            _ => return false,
        }
        true
    }
}
