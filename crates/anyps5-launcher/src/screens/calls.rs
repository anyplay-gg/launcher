//! Calls on screen (docs/DESIGN.md 17, "Calls"): the card that rings when a friend calls, and the
//! small strip that stays while the player is on a call.
//!
//! Both are glass surfaces at the top centre, below the toast, on their own layer. Neither ever
//! shows at the same time as the other: the cloud does not ring someone who is in a party. They
//! come in with the overlay spring (fade and a short slide down) and leave the same way, and
//! nothing on them moves while they stay: no pulse on the ring, no waveform. Who speaks is said in
//! words ("Lumen · speaking") and in the ink of the name.
//!
//! The pad and keyboard reach a ringing call through the Online panel's Invitations row; the card
//! is for the pointer and for Tab, and each button shows a focus ring when it has the keyboard.

use std::time::{SystemTime, UNIX_EPOCH};

use anyps5_core::online::{self as net, CallState, VoiceProvider};
use egui::{Align2, Area, Color32, Context, Id, Order, Rect, Sense, Ui, UiBuilder, pos2, vec2};

use crate::app::model::Model;
use crate::screens::online_call_strings::CallText as CT;
use crate::screens::shared::paint_gradient_circle;
use crate::ui::Shell;
use crate::ui::anim::{AnimExt, Motion};
use crate::ui::components::{ConsoleButton, paint_shadow};
use crate::ui::glass::{GlassStyle, paint_glass};
use crate::ui::i18n::{Arg, Language};
use crate::ui::icons::Icon;
use crate::ui::sound::Cue;
use crate::ui::text;
use crate::ui::theme::{self, Weight, fade, radius, shadows, size, ty, white};

/// Where the surfaces hang: below the toast, which owns the top 28 dp.
const TOP: f32 = size::TOAST_TOP + 72.0;
const CARD_WIDTH: f32 = 400.0;
const PAD: f32 = 24.0;
const AVATAR: f32 = 56.0;
/// The gap between controls: 12 dp, as everywhere in the launcher's rows of buttons.
const GAP: f32 = 12.0;
/// A ring every this many seconds while a call rings.
const RING_EVERY: f64 = 3.2;

/// What the player did on the ringing card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CardAction {
    Accept(String),
    Decline(String),
}

/// What the player did on the strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StripAction {
    ToggleMute,
    ToggleDeafen,
    Leave,
    OpenDiscord,
}

/// One member of the call as the strip shows them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub handle: String,
    pub speaking: bool,
}

/// What the strip says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StripKind {
    /// The player called and waits; the strip offers Cancel.
    Calling(String),
    Connected(Vec<Member>),
}

/// The strip's content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StripData {
    pub kind: StripKind,
    pub muted: bool,
    pub deafened: bool,
    /// The player's voice is on Discord: no microphone controls, the party's link instead.
    pub discord: Option<DiscordLine>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscordLine {
    pub link: Option<String>,
}

/// The ringing card's content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardData {
    pub invitation: String,
    pub handle: String,
    pub discord: bool,
}

/// State kept between frames: the ring.
#[derive(Debug, Default)]
pub struct CallsUi {
    ringing: Option<String>,
    next_ring: f64,
    /// What each surface showed last, so it can fade out with its content.
    last_card: Option<CardData>,
    last_strip: Option<StripData>,
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl CallsUi {
    /// Reads the model, rings, paints the surfaces and carries out what the player pressed.
    pub fn show(&mut self, ctx: &Context, model: &mut Model, shell: &mut Shell) {
        let language = model.language();
        let card = card_data(&model.online, unix_now());
        self.ring(ctx, shell, card.as_ref());
        let strip = strip_data(&model.online);

        if card.is_some() {
            self.last_card = card.clone();
        }
        let shown = ctx.animate_to(
            "call-card",
            f32::from(u8::from(card.is_some())),
            Motion::Overlay,
        );
        if let Some(data) = card.as_ref().or(self.last_card.as_ref())
            && let Some(action) = incoming_card(
                ctx,
                shell.accent_color(),
                data,
                language,
                shown,
                card.is_some(),
            )
        {
            match action {
                CardAction::Accept(id) => model.online.accept_call(&id),
                CardAction::Decline(id) => model.online.decline_call(&id),
            }
        }
        if shown <= 0.003 && card.is_none() {
            self.last_card = None;
        }

        if strip.is_some() {
            self.last_strip = strip.clone();
        }
        let shown = ctx.animate_to(
            "call-strip",
            f32::from(u8::from(strip.is_some())),
            Motion::Overlay,
        );
        if let Some(data) = strip.as_ref().or(self.last_strip.as_ref())
            && let Some(action) = call_strip(ctx, data, language, shown, strip.is_some())
        {
            match action {
                StripAction::ToggleMute => {
                    let muted = !model.online.voice.muted;
                    model.online.set_mic_mute(muted);
                }
                StripAction::ToggleDeafen => {
                    let deafened = !model.online.voice.deafened;
                    model.online.set_deafen(deafened);
                }
                StripAction::Leave => model.online.hang_up(),
                StripAction::OpenDiscord => open_party_link(model),
            }
        }
        if shown <= 0.003 && strip.is_none() {
            self.last_strip = None;
        }
    }

    /// Plays the ring once a call starts to ring and again every few seconds, while the card
    /// stays. The cue respects the sound settings like every other.
    fn ring(&mut self, ctx: &Context, shell: &mut Shell, card: Option<&CardData>) {
        let now = ctx.clock();
        match card {
            Some(card) => {
                if self.ringing.as_deref() != Some(&card.invitation) {
                    self.ringing = Some(card.invitation.clone());
                    self.next_ring = now;
                }
                if now >= self.next_ring {
                    shell.play(Cue::Ring);
                    self.next_ring = now + RING_EVERY;
                }
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                    (self.next_ring - now).max(0.0) + 0.02,
                ));
            }
            None => self.ringing = None,
        }
    }
}

/// The call that rings now, if any.
pub fn card_data(online: &net::Online, now: u64) -> Option<CardData> {
    let invitation = online.incoming_call(now)?;
    Some(CardData {
        invitation: invitation.id.clone(),
        handle: invitation.from.handle.clone(),
        discord: online.voice.provider() == VoiceProvider::Discord,
    })
}

/// What the strip shows now: a call being placed, or one in progress.
pub fn strip_data(online: &net::Online) -> Option<StripData> {
    let discord = (online.voice.provider() == VoiceProvider::Discord).then(|| DiscordLine {
        link: online
            .social
            .party
            .as_ref()
            .and_then(|p| p.discord.clone())
            .filter(|link| net::api::is_discord_link(link)),
    });
    let kind = match online.call_state() {
        CallState::Idle => return None,
        CallState::Calling(account) => StripKind::Calling(account.handle.clone()),
        CallState::Connected => StripKind::Connected(
            online
                .call_members()
                .into_iter()
                .map(|m| Member {
                    handle: m.handle.clone(),
                    speaking: online.voice.is_speaking(m.id),
                })
                .collect(),
        ),
    };
    Some(StripData {
        kind,
        muted: online.voice.muted,
        deafened: online.voice.deafened,
        discord,
    })
}

/// Opens the party's Discord link in the system browser. Only a Discord address is opened,
/// whatever the party says.
pub fn open_party_link(model: &Model) {
    let Some(link) = model
        .online
        .social
        .party
        .as_ref()
        .and_then(|p| p.discord.as_deref())
    else {
        return;
    };
    open_discord_link(link);
}

pub fn open_discord_link(link: &str) {
    if !net::api::is_discord_link(link) {
        tracing::warn!("not opening a link that is not Discord's: {link}");
        return;
    }
    if let Err(error) = anyps5_core::platform::open_url(link) {
        tracing::warn!("cannot open the Discord link: {error}");
    }
}

/// The slide and fade of both surfaces: they drop in from 24 dp above and leave the same way.
fn entrance(shown: f32) -> (f32, f32) {
    let v = shown.clamp(0.0, 1.0);
    (v, -(1.0 - v) * 24.0)
}

/// A button's focus from the keyboard: the id it will get, looked up in egui's memory.
fn keyboard_focus(ui: &Ui) -> bool {
    let id = ui.next_auto_id();
    ui.ctx().memory(|m| m.has_focus(id))
}

/// The ringing card. `live` is false while it fades out: it can no longer be pressed.
pub(crate) fn incoming_card(
    ctx: &Context,
    accent: Color32,
    data: &CardData,
    language: Language,
    shown: f32,
    live: bool,
) -> Option<CardAction> {
    let (a, slide) = entrance(shown);
    if a <= 0.003 {
        return None;
    }
    let screen = ctx.content_rect();
    let height = PAD * 2.0 + AVATAR + 20.0 + size::BUTTON_MIN_HEIGHT;
    let rect = Rect::from_min_size(
        pos2(
            screen.center().x - CARD_WIDTH * 0.5,
            screen.min.y + TOP + slide,
        ),
        vec2(CARD_WIDTH, height),
    );
    let mut action = None;
    Area::new(Id::unique("incoming-call"))
        .order(Order::Foreground)
        .fixed_pos(rect.min)
        .interactable(live)
        .show(ctx, |ui| {
            let painter = ui.painter().clone();
            paint_shadow(
                &painter,
                rect,
                radius::TOAST * 0.5,
                egui::epaint::Shadow {
                    color: fade(shadows::toast().color, a * a),
                    ..shadows::toast()
                },
            );
            let mut style = GlassStyle::glass(radius::TOAST * 0.5, 0.6);
            style.black *= a;
            style.alpha = a;
            paint_glass(ctx, &painter, rect, style);

            // The caller: avatar, name, and what is happening.
            let inner = rect.shrink(PAD);
            let centre = pos2(inner.min.x + AVATAR * 0.5, inner.min.y + AVATAR * 0.5);
            paint_gradient_circle(
                &painter,
                centre,
                AVATAR * 0.5,
                fade(accent, a),
                fade(theme::fade(accent, 0.55), a),
            );
            let initial: String = data
                .handle
                .chars()
                .take(1)
                .flat_map(char::to_uppercase)
                .collect();
            text::paint(
                &painter,
                centre,
                Align2::CENTER_CENTER,
                theme::TextStyle::new(24.0, Weight::Bold),
                &initial,
                fade(Color32::WHITE, a),
            );
            let text_x = inner.min.x + AVATAR + 16.0;
            let max_width = inner.max.x - text_x;
            let name = text::middle_truncate(&painter, ty::BUTTON, &data.handle, max_width);
            text::paint(
                &painter,
                pos2(text_x, centre.y - 2.0),
                Align2::LEFT_BOTTOM,
                ty::BUTTON,
                &name,
                fade(Color32::WHITE, a),
            );
            let line = if data.discord {
                CT::IncomingDiscordLine
            } else {
                CT::IncomingLine
            };
            text::paint(
                &painter,
                pos2(text_x, centre.y + 2.0),
                Align2::LEFT_TOP,
                ty::NOTE,
                line.text(language),
                fade(white(0.7), a),
            );
            let status = ui.interact(
                rect,
                Id::unique(("incoming-call-status", &data.invitation)),
                Sense::hover(),
            );
            let label = CT::IncomingName.format(language, &[Arg::Str(&data.handle)]);
            status.widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::Status, true, label.as_str())
            });

            // The two answers: the quiet one first, the primary one at the trailing edge.
            let buttons = Rect::from_min_size(
                pos2(inner.min.x, inner.max.y - size::BUTTON_MIN_HEIGHT),
                vec2(inner.width(), size::BUTTON_MIN_HEIGHT),
            );
            let mut child = ui.new_child(
                UiBuilder::new()
                    .max_rect(buttons)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            child.spacing_mut().item_spacing.x = GAP;
            child.set_opacity(a);
            let half = (inner.width() - GAP) * 0.5 - 6.0;
            let focused = keyboard_focus(&child);
            if ConsoleButton::new(CT::Decline.text(language))
                .icon(Icon::X)
                .min_width(half)
                .focused(focused)
                .enabled(live)
                .show(&mut child)
                .clicked()
            {
                action = Some(CardAction::Decline(data.invitation.clone()));
            }
            let focused = keyboard_focus(&child);
            if ConsoleButton::new(CT::Accept.text(language))
                .icon(Icon::CHECK)
                .prominent(true)
                .min_width(half)
                .focused(focused)
                .enabled(live)
                .show(&mut child)
                .clicked()
            {
                action = Some(CardAction::Accept(data.invitation.clone()));
            }
        });
    action
}

/// The line the strip writes under its title: who is in the call and who speaks.
fn members_line(members: &[Member], language: Language) -> String {
    if members.is_empty() {
        return CT::WaitingForOthers.text(language).to_owned();
    }
    members
        .iter()
        .map(|m| {
            if m.speaking {
                format!("{} · {}", m.handle, CT::SpeakingSuffix.text(language))
            } else {
                m.handle.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The strip: who is on the call, and the controls.
pub(crate) fn call_strip(
    ctx: &Context,
    data: &StripData,
    language: Language,
    shown: f32,
    live: bool,
) -> Option<StripAction> {
    let (a, slide) = entrance(shown);
    if a <= 0.003 {
        return None;
    }
    let screen = ctx.content_rect();
    let button = size::BUTTON_MIN_HEIGHT;
    let calling = matches!(data.kind, StripKind::Calling(_));
    let discord = data.discord.as_ref();
    // Controls: mute, deafen and leave on AnyPlay; the Discord link and leave on Discord; only
    // cancel while calling.
    let measure = ctx.layer_painter(egui::LayerId::new(
        Order::Foreground,
        Id::unique("call-measure"),
    ));
    let labelled = |label: &str, icon: bool| {
        text::width(&measure, ty::BUTTON, label)
            + size::BUTTON_PADDING * 2.0
            + if icon {
                size::BUTTON_ICON + size::BUTTON_GAP
            } else {
                0.0
            }
    };
    let icon_only = size::BUTTON_PADDING_ICON_ONLY * 2.0 + size::BUTTON_ICON_ONLY;
    let controls_width = if calling {
        labelled(CT::Cancel.text(language), false)
    } else if let Some(d) = discord {
        icon_only
            + if d.link.is_some() {
                GAP + labelled(CT::AnswerOnDiscord.text(language), true)
            } else {
                0.0
            }
    } else {
        icon_only * 3.0 + GAP * 2.0
    };
    let text_width = 260.0;
    let width = PAD + text_width + GAP + controls_width + PAD;
    let height = PAD + button;
    let rect = Rect::from_min_size(
        pos2(screen.center().x - width * 0.5, screen.min.y + TOP + slide),
        vec2(width, height),
    );
    let mut action = None;
    Area::new(Id::unique("call-strip"))
        .order(Order::Foreground)
        .fixed_pos(rect.min)
        .interactable(live)
        .show(ctx, |ui| {
            let painter = ui.painter().clone();
            paint_shadow(
                &painter,
                rect,
                radius::TOAST * 0.5,
                egui::epaint::Shadow {
                    color: fade(shadows::toast().color, a * a),
                    ..shadows::toast()
                },
            );
            let mut style = GlassStyle::glass(radius::TOAST * 0.5, 0.6);
            style.black *= a;
            style.alpha = a;
            paint_glass(ctx, &painter, rect, style);

            // Title and the people.
            let (title, line) = match (&data.kind, discord) {
                (StripKind::Calling(who), _) => (
                    CT::Calling.format(language, &[Arg::Str(who)]),
                    String::new(),
                ),
                (StripKind::Connected(members), Some(_)) => (
                    CT::VoiceOnDiscord.text(language).to_owned(),
                    members_line(members, language),
                ),
                (StripKind::Connected(members), None) => {
                    let state = if data.deafened {
                        Some(CT::YouDeafened)
                    } else if data.muted {
                        Some(CT::YouMuted)
                    } else {
                        None
                    };
                    let title = match state {
                        Some(state) => format!(
                            "{} · {}",
                            CT::InCall.text(language),
                            state.text(language)
                        ),
                        None => CT::InCall.text(language).to_owned(),
                    };
                    (title, members_line(members, language))
                }
            };
            let text_x = rect.min.x + PAD;
            let title = text::middle_truncate(&painter, ty::ROW_TITLE, &title, text_width);
            let lines = if line.is_empty() { 1.0 } else { 2.0 };
            let block = ty::ROW_TITLE.line_height() + (lines - 1.0) * ty::NOTE.line_height();
            let top = rect.center().y - block * 0.5;
            text::paint(
                &painter,
                pos2(text_x, top),
                Align2::LEFT_TOP,
                ty::ROW_TITLE,
                &title,
                fade(Color32::WHITE, a),
            );
            if !line.is_empty() {
                let line = text::middle_truncate(&painter, ty::NOTE, &line, text_width);
                let ink = if matches!(&data.kind, StripKind::Connected(m) if m.iter().any(|m| m.speaking))
                {
                    white(0.95)
                } else {
                    white(0.7)
                };
                text::paint(
                    &painter,
                    pos2(text_x, top + ty::ROW_TITLE.line_height()),
                    Align2::LEFT_TOP,
                    ty::NOTE,
                    &line,
                    fade(ink, a),
                );
            }
            let status = ui.interact(rect, Id::unique("call-strip-status"), Sense::hover());
            let label = format!("{title} {line}");
            status.widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::Status, true, label.as_str())
            });

            // Controls at the trailing edge, 12 dp apart.
            let area = Rect::from_min_max(
                pos2(rect.min.x + PAD + text_width + GAP, rect.min.y),
                pos2(rect.max.x - PAD, rect.max.y),
            );
            let mut child = ui.new_child(
                UiBuilder::new()
                    .max_rect(area)
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
            );
            child.spacing_mut().item_spacing.x = GAP;
            child.set_opacity(a);
            if calling {
                let focused = keyboard_focus(&child);
                if ConsoleButton::new(CT::Cancel.text(language))
                    .focused(focused)
                    .enabled(live)
                    .show(&mut child)
                    .clicked()
                {
                    action = Some(StripAction::Leave);
                }
                return;
            }
            // Right to left: the last in reading order is drawn first.
            let focused = keyboard_focus(&child);
            if ConsoleButton::icon_only(Icon::SIGN_OUT, CT::Leave.text(language))
                .destructive(true)
                .focused(focused)
                .enabled(live)
                .show(&mut child)
                .clicked()
            {
                action = Some(StripAction::Leave);
            }
            if discord.is_some() {
                if discord.is_some_and(|d| d.link.is_some()) {
                    let focused = keyboard_focus(&child);
                    if ConsoleButton::new(CT::AnswerOnDiscord.text(language))
                        .icon(Icon::DISCORD_LOGO)
                        .prominent(true)
                        .focused(focused)
                    .enabled(live)
                    .show(&mut child)
                    .clicked()
                    {
                        action = Some(StripAction::OpenDiscord);
                    }
                }
                return;
            }
            let focused = keyboard_focus(&child);
            let (icon, name) = if data.deafened {
                (Icon::SPEAKER_SLASH, CT::Undeafen)
            } else {
                (Icon::HEADPHONES, CT::Deafen)
            };
            if ConsoleButton::icon_only(icon, name.text(language))
                .focused(focused)
                .enabled(live)
                .show(&mut child)
                .clicked()
            {
                action = Some(StripAction::ToggleDeafen);
            }
            let focused = keyboard_focus(&child);
            let (icon, name) = if data.muted || data.deafened {
                (Icon::MICROPHONE_SLASH, CT::Unmute)
            } else {
                (Icon::MICROPHONE, CT::Mute)
            };
            if ConsoleButton::icon_only(icon, name.text(language))
                .focused(focused)
                .enabled(live && !data.deafened)
                .show(&mut child)
                .clicked()
            {
                action = Some(StripAction::ToggleMute);
            }
        });
    action
}
