//! Texts of the Party room (docs/DESIGN.md 17, "Party room"). English and Turkish like the rest;
//! the Turkish has not been read by a native speaker yet. Buttons are in title case.

use crate::ui::i18n::{Arg, Language, substitute};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RoomText {
    /// Switches the party voice on, where Mute and Deafen would be.
    VoiceOn,
    /// The button of a party invitation's card.
    ViewInvitation,
    /// Under a member the player muted: not their own mute, which nobody reports.
    MutedByYou,
    /// The empty seat of the grid.
    InviteAFriend,
}

impl RoomText {
    pub const fn english(self) -> &'static str {
        match self {
            RoomText::VoiceOn => "Turn On Voice",
            RoomText::ViewInvitation => "View Invitation",
            RoomText::MutedByYou => "Muted by you",
            RoomText::InviteAFriend => "Invite a Friend",
        }
    }

    pub const fn turkish(self) -> &'static str {
        match self {
            RoomText::VoiceOn => "Sesi Aç",
            RoomText::ViewInvitation => "Daveti Görüntüle",
            RoomText::MutedByYou => "Senin tarafından susturuldu",
            RoomText::InviteAFriend => "Arkadaş Davet Et",
        }
    }

    pub const fn text(self, language: Language) -> &'static str {
        match language {
            Language::Turkish => self.turkish(),
            Language::English => self.english(),
        }
    }

    pub fn format(self, language: Language, args: &[Arg<'_>]) -> String {
        substitute(self.text(language), args)
    }
}
