//! The online panel in the panel host: what the player asks for becomes a command for the net
//! service (`Model::online`), and a join first checks the library and the versions.

use anyps5_core::online::api::AccountId;
use anyps5_core::online::api::MAX_MESSAGE_CHARS;
use anyps5_core::online::{self as net, Command, Join, Purpose, Status};

use super::PanelHost;
use crate::app::model::{Effect, Model};
use crate::app::online::OnlineService;
use crate::app::overlay::{Overlay, OverlayCx};
use crate::screens::online::{OnlineAction, OnlineData};
use crate::screens::online_social_strings::SocialText as S;
use crate::screens::online_strings::OnlineText as T;
use crate::ui::icons::Icon;

impl PanelHost {
    pub(super) fn online_ui(&mut self, cx: &mut OverlayCx<'_>, wanted: bool) {
        let events = self.take_events(wanted);
        let side_next = cx.model.overlay.is_some_and(Overlay::is_side_panel);
        let (language, accent) = (cx.model.language(), cx.shell.accent_color());
        let cloud = net::cloud(cx.model.prefs.online_cloud.as_deref()).map(str::to_owned);
        // Opening the panel brings the net service up: it is local until the player signs in.
        if wanted
            && !self.online_panel.is_active()
            && cloud.is_some()
            && !matches!(cx.model.online.status, Status::Running | Status::Missing)
        {
            cx.model.effects.push(Effect::StartOnline);
        }
        let actions = {
            let data = OnlineData {
                online: &cx.model.online,
                cloud: cloud.as_deref(),
                custom_cloud: cx.model.prefs.online_cloud.is_some(),
                official_cloud: net::DEFAULT_CLOUD.is_some(),
                relay_only: cx.model.prefs.online_relay_only,
                language,
                accent,
                now: crate::app::clock::unix_seconds(),
                utc_offset: None,
            };
            if wanted && !self.online_panel.is_open() {
                self.online_panel.open(&data);
            } else if !wanted && self.online_panel.is_active() && self.online_panel.is_open() {
                if side_next {
                    self.online_panel.dismiss();
                } else {
                    self.online_panel.close();
                }
            }
            self.online_panel.show(cx.shell, cx.ctx, &events, &data)
        };
        if wanted && cx.model.online.status == Status::Missing {
            let text = T::ServiceMissing.text(language);
            if cx.model.toast.as_ref().is_none_or(|t| t.text != text) {
                cx.model.toast(text, Icon::WARNING);
            }
        }
        for action in actions {
            match action {
                OnlineAction::CopyText(text) => {
                    cx.ctx.copy_text(text);
                    let copied = S::Copied.text(language);
                    cx.model.toast(copied, Icon::CHECK);
                }
                action => online_action(cx.model, action),
            }
        }
    }
}

fn online_action(model: &mut Model, action: OnlineAction) {
    let online = &mut model.online;
    match action {
        OnlineAction::SetCloud(url) => {
            let url = url.trim().trim_end_matches('/').to_owned();
            model.edit_prefs(|p| p.online_cloud = Some(url));
            model.effects.push(Effect::StartOnline);
        }
        OnlineAction::UseOfficialCloud => {
            model.edit_prefs(|p| p.online_cloud = None);
            model.effects.push(Effect::StartOnline);
        }
        OnlineAction::RelayOnly(on) => {
            model.edit_prefs(|p| p.online_relay_only = on);
            model.effects.push(Effect::RelayOnlyChanged);
        }
        OnlineAction::TrophyAudience(trophies) => {
            online.set_trophy_audience(trophies);
        }
        OnlineAction::BrowserSignIn { create } => {
            online.send(
                Purpose::BrowserSignIn { create },
                Command::BrowserSignIn { create },
            );
        }
        OnlineAction::OpenSignInPage => {
            if let Some(url) = model.online.sign_in_url.clone() {
                OnlineService::open_cloud_page(model, &url);
            }
        }
        OnlineAction::CancelSignIn => {
            online.send(Purpose::CancelSignIn, Command::CancelSignIn);
        }
        OnlineAction::ManageAccount => {
            online.send(Purpose::AccountPage, Command::AccountPage);
        }
        OnlineAction::Appeal { action, text } => {
            online.send(Purpose::Appeal(action), Command::Appeal { action, text });
        }
        OnlineAction::AddFriend(handle) => {
            online.send(
                Purpose::AddFriend(handle.clone()),
                Command::AddFriend { handle },
            );
        }
        OnlineAction::Accept(account) => {
            online.send(Purpose::Accept(account), Command::Accept { account });
        }
        OnlineAction::Decline(account) => {
            online.send(Purpose::Decline(account), Command::Decline { account });
        }
        OnlineAction::RemoveFriend(account) => {
            online.send(
                Purpose::RemoveFriend(account),
                Command::RemoveFriend { account },
            );
        }
        OnlineAction::Block(account) => {
            online.send(Purpose::Block(account), Command::Block { account });
        }
        OnlineAction::Unblock(account) => {
            online.send(Purpose::Unblock(account), Command::Unblock { account });
        }
        OnlineAction::Report(account, reason) => {
            online.send(
                Purpose::Report(account),
                Command::Report {
                    account,
                    reason,
                    details: String::new(),
                },
            );
        }
        OnlineAction::Join(friend) => {
            if let Some(join) = friend_join(model, friend) {
                OnlineService::request_join(model, join);
            }
        }
        OnlineAction::Invite(account) => {
            online.send(
                Purpose::Invite,
                Command::Invite {
                    accounts: vec![account],
                },
            );
        }
        OnlineAction::AnswerInvitation { id, accept: true } => {
            // Accepting is joining: the net service finds the invitation by its session.
            let invitation = online.invitations.iter().find(|i| i.id == id).cloned();
            if let Some(invitation) = invitation {
                OnlineService::request_join(
                    model,
                    Join {
                        session: invitation.session,
                        title_id: invitation.title_id,
                        title: invitation.title,
                        version: invitation.version,
                        build: String::new(),
                        friend: invitation.from.handle,
                    },
                );
            }
        }
        OnlineAction::AnswerInvitation { id, accept: false } => {
            online.send(
                Purpose::AnswerInvitation {
                    id: id.clone(),
                    accept: false,
                },
                Command::AnswerInvitation { id, accept: false },
            );
        }
        OnlineAction::SignOut => {
            online.send(Purpose::SignOut, Command::SignOut);
        }
        OnlineAction::Dialog(accounts) => {
            if let Some(dialog) = online.dialog.take() {
                online.send(
                    Purpose::Dialog,
                    Command::Dialog {
                        id: dialog.id,
                        accounts,
                    },
                );
            }
        }
        OnlineAction::PartyInvite(accounts) => {
            online.send(Purpose::PartyInvite, Command::PartyInvite { accounts });
        }
        OnlineAction::PartyAnswer { id, accept } => {
            online.send(
                Purpose::PartyAnswer {
                    id: id.clone(),
                    accept,
                },
                Command::PartyAnswer { id, accept },
            );
        }
        OnlineAction::PartyLeave => {
            online.send(Purpose::PartyLeave, Command::PartyLeave);
        }
        OnlineAction::PartyKick(account) => {
            online.send(Purpose::PartyKick(account), Command::PartyKick { account });
        }
        OnlineAction::PartyLeader(account) => {
            online.send(
                Purpose::PartyLeader(account),
                Command::PartyLeader { account },
            );
        }
        OnlineAction::PartyMove(session) => {
            online.send(Purpose::PartyMove, Command::PartyMove { session });
        }
        OnlineAction::OpenDirect(account) => online.open_direct(account),
        OnlineAction::OpenThread(conversation) => online.open_thread(&conversation),
        OnlineAction::LoadEarlier(conversation) => online.load_earlier(&conversation),
        OnlineAction::Draft { conversation, text } => {
            // The field takes what the cloud takes; a longer text is cut where the cloud stops.
            let text: String = text.chars().take(MAX_MESSAGE_CHARS + 1).collect();
            online.social.set_draft(&conversation, text);
        }
        OnlineAction::SendMessage(conversation) => {
            online.send_draft(&conversation);
        }
        OnlineAction::MarkRead(conversation) => online.mark_read(&conversation),
        OnlineAction::AcceptRequest(conversation) => {
            online.send(
                Purpose::AcceptRequest(conversation.clone()),
                Command::AcceptRequest { conversation },
            );
        }
        OnlineAction::LeaveConversation(conversation) => {
            online.send(
                Purpose::LeaveConversation(conversation.clone()),
                Command::LeaveConversation { conversation },
            );
        }
        // The panel host copies it: only the host has the window's clipboard.
        OnlineAction::CopyText(_) => {}
        OnlineAction::Close => model.dismiss_overlay(),
    }
}

/// The session a friend plays in, when friends may join it.
fn friend_join(model: &Model, friend: AccountId) -> Option<Join> {
    let friend = model.online.friend(friend)?;
    let activity = friend.presence.activity.as_ref()?;
    let session = activity.session.as_ref().filter(|s| s.joinable)?;
    Some(Join {
        session: session.id.clone(),
        title_id: activity.title_id.clone(),
        title: activity.title.clone(),
        version: activity.version.clone(),
        build: String::new(),
        friend: friend.account.handle.clone(),
    })
}
