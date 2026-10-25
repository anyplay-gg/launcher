//! The Friends screen in the panel host: what the player asks for becomes a command for the net
//! service (`Model::online`), and a join first checks the library and the versions.

use anyps5_core::online::api::AccountId;
use anyps5_core::online::api::MAX_MESSAGE_CHARS;
use anyps5_core::online::{self as net, Command, Join, Purpose, Status};

use super::PanelHost;
use crate::app::model::{Effect, Model};
use crate::app::online::OnlineService;
use crate::app::overlay::OverlayCx;
use crate::screens::online::{OnlineAction, OnlineData};
use crate::screens::online_call_strings::CallText as CT;
use crate::screens::online_social_strings::SocialText as S;
use crate::screens::online_strings::OnlineText as T;
use crate::ui::icons::Icon;

impl PanelHost {
    pub(super) fn friends_ui(&mut self, cx: &mut OverlayCx<'_>, wanted: bool) {
        let events = self.take_events(wanted);
        let (language, accent) = (cx.model.language(), cx.shell.accent_color());
        let cloud = net::cloud(cx.model.prefs.online_cloud.as_deref()).map(str::to_owned);
        // Opening the screen brings the net service up: it is local until the player signs in.
        if wanted
            && !self.friends.is_active()
            && cloud.is_some()
            && !matches!(cx.model.online.status, Status::Running | Status::Missing)
        {
            cx.model.effects.push(Effect::StartOnline);
        }
        if self.friends.wants_local() {
            let local = crate::app::online_local::build(cx, cloud.as_deref());
            self.friends.set_local(local);
        }
        // Coming back from the profile screen: to the page the player left, or to the page the
        // screen named.
        let opening = wanted && !self.friends.is_open();
        let resume = opening && std::mem::take(&mut cx.model.online_resume);
        let start = if opening {
            self.online_start.take()
        } else {
            None
        };
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
                game_running: cx.model.running.is_some(),
            };
            if wanted && !self.friends.is_open() {
                if let Some((page, item)) = start {
                    // The profile screen sent the player to a page.
                    self.friends.show_page(page, &data);
                    if let Some(index) =
                        item.and_then(|id| data.online.feed.items.iter().position(|i| i.id == id))
                    {
                        self.friends.set_feed_focus(index, None);
                    }
                } else if resume {
                    // Back from the profile screen: the page the player left.
                    self.friends.resume(&data);
                } else {
                    self.friends.open(&data);
                }
            } else if !wanted && self.friends.is_active() && self.friends.is_open() {
                self.friends.close();
            }
            self.friends.show(cx.shell, cx.ctx, &events, &data)
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

pub(super) fn online_action(model: &mut Model, action: OnlineAction) {
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
        OnlineAction::AppearOffline(on) => online.set_appear_offline(on),
        OnlineAction::TrophyAudience(trophies) => {
            online.set_trophy_audience(trophies);
        }
        OnlineAction::FeedRefresh => online.request_feed(),
        OnlineAction::FeedOlder => online.request_older_feed(),
        OnlineAction::React(item, reaction) => online.react(item, reaction),
        OnlineAction::Profile(account) => online.request_profile(account),
        OnlineAction::OpenProfile(account) => {
            model.open_profile(account, crate::app::profile::Back::Friends);
        }
        OnlineAction::Voice(on) => online.set_voice(on),
        OnlineAction::VoiceProvider(provider) => online.set_voice_provider(provider),
        OnlineAction::Call(account) => {
            let friend = online.known(account).cloned();
            let started = friend.as_ref().map(|friend| online.start_call(friend));
            if started == Some(false) {
                let text = CT::ToastNeedsLeader.text(model.language());
                model.toast(text, Icon::WARNING);
            }
        }
        OnlineAction::CallsSeen(account) => online.calls_seen(account),
        OnlineAction::PartyLink(url) => online.set_party_link(&url),
        OnlineAction::OpenDiscord(link) => crate::screens::calls::open_discord_link(&link),
        OnlineAction::VoiceMode(mode) => online.set_voice_mode(mode),
        OnlineAction::VoiceKey(key) => online.set_voice_key(Some(key)),
        OnlineAction::VoiceDevice(kind, name) => online.set_voice_device(kind, name),
        OnlineAction::RefreshVoiceDevices => online.request_voice_devices(),
        OnlineAction::PeerVolume(account, percent) => online.set_peer_volume(account, percent),
        OnlineAction::PeerMute(account, muted) => online.set_peer_mute(account, muted),
        OnlineAction::Screen(net::ScreenCommand::Start) => {
            model.effects.push(Effect::ScreenStart);
        }
        OnlineAction::Screen(command) => online.screen_send(command),
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
        OnlineAction::Reconnect => model.effects.push(Effect::StartOnline),
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
            // The join code of the Discord button goes back first: afterwards nobody is signed in.
            if online.join.take_for_sign_out() {
                online.send(Purpose::JoinCodeRevoke, Command::JoinCodeRevoke);
            }
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
            // Adding to a call rings; inviting to a party does not.
            let call = online.calls.active;
            online.send(
                Purpose::PartyInvite,
                Command::PartyInvite { accounts, call },
            );
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
        OnlineAction::Close => model.close_friends(),
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
