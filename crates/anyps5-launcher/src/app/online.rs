//! The net service behind [`Model::online`]: it starts and stops the process, hands it the
//! commands the panels queued and brings its answers back, turns what the player should know
//! into toasts, and holds a game's start until the net service gave it the bridge's variables.
//!
//! A start never waits long on the network: without an answer in three seconds the game starts
//! offline, as it would without an account. Only a join, which makes no sense offline, is
//! called off when it fails.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use anyps5_core::events::Notifier;
use anyps5_core::library::Game;
use anyps5_core::online::api::{ErrorCode, FriendState, SessionKind};
use anyps5_core::online::{
    self as net, Command, Failure, Join, NetProcess, Notice, Purpose, Status,
};
use anyps5_core::paths::GameId;
use anyps5_core::play_together::Blocker;

use crate::app::model::{Effect, GameState, Model};
use crate::app::overlay::Overlay;
use crate::screens::discord_strings::redeem_failure;
use crate::screens::online_call_strings::CallText as CT;
use crate::screens::online_feed_strings::FeedText as FT;
use crate::screens::online_restriction_strings as restriction_text;
use crate::screens::online_social_strings::SocialText as S;
use crate::screens::online_strings::OnlineText as T;
use crate::screens::online_voice_strings::VoiceText as VT;
use crate::screens::together_strings::TogetherText as TT;
use crate::ui::focus::HomeFocus;
use crate::ui::i18n::{Arg, Language};
use crate::ui::icons::Icon;
use crate::ui::sound::Cue;

/// How much of a message a toast shows.
const MESSAGE_TOAST_CHARS: usize = 60;

/// How long a game start waits for the bridge's variables.
const START_PATIENCE: Duration = Duration::from_secs(3);

/// A start the net service was asked about.
struct WaitingStart {
    game: GameId,
    number: u64,
    since: Instant,
    joining: bool,
    /// Play Together: the start asked for a session of the player's own.
    hosting: bool,
}

pub struct OnlineService {
    process: Option<NetProcess>,
    /// Previews, the demo and tests: never start a process.
    simulated: bool,
    wake: Notifier,
    waiting: Option<WaitingStart>,
    starts: u64,
    /// The session the next start of this title joins.
    join: Option<Join>,
    /// A join waiting for its game to be unlocked.
    unlocking: Option<Join>,
    /// The game the net service was told about and that has not ended yet.
    started: Option<GameId>,
    /// Relay only changed while that game played: the net service hears of it at the end.
    relay_only_later: bool,
    /// The player was online in the last frame: coming online syncs the trophies.
    was_online: bool,
}

/// A game ready to start, with the variables it gets beyond its settings.
pub struct ReadyStart {
    pub game: GameId,
    pub env: BTreeMap<String, String>,
}

impl OnlineService {
    pub fn new(wake: Notifier, simulated: bool) -> Self {
        Self {
            process: None,
            simulated,
            wake,
            waiting: None,
            starts: 0,
            join: None,
            unlocking: None,
            started: None,
            relay_only_later: false,
            was_online: false,
        }
    }

    /// Starts the net service and says hello with the cloud of the preferences. Already running,
    /// it starts again: a new cloud needs a new hello.
    pub fn start(&mut self, model: &mut Model) {
        let Some(cloud) = net::cloud(model.prefs.online_cloud.as_deref()).map(str::to_owned) else {
            return;
        };
        if self.process.is_some() {
            self.stop(model);
        }
        if self.simulated {
            return;
        }
        let Some(binary) = NetProcess::locate().or_else(|| bundled(&model.paths)) else {
            tracing::warn!("no {} next to the launcher", net::BINARY);
            model.online.status = Status::Missing;
            return;
        };
        self.relay_only_later = false;
        match NetProcess::spawn(
            &binary,
            &model.paths.network_dir(),
            model.prefs.online_relay_only,
            &model.paths.network_log(),
            self.wake.clone(),
        ) {
            Ok(process) => {
                tracing::info!("net service started: {}", binary.display());
                self.process = Some(process);
                model.online.status = Status::Running;
                model.online.send(
                    Purpose::Hello,
                    Command::Hello {
                        launcher: env!("CARGO_PKG_VERSION").to_owned(),
                        cloud,
                    },
                );
                tell_worker(model);
            }
            Err(error) => {
                tracing::warn!("the net service could not start: {error}");
                model.online.status = Status::Ended(error.to_string());
            }
        }
    }

    pub fn stop(&mut self, model: &mut Model) {
        self.process = None;
        self.waiting = None;
        self.unlocking = None;
        self.started = None;
        self.relay_only_later = false;
        model.online.stopped(Status::Off);
    }

    /// Relay only changed in the preferences. A stopped net service gets it when it starts. A
    /// running one starts its tunnels again, which a game in a session would notice, so during
    /// a game the change waits for its end.
    pub fn relay_only_changed(&mut self, model: &mut Model) {
        if model.online.status != Status::Running {
            return;
        }
        if self.started.is_some() {
            self.relay_only_later = true;
        } else {
            send_relay_only(model);
        }
    }

    pub fn is_running(&self) -> bool {
        self.process.is_some()
    }

    /// Every frame: the queued commands go out, the output comes in. Returns the starts that may
    /// go ahead now.
    pub fn frame(&mut self, model: &mut Model) -> Vec<ReadyStart> {
        let mut ready = Vec::new();
        if let Some(process) = &mut self.process {
            let lines = model.online.take_outbox();
            if !lines.is_empty()
                && let Err(error) = process.write(&lines)
            {
                tracing::warn!("cannot write to the net service: {error}");
            }
            // Cisco's OpenH264 came or went: the worker is asked again what it can do.
            if anyps5_core::openh264::take_changed() {
                tell_worker(model);
            }
            for line in process.drain() {
                let ended = line == net::Line::Ended;
                for notice in model.online.apply(line) {
                    self.notice(model, notice, &mut ready);
                }
                if ended {
                    self.process = None;
                    break;
                }
            }
        } else {
            // Nothing to send to: commands fail at once instead of waiting forever.
            let _ = model.online.take_outbox();
        }
        let online = model.online.is_online();
        if online && !self.was_online {
            model.effects.push(Effect::SyncTrophies);
            model.sync_playtime();
            model.request_selected_runs();
        }
        self.was_online = online;
        if let Some(waiting) = &self.waiting
            && waiting.since.elapsed() >= START_PATIENCE
        {
            let waiting = self.waiting.take().expect("checked above");
            tracing::info!("no answer to the game start in time; starting offline");
            if waiting.joining {
                self.call_off_start(model);
                let text = T::Offline.text(model.language()).to_owned();
                model.toast(text, Icon::WARNING);
            } else {
                if waiting.hosting {
                    let text = TT::ToastHostTimedOut.text(model.language());
                    model.toast(text, Icon::WARNING);
                }
                ready.push(ReadyStart {
                    env: record_env(model, &waiting.game),
                    game: waiting.game,
                });
            }
        }
        self.continue_unlocking(model);
        ready
    }

    /// A game is about to start. `Some` starts it now with these variables; `None` holds it
    /// until the net service answers (see [`OnlineService::frame`]).
    pub fn on_play(&mut self, model: &mut Model, game: &Game) -> Option<BTreeMap<String, String>> {
        let join = self.join.take().filter(|j| j.title_id == game.title_id);
        // A join wins over a host plan, as in the net service; the plan is spent either way.
        let host = model
            .together
            .take_host(&game.id)
            .filter(|_| join.is_none());
        if model.online.status != Status::Running || !model.online.is_signed_in() {
            return Some(record_env(model, &game.id));
        }
        self.starts += 1;
        model.online.send(
            Purpose::GameStart(self.starts),
            Command::GameStart {
                title_id: game.title_id.clone(),
                title: game.name.clone(),
                version: game.version.clone(),
                build: build(model),
                join: join.as_ref().map(|j| j.session.clone()),
                host: host.clone(),
            },
        );
        self.waiting = Some(WaitingStart {
            game: game.id.clone(),
            number: self.starts,
            since: Instant::now(),
            joining: join.is_some(),
            hosting: host.is_some(),
        });
        self.started = Some(game.id.clone());
        None
    }

    /// The game's process ended (or never started).
    pub fn on_exited(&mut self, model: &mut Model, game: &GameId) {
        if self.started.as_ref() == Some(game) {
            self.started = None;
            if self.waiting.as_ref().is_some_and(|w| &w.game == game) {
                self.waiting = None;
            }
            model.online.send(Purpose::GameEnd, Command::GameEnd);
            model.together.game_ended();
            model.session_ended(game);
            if std::mem::take(&mut self.relay_only_later) {
                send_relay_only(model);
            }
        }
    }

    /// The net service agreed to the join: the game starts in the session, unlocked first when it
    /// is owned but not yet unlocked.
    fn begin_join(&mut self, model: &mut Model, join: Join) {
        let language = model.language();
        let Some(index) = model.games.iter().position(|g| g.title_id == join.title_id) else {
            return;
        };
        let game = model.games[index].clone();
        model.together.dismiss(&join.session);
        model.selected = index;
        model.focus = HomeFocus::Tiles;
        model.dismiss_overlay();
        match model.state_of(&game) {
            GameState::Installed { .. } => {
                let text =
                    T::Joining.format(language, &[Arg::Str(&join.friend), Arg::Str(&join.title)]);
                model.toast(text, Icon::GAME_CONTROLLER);
                self.start_join(model, join, &game.id);
            }
            GameState::Locked | GameState::Failed(_) => {
                let text = TT::ToastUnlocking.format(language, &[Arg::Str(&game.name)]);
                model.toast(text, Icon::LOCK);
                if matches!(model.state_of(&game), GameState::Failed(_)) {
                    model.retry(&game.id);
                } else {
                    model.unlock(&game.id);
                }
                self.unlocking = Some(join);
            }
            GameState::Installing { .. } | GameState::Queued => {
                let text = TT::ToastUnlocking.format(language, &[Arg::Str(&game.name)]);
                model.toast(text, Icon::LOCK);
                self.unlocking = Some(join);
            }
            _ => {}
        }
    }

    fn start_join(&mut self, model: &mut Model, join: Join, id: &GameId) {
        self.join = Some(join);
        model.play(id);
        // `play` refuses while another game starts: the join is not kept for a later Play.
        if model.starting.as_ref() != Some(id) {
            self.join = None;
        }
    }

    /// Once per frame: a join that waited for the unlock goes on when the game is unlocked, and
    /// is dropped when the unlock stopped.
    fn continue_unlocking(&mut self, model: &mut Model) {
        let Some(join) = self.unlocking.take() else {
            return;
        };
        let Some(game) = super::together::game_of(model, &join).cloned() else {
            return;
        };
        match model.state_of(&game) {
            GameState::Installed { .. } => self.start_join(model, join, &game.id),
            GameState::Installing { .. } | GameState::Queued => self.unlocking = Some(join),
            _ => {
                let text = TT::ToastUnlockFailed.text(model.language());
                model.toast(text, Icon::WARNING);
            }
        }
    }

    fn notice(&mut self, model: &mut Model, notice: Notice, ready: &mut Vec<ReadyStart>) {
        let language = model.language();
        match notice {
            Notice::Answered(purpose, result) => self.answered(model, purpose, result, ready),
            Notice::Request(account) => {
                let text = T::RequestArrived.format(language, &[Arg::Str(&account.handle)]);
                model.toast_with(text, Icon::USER_PLUS, Cue::Notify);
            }
            // A Play Together invitation is the card's: "Mira is playing Minecraft. Join?".
            Notice::Invitation(invitation) if invitation.kind == SessionKind::Lan => {}
            Notice::SessionEnded { session, host, .. } => {
                model.together.dismiss(&session);
                let text = TT::ToastSessionEnded.format(language, &[Arg::Str(&host.handle)]);
                model.toast(text, Icon::USERS);
            }
            Notice::HostFailed(reason) => {
                tracing::info!("Play Together could not open its session: {reason}");
                let text = TT::ToastHostFailed.text(language).to_owned();
                model.toast(text, Icon::WARNING);
            }
            Notice::Invitation(invitation) => {
                let text = T::InvitationArrived.format(
                    language,
                    &[
                        Arg::Str(&invitation.from.handle),
                        Arg::Str(&invitation.title),
                    ],
                );
                model.toast_with(text, Icon::ENVELOPE_SIMPLE, Cue::Invite);
            }
            // A call rings on its own card; the window comes forward so it can be answered.
            Notice::PartyInvitation(invitation) if invitation.call => {
                model.effects.push(Effect::RestoreWindow);
            }
            Notice::MissedCall(call) => {
                let text = CT::ToastMissed.format(language, &[Arg::Str(&call.from.handle)]);
                model.toast_with(text, Icon::USERS, Cue::Notify);
            }
            Notice::CallEnded(callee, reason) => {
                let text = match reason {
                    net::api::CallEnd::Declined => CT::ToastDeclined,
                    _ => CT::ToastNoAnswer,
                }
                .format(language, &[Arg::Str(&callee.handle)]);
                model.toast_with(text, Icon::USERS, Cue::Notify);
            }
            Notice::CallUnreachable(account) => {
                let text = CT::ToastUnreachable.format(language, &[Arg::Str(&account.handle)]);
                model.toast_with(text, Icon::USERS, Cue::Notify);
            }
            Notice::PartyInvitation(invitation) => {
                let text = S::PartyInvitationArrived
                    .format(language, &[Arg::Str(&invitation.from.handle)]);
                model.toast_with(text, Icon::USERS, Cue::Invite);
            }
            // A message toast only tells what the open panel does not already show.
            Notice::Message { message, .. } if model.overlay != Some(Overlay::Friends) => {
                let text = S::MessageArrived.format(
                    language,
                    &[
                        Arg::Str(&message.sender.handle),
                        Arg::Str(&net::snippet(&message.text, MESSAGE_TOAST_CHARS)),
                    ],
                );
                model.toast_with(text, Icon::ENVELOPE_SIMPLE, Cue::Message);
            }
            Notice::MessageRequest(request) if model.overlay != Some(Overlay::Friends) => {
                let me = model.online.account.as_ref().map(|a| a.id);
                let from = net::others(&request, me.unwrap_or(net::api::AccountId(0)))
                    .first()
                    .map(|a| a.handle.clone())
                    .unwrap_or_default();
                let text = S::MessageRequestArrived.format(language, &[Arg::Str(&from)]);
                model.toast_with(text, Icon::ENVELOPE_SIMPLE, Cue::Message);
            }
            // The open panel shows it; the sound still says it arrived.
            Notice::Message { .. } | Notice::MessageRequest(_) => model.sound(Cue::Message),
            Notice::Dialog(_) => {
                // The game asked to invite friends: the launcher comes forward with the picker.
                model.effects.push(Effect::RestoreWindow);
                if model.overlay != Some(Overlay::Friends) {
                    model.open(Overlay::Friends);
                }
            }
            Notice::VoiceFailed(problem) => {
                let text = if problem.denied {
                    VT::ToastDenied
                } else {
                    VT::ToastFailed
                };
                model.toast(text.text(language), Icon::MICROPHONE_SLASH);
            }
            Notice::Screen(notice) => super::online_screen::notice(model, notice),
            Notice::Restricted(restriction) => {
                // The panel shows it as a row; the toast is for a player who looks elsewhere.
                let text = restriction_text::toast(&restriction, None, None, language);
                model.toast(text, Icon::WARNING);
            }
            Notice::Banned(restriction) => {
                let text = restriction_text::toast(&restriction, None, None, language);
                model.toast(text, Icon::WARNING);
            }
            Notice::SignedIn => self.signed_in(model),
            Notice::SignInTimedOut => {
                let text = T::SignInTimedOut.text(language).to_owned();
                model.toast(text, Icon::CLOCK);
            }
            Notice::Ended => {
                self.process = None;
                if model.prefs.online {
                    let text = T::ServiceEnded.text(language).to_owned();
                    model.toast(text, Icon::WARNING);
                }
            }
        }
    }

    fn answered(
        &mut self,
        model: &mut Model,
        purpose: Purpose,
        result: Result<serde_json::Value, Failure>,
        ready: &mut Vec<ReadyStart>,
    ) {
        let language = model.language();
        let handle_of = |model: &Model, id| {
            model
                .online
                .known(id)
                .map(|a| a.handle.clone())
                .unwrap_or_default()
        };
        match (purpose, result) {
            (Purpose::GameStart(number), result) => {
                let Some(waiting) = self.waiting.take_if(|w| w.number == number) else {
                    return;
                };
                match result {
                    Ok(value) => {
                        let started =
                            serde_json::from_value::<net::control::GameStarted>(value).ok();
                        model.together.hosting = started
                            .as_ref()
                            .and_then(|g| g.session.clone())
                            .filter(|_| waiting.hosting);
                        let bridge = started.map(|g| g.env);
                        let mut env = record_env(model, &waiting.game);
                        env.extend(net::game_environment(bridge.as_ref(), None));
                        ready.push(ReadyStart {
                            game: waiting.game,
                            env,
                        });
                    }
                    Err(failure) if waiting.joining => {
                        self.call_off_start(model);
                        let (title, version) = model
                            .game(&waiting.game)
                            .map(|g| (g.name.clone(), g.version.clone()))
                            .unwrap_or_default();
                        match Blocker::from_failure(&failure, &title, &version, &build(model)) {
                            Some(blocker) => model.together.block(blocker),
                            None => {
                                let text = failure_text(&failure, language);
                                model.toast(text, Icon::WARNING);
                            }
                        }
                    }
                    Err(failure) => {
                        tracing::info!("game start without the network: {}", failure.message);
                        if waiting.hosting {
                            let text = TT::ToastHostFailed.text(language).to_owned();
                            model.toast(text, Icon::WARNING);
                        }
                        ready.push(ReadyStart {
                            env: record_env(model, &waiting.game),
                            game: waiting.game,
                        });
                    }
                }
            }
            (Purpose::CheckJoin(join), Ok(_)) => self.begin_join(model, join),
            (Purpose::CheckJoin(join), Err(failure)) => {
                // `join.version` and `join.build` are the player's own.
                match Blocker::from_failure(&failure, &join.title, &join.version, &join.build) {
                    Some(blocker) => model.together.block(blocker),
                    None => {
                        let text = failure_text(&failure, language);
                        model.toast(text, Icon::WARNING);
                    }
                }
            }
            (Purpose::BrowserSignIn { .. }, Ok(_)) => {
                if let Some(url) = model.online.sign_in_url.clone() {
                    Self::open_cloud_page(model, &url);
                }
            }
            (Purpose::AccountPage, Ok(value)) => {
                if let Ok(opened) = serde_json::from_value::<net::control::Opened>(value) {
                    Self::open_cloud_page(model, &opened.url);
                }
            }
            (Purpose::Appeal(_), Ok(_)) => {
                let text = restriction_text::RestrictionText::AppealSent
                    .text(language)
                    .to_owned();
                model.toast(text, Icon::CHECK);
            }
            (Purpose::CancelSignIn, _) => {}
            (Purpose::SignOut, _) => {
                model.edit_prefs(|p| p.online = false);
                let text = T::SignedOut.text(language).to_owned();
                model.toast(text, Icon::SIGN_OUT);
                model.effects.push(Effect::StopOnline);
            }
            (Purpose::AddFriend(handle), Ok(value)) => {
                let friends = serde_json::from_value::<net::control::FriendAdded>(value)
                    .is_ok_and(|a| a.result == FriendState::Friends);
                let text = if friends {
                    T::NowFriends
                } else {
                    T::RequestSent
                }
                .format(language, &[Arg::Str(&handle)]);
                model.toast(text, Icon::USER_PLUS);
            }
            (Purpose::Accept(id), Ok(_)) => {
                let text = T::NowFriends.format(language, &[Arg::Str(&handle_of(model, id))]);
                model.toast(text, Icon::USERS);
            }
            (Purpose::Block(id), Ok(_)) => {
                let text = T::BlockedToast.format(language, &[Arg::Str(&handle_of(model, id))]);
                model.toast(text, Icon::PROHIBIT);
            }
            (Purpose::Report(_), Ok(_)) => {
                let text = T::Reported.text(language).to_owned();
                model.toast(text, Icon::FLAG);
            }
            (Purpose::Invite | Purpose::Dialog, Ok(_)) => {}
            // The invitation a join link made arrives as the usual event; the panel's Home page
            // lists it with Join and Decline. Nothing joins by itself.
            (Purpose::JoinRedeem, Ok(_)) => {
                model.effects.push(Effect::RestoreWindow);
                if model.overlay != Some(Overlay::Friends) {
                    model.open(Overlay::Friends);
                }
            }
            (Purpose::JoinRedeem, Err(failure)) => {
                let text = redeem_failure(&failure, language)
                    .map_or_else(|| failure_text(&failure, language), str::to_owned);
                model.toast(text, Icon::WARNING);
            }
            (Purpose::PartyInvite | Purpose::PartyMove, Ok(_)) => {
                let text = T::InvitationsSent.text(language).to_owned();
                model.toast(text, Icon::PAPER_PLANE_RIGHT);
            }
            (
                Purpose::Hello
                | Purpose::ScreenWorker
                | Purpose::Playing
                | Purpose::GameEnd
                | Purpose::UploadTrophies { .. }
                | Purpose::UploadPlaytime(_)
                | Purpose::Cards
                | Purpose::Feed { .. }
                | Purpose::Profile(_)
                | Purpose::Compare(_)
                | Purpose::Secrets { .. }
                | Purpose::Privacy
                | Purpose::Party
                | Purpose::JoinCode
                | Purpose::JoinCodeRevoke
                | Purpose::Conversations
                | Purpose::Messages { .. }
                | Purpose::MarkRead { .. },
                Err(failure),
            ) => {
                tracing::info!("net service: {}: {}", failure.code, failure.message);
            }
            // Held keys and levels come and go many times a second: a failure is only logged.
            (Purpose::CallsSeen, Err(failure)) => {
                tracing::info!("net service: {}: {}", failure.code, failure.message);
            }
            (Purpose::PartyLink, Err(failure))
                if failure.code == ErrorCode::BadRequest.as_str() =>
            {
                model.toast(CT::LinkInvalid.text(language), Icon::WARNING);
            }
            (Purpose::Voice | Purpose::PushToTalk | Purpose::VoiceDevices, Err(failure)) => {
                tracing::info!("net service: {}: {}", failure.code, failure.message);
            }
            (Purpose::SetVoiceDevice(_), Err(failure)) => {
                tracing::info!("net service: {}: {}", failure.code, failure.message);
                // A device chosen earlier may be unplugged now; only a player who looks is told.
                if model.overlay == Some(Overlay::Friends) {
                    model.toast(VT::ToastNoDevice.text(language), Icon::WARNING);
                }
                model.online.request_voice_devices();
            }
            (
                Purpose::ScreenStart
                | Purpose::ScreenStop
                | Purpose::ScreenWatch(_)
                | Purpose::ScreenLeave,
                Err(failure),
            ) => super::online_screen::failed(model, &failure),
            // The count went back by itself; the player is told why it did.
            (Purpose::React { .. }, Err(failure)) => {
                tracing::info!("net service: {}: {}", failure.code, failure.message);
                let text = if failure.is(ErrorCode::RateLimited) {
                    failure_text(&failure, language)
                } else {
                    FT::ReactFailed.text(language).to_owned()
                };
                model.toast(text, Icon::WARNING);
            }
            (Purpose::SetPrivacy, Err(failure)) => {
                // The choice the panel shows goes back to what the cloud keeps.
                model.online.send(Purpose::Privacy, Command::Privacy);
                let text = failure_text(&failure, language);
                model.toast(text, Icon::WARNING);
            }
            (Purpose::ScreenWorker, Ok(value)) => {
                model.share_codec =
                    serde_json::from_value::<net::control::ScreenWorkerReady>(value)
                        .ok()
                        .and_then(|ready| {
                            ready
                                .libraries
                                .into_iter()
                                .find(|library| library.name == "openh264")
                        })
                        .map(|library| (library.state, library.detail));
            }
            (_, Ok(_)) => {}
            (purpose, Err(failure)) => {
                let what = model.online.blocked_by(&purpose);
                let text = match &failure.restriction {
                    Some(restriction) => restriction_text::toast(restriction, what, None, language),
                    None => failure_text(&failure, language),
                };
                model.toast(text, Icon::WARNING);
            }
        }
    }

    fn signed_in(&mut self, model: &mut Model) {
        model.edit_prefs(|p| p.online = true);
        if let Some(account) = &model.online.account {
            let text = T::Welcome.format(model.language(), &[Arg::Str(&account.handle)]);
            model.toast(text, Icon::USER_CIRCLE);
        }
    }

    /// Opens a page of the cloud in the system browser: the sign-in page or the account web. Only
    /// an address under the cloud the player uses is opened, whatever the net service says.
    pub fn open_cloud_page(model: &Model, url: &str) {
        let Some(cloud) = net::cloud(model.prefs.online_cloud.as_deref()) else {
            return;
        };
        let base = format!("{}/", cloud.trim_end_matches('/'));
        if !url.starts_with(&base) || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
            tracing::warn!(
                "not opening a page outside the cloud: {}",
                loggable_url(url)
            );
            return;
        }
        if let Err(error) = anyps5_core::platform::open_url(url) {
            tracing::warn!("cannot open the cloud's page: {error}");
        }
    }

    /// A join failed before its game started: the splash goes away.
    fn call_off_start(&mut self, model: &mut Model) {
        self.started = None;
        model.cancel_start();
    }
}

/// An address as a log line may carry it. A sign-in state sits in the query or the fragment, so
/// both are cut; what is left is escaped, so a newline in the address cannot start a line of its own.
fn loggable_url(url: &str) -> String {
    format!("{:?}", url.split(['?', '#']).next().unwrap_or_default())
}

/// The net service this launcher carries (release builds), written once per version to
/// `Network\bin` and run from there.
#[cfg(embedded_net)]
fn bundled(paths: &anyps5_core::paths::AppPaths) -> Option<std::path::PathBuf> {
    static NET: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/anyplay-net.bin"));
    extract(paths, NET, "anyplay-net-")
}

#[cfg(not(embedded_net))]
fn bundled(_paths: &anyps5_core::paths::AppPaths) -> Option<std::path::PathBuf> {
    None
}

/// The share worker this launcher carries (release builds), written like the net service.
#[cfg(embedded_share)]
fn bundled_worker(paths: &anyps5_core::paths::AppPaths) -> Option<std::path::PathBuf> {
    static WORKER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/anyplay-share.bin"));
    extract(paths, WORKER, "anyplay-share-")
}

#[cfg(not(embedded_share))]
fn bundled_worker(_paths: &anyps5_core::paths::AppPaths) -> Option<std::path::PathBuf> {
    None
}

/// Writes `bytes` once per version to `Network\bin\<prefix><version>-<size>` and returns the
/// path; older files with the same prefix go.
#[cfg(any(embedded_net, embedded_share))]
fn extract(
    paths: &anyps5_core::paths::AppPaths,
    bytes: &'static [u8],
    prefix: &str,
) -> Option<std::path::PathBuf> {
    let folder = paths.network_dir().join("bin");
    let name = format!(
        "{prefix}{}-{}{}",
        env!("CARGO_PKG_VERSION"),
        bytes.len(),
        std::env::consts::EXE_SUFFIX
    );
    let path = folder.join(&name);
    if std::fs::metadata(&path).is_ok_and(|m| m.len() == bytes.len() as u64) {
        return Some(path);
    }
    let write = || -> std::io::Result<()> {
        std::fs::create_dir_all(&folder)?;
        let partial = folder.join(format!("{name}.partial"));
        std::fs::write(&partial, bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&partial, std::fs::Permissions::from_mode(0o755))?;
        }
        std::fs::rename(&partial, &path)?;
        // Older versions go; one that still runs keeps its file until the next start.
        for entry in std::fs::read_dir(&folder)?.flatten() {
            let file = entry.file_name();
            if file != name.as_str() && file.to_string_lossy().starts_with(prefix) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
        Ok(())
    };
    match write() {
        Ok(()) => Some(path),
        Err(error) => {
            tracing::warn!("cannot write {prefix}: {error}");
            None
        }
    }
}

/// Share screen is switched on: by the preference, or by `ANYPLAY_SHARE_SCREEN=1` for one run.
pub(crate) fn share_screen_enabled(model: &Model) -> bool {
    model.prefs.online_share_screen
        || std::env::var_os(net::SHARE_SCREEN_VARIABLE).is_some_and(|v| v == "1")
}

/// The share worker on this PC: the one named by `ANYPLAY_SHARE`, next to the launcher, or the one
/// the launcher carries. Clips run it too.
pub fn share_worker_path(model: &Model) -> Option<std::path::PathBuf> {
    net::locate_worker().or_else(|| bundled_worker(&model.paths))
}

/// Tells the net service where the share worker is, when share screen is on and there is one.
/// Without this the net service reports neither `can_share` nor `can_watch` and the launcher
/// shows nothing of it.
fn tell_worker(model: &mut Model) {
    if !share_screen_enabled(model) {
        return;
    }
    // On Linux the codec is Cisco's download: while the player keeps it on, a missing one is
    // fetched again, and the worker is told once it is there.
    if cfg!(target_os = "linux") && model.prefs.online_openh264 {
        let source = anyps5_core::openh264::Source::default();
        if anyps5_core::openh264::state(&model.paths, &source)
            == anyps5_core::openh264::CodecState::Missing
        {
            anyps5_core::openh264::install_in_background(&model.paths, source, None);
        }
    }
    let Some(path) = net::locate_worker().or_else(|| bundled_worker(&model.paths)) else {
        tracing::info!("share screen is on, but there is no share worker");
        return;
    };
    send_worker(model, &path);
}

fn send_worker(model: &mut Model, path: &std::path::Path) {
    model.online.send(
        Purpose::ScreenWorker,
        Command::ScreenWorker {
            path: path.to_string_lossy().into_owned(),
        },
    );
}

/// The AnyPS5 build the version guard compares: the toolkit's version.
fn send_relay_only(model: &mut Model) {
    let on = model.prefs.online_relay_only;
    model
        .online
        .send(Purpose::RelayOnly, Command::RelayOnly { on });
}

pub(crate) fn build(model: &Model) -> String {
    model
        .toolkit
        .clone()
        .unwrap_or_else(|| "unknown".to_owned())
}

/// The observation log's variable, when the player records network calls.
fn record_env(model: &Model, game: &GameId) -> BTreeMap<String, String> {
    let record = model.prefs.record_network.then(|| {
        let title = model
            .game(game)
            .map(|g| g.title_id.clone())
            .unwrap_or_default();
        model.paths.network_record(&title)
    });
    if let Some(path) = &record
        && let Some(parent) = path.parent()
    {
        let _ = std::fs::create_dir_all(parent);
    }
    net::game_environment(None, record.as_deref())
}

/// Once per frame: the sign-in the player chose at the end of the first start. Opening the
/// online area brings the net service up; the browser sign-in starts once it has said hello.
/// A service that is missing, or a player already signed in, drops the wish.
pub fn poll_onboarding_sign_in(model: &mut Model) {
    let Some(create) = model.sign_in_after_onboarding else {
        return;
    };
    if model.online.is_signed_in() || model.online.status == Status::Missing {
        model.sign_in_after_onboarding = None;
        return;
    }
    if model.online.status == Status::Running && model.online.cloud.is_some() {
        model.sign_in_after_onboarding = None;
        model.online.send(
            Purpose::BrowserSignIn { create },
            Command::BrowserSignIn { create },
        );
    }
}

/// What a failure means for the player, and what to do about it.
pub fn failure_text(failure: &Failure, language: Language) -> String {
    let code = |c: ErrorCode| failure.is(c);
    // A restriction says what is limited and until when; an older net service sends none.
    if let Some(restriction) = &failure.restriction {
        return restriction_text::toast(restriction, None, None, language);
    }
    if code(ErrorCode::Restricted) {
        return restriction_text::RestrictionText::ToastUnknown
            .text(language)
            .to_owned();
    }
    let text = if failure.code == Failure::OFFLINE {
        T::Offline
    } else if code(ErrorCode::CodeWrong) {
        T::CodeWrong
    } else if code(ErrorCode::CodeExpired) {
        T::CodeExpired
    } else if code(ErrorCode::HandleTaken) {
        T::HandleTaken
    } else if code(ErrorCode::HandleInvalid) {
        T::HandleInvalid
    } else if code(ErrorCode::NotFound) {
        T::NotFound
    } else if code(ErrorCode::RateLimited) {
        T::RateLimited
    } else if code(ErrorCode::Banned) {
        T::Banned
    } else if code(ErrorCode::Full) {
        T::Full
    } else {
        return T::Failed.format(language, &[Arg::Str(&failure.message)]);
    };
    text.text(language).to_owned()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use anyps5_core::library::ExecutableKind;
    use anyps5_core::online::SignedIn;
    use anyps5_core::online::api::AccountId;
    use anyps5_core::paths::AppPaths;
    use anyps5_core::settings::Preferences;

    use super::*;

    #[test]
    fn a_logged_address_loses_query_and_fragment() {
        assert_eq!(
            loggable_url("https://cloud.example/sign-in?state=s3cret"),
            "\"https://cloud.example/sign-in\""
        );
        assert_eq!(
            loggable_url("https://cloud.example/sign-in#state=s3cret"),
            "\"https://cloud.example/sign-in\""
        );
        assert_eq!(
            loggable_url("https://cloud.example/a#f?q=s3cret"),
            "\"https://cloud.example/a\""
        );
    }

    #[test]
    fn a_logged_address_cannot_start_a_line() {
        let line =
            loggable_url("https://cloud.example/a\nPANIC thread=main at x:1: fake\r\u{1b}[2J");
        assert!(!line.contains(['\n', '\r', '\u{1b}']));
        assert!(line.starts_with('"') && line.ends_with('"'));
    }

    fn game() -> Game {
        let folder = PathBuf::from("C:/games/test");
        Game {
            id: GameId::from_path(&folder),
            folder,
            title_id: "TEST00001".to_owned(),
            name: "Test Game".to_owned(),
            names: Default::default(),
            version: "01.000".to_owned(),
            content_id: None,
            icon: None,
            background: None,
            music: None,
            executable: ExecutableKind::Elf,
            size: None,
            unlocked_with: Some("v1".to_owned()),
        }
    }

    fn signed_in_model() -> Model {
        let prefs = Preferences {
            onboarded: true,
            games_folder: Some(PathBuf::from("C:/games")),
            ..Preferences::default()
        };
        let mut model = Model::new(
            prefs,
            AppPaths::under(std::path::Path::new("C:/none")),
            false,
        );
        model.toolkit = Some("v1".to_owned());
        model.games = vec![game()];
        model.online.status = Status::Running;
        model.online.account = Some(SignedIn {
            id: AccountId(1),
            handle: "Alec".into(),
            email: "a@example.org".into(),
            appear_offline: false,
        });
        model
    }

    fn service() -> OnlineService {
        OnlineService::new(std::sync::Arc::new(|| {}), true)
    }

    #[test]
    fn offline_a_game_starts_at_once_without_the_bridge() {
        let mut model = signed_in_model();
        model.online.account = None;
        let env = service().on_play(&mut model, &game()).expect("starts now");
        assert!(env.is_empty());
    }

    #[test]
    fn signed_in_a_start_waits_for_the_bridge_and_then_carries_it() {
        let mut model = signed_in_model();
        let mut online = service();
        assert_eq!(online.on_play(&mut model, &game()), None);
        let sent = model.online.take_outbox();
        assert!(sent[0].contains("\"do\":\"game_start\""));
        assert!(sent[0].contains("\"build\":\"v1\""));
        let mut ready = Vec::new();
        let answer = serde_json::json!({"env": {"ANYPS5_NET_ENDPOINT": "127.0.0.1:5000", "ANYPS5_NET_TOKEN": "ab"}});
        online.answered(&mut model, Purpose::GameStart(1), Ok(answer), &mut ready);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].game, game().id);
        assert_eq!(ready[0].env["ANYPS5_NET_TOKEN"], "ab");
        // The end of the game is reported once.
        online.on_exited(&mut model, &game().id);
        online.on_exited(&mut model, &game().id);
        let ends = model.online.take_outbox();
        assert_eq!(ends.len(), 1);
        assert!(ends[0].contains("game_end"));
    }

    /// Plays the game and ends the session, as the launcher sees it.
    fn play_and_end(model: &mut Model, online: &mut OnlineService) {
        online.on_play(model, &game());
        online.on_exited(model, &game().id);
        model.online.take_outbox();
    }

    #[test]
    fn a_session_end_asks_how_it_ran_once_and_the_answer_goes_out() {
        let mut model = signed_in_model();
        model.online.cloud = Some(anyps5_core::online::Cloud::Online);
        let mut online = service();
        play_and_end(&mut model, &mut online);
        assert!(model.run_question().is_some(), "asked after the session");

        model.answer_run(3);
        let sent = model.online.take_outbox();
        let report = sent
            .iter()
            .find(|line| line.contains("\"do\":\"run_report\""))
            .expect("the answer is sent");
        for part in [
            "\"title_id\":\"TEST00001\"",
            "\"version\":\"01.000\"",
            "\"toolkit\":\"v1\"",
            "\"verdict\":\"playable\"",
        ] {
            assert!(report.contains(part), "{report}");
        }
        assert!(model.run_question().is_none());

        // The same game version with the same toolkit is not asked again.
        play_and_end(&mut model, &mut online);
        assert!(model.run_prompt.is_none());
        // A new toolkit version asks again.
        model.games[0].unlocked_with = Some("v2".to_owned());
        play_and_end(&mut model, &mut online);
        assert!(model.run_question().is_some());
    }

    #[test]
    fn a_dismiss_sends_nothing_and_counts_as_asked() {
        let mut model = signed_in_model();
        let mut online = service();
        play_and_end(&mut model, &mut online);
        model.answer_run(crate::ui::focus::ASK_LAST);
        assert!(model.online.take_outbox().is_empty());
        assert!(model.run_prompt.is_none());
        play_and_end(&mut model, &mut online);
        assert!(model.run_prompt.is_none(), "dismissed once, done");
    }

    #[test]
    fn signed_out_nothing_is_asked_or_sent() {
        let mut model = signed_in_model();
        let mut online = service();
        online.on_play(&mut model, &game());
        model.online.take_outbox();
        model.online.account = None;
        online.on_exited(&mut model, &game().id);
        assert!(model.run_prompt.is_none());
        model.answer_run(3);
        assert!(
            model
                .online
                .take_outbox()
                .iter()
                .all(|line| !line.contains("run_report"))
        );
    }

    #[test]
    fn a_failed_join_calls_the_start_off_but_a_failed_start_plays_offline() {
        let mut model = signed_in_model();
        let mut online = service();
        online.join = Some(Join {
            session: "s1".into(),
            title_id: "TEST00001".into(),
            title: "Test Game".into(),
            version: "01.000".into(),
            build: "v1".into(),
            friend: "Lumen".into(),
        });
        model.play(&game().id);
        assert!(model.starting.is_some());
        assert_eq!(online.on_play(&mut model, &game()), None);
        assert!(model.online.take_outbox()[0].contains("\"join\":\"s1\""));
        let mut ready = Vec::new();
        online.answered(
            &mut model,
            Purpose::GameStart(1),
            Err(Failure::new("full", "Full.")),
            &mut ready,
        );
        assert!(ready.is_empty());
        assert!(model.starting.is_none());
        assert_eq!(
            model.together.blocked,
            Some(anyps5_core::play_together::Blocker::Full),
            "a full session is a blocker on the card, not a toast"
        );

        // Without a join, a start the network refuses goes ahead offline.
        assert_eq!(online.on_play(&mut model, &game()), None);
        online.answered(
            &mut model,
            Purpose::GameStart(2),
            Err(Failure::new("offline", "x")),
            &mut ready,
        );
        assert_eq!(ready.len(), 1);
        assert!(ready[0].env.is_empty());
    }

    #[test]
    fn relay_only_reaches_the_net_service_at_once_or_after_the_game() {
        let mut model = signed_in_model();
        let mut online = service();
        model.prefs.online_relay_only = true;
        online.relay_only_changed(&mut model);
        let sent = model.online.take_outbox();
        assert_eq!(sent.len(), 1);
        assert!(sent[0].contains("\"do\":\"relay_only\",\"on\":true"));

        // During a game the tunnels stay as they are until it ends.
        assert_eq!(online.on_play(&mut model, &game()), None);
        model.online.take_outbox();
        model.prefs.online_relay_only = false;
        online.relay_only_changed(&mut model);
        assert!(model.online.take_outbox().is_empty());
        online.on_exited(&mut model, &game().id);
        let sent = model.online.take_outbox();
        assert!(sent[0].contains("game_end"));
        assert!(sent[1].contains("\"do\":\"relay_only\",\"on\":false"));

        // A stopped service hears of it when it starts.
        model.online.status = Status::Off;
        online.relay_only_changed(&mut model);
        assert!(model.online.take_outbox().is_empty());
    }

    #[test]
    fn the_share_worker_is_only_handed_over_when_share_screen_is_on() {
        let mut model = signed_in_model();
        model.prefs.online_share_screen = false;
        // Off: nothing goes out, whatever worker there may be.
        tell_worker(&mut model);
        assert!(model.online.take_outbox().is_empty());
        // On, with a worker: the net service is told where it is.
        model.prefs.online_share_screen = true;
        send_worker(&mut model, std::path::Path::new("C:/x/anyplay-share.exe"));
        let sent = model.online.take_outbox();
        assert_eq!(sent.len(), 1);
        assert!(sent[0].contains("\"do\":\"screen_worker\""), "{}", sent[0]);
        assert!(sent[0].contains("anyplay-share.exe"));
    }

    fn message(from: u64, handle: &str, text: &str) -> net::api::Message {
        net::api::Message {
            conversation: "c".into(),
            seq: 1,
            sender: net::api::Account {
                id: AccountId(from),
                handle: handle.into(),
            },
            text: text.into(),
            created: 0,
            reply: None,
            edited: None,
            deleted: false,
            reactions: Vec::new(),
            rev: 0,
        }
    }

    #[test]
    fn a_new_message_toasts_only_while_the_panel_is_closed() {
        let mut model = signed_in_model();
        let mut ready = Vec::new();
        let notice = || Notice::Message {
            conversation: "c".into(),
            message: message(2, "Lumen", "Ready when you are.\n\nSee you at the lobby"),
        };
        service().notice(&mut model, notice(), &mut ready);
        assert_eq!(
            model.toast.as_ref().map(|t| t.text.as_str()),
            Some("Lumen: Ready when you are. See you at the lobby"),
            "one line, the sender first"
        );
        model.toast = None;
        model.open(Overlay::Friends);
        service().notice(&mut model, notice(), &mut ready);
        assert!(model.toast.is_none(), "the open panel shows it already");
    }

    #[test]
    fn a_long_message_is_cut_for_the_toast() {
        let mut model = signed_in_model();
        let mut ready = Vec::new();
        let text = "word ".repeat(40);
        service().notice(
            &mut model,
            Notice::Message {
                conversation: "c".into(),
                message: message(2, "Lumen", &text),
            },
            &mut ready,
        );
        let shown = model.toast.as_ref().unwrap().text.clone();
        assert!(shown.starts_with("Lumen: word word"));
        assert!(
            shown.ends_with('…') && shown.chars().count() <= "Lumen: ".len() + MESSAGE_TOAST_CHARS
        );
    }

    #[test]
    fn a_party_invitation_and_a_stranger_toast() {
        let mut model = signed_in_model();
        let mut ready = Vec::new();
        service().notice(
            &mut model,
            Notice::PartyInvitation(net::api::PartyInvitation {
                id: "i".into(),
                from: net::api::Account {
                    id: AccountId(2),
                    handle: "Lumen".into(),
                },
                party: "p".into(),
                members: 2,
                expires: 0,
                call: false,
            }),
            &mut ready,
        );
        assert_eq!(
            model.toast.as_ref().map(|t| t.text.as_str()),
            Some("Lumen invites you to a party"),
            "even with the panel open: the invitation expires"
        );
        model.toast = None;
        let request = net::api::Conversation {
            id: "r".into(),
            kind: net::api::ConversationKind::Direct,
            name: None,
            members: vec![
                net::api::Account {
                    id: AccountId(1),
                    handle: "Alec".into(),
                },
                net::api::Account {
                    id: AccountId(9),
                    handle: "Vale".into(),
                },
            ],
            last: 1,
            read: 0,
            marks: Vec::new(),
            updated: 0,
            request: true,
            party: None,
            unread: 0,
            muted: false,
            pinned: false,
            rev: 0,
        };
        service().notice(&mut model, Notice::MessageRequest(request), &mut ready);
        assert_eq!(
            model.toast.as_ref().map(|t| t.text.as_str()),
            Some("Vale wants to message you")
        );
    }

    fn with_restriction(code: &str, kind: &str, until: Option<u64>) -> Failure {
        Failure {
            restriction: Some(Box::new(net::control::Restriction::new(kind, until))),
            ..Failure::new(code, "x")
        }
    }

    // 2026-10-25 12:00 UTC, which is the 25th or the 26th in any time zone.
    const END: u64 = 1_792_929_600;

    #[test]
    fn a_restricted_account_is_told_what_and_until_when() {
        let muted = with_restriction("restricted", "muted", Some(END));
        let text = failure_text(&muted, Language::English);
        assert!(
            text.starts_with("Messages and friend requests are paused until Oct 2"),
            "{text}"
        );
        assert!(failure_text(&muted, Language::Turkish).contains("durduruldu"));
        let forever = with_restriction("restricted", "muted", None);
        assert_eq!(
            failure_text(&forever, Language::English),
            "Messages and friend requests are paused."
        );
    }

    #[test]
    fn an_older_net_service_says_nothing_of_what_is_restricted() {
        assert_eq!(
            failure_text(&Failure::new("restricted", "Too new."), Language::English),
            "This account cannot do that right now."
        );
    }

    #[test]
    fn a_refused_friend_request_names_the_action_in_its_toast() {
        let mut model = signed_in_model();
        let mut ready = Vec::new();
        let failure = with_restriction("restricted", "new_account", Some(END));
        service().notice(
            &mut model,
            Notice::Answered(Purpose::AddFriend("Lumen".into()), Err(failure)),
            &mut ready,
        );
        let text = model
            .toast
            .as_ref()
            .map(|t| t.text.clone())
            .unwrap_or_default();
        assert!(
            text.starts_with(
                "New accounts can send a few friend requests. You can send more after Oct 2"
            ),
            "{text}"
        );
    }

    #[test]
    fn a_ban_toasts_where_to_read_why() {
        let mut model = signed_in_model();
        let mut ready = Vec::new();
        let ban = net::control::Restriction::new(net::control::Restriction::BANNED, None);
        service().notice(&mut model, Notice::Banned(ban), &mut ready);
        assert_eq!(
            model.toast.as_ref().map(|t| t.text.as_str()),
            Some("This account is banned. Open Online to read why and how to appeal.")
        );
    }

    #[test]
    fn the_worker_s_word_on_openh264_reaches_the_codec_row() {
        let mut model = signed_in_model();
        let mut ready = Vec::new();
        let answer = |value| Notice::Answered(Purpose::ScreenWorker, Ok(value));
        service().notice(
            &mut model,
            answer(serde_json::json!({
                "encoders": [], "decoders": [], "capture": true,
                "libraries": [{"name": "openh264", "state": "refused", "detail": "/x: SHA-256 ab"}]
            })),
            &mut ready,
        );
        assert_eq!(
            model.share_codec,
            Some(("refused".into(), Some("/x: SHA-256 ab".into())))
        );
        // A worker that loads no library (Windows, or an older one) says nothing of it.
        service().notice(
            &mut model,
            answer(serde_json::json!({"encoders": [], "decoders": [], "capture": true})),
            &mut ready,
        );
        assert_eq!(model.share_codec, None);
    }

    #[test]
    fn a_sent_appeal_is_confirmed() {
        let mut model = signed_in_model();
        let mut ready = Vec::new();
        service().notice(
            &mut model,
            Notice::Answered(Purpose::Appeal(4), Ok(serde_json::json!({}))),
            &mut ready,
        );
        assert_eq!(
            model.toast.as_ref().map(|t| t.text.as_str()),
            Some("Appeal sent.")
        );
    }

    #[test]
    fn a_redeemed_join_code_opens_the_panel_and_joins_nothing() {
        let mut model = signed_in_model();
        let mut ready = Vec::new();
        service().notice(
            &mut model,
            Notice::Answered(
                Purpose::JoinRedeem,
                Ok(serde_json::json!({"kind": "party"})),
            ),
            &mut ready,
        );
        assert_eq!(model.overlay, Some(Overlay::Friends));
        assert!(ready.is_empty(), "no game starts");
        assert!(model.online.take_outbox().is_empty(), "no join is sent");
        assert!(model.starting.is_none() && model.running.is_none());
    }

    #[test]
    fn a_refused_join_code_says_why_in_plain_words() {
        for (code, expected) in [
            ("not_found", "expired"),
            ("full", "full"),
            ("limit", "already"),
            ("bad_request", "your own"),
            ("rate_limited", "Too many tries"),
        ] {
            let mut model = signed_in_model();
            let mut ready = Vec::new();
            service().notice(
                &mut model,
                Notice::Answered(Purpose::JoinRedeem, Err(Failure::new(code, "x"))),
                &mut ready,
            );
            let toast = model
                .toast
                .as_ref()
                .map(|t| t.text.clone())
                .unwrap_or_default();
            assert!(toast.contains(expected), "{code}: {toast}");
            assert_eq!(model.overlay, None, "{code}");
        }
    }

    #[test]
    fn a_failed_join_code_for_the_button_is_only_logged() {
        let mut model = signed_in_model();
        let mut ready = Vec::new();
        for purpose in [Purpose::JoinCode, Purpose::JoinCodeRevoke] {
            service().notice(
                &mut model,
                Notice::Answered(purpose, Err(Failure::new("not_found", "x"))),
                &mut ready,
            );
        }
        assert!(model.toast.is_none(), "the player did not ask for it");
    }

    #[test]
    fn failures_say_what_to_do() {
        let en = |code: &str| failure_text(&Failure::new(code, "x"), Language::English);
        assert_eq!(
            en("offline"),
            "The cloud cannot be reached. Check your connection and try again."
        );
        assert_eq!(
            en("code_wrong"),
            "That code does not match. Check the latest e-mail."
        );
        assert_eq!(en("handle_taken"), "That name is taken. Try another one.");
        assert_eq!(
            failure_text(
                &Failure::new("limit", "Too many friends."),
                Language::English
            ),
            "That did not work: Too many friends."
        );
    }

    // --- Play Together ---------------------------------------------------------------------

    fn lan_game() -> Game {
        let folder = PathBuf::from("C:/games/mc");
        Game {
            id: GameId::from_path(&folder),
            folder,
            title_id: "PPSA17221".to_owned(),
            name: "Minecraft".to_owned(),
            version: "01.044.000".to_owned(),
            ..game()
        }
    }

    fn lan_model() -> Model {
        let mut model = signed_in_model();
        model.games = vec![lan_game()];
        model.online.cloud = Some(anyps5_core::online::Cloud::Online);
        model
    }

    fn offer(version: &str) -> anyps5_core::play_together::Offer {
        anyps5_core::play_together::Offer {
            session: "s1".into(),
            title_id: "PPSA17221".into(),
            title: "Minecraft".into(),
            version: version.into(),
            kind: anyps5_core::online::api::SessionKind::Lan,
            host: "Mira".into(),
            places: Some((2, 4)),
            build: None,
        }
    }

    fn mira() -> net::api::Account {
        net::api::Account {
            id: AccountId(2),
            handle: "Mira".into(),
        }
    }

    #[test]
    fn a_start_carries_a_host_plan_only_for_play_together() {
        let mut model = lan_model();
        let mut online = service();
        // A plain Play hosts nothing.
        online.on_play(&mut model, &lan_game());
        let sent = model.online.take_outbox();
        assert!(sent[0].contains("\"do\":\"game_start\""));
        assert!(!sent[0].contains("\"host\""), "{}", sent[0]);
        online.on_exited(&mut model, &lan_game().id);
        model.online.take_outbox();

        // Play Together does, with the table's players and no party.
        crate::app::together::host_game(&mut model, &lan_game().id);
        assert!(model.starting.is_some());
        online.on_play(&mut model, &lan_game());
        let sent = model.online.take_outbox();
        assert!(sent[0].contains("\"max_members\":8"), "{}", sent[0]);
        assert!(sent[0].contains("\"visibility\":\"friends\""));
        assert!(sent[0].contains("\"party\":false"));

        let mut ready = Vec::new();
        let answer = serde_json::json!({"env": {}, "session": "s9"});
        online.answered(&mut model, Purpose::GameStart(2), Ok(answer), &mut ready);
        assert_eq!(ready.len(), 1);
        assert_eq!(model.together.hosting.as_deref(), Some("s9"));
        online.on_exited(&mut model, &lan_game().id);
        assert_eq!(model.together.hosting, None);
    }

    #[test]
    fn play_together_is_refused_outside_the_table_and_while_offline() {
        let mut model = signed_in_model();
        model.online.cloud = Some(anyps5_core::online::Cloud::Online);
        crate::app::together::host_game(&mut model, &game().id);
        assert!(matches!(
            model.together.blocked,
            Some(anyps5_core::play_together::Blocker::NotSupported { .. })
        ));
        assert!(model.starting.is_none());

        let mut model = lan_model();
        model.online.cloud = None;
        crate::app::together::host_game(&mut model, &lan_game().id);
        assert_eq!(
            model.together.blocked,
            Some(anyps5_core::play_together::Blocker::Offline)
        );
        assert!(model.starting.is_none());
    }

    #[test]
    fn a_blocked_join_sends_nothing_and_says_what_is_missing() {
        use anyps5_core::play_together::Blocker;
        // Not owned.
        let mut model = lan_model();
        model.games = vec![game()];
        crate::app::together::join(&mut model, offer("01.044.000"));
        assert_eq!(
            model.together.blocked,
            Some(Blocker::NotOwned {
                title: "Minecraft".into()
            })
        );
        assert!(model.online.take_outbox().is_empty());

        // Wrong version.
        let mut model = lan_model();
        crate::app::together::join(&mut model, offer("01.100.000"));
        assert_eq!(
            model.together.blocked,
            Some(Blocker::WrongVersion {
                title: "Minecraft".into(),
                need: "01.100.000".into(),
                have: "01.044.000".into()
            })
        );
        assert!(model.online.take_outbox().is_empty());

        // Busy.
        let mut model = lan_model();
        model.starting = Some(lan_game().id);
        crate::app::together::join(&mut model, offer("01.044.000"));
        assert_eq!(model.together.blocked, Some(Blocker::Busy));
        assert!(model.online.take_outbox().is_empty());
    }

    fn mc_join(version: &str) -> Join {
        Join {
            session: "s1".into(),
            title_id: "PPSA17221".into(),
            title: "Minecraft".into(),
            version: version.into(),
            build: "v1".into(),
            friend: "Mira".into(),
        }
    }

    #[test]
    fn a_passing_join_asks_the_net_service_then_starts_in_the_session() {
        let mut model = lan_model();
        let mut online = service();
        crate::app::together::join(&mut model, offer("01.044.000"));
        assert!(model.together.blocked.is_none());
        let sent = model.online.take_outbox();
        assert!(sent[0].contains("\"do\":\"check_join\""), "{}", sent[0]);
        let mut ready = Vec::new();
        online.answered(
            &mut model,
            Purpose::CheckJoin(mc_join("01.044.000")),
            Ok(serde_json::json!({})),
            &mut ready,
        );
        assert!(model.starting.is_some());
        online.on_play(&mut model, &lan_game());
        let sent = model.online.take_outbox();
        assert!(sent[0].contains("\"join\":\"s1\""), "{}", sent[0]);
        assert!(!sent[0].contains("\"host\""));
    }

    #[test]
    fn the_net_services_refusals_become_the_same_blockers() {
        use anyps5_core::play_together::Blocker;
        let mut model = lan_model();
        let mut online = service();
        let mut ready = Vec::new();
        let mut mismatch = Failure::new("version_mismatch", "x");
        mismatch.expected = Some(anyps5_core::online::api::Expected {
            version: "01.044.000".into(),
            build: "v1".into(),
        });
        online.answered(
            &mut model,
            Purpose::CheckJoin(mc_join("01.008.000")),
            Err(mismatch),
            &mut ready,
        );
        assert_eq!(
            model.together.blocked,
            Some(Blocker::WrongVersion {
                title: "Minecraft".into(),
                need: "01.044.000".into(),
                have: "01.008.000".into()
            })
        );
        online.answered(
            &mut model,
            Purpose::CheckJoin(mc_join("01.044.000")),
            Err(Failure::new("not_found", "x")),
            &mut ready,
        );
        assert_eq!(model.together.blocked, Some(Blocker::Ended));
    }

    #[test]
    fn an_ended_session_and_a_failed_host_are_told_and_the_game_goes_on() {
        let mut model = lan_model();
        let mut online = service();
        let mut ready = Vec::new();
        online.notice(
            &mut model,
            Notice::SessionEnded {
                session: "s1".into(),
                title_id: "PPSA17221".into(),
                title: "Minecraft".into(),
                host: mira(),
            },
            &mut ready,
        );
        let text = model.toast.as_ref().map(|t| t.text.clone()).unwrap();
        assert!(text.starts_with("Mira\u{2019}s session ended"), "{text}");
        assert!(ready.is_empty());

        online.notice(&mut model, Notice::HostFailed("x".into()), &mut ready);
        let text = model.toast.as_ref().map(|t| t.text.clone()).unwrap();
        assert!(text.starts_with("Nobody can join"), "{text}");
    }

    #[test]
    fn a_lan_invitation_is_a_card_not_a_toast_and_not_now_declines_it() {
        let mut model = lan_model();
        let mut online = service();
        let invitation = net::api::Invitation {
            id: "i1".into(),
            from: mira(),
            session: "s1".into(),
            kind: net::api::SessionKind::Lan,
            title_id: "PPSA17221".into(),
            title: "Minecraft".into(),
            version: "01.044.000".into(),
            expires: 0,
        };
        model.online.invitations.push(invitation.clone());
        let mut ready = Vec::new();
        online.notice(&mut model, Notice::Invitation(invitation), &mut ready);
        assert!(model.toast.is_none());
        let Some(crate::app::together::Card::Offer {
            key, invitation, ..
        }) = crate::app::together::card(&model)
        else {
            panic!("an offer card");
        };
        crate::app::together::apply(
            &mut model,
            crate::app::together::Action::Dismiss { key, invitation },
        );
        assert!(crate::app::together::card(&model).is_none());
        assert!(model.online.take_outbox()[0].contains("answer_invitation"));
    }
}
