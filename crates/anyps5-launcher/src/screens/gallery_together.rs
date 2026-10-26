//! Gallery scenes of Play Together: the prompt card, every blocker, and the panel pages that carry
//! its wording (docs/DESIGN.md 17, "Play Together"). Kept apart from `gallery.rs`, which only
//! routes to the functions here.

use anyps5_core::online::Online;
use anyps5_core::online::api::{
    Account, AccountId, Activity, Friend, Presence, SessionKind, SessionRef,
};
use anyps5_core::play_together::{Blocker, Offer};
use egui::Context;

use crate::app::together::Card;
use crate::screens::together;

/// The blockers the gallery shows, in the order of the spec's table.
fn every_blocker() -> Vec<Blocker> {
    vec![
        Blocker::SignedOut,
        Blocker::Offline,
        Blocker::OnlineOff,
        Blocker::NotOwned {
            title: "Minecraft".into(),
        },
        Blocker::WrongVersion {
            title: "Minecraft".into(),
            need: "01.044.000".into(),
            have: "01.008.000".into(),
        },
        // You are ahead of the session: the longest line the card has to hold.
        Blocker::WrongVersion {
            title: "Minecraft".into(),
            need: "01.008.000".into(),
            have: "01.044.000".into(),
        },
        Blocker::WrongBuild {
            need: "v0.1.5".into(),
            have: "v0.1.4".into(),
        },
        Blocker::Full,
        Blocker::Busy,
        Blocker::Ended,
        Blocker::NoSession {
            host: "Mira".into(),
            title: "Minecraft".into(),
        },
    ]
}

/// The card scenes, over the real home with a game that can be played together.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TogetherLook {
    /// "Mira is playing Minecraft. Join?"
    Offer,
    /// The same without the places (an invitation).
    OfferInvitation,
    /// One blocker, by position in [`together::every_blocker`].
    Blocked(usize),
    /// The same with the pad's focus on a button (its position in the card's actions).
    Focused(usize, usize),
}

const BLOCKERS: [&str; 11] = [
    "signed-out",
    "offline",
    "online-off",
    "not-owned",
    "wrong-version",
    "wrong-version-ahead",
    "wrong-build",
    "full",
    "busy",
    "ended",
    "no-session",
];

/// Scene names: `together-offer`, `together-offer-invitation`, `together-<blocker>`.
pub fn parse(name: &str) -> Option<TogetherLook> {
    let rest = name.strip_prefix("together-")?;
    match rest {
        "offer" => Some(TogetherLook::Offer),
        "offer-invitation" => Some(TogetherLook::OfferInvitation),
        // The pad's focus on the version blocker's quiet button ("Try Anyway").
        "wrong-version-pad" => Some(TogetherLook::Focused(4, 0)),
        other => BLOCKERS
            .iter()
            .position(|b| *b == other)
            .map(TogetherLook::Blocked),
    }
}

fn offer(places: Option<(u16, u16)>) -> Offer {
    Offer {
        session: "s1".into(),
        title_id: "PPSA17221".into(),
        title: "Minecraft".into(),
        version: "01.044.000".into(),
        kind: SessionKind::Lan,
        host: "Mira".into(),
        places,
        build: None,
    }
}

/// The card of a look.
pub fn card(look: TogetherLook) -> Card {
    match look {
        TogetherLook::Offer => Card::Offer {
            key: "s1".into(),
            offer: offer(Some((2, 4))),
            invitation: None,
        },
        TogetherLook::OfferInvitation => Card::Offer {
            key: "s1".into(),
            offer: offer(None),
            invitation: Some("i1".into()),
        },
        TogetherLook::Blocked(n) | TogetherLook::Focused(n, _) => {
            Card::Blocked(every_blocker().into_iter().nth(n).unwrap_or(Blocker::Full))
        }
    }
}

/// Paints the card of a look.
pub fn paint(
    ctx: &Context,
    accent: egui::Color32,
    look: TogetherLook,
    language: crate::ui::i18n::Language,
) {
    let pad = match look {
        TogetherLook::Focused(_, button) => Some(button),
        _ => None,
    };
    together::paint(ctx, accent, &card(look), language, 1.0, true, 0.0, pad);
}

/// Friends of the panel pages: Lumen plays Minecraft together (2/4), Juno plays it with no
/// session (online play is off), Kit is a plain friend.
pub fn sample(state: &mut Online) {
    let account = |id: u64, handle: &str| Account {
        id: AccountId(id),
        handle: handle.into(),
    };
    let minecraft = |session: Option<SessionRef>| {
        Some(Activity {
            title_id: "PPSA17221".into(),
            title: "Minecraft".into(),
            version: "01.044.000".into(),
            session,
        })
    };
    let friend = |id, handle: &str, activity| Friend {
        account: account(id, handle),
        since: 0,
        presence: Presence {
            online: true,
            activity,
            revision: 1,
        },
    };
    state.friends.friends = vec![
        friend(
            2,
            "Lumen",
            minecraft(Some(SessionRef {
                id: "s1".into(),
                joinable: true,
                members: 2,
                max_members: 4,
                kind: SessionKind::Lan,
            })),
        ),
        friend(3, "Juno", minecraft(None)),
        friend(4, "Kit", None),
    ];
    state.invitations.clear();
    state.friends.incoming.clear();
    state.social.party_invitations.clear();
}
