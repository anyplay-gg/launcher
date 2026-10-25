//! Share screen in the online panel: the group on the party page and the line on Home
//! (docs/DESIGN.md 17, "Share screen"). Rows like any other, so the keyboard, the pad and the
//! screen reader get them from the panel: state is said in words, an icon only repeats it, and
//! nothing here moves. The picture itself is not here; the share worker opens its own window.
//!
//! The group shows only what there is: no caption without a row under it, and nothing at all
//! while the net service reports that sharing and watching are impossible and no stream exists.

use anyps5_core::online::api::AccountId;
use anyps5_core::online::control::{ScreenPhase, ScreenViewer};
use anyps5_core::online::{Link, Online, ScreenCommand};

use super::{OnlineAction, OnlineData, OnlinePanel, link_text};
use crate::screens::online_screen_strings::ScreenText as X;
use crate::screens::rows::Row;
use crate::ui::i18n::{Arg, Language};
use crate::ui::icons::Icon;

/// From this many percent lost, the host is told that a viewer loses part of the picture.
const LOSS_NOTICE_PERCENT: u8 = 5;

/// The name of a member of the party, else of anyone the launcher knows.
pub fn handle_of(online: &Online, account: AccountId) -> Option<String> {
    online
        .social
        .party
        .as_ref()
        .and_then(|p| p.members.iter().find(|m| m.account.id == account))
        .map(|m| m.account.handle.clone())
        .or_else(|| online.known(account).map(|a| a.handle.clone()))
}

/// The name for a sentence: the handle, else a plain word for someone unknown.
pub fn name_of(online: &Online, account: AccountId, language: Language) -> String {
    handle_of(online, account).unwrap_or_else(|| X::SomeoneUnknown.text(language).to_owned())
}

/// The way to a member in words: "Direct, 24 ms", "Through a relay, 80 ms".
pub fn path_text(link: Option<Link>, rtt_ms: Option<u32>, language: Language) -> String {
    match (link, rtt_ms) {
        (Some(Link::Direct), Some(ms)) => X::DirectMs.format(language, &[Arg::Int(i64::from(ms))]),
        (Some(Link::Relay), Some(ms)) => X::RelayMs.format(language, &[Arg::Int(i64::from(ms))]),
        (link, _) => link_text(link, language).to_owned(),
    }
}

/// "Your screen is shared with 2 people".
fn shared_with(watchers: usize, language: Language) -> String {
    match watchers {
        0 => X::SharedNobody.text(language).to_owned(),
        1 => X::SharedOne.text(language).to_owned(),
        n => X::SharedMany.format(language, &[Arg::Int(n as i64)]),
    }
}

/// What a member of the party gets of the player's stream: the way it goes to them as the value
/// of their row, "Not watching" for one who only knows, and under it a note while much is lost.
fn viewer_row(online: &Online, viewer: &ScreenViewer, language: Language) -> Row {
    let name = name_of(online, viewer.account, language);
    if !viewer.watching {
        return Row::info(
            "screenViewer",
            name,
            Some(X::ViewerNotWatching.text(language).to_owned()),
        )
        .icon(Icon::USER_CIRCLE);
    }
    let row = Row::info(
        "screenViewer",
        name,
        Some(path_text(viewer.link, viewer.rtt_ms, language)),
    )
    .icon(Icon::USER_CIRCLE);
    if viewer.loss_percent >= LOSS_NOTICE_PERCENT {
        row.detail(X::ViewerLosing.text(language))
    } else {
        row
    }
}

/// The group on the party page. Adds nothing when there is nothing to show.
pub(super) fn rows(data: &OnlineData<'_>, rows: &mut Vec<Row>) {
    let online = data.online;
    let screen = &online.screen;
    let language = data.language;
    if !screen.visible() {
        return;
    }
    let t = |text: X| text.text(language);
    let mut group: Vec<Row> = Vec::new();

    if let Some(hosting) = screen.hosting() {
        let watchers = screen.watchers();
        let title = shared_with(watchers, language);
        let phase = match hosting.phase {
            ScreenPhase::Connecting => X::HostStarting,
            ScreenPhase::Live => X::HostLive,
            ScreenPhase::Stalled => X::HostStalled,
        };
        // The sentence is a note, the game a row with its state as the value.
        group.push(Row::info("screenShared", title, None));
        group.push(
            Row::info(
                "screenGame",
                hosting.title.clone(),
                Some(t(phase).to_owned()),
            )
            .icon(Icon::APP_WINDOW),
        );
        for viewer in &hosting.viewers {
            group.push(viewer_row(online, viewer, language));
        }
        group.push(Row::command("stopSharing", t(X::StopSharing)).icon(Icon::X));
    } else if screen.can_start() {
        group.push(
            Row::command("shareScreen", t(X::ShareScreen))
                .icon(Icon::APP_WINDOW)
                .detail(t(X::ShareDetail)),
        );
    }

    for offer in screen.offers() {
        let name = name_of(online, offer.account, language);
        if screen.can_watch() {
            group.push(
                Row::command("watchScreen", X::Watch.format(language, &[Arg::Str(&name)]))
                    .icon(Icon::PLAY)
                    .detail(X::SharingTitle.format(language, &[Arg::Str(&offer.title)])),
            );
        } else {
            group.push(Row::info(
                "screenOffer",
                X::IsSharing.format(language, &[Arg::Str(&name), Arg::Str(&offer.title)]),
                None,
            ));
            group.push(Row::info("screenOfferNote", t(X::CannotShow), None).attached());
        }
    }

    if let Some(watching) = screen.watching() {
        let name = name_of(online, watching.account, language);
        let phase = match watching.phase {
            ScreenPhase::Connecting => X::PhaseConnecting,
            ScreenPhase::Live => X::PhaseLive,
            ScreenPhase::Stalled => X::PhaseStalled,
        };
        group.push(
            Row::info(
                "screenWatching",
                X::Watching.format(language, &[Arg::Str(&name)]),
                Some(t(phase).to_owned()),
            )
            .icon(Icon::APP_WINDOW)
            .detail(format!(
                "{} · {}",
                watching.title,
                path_text(watching.link, watching.rtt_ms, language)
            )),
        );
        group.push(Row::info("screenNote", t(X::WatchNote), None).attached());
        group.push(Row::command("stopWatching", t(X::StopWatching)).icon(Icon::X));
    }

    if group.is_empty() {
        return;
    }
    rows.push(Row::caption("screenCaption", t(X::Screen)).group_start());
    rows.extend(group);
}

/// The line on Home: only while there is a stream to say, and only in a party.
pub(super) fn home_row(data: &OnlineData<'_>, rows: &mut Vec<Row>) {
    let online = data.online;
    let screen = &online.screen;
    let language = data.language;
    if online.social.party.is_none() {
        return;
    }
    let detail = if let Some(watching) = screen.watching() {
        X::HomeWatching.format(
            language,
            &[Arg::Str(&name_of(online, watching.account, language))],
        )
    } else if screen.hosting().is_some() {
        shared_with(screen.watchers(), language)
    } else if let Some(offer) = screen.offers().next() {
        X::IsSharing.format(
            language,
            &[
                Arg::Str(&name_of(online, offer.account, language)),
                Arg::Str(&offer.title),
            ],
        )
    } else {
        return;
    };
    rows.push(
        Row::action("screenShare", X::Screen.text(language))
            .icon(Icon::APP_WINDOW)
            .detail(detail),
    );
}

impl OnlinePanel {
    /// What the rows of the group do. True when `row` was one of them.
    pub(super) fn screen_run(
        &mut self,
        row: &str,
        data: &OnlineData<'_>,
        actions: &mut Vec<OnlineAction>,
    ) -> bool {
        match row {
            "shareScreen" => actions.push(OnlineAction::Screen(ScreenCommand::Start)),
            "stopSharing" => actions.push(OnlineAction::Screen(ScreenCommand::Stop)),
            "stopWatching" => actions.push(OnlineAction::Screen(ScreenCommand::Leave)),
            "watchScreen" => {
                let index = self.nth(data, "watchScreen");
                if let Some(offer) = data.online.screen.offers().nth(index) {
                    actions.push(OnlineAction::Screen(ScreenCommand::Watch(offer.account)));
                }
            }
            // The Home line leads to where the stream is handled.
            "screenShare" => self.go(super::Page::Party, data),
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests;
