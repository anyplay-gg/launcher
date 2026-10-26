//! The home screen (docs/DESIGN.md 5.3): top bar, the tile row with the growing selected tile, the
//! hero (title, action row, facts line), the hint bar, and the empty library.
//!
//! The model decides what is selected and focused; this module paints it and reports presses back.

use anyps5_core::library::Game;
use anyps5_core::paths::GameId;
use egui::{
    Align, Align2, Color32, Context, CursorIcon, Id, Layout, Rect, Role, Sense, Ui, UiBuilder,
    WidgetInfo, pos2, vec2,
};

use crate::app::model::{GameState, Model, SizeState};
use crate::screens::glibc_strings::shortfall_text;
use crate::screens::ludus_strings::LudusText;
use crate::screens::ludus_web_strings::BrowserText;
use crate::screens::online_social_strings::SocialText;
use crate::screens::online_strings::OnlineText;
use crate::screens::phone_strings::PhoneText;
use crate::screens::run_strings::{RunText, friends_line};
use crate::screens::strings::{Extra, playtime_text};
use crate::screens::together_strings::TogetherText;
use crate::screens::toolkit_pin;
use crate::screens::update_strings::UpdateText;
use crate::screens::{
    ScreenCx, StateLabel, paint_gradient_circle, paint_label, paint_spinner, place, state_label,
    wrapped,
};
use crate::ui::anim::{AnimExt, Motion, lerp};
use crate::ui::chrome::{self, TileImage, TileOverlay, TileStyle};
use crate::ui::components::{
    self as components, ConsoleButton, ControlSize, IconButton, IndeterminateLine, ProgressLine,
    Variant, hint_bar, hint_bar_size,
};
use crate::ui::focus::HomeFocus;
use crate::ui::i18n::{Arg, Key, Language, byte_text, substitute};
use crate::ui::icons::Icon;
use crate::ui::input;
use crate::ui::intro;
use crate::ui::pad_info;
use crate::ui::text;
use crate::ui::theme::{self, Weight, color, pad, size, ty, white};

/// Gap between the stacked parts of the hero.
const HERO_GAP: f32 = 22.0;
/// Between the state line and the hero title.
const STATE_GAP: f32 = 4.0;
/// Space between the action row and the facts, and between two facts.
const FACTS_ABOVE: f32 = 46.0;
const FACTS_SPACING: f32 = 48.0;
/// Between a fact's overline and its value.
const FACT_GAP: f32 = 6.0;
/// Between the facts and the line that asks "How did it run?" below them. The row is kept free
/// whether it shows or not, so nothing moves when it appears or goes.
const ASK_GAP: f32 = 18.0;
/// Between the question and its first answer, and after the last.
const ASK_LABEL_GAP: f32 = 16.0;
/// Under the action row to the friends line.
const FRIENDS_GAP: f32 = 12.0;
/// Height of the action row (the console button).
const ACTION_ROW: f32 = size::BUTTON_MIN_HEIGHT;
/// Longest a failure or diagnosis text gets.
const MESSAGE_WIDTH: f32 = 520.0;

/// A press the model has to hear about, collected while painting.
#[derive(Clone, Copy, Debug)]
enum Press {
    Tile { index: usize, double: bool },
    Focus(HomeFocus),
    ChooseFolder,
}

/// The hero that is on screen and the ones still fading out.
#[derive(Clone, Default)]
struct HeroTrack {
    current: Option<GameId>,
    leaving: Vec<GameId>,
}

/// Paints the whole home screen and applies the presses.
pub fn show(ui: &mut Ui, cx: &mut ScreenCx<'_>) {
    let ctx = cx.ctx.clone();
    let screen = ctx.content_rect();
    let mut presses = Vec::new();
    crate::ui::shade::paint(ui.painter(), screen, !cx.model.games.is_empty());
    // At launch the parts rise into place one after another (ui::intro).
    staged(ui, &ctx, intro::TOP_BAR, screen, |ui, screen| {
        top_bar(ui, cx, screen, &mut presses);
    });
    if cx.model.games.is_empty() {
        staged(ui, &ctx, intro::HERO, screen, |ui, screen| {
            empty_library(ui, cx, screen, &mut presses);
        });
    } else {
        staged(ui, &ctx, intro::TILES, screen, |ui, screen| {
            tile_row(ui, cx, screen, &mut presses);
        });
        staged(ui, &ctx, intro::HERO, screen, |ui, screen| {
            hero(ui, cx, screen, &mut presses);
        });
        let language = cx.language();
        staged(ui, &ctx, intro::HINTS, screen, |ui, screen| {
            hints(ui, language, screen);
        });
    }
    for press in presses {
        match press {
            Press::Tile { index, double } => cx.model.click_tile(index, double),
            Press::Focus(focus) => cx.model.click(focus),
            Press::ChooseFolder => cx.model.request_folder_dialog(),
        }
    }
}

/// Draws one part of the screen at its intro stage: faded and lowered while it arrives, directly
/// once it is in place.
fn staged(
    ui: &mut Ui,
    ctx: &Context,
    timing: (f32, f32),
    screen: Rect,
    add: impl FnOnce(&mut Ui, Rect),
) {
    let t = intro::stage(ctx, timing);
    if t >= 1.0 {
        add(ui, screen);
        return;
    }
    let mut child = ui.new_child(UiBuilder::new().max_rect(screen));
    child.multiply_opacity(t);
    add(&mut child, intro::lift(screen, t));
}

/// Focus is only drawn while no overlay covers the home screen.
fn focus_visible(model: &Model) -> bool {
    model.overlay.is_none() && model.starting.is_none()
}

// ---------------------------------------------------------------------------------------------
// Top bar
// ---------------------------------------------------------------------------------------------

fn top_bar(ui: &mut Ui, cx: &mut ScreenCx<'_>, screen: Rect, presses: &mut Vec<Press>) {
    let language = cx.language();
    let accent = cx.accent();
    let painter = ui.painter().clone();
    let top = screen.min.y + pad::HOME_TOP;
    let mid = top + size::ICON_BUTTON * 0.5;
    // One gap between all four controls of the strip; a wider one before the clock.
    let (button_gap, group_gap) = (size::CONTROL_GAP, 28.0);

    let title = text::paint(
        &painter,
        pos2(screen.min.x + pad::HOME_X, mid),
        Align2::LEFT_CENTER,
        ty::TOP_BAR_TITLE,
        language.text(Key::Games),
        Color32::WHITE,
    );
    // An empty library shows its own scanning state in the middle.
    if cx.model.scanning && !cx.model.games.is_empty() {
        paint_spinner(
            &painter,
            pos2(title.max.x + group_gap * 0.5 + 8.0, mid),
            7.0,
            cx.ctx.clock(),
            Color32::WHITE,
        );
    }

    // A connected phone sits after the title, out of the focus order: it only reports.
    let mut after_title = title.max.x;
    if let Some(name) = &cx.model.phone.connected {
        let label = PhoneText::BarLabel.format(language, &[Arg::Str(name)]);
        after_title = phone_status(
            ui,
            pos2(title.max.x + PHONE_STATUS_GAP, mid),
            name,
            &label,
            None,
        )
        .unwrap_or(after_title);
    }

    // From the right: clock, then profile, settings, friends and library, all 12 apart.
    let mut x = screen.max.x - pad::HOME_X;
    x = paint_clock(&painter, x, mid) - group_gap;

    let visible = focus_visible(cx.model);
    // Right to left in one strip. A focused control grows nothing (the ring is outside), so the
    // gap is the same at rest, under the pointer and with the focus.
    let strip = Rect::from_min_max(pos2(screen.min.x, top), pos2(x, top + size::ICON_BUTTON));
    let mut row = ui.new_child(
        UiBuilder::new()
            .max_rect(strip)
            .layout(Layout::right_to_left(Align::Center)),
    );
    row.spacing_mut().item_spacing.x = button_gap;
    let online = &cx.model.online;
    let focused = |focus: HomeFocus| visible && cx.model.focus == focus;

    // The profile opens the online area. Signed out it says what to do; signed in it carries the
    // network name and, while the cloud answers, a dot in the accent: the player is online.
    let unread = online.social.unread_total();
    let handle = online.account.as_ref().map(|a| a.handle.as_str());
    let appearing = online.is_online() && online.appears_offline();
    let profile = Profile {
        name: handle.unwrap_or_else(|| OnlineText::SignIn.text(language)),
        signed_in: handle.is_some(),
        online: online.is_online(),
        appearing: appearing.then(|| OnlineText::AppearingOffline.text(language)),
        unread,
        label: profile_label(handle, online.is_online(), appearing, unread, language),
    };
    if profile_button(&mut row, &profile, accent, focused(HomeFocus::PROFILE)).clicked() {
        presses.push(Press::Focus(HomeFocus::PROFILE));
    }
    // An update that waits and whose notice was sent away: a quiet dot on the gear, and the name
    // says why. About holds the rest.
    let update_waiting =
        cx.model.update.staged_version().is_some() && cx.model.update_notice().is_none();
    let gear_name = if update_waiting {
        UpdateText::SettingsBadge.format(language, &[Arg::Str(language.text(Key::Settings))])
    } else {
        language.text(Key::Settings).to_owned()
    };
    let response = IconButton::new(Icon::GEAR_SIX, gear_name)
        .focused(focused(HomeFocus::SETTINGS))
        .show(&mut row);
    if update_waiting {
        let dot = response.rect.right_top() + vec2(-8.0, 8.0);
        painter.circle_filled(dot, 6.0, theme::black(0.62));
        painter.circle_filled(dot, 4.0, white(0.92));
    }
    if response.clicked() {
        presses.push(Press::Focus(HomeFocus::SETTINGS));
    }
    // Friends sit between the library and the settings; the count is the friends online now.
    let friends_online = online.friends_online();
    let label = match friends_online {
        0 => OnlineText::Friends.text(language).to_owned(),
        n => SocialText::FriendsOnline.format(language, &[Arg::Int(n as i64)]),
    };
    let friends = friends_button(
        &mut row,
        friends_online,
        accent,
        focused(HomeFocus::FRIENDS),
        &label,
    );
    if friends.clicked() {
        presses.push(Press::Focus(HomeFocus::FRIENDS));
    }
    let response = IconButton::new(Icon::SQUARES_FOUR, language.text(Key::Library))
        .focused(focused(HomeFocus::LIBRARY))
        .show(&mut row);
    if response.clicked() {
        presses.push(Press::Focus(HomeFocus::LIBRARY));
    }
    // The first status after the title keeps its distance; the next ones sit a control gap apart.
    let next_left = |after_title: f32| {
        if after_title > title.max.x {
            after_title + size::CONTROL_GAP
        } else {
            title.max.x + PHONE_STATUS_GAP
        }
    };
    // A LudusCtrl phone that holds the pad sits after the phone the same way; left out when
    // the strip has no room for it. A browser client holds the same pad and wears the same pill.
    let ludus = &cx.model.ludus;
    let holder = match (&ludus.connected, &ludus.browser.peer) {
        (Some(name), _) => Some((
            name.clone(),
            LudusText::BarLabel.format(language, &[Arg::Str(name)]),
        )),
        (None, Some(peer)) => {
            // What the browser runs on names the pill when it said; else "Browser".
            let device = ludus.browser.device.as_deref();
            Some((
                device
                    .unwrap_or(BrowserText::BarName.text(language))
                    .to_owned(),
                BrowserText::BarLabel.format(language, &[Arg::Str(device.unwrap_or(peer))]),
            ))
        }
        (None, None) => None,
    };
    if let Some((name, label)) = holder
        && let Some(right) = phone_status(
            ui,
            pos2(next_left(after_title), mid),
            &name,
            &label,
            Some(row.min_rect().min.x - group_gap),
        )
    {
        after_title = right;
    }
    // A connected controller sits after the phones (or the title), the same way: it only reports,
    // and it is left out when the strip has no room for it.
    if pad_info::connected() {
        let left = next_left(after_title);
        if let Some(right) = pad_status(
            ui,
            pos2(left, mid),
            row.min_rect().min.x - group_gap,
            language,
        ) {
            after_title = right;
        }
    }
    // A launcher update that waits sits after the title (and the phone), in the room the strip
    // leaves. It is drawn, and reachable by the pad, only when it fits there.
    update_notice(
        ui,
        cx,
        pos2(after_title + group_gap, mid),
        row.min_rect().min.x - group_gap,
        presses,
    );
}

/// The launcher update, quiet: one line in the top bar with the two things to do about it. It
/// says what is ready; "Restart Now" brings the player back to the same place, "Later" sends it
/// away until the next start. While a game runs and the restart waits, it says so and offers
/// "Cancel". It does not grab the focus; the pad reaches it from the library button.
fn update_notice(
    ui: &mut Ui,
    cx: &mut ScreenCx<'_>,
    left_middle: egui::Pos2,
    limit_x: f32,
    presses: &mut Vec<Press>,
) {
    let Some(notice) = cx.model.update_notice_wanted() else {
        return;
    };
    let language = cx.language();
    let ctx = cx.ctx.clone();
    let visible = focus_visible(cx.model);
    let t = ctx
        .animate_from_to(
            ("home-update-notice", notice.version.clone(), notice.holding),
            0.0,
            1.0,
            Motion::Fade,
        )
        .clamp(0.0, 1.0);
    let version = [Arg::Str(&notice.version)];
    let text = if notice.holding {
        UpdateText::NoticeHolding.text(language).to_owned()
    } else {
        UpdateText::NoticeReady.format(language, &version)
    };
    let group = if notice.holding {
        UpdateText::NoticeHoldingName.format(language, &version)
    } else {
        UpdateText::NoticeName.format(language, &version)
    };
    let height = ControlSize::Small.height() + 8.0;
    let labels: Vec<&str> = if notice.holding {
        vec![UpdateText::CancelRestart.text(language)]
    } else {
        vec![
            Extra::RestartNow.text(language),
            UpdateText::Later.text(language),
        ]
    };
    // What it needs: the margin, the glyph, the text and its buttons, each 12 apart.
    let measure = ui.painter().clone();
    let needed = 16.0
        + 18.0
        + 12.0
        + text::width(&measure, ty::DETAIL_LARGE, &text)
        + labels
            .iter()
            .map(|label| {
                12.0 + 2.0 * ControlSize::Small.text_pad() + text::width(&measure, ty::CHIP, label)
            })
            .sum::<f32>()
        + 4.0;
    // One rule for drawing it and for reaching it (the model's focus order reads this).
    let fits = left_middle.x + needed <= limit_x;
    cx.model.notice_fits = fits;
    if !fits {
        return;
    }
    let rect = Rect::from_min_size(
        pos2(left_middle.x, left_middle.y - height * 0.5),
        vec2(needed, height),
    );
    let background = ui.painter().add(egui::Shape::Noop);
    let mut row = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    row.multiply_opacity(t);
    row.spacing_mut().item_spacing.x = 12.0;
    row.add_space(16.0);
    let painter = row.painter().clone();
    let glyph = 18.0;
    let (icon, _) = row.allocate_exact_size(vec2(glyph, glyph), Sense::hover());
    painter.text(
        icon.center(),
        Align2::CENTER_CENTER,
        Icon::ARROW_CIRCLE_DOWN.glyph(),
        Icon::ARROW_CIRCLE_DOWN.font(text::quantize_size(glyph), false),
        white(0.92),
    );
    let label_width = text::width(&painter, ty::DETAIL_LARGE, &text);
    let (label, response) = row.allocate_exact_size(vec2(label_width, height), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(Role::Status, true, &group));
    text::paint(
        &painter,
        label.left_center(),
        Align2::LEFT_CENTER,
        ty::DETAIL_LARGE,
        &text,
        color::text_subtitle(),
    );
    for (i, label) in labels.into_iter().enumerate() {
        let response = ConsoleButton::new(label)
            .size(ControlSize::Small)
            .accessible_name(format!("{label}, {group}"))
            .focused(visible && cx.model.focus == HomeFocus::Update(i as u8))
            .show(&mut row);
        if response.clicked() {
            presses.push(Press::Focus(HomeFocus::Update(i as u8)));
        }
    }
    let used = row.min_rect();
    let capsule = Rect::from_min_max(
        pos2(used.min.x, rect.min.y),
        pos2(used.max.x + 4.0, rect.max.y),
    );
    ui.painter().set(
        background,
        egui::Shape::rect_filled(
            capsule,
            egui::CornerRadius::same(255),
            color::surface().gamma_multiply(t),
        ),
    );
}

/// Room the colon of the clock takes between the hours and the minutes.
const CLOCK_COLON: f32 = 7.0;

/// The clock: hours and minutes with the colon fixed in the middle of a box as wide as four of
/// the widest digit, so the minute ticking over moves nothing. The hours end at the colon and the
/// minutes begin at it; the colon is two dots centred on the digits' cap height, which the font's
/// own colon (set for lower case) is not. Ends at `right`, on the bar's centre line `mid`. Returns
/// the box's left edge.
fn paint_clock(painter: &egui::Painter, right: f32, mid: f32) -> f32 {
    let time = crate::app::clock::local_time();
    let style = ty::CLOCK;
    let ink = color::text_clock();
    let cell = text::digit_cell(painter, style);
    let left = right - (4.0 * cell + CLOCK_COLON);
    let colon_x = left + 2.0 * cell + CLOCK_COLON * 0.5;
    text::paint(
        painter,
        pos2(colon_x - CLOCK_COLON * 0.5, mid),
        Align2::RIGHT_CENTER,
        style,
        &format!("{:02}", time.hour),
        ink,
    );
    text::paint(
        painter,
        pos2(colon_x + CLOCK_COLON * 0.5, mid),
        Align2::LEFT_CENTER,
        style,
        &format!("{:02}", time.minute),
        ink,
    );
    // Digits are as tall as capitals: 0.727 em in Inter.
    let reach = style.size * 0.727 * 0.5 * 0.52;
    for dy in [-reach, reach] {
        painter.circle_filled(pos2(colon_x, mid + dy), style.size * 0.075, ink);
    }
    left
}

/// Between the title and the phone status; clear of the scanning spinner.
const PHONE_STATUS_GAP: f32 = 48.0;
const PHONE_STATUS_HEIGHT: f32 = 32.0;
const PHONE_STATUS_NAME_MAX: f32 = 150.0;

/// The status of a connected phone (the phone server's, or a LudusCtrl phone): its icon and name
/// on the pills' surface, 32 high so it reads as a note beside the 44 high buttons. Not a button:
/// nothing happens on a press. `label` is what a screen reader says. With `limit_x` it is left
/// out (`None`) when it would reach past it.
fn phone_status(
    ui: &mut Ui,
    left_middle: egui::Pos2,
    name: &str,
    label: &str,
    limit_x: Option<f32>,
) -> Option<f32> {
    let painter = ui.painter().clone();
    let shown = text::middle_truncate(&painter, ty::CHIP, name, PHONE_STATUS_NAME_MAX);
    let glyph = 18.0;
    let width = 12.0 + glyph + 8.0 + text::width(&painter, ty::CHIP, &shown) + 14.0;
    if limit_x.is_some_and(|limit| left_middle.x + width > limit) {
        return None;
    }
    let rect = Rect::from_min_size(
        pos2(left_middle.x, left_middle.y - PHONE_STATUS_HEIGHT * 0.5),
        vec2(width, PHONE_STATUS_HEIGHT),
    );
    let response = ui.allocate_rect(rect, Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(Role::Label, true, label));
    if !ui.is_rect_visible(rect) {
        return Some(rect.max.x);
    }
    components::paint_capsule(&painter, rect, color::surface());
    let ink = white(0.92);
    painter.text(
        pos2(rect.min.x + 12.0 + glyph * 0.5, rect.center().y),
        Align2::CENTER_CENTER,
        Icon::DEVICE_MOBILE.glyph(),
        Icon::DEVICE_MOBILE.font(text::quantize_size(glyph), false),
        ink,
    );
    text::paint(
        &painter,
        pos2(rect.min.x + 12.0 + glyph + 8.0, rect.center().y),
        Align2::LEFT_CENTER,
        ty::CHIP,
        &shown,
        ink,
    );
    Some(rect.max.x)
}

/// The status of the connected controller: its icon and, where the pad reports one, its battery.
/// Same surface and height as [`phone_status`]; not a button.
fn pad_status(
    ui: &mut Ui,
    left_middle: egui::Pos2,
    limit_x: f32,
    language: Language,
) -> Option<f32> {
    let painter = ui.painter().clone();
    let battery = pad_info::battery();
    let (shown, label) = match battery {
        pad_info::Battery::Level { percent, charging } => {
            let args = [Arg::Int(i64::from(percent))];
            let extra = if charging {
                Extra::PadStatusCharging
            } else {
                Extra::PadStatusBattery
            };
            (
                Some(format!("{percent}%")),
                substitute(extra.text(language), &args),
            )
        }
        pad_info::Battery::Unknown => (None, Extra::PadStatus.text(language).to_owned()),
    };
    let glyph = 18.0;
    let charging = matches!(battery, pad_info::Battery::Level { charging: true, .. });
    // A small bolt after the percent says "charging" to the eye; the label says it to the ear.
    let bolt = if charging { 6.0 + BOLT_WIDTH } else { 0.0 };
    let text_width = shown
        .as_deref()
        .map_or(0.0, |s| 8.0 + text::width(&painter, ty::CHIP, s) + bolt);
    let width = 12.0 + glyph + text_width + 14.0;
    if left_middle.x + width > limit_x {
        return None;
    }
    let rect = Rect::from_min_size(
        pos2(left_middle.x, left_middle.y - PHONE_STATUS_HEIGHT * 0.5),
        vec2(width, PHONE_STATUS_HEIGHT),
    );
    let response = ui.allocate_rect(rect, Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(Role::Label, true, &label));
    if !ui.is_rect_visible(rect) {
        return Some(rect.max.x);
    }
    components::paint_capsule(&painter, rect, color::surface());
    let ink = white(0.92);
    painter.text(
        pos2(rect.min.x + 12.0 + glyph * 0.5, rect.center().y),
        Align2::CENTER_CENTER,
        Icon::GAME_CONTROLLER.glyph(),
        Icon::GAME_CONTROLLER.font(text::quantize_size(glyph), false),
        ink,
    );
    if let Some(shown) = shown {
        let text_rect = text::paint(
            &painter,
            pos2(rect.min.x + 12.0 + glyph + 8.0, rect.center().y),
            Align2::LEFT_CENTER,
            ty::CHIP,
            &shown,
            ink,
        );
        if charging {
            let c = pos2(text_rect.max.x + 6.0 + BOLT_WIDTH * 0.5, rect.center().y);
            let (w, h) = (BOLT_WIDTH * 0.5, 6.5);
            painter.add(egui::Shape::closed_line(
                vec![
                    c + vec2(w * 0.3, -h),
                    c + vec2(-w, h * 0.15),
                    c + vec2(-w * 0.05, h * 0.15),
                    c + vec2(-w * 0.3, h),
                    c + vec2(w, -h * 0.15),
                    c + vec2(w * 0.05, -h * 0.15),
                ],
                egui::Stroke::new(1.5, ink),
            ));
        }
    }
    Some(rect.max.x)
}

/// Width of the charging bolt in the controller status.
const BOLT_WIDTH: f32 = 8.0;

/// What the profile button shows.
struct Profile<'a> {
    /// The account's handle, or the invitation to sign in.
    name: &'a str,
    signed_in: bool,
    /// The cloud answers: a dot at the avatar.
    online: bool,
    /// The player appears offline to others: the dot is a ring and the words follow the name.
    appearing: Option<&'a str>,
    unread: u64,
    /// The accessible name.
    label: String,
}

/// The profile's accessible name: the handle (what is on screen), then what it tells besides.
fn profile_label(
    handle: Option<&str>,
    online: bool,
    appearing: bool,
    unread: u64,
    language: Language,
) -> String {
    let Some(handle) = handle else {
        return OnlineText::SignIn.text(language).to_owned();
    };
    let mut label = handle.to_owned();
    if online && appearing {
        label = format!("{label}, {}", OnlineText::AppearingOffline.text(language));
    } else if online {
        label = format!("{label}, {}", OnlineText::Online.text(language));
    }
    if unread > 0 {
        label = format!(
            "{label}, {}",
            SocialText::ProfileUnread.format(language, &[Arg::Int(unread as i64)])
        );
    }
    label
}

/// The longest a handle gets in the profile button before it is cut in the middle.
const PROFILE_NAME_MAX: f32 = 160.0;

/// The surface the top bar's pills share with its icon buttons (the secondary control at 44 dp):
/// returns the rectangle to paint into, pressed in, and its scale.
fn paint_pill(painter: &egui::Painter, it: &components::Interaction) -> (Rect, f32) {
    let rect = it.painted(painter);
    components::paint_control(painter, rect, Variant::Secondary, it, false);
    (rect, rect.height() / size::ICON_BUTTON)
}

/// The ink of a pill: the same white as the icon buttons, focused or not.
fn pill_ink(_focus: f32) -> Color32 {
    Color32::WHITE
}

/// The avatar (or the sign-in glyph) and the name as one button, on the surface of the icon
/// buttons beside it. The pill's radius is the avatar's plus the inset, so the two stay
/// concentric.
fn profile_button(
    ui: &mut Ui,
    profile: &Profile<'_>,
    accent: Color32,
    focused: bool,
) -> egui::Response {
    let painter = ui.painter().clone();
    let inset = (size::ICON_BUTTON - size::AVATAR) * 0.5;
    let name = text::middle_truncate(&painter, ty::PROFILE, profile.name, PROFILE_NAME_MAX);
    let name_width = text::width(&painter, ty::PROFILE, &name);
    // Unread messages: a small count in ink after the name, so the dot of being online stays the
    // accent's alone.
    let count = match profile.unread {
        0 => String::new(),
        1..=9 => profile.unread.to_string(),
        _ => "9+".to_owned(),
    };
    let badge_width = if count.is_empty() {
        0.0
    } else {
        (text::width(&painter, BADGE, &count) + 12.0).max(BADGE_HEIGHT) + 8.0
    };
    // Appearing offline: the words in quiet ink after the name.
    let quiet_width = profile
        .appearing
        .map_or(0.0, |words| text::width(&painter, ty::CHIP, words) + 10.0);
    let width = inset + size::AVATAR + 10.0 + name_width + 16.0 + quiet_width + badge_width;
    let it = components::interact(ui, vec2(width, size::ICON_BUTTON), focused, true, None);
    it.response
        .widget_info(|| WidgetInfo::labeled(Role::Button, true, &profile.label));
    if !ui.is_rect_visible(it.rect) {
        return it.response;
    }
    let (rect, s) = paint_pill(&painter, &it);
    let avatar_r = size::AVATAR * 0.5 * s;
    let avatar = pos2(rect.min.x + inset * s + avatar_r, rect.center().y);
    let ink = pill_ink(it.focus);
    if profile.signed_in {
        paint_gradient_circle(
            &painter,
            avatar,
            avatar_r,
            accent,
            theme::fade(accent, 0.55),
        );
        let initial: String = profile
            .name
            .chars()
            .take(1)
            .flat_map(char::to_uppercase)
            .collect();
        // The initial sits on the middle of the gradient, over the pill: white on a dark accent,
        // dark on a light one.
        text::paint(
            &painter,
            avatar,
            Align2::CENTER_CENTER,
            theme::TextStyle::new(text::quantize_size(16.0 * s), Weight::Bold),
            &initial,
            theme::readable_ink(avatar_middle(accent)),
        );
    } else {
        // Nobody yet: the outline of a person in the avatar's footprint, in ink, not accent.
        painter.text(
            avatar,
            Align2::CENTER_CENTER,
            Icon::USER_CIRCLE.glyph(),
            Icon::USER_CIRCLE.font(text::quantize_size(avatar_r * 2.0 * 0.78), false),
            ink,
        );
    }
    if profile.signed_in && profile.online {
        // The ring takes the colour of the pill around it, so the dot sits apart from the avatar.
        let dot = avatar + vec2(avatar_r, avatar_r) * 0.72;
        let ring = theme::black(0.62);
        painter.circle_filled(dot, 6.0 * s, ring);
        if profile.appearing.is_some() {
            // Online for the player, offline for the others: a ring, not the accent's dot.
            painter.circle_stroke(dot, 3.4 * s, egui::Stroke::new(1.5 * s, white(0.9)));
        } else {
            painter.circle_filled(dot, 4.0 * s, accent);
        }
    }
    let shown = text::paint(
        &painter,
        pos2(avatar.x + avatar_r + 10.0 * s, rect.center().y),
        Align2::LEFT_CENTER,
        ty::PROFILE,
        &name,
        ink,
    );
    let mut after = shown.max.x;
    if let Some(words) = profile.appearing {
        after = text::paint(
            &painter,
            pos2(after + 10.0 * s, rect.center().y),
            Align2::LEFT_CENTER,
            ty::CHIP,
            words,
            white(0.72),
        )
        .max
        .x;
    }
    if !count.is_empty() {
        // Light, with dark type: the one place a pill carries ink as a fill.
        let w = (text::width(&painter, BADGE, &count) + 12.0).max(BADGE_HEIGHT) * s;
        let h = BADGE_HEIGHT * s;
        let badge =
            Rect::from_min_size(pos2(after + 8.0 * s, rect.center().y - h * 0.5), vec2(w, h));
        components::paint_capsule(&painter, badge, white(0.92));
        text::paint(
            &painter,
            badge.center(),
            Align2::CENTER_CENTER,
            BADGE,
            &count,
            Color32::BLACK,
        );
    }
    it.response
}

/// The friends button: the icon button's circle with the friends icon. While friends are online it
/// grows into a pill that ends in a dot in the accent and their number; at none (or signed out) it
/// is the plain circle.
fn friends_button(
    ui: &mut Ui,
    online: usize,
    accent: Color32,
    focused: bool,
    label: &str,
) -> egui::Response {
    let painter = ui.painter().clone();
    let count = match online {
        0 => String::new(),
        1..=99 => online.to_string(),
        _ => "99+".to_owned(),
    };
    // The icon keeps the circle's centre; the dot starts 8 after the glyph and the number ends 16
    // short of the pill's end.
    let tail = if count.is_empty() {
        0.0
    } else {
        let number = text::tabular_width(&painter, ty::PROFILE, &count);
        FRIENDS_ICON_END + FRIENDS_DOT_GAP + FRIENDS_DOT + FRIENDS_COUNT_GAP + number + 16.0
            - size::ICON_BUTTON
    };
    let it = components::interact(
        ui,
        vec2(size::ICON_BUTTON + tail, size::ICON_BUTTON),
        focused,
        true,
        None,
    );
    it.response
        .widget_info(|| WidgetInfo::labeled(Role::Button, true, label));
    if !ui.is_rect_visible(it.rect) {
        return it.response;
    }
    let (rect, s) = paint_pill(&painter, &it);
    let ink = pill_ink(it.focus);
    let centre = pos2(rect.min.x + size::ICON_BUTTON * 0.5 * s, rect.center().y);
    painter.text(
        centre,
        Align2::CENTER_CENTER,
        Icon::USERS.glyph(),
        Icon::USERS.font(text::quantize_size(size::ICON_BUTTON_GLYPH * s), false),
        ink,
    );
    if !count.is_empty() {
        let dot = pos2(
            rect.min.x + (FRIENDS_ICON_END + FRIENDS_DOT_GAP + FRIENDS_DOT * 0.5) * s,
            rect.center().y,
        );
        painter.circle_filled(dot, FRIENDS_DOT * 0.5 * s, accent);
        text::paint_tabular(
            &painter,
            pos2(
                dot.x + (FRIENDS_DOT * 0.5 + FRIENDS_COUNT_GAP) * s,
                rect.center().y - ty::PROFILE.line_height() * 0.5,
            ),
            ty::PROFILE,
            &count,
            ink,
        );
    }
    it.response
}

/// Where the friends icon ends inside its 44 dp circle (the glyph is 20 dp, centred).
const FRIENDS_ICON_END: f32 = (size::ICON_BUTTON + size::ICON_BUTTON_GLYPH) * 0.5;
/// The online dot, its distance from the icon and from the number.
const FRIENDS_DOT: f32 = 8.0;
const FRIENDS_DOT_GAP: f32 = 8.0;
const FRIENDS_COUNT_GAP: f32 = 6.0;

/// The unread count's type and height.
const BADGE: theme::TextStyle = theme::TextStyle::new(13.0, Weight::Bold);
const BADGE_HEIGHT: f32 = 20.0;

// ---------------------------------------------------------------------------------------------
// Tile row
// ---------------------------------------------------------------------------------------------

/// The overlay a tile shows for a state (DESIGN.md 5.1).
pub fn tile_overlay(state: &GameState, weights: &crate::app::pipeline::StepWeights) -> TileOverlay {
    match state {
        GameState::Locked => TileOverlay::Locked,
        GameState::Unreadable => TileOverlay::Unreadable,
        GameState::Queued => TileOverlay::Queued,
        GameState::Installing { step, fraction } => {
            TileOverlay::Installing(weights.overall(*step, *fraction))
        }
        GameState::Preparing { progress, paused } => TileOverlay::Preparing {
            fraction: progress.map(|(step, fraction)| weights.overall(step, fraction)),
            paused: *paused,
        },
        GameState::Failed(_) => TileOverlay::Failed,
        GameState::Installed { .. } => TileOverlay::None,
        GameState::Starting => TileOverlay::Starting,
        GameState::Running => TileOverlay::Running,
    }
}

/// Decoded icon size to ask for: small tiles take the smaller texture.
pub fn icon_resolution(ctx: &Context, side: f32) -> u32 {
    if side * ctx.pixels_per_point() > 256.0 {
        400
    } else {
        256
    }
}

/// The tile style of a game: icon, state overlay, time.
pub fn tile_style(cx: &mut ScreenCx<'_>, game: &Game, side: f32, accent: Color32) -> TileStyle {
    let mut style = TileStyle::new(accent);
    style.time = cx.ctx.clock();
    style.overlay = tile_overlay(&cx.model.state_of(game), &cx.model.prep.weights);
    if let Some(icon) = &game.icon {
        let max = icon_resolution(cx.ctx, side);
        style.image = cx.textures.get(icon, max).map(|l| TileImage {
            texture: l.id,
            aspect: l.aspect,
        });
    }
    style
}

fn tile_row(ui: &mut Ui, cx: &mut ScreenCx<'_>, screen: Rect, presses: &mut Vec<Press>) {
    let ctx = cx.ctx.clone();
    let painter = ui.painter().clone();
    let accent = cx.accent();
    let language = cx.language();
    let (small, large, gap, label_column) = (
        size::TILE_SMALL,
        size::TILE_LARGE,
        size::TILE_GAP,
        size::TILE_LABEL_COLUMN,
    );
    let leading = screen.min.x + pad::HOME_X;
    let row_top = screen.min.y + pad::HOME_TOP + size::ICON_BUTTON + 34.0;
    let selected = cx.model.selected;
    let ring_on = focus_visible(cx.model) && cx.model.focus == HomeFocus::Tiles;
    let interactive = focus_visible(cx.model);
    let count = cx.model.games.len();
    // Tiles further than a screen away are not worth animating.
    let first = selected.saturating_sub(3);
    let last = (selected + ((screen.width() / (small + gap)).ceil() as usize) + 3).min(count);

    for index in first..last {
        let game = cx.model.games[index].clone();
        let is_selected = index == selected;
        let offset = index as isize - selected as isize;
        let target_x = if offset < 0 {
            leading + offset as f32 * (small + gap)
        } else if offset == 0 {
            leading
        } else {
            leading + large + gap + label_column + (offset - 1) as f32 * (small + gap)
        };
        let key = |what: &'static str| (what, game.id.as_str().to_owned());
        let t = ctx.animate_to(
            key("tile-size"),
            if is_selected { 1.0 } else { 0.0 },
            Motion::Layout,
        );
        let x = ctx.animate_to(key("tile-x"), target_x, Motion::Layout);
        let opacity = ctx.animate_to(
            key("tile-opacity"),
            if offset < 0 { 0.0 } else { 1.0 },
            Motion::Layout,
        );
        let side = lerp(small, large, t);
        // Small tiles share the large tile's centre line.
        let y = row_top + (large - small) * 0.5 * (1.0 - t);
        let rect = Rect::from_min_size(pos2(x, y), vec2(side, side));
        if rect.max.x < screen.min.x || rect.min.x > screen.max.x || opacity <= 0.003 {
            continue;
        }

        let ring_t = ctx.focus_t(key("tile-ring"), is_selected && ring_on);
        let mut style = tile_style(cx, &game, side, accent);
        // Only the selected tile says it is locked; a row of padlocks is noise.
        if !is_selected && matches!(style.overlay, TileOverlay::Locked) {
            style.overlay = TileOverlay::None;
        }
        style.focus = ring_t;
        style.ring = ring_t > 0.002;
        style.opacity = opacity.clamp(0.0, 1.0);
        let under_pointer = interactive
            && opacity > 0.5
            && input::pointer_in_use(&ctx)
            && ctx.pointer_hover_pos().is_some_and(|p| rect.contains(p));
        style.hover = ctx.animate_to(key("tile-hover"), f32::from(under_pointer), Motion::Hover);
        chrome::paint_tile(&painter, rect, &style);
        if let TileOverlay::Preparing { fraction, paused } = style.overlay {
            let percent = fraction.map(|f| (f * 100.0) as u32);
            let caption = crate::screens::prepare_strings::tile_label(percent, paused, language);
            let caption = text::middle_truncate(
                &painter,
                ty::STATE_LABEL_SMALL,
                &caption,
                chrome::preparing_caption_width(rect),
            );
            chrome::paint_preparing_caption(&painter, rect, &caption, style.opacity);
        }

        if interactive && opacity > 0.5 {
            let state = cx.model.state_of(&game);
            let label = format!("{}, {}", game.name, state_label(&state, language).text);
            let response =
                ui.interact(rect, Id::unique(("tile", game.id.as_str())), Sense::click());
            response.widget_info(|| WidgetInfo::labeled(Role::Button, true, label.as_str()));
            crate::ui::a11y::follow(&ctx, response.id, ring_on && is_selected);
            if input::hovered(&ctx, &response) {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
            }
            if response.double_clicked() {
                presses.push(Press::Tile {
                    index,
                    double: true,
                });
            } else if response.clicked() {
                presses.push(Press::Tile {
                    index,
                    double: false,
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Hero
// ---------------------------------------------------------------------------------------------

fn hints(ui: &mut Ui, language: Language, screen: Rect) {
    let painter = ui.painter().clone();
    let (width, height) = hint_bar_size(&painter, language, true);
    let rect = Rect::from_min_size(
        pos2(
            screen.max.x - pad::HOME_X - width,
            screen.max.y - pad::HINT_Y - height,
        ),
        vec2(width, height),
    );
    place(ui, rect, |ui| hint_bar(ui, language, true));
}

fn hero(ui: &mut Ui, cx: &mut ScreenCx<'_>, screen: Rect, presses: &mut Vec<Press>) {
    let ctx = cx.ctx.clone();
    let Some(selected) = cx.model.selected_game().map(|g| g.id.clone()) else {
        return;
    };
    let track_id = Id::unique("home-hero-track");
    let mut track: HeroTrack = ctx.data_mut(|d| d.get_temp(track_id)).unwrap_or_default();
    if track.current.as_ref() != Some(&selected) {
        if let Some(previous) = track.current.replace(selected.clone()) {
            track.leaving.retain(|id| *id != previous);
            track.leaving.push(previous);
            let keep = track.leaving.len().saturating_sub(2);
            track.leaving.drain(..keep);
        }
        track.leaving.retain(|id| *id != selected);
    }
    // The ones on their way out are drawn first, so the incoming hero sits on top.
    let mut still_leaving = Vec::new();
    for id in track.leaving.clone() {
        let alpha = ctx.animate_to(("hero", id.as_str().to_owned()), 0.0, Motion::Exit);
        if alpha > 0.01
            && let Some(game) = cx.model.game(&id).cloned()
        {
            paint_hero(ui, cx, screen, &game, alpha, 0.0, false, presses);
            still_leaving.push(id);
        }
    }
    track.leaving = still_leaving;
    let t = ctx.animate_from_to(
        ("hero", selected.as_str().to_owned()),
        0.0,
        1.0,
        Motion::Layout,
    );
    if let Some(game) = cx.model.game(&selected).cloned() {
        paint_hero(
            ui, cx, screen, &game, t,
            // A crossfade only: the hero changes on every arrow press, so it must not travel.
            0.0, true, presses,
        );
    }
    ctx.data_mut(|d| d.insert_temp(track_id, track));
}

/// Where the parts of the hero sit, from the bottom up.
struct HeroGeometry {
    x: f32,
    title_bottom: f32,
    actions_top: f32,
    facts_top: f32,
    ask_top: f32,
}

/// Overline, a small gap, then the value.
fn facts_height() -> f32 {
    ty::INFO_OVERLINE.line_height() + FACT_GAP + ty::INFO_VALUE.line_height()
}

fn hero_geometry(screen: Rect, hint_height: f32, dy: f32) -> HeroGeometry {
    let bottom = screen.max.y - pad::HINT_Y - hint_height - pad::HINT_Y + dy;
    let ask_top = bottom - size::CHIP_HEIGHT;
    let facts_top = ask_top - ASK_GAP - facts_height();
    let actions_top = facts_top - FACTS_ABOVE - ACTION_ROW;
    HeroGeometry {
        x: screen.min.x + pad::HERO_X,
        title_bottom: actions_top - HERO_GAP,
        actions_top,
        facts_top,
        ask_top,
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_hero(
    ui: &mut Ui,
    cx: &mut ScreenCx<'_>,
    screen: Rect,
    game: &Game,
    alpha: f32,
    dy: f32,
    live: bool,
    presses: &mut Vec<Press>,
) {
    let language = cx.language();
    let state = cx.model.state_of(game);
    let mut child = ui.new_child(UiBuilder::new().max_rect(screen));
    // Multiplied, so the hero also follows the intro and the onboarding cross-fade.
    child.multiply_opacity(alpha.clamp(0.0, 1.0));
    let ui = &mut child;
    let painter = ui.painter().clone();
    let (_, hint_height) = hint_bar_size(&painter, language, true);
    let geometry = hero_geometry(screen, hint_height, dy);
    let x = geometry.x;
    let width = screen.width() - 2.0 * pad::HERO_X;
    let visible = live && focus_visible(cx.model);

    // 1. Title: 58 light, two lines at most, shrinks to 60 %.
    let mut title = wrapped(
        &painter,
        ty::HERO_TITLE,
        &game.name,
        Color32::WHITE,
        width,
        2,
    );
    for step in 1..=4 {
        if !title.elided {
            break;
        }
        let style = text::scaled(ty::HERO_TITLE, 1.0 - 0.1 * step as f32);
        title = wrapped(&painter, style, &game.name, Color32::WHITE, width, 2);
    }
    // A low window must not push a two-line title (and the state above it) over the tile row.
    let state_height = ty::STATE_LABEL.line_height() + STATE_GAP;
    let tiles_bottom =
        screen.min.y + pad::HOME_TOP + size::ICON_BUTTON + 34.0 + size::TILE_LARGE + 24.0;
    if geometry.title_bottom - title.size().y - state_height < tiles_bottom {
        title = wrapped(
            &painter,
            ty::HERO_TITLE,
            &game.name,
            Color32::WHITE,
            width,
            1,
        );
    }
    let title_top = geometry.title_bottom - title.size().y;
    painter.galley(pos2(x, title_top), title, Color32::WHITE);

    // The state, right above the title (the selected tile carries no label of its own).
    paint_label(
        &painter,
        pos2(
            x,
            title_top - STATE_GAP - ty::STATE_LABEL.line_height() * 0.5,
        ),
        &state_label(&state, language),
        ty::STATE_LABEL,
        width,
        1,
        1.0,
    );

    // 2. Action row.
    let row = Rect::from_min_size(pos2(x, geometry.actions_top), vec2(width, ACTION_ROW));
    let has_trophies = cx.model.trophies.get(&game.id);
    let trophy_text =
        has_trophies.map(|set| format!("{}/{}", set.unlocked_count(), set.trophies.len()));
    // Play, then Play Together when the game can be played together, then trophies, then "...".
    let together = crate::app::together::hostable(cx.model).is_some();
    let trophies_index = 1 + usize::from(together);
    let settings_index = trophies_index + usize::from(trophy_text.is_some());
    let (primary_title, primary_icon, primary_fill) = primary_look(&state, language);
    let mut pressed = None;
    let after_buttons = place(ui, row, |ui| {
        ui.spacing_mut().item_spacing.x = size::CONTROL_GAP;
        let focused = |i: usize| visible && cx.model.focus == HomeFocus::Actions(i);
        let primary = ConsoleButton::new(primary_title)
            .icon(primary_icon)
            .icon_fill(primary_fill)
            .prominent(true)
            .min_width(size::PLAY_MIN_WIDTH)
            .focused(focused(0))
            .show(ui);
        if primary.clicked() {
            pressed = Some(HomeFocus::Actions(0));
        }
        if together {
            let response = ConsoleButton::new(TogetherText::PlayTogether.text(language))
                .icon(Icon::USERS)
                .focused(focused(1))
                .show(ui);
            if response.clicked() {
                pressed = Some(HomeFocus::Actions(1));
            }
        }
        if let Some(text) = &trophy_text {
            let response = ConsoleButton::new(text)
                .accessible_name(format!("{}, {}", language.text(Key::Trophies), text))
                .icon(Icon::TROPHY)
                .icon_fill(true)
                .tabular(true)
                .focused(focused(trophies_index))
                .show(ui);
            if response.clicked() {
                pressed = Some(HomeFocus::Actions(trophies_index));
            }
        }
        let more = ConsoleButton::icon_only(Icon::DOTS_THREE, language.text(Key::Options))
            .focused(focused(settings_index))
            .show(ui);
        if more.clicked() {
            pressed = Some(HomeFocus::Actions(settings_index));
        }
        ui.cursor().min
    });
    if live && let Some(focus) = pressed {
        presses.push(Press::Focus(focus));
    }
    action_row_status(ui, cx, game, &state, after_buttons.x, geometry.actions_top);

    // The glibc notice takes the line under Play while it applies: it says why a start may fail.
    let glibc_line = cx
        .model
        .glibc
        .filter(|_| glibc_notice_applies(&state))
        .map(|short| shortfall_text(&short, language));
    if let Some(line) = &glibc_line {
        let top = geometry.actions_top + ACTION_ROW + FRIENDS_GAP;
        let rect = Rect::from_min_size(pos2(x, top), vec2(width, ty::DETAIL_LARGE.line_height()));
        let response = place(ui, rect, |ui| {
            ui.allocate_exact_size(rect.size(), Sense::hover()).1
        });
        response.widget_info(|| WidgetInfo::labeled(Role::Label, true, line.as_str()));
        paint_label(
            &painter,
            pos2(x, rect.center().y),
            &StateLabel {
                icon: Icon::WARNING,
                fill: true,
                tint: color::warning_text(),
                text: line.clone(),
            },
            ty::DETAIL_LARGE,
            width,
            1,
            1.0,
        );
    }
    // Friends who play this game now, under Play.
    let names = cx.model.online.friends_playing(&game.title_id);
    if let Some(line) = friends_line(&names, language).filter(|_| glibc_line.is_none()) {
        let galley = wrapped(
            &painter,
            ty::DETAIL_LARGE,
            &line,
            color::text_subtitle(),
            width,
            1,
        );
        let top = geometry.actions_top + ACTION_ROW + FRIENDS_GAP;
        painter.galley(pos2(x, top), galley, color::text_subtitle());
    }

    // 3. Facts: size, title ID, version. The state already stands next to the selected tile.
    let size_text = match cx.model.sizes.get(&game.id) {
        Some(SizeState::Known(bytes)) => byte_text(*bytes as i64, language),
        _ => byte_text(-1, language),
    };
    let mut facts = vec![
        (language.text(Key::Size), size_text),
        (language.text(Key::TitleID), game.title_id.clone()),
        (language.text(Key::Version), game.version.clone()),
    ];
    // Said only once there is something to say: a game never played shows no "0 min".
    let played = cx.model.prefs.playtime(&game.id);
    if played.is_played() {
        facts.push((
            Extra::Played.text(language),
            playtime_text(played.seconds, language),
        ));
    }
    // The toolkit the game is unlocked and started with: the global one, or the release it stays on.
    if let Some(value) = toolkit_pin::fact_value(&cx.model.toolkit_of(&game.id), language) {
        facts.push((Extra::Toolkit.text(language), value));
    }
    // What players found, once enough of them said (run reports).
    if let Some((verdict, reports)) = cx.model.online.runs.verdict(&game.title_id, &game.version) {
        let value = RunText::Verdict.format(
            language,
            &[
                Arg::Str(RunText::of(verdict).text(language)),
                Arg::Int(i64::from(reports)),
            ],
        );
        facts.insert(3, (RunText::Runs.text(language), value));
    }
    let mut left = x;
    for (title, value) in &facts {
        left += fact(ui, pos2(left, geometry.facts_top), title, value, language) + FACTS_SPACING;
    }

    // 4. "How did it run?", once, after a session.
    if live {
        ask_line(
            ui,
            cx,
            game,
            pos2(x, geometry.ask_top),
            width,
            visible,
            presses,
        );
    }
}

/// The question below the facts: a quiet line of four answers and a dismiss. It fades in and does
/// nothing else; one press answers it, and it never comes back for this game version.
fn ask_line(
    ui: &mut Ui,
    cx: &mut ScreenCx<'_>,
    game: &Game,
    at: egui::Pos2,
    width: f32,
    visible: bool,
    presses: &mut Vec<Press>,
) {
    let Some(prompt) = cx
        .model
        .run_question()
        .filter(|p| p.game == game.id)
        .cloned()
    else {
        return;
    };
    let language = cx.language();
    let ctx = cx.ctx.clone();
    let t = ctx.animate_from_to(
        (
            "home-ask",
            game.id.as_str().to_owned(),
            prompt.version.clone(),
            prompt.toolkit.clone(),
        ),
        0.0,
        1.0,
        Motion::Fade,
    );
    let row = Rect::from_min_size(at, vec2(width, size::CHIP_HEIGHT));
    let mut child = ui.new_child(UiBuilder::new().max_rect(row));
    child.multiply_opacity(t.clamp(0.0, 1.0));
    let question = RunText::Question.text(language);
    let mut pressed = None;
    place(&mut child, row, |ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        let painter = ui.painter().clone();
        let label_width = text::width(&painter, ty::DETAIL_LARGE, question);
        let (label, _) = ui.allocate_exact_size(
            vec2(label_width + ASK_LABEL_GAP - 12.0, size::CHIP_HEIGHT),
            Sense::hover(),
        );
        text::paint(
            &painter,
            label.left_center(),
            Align2::LEFT_CENTER,
            ty::DETAIL_LARGE,
            question,
            color::text_subtitle(),
        );
        for (i, verdict) in crate::app::run_question::ANSWERS.iter().enumerate() {
            let answer = RunText::of(*verdict).text(language);
            let response = ConsoleButton::new(answer)
                .size(ControlSize::Small)
                .accessible_name(format!("{question} {answer}"))
                .focused(visible && cx.model.focus == HomeFocus::Ask(i))
                .show(ui);
            if response.clicked() {
                pressed = Some(HomeFocus::Ask(i));
            }
        }
        let dismiss = ConsoleButton::icon_only(Icon::X, RunText::Dismiss.text(language))
            .size(ControlSize::Small)
            .focused(visible && cx.model.focus == HomeFocus::Ask(crate::ui::focus::ASK_LAST))
            .show(ui);
        if dismiss.clicked() {
            pressed = Some(HomeFocus::Ask(crate::ui::focus::ASK_LAST));
        }
    });
    if let Some(focus) = pressed {
        presses.push(Press::Focus(focus));
    }
}

fn primary_look(state: &GameState, language: Language) -> (&'static str, Icon, bool) {
    match state {
        GameState::Installed { .. } => (language.text(Key::Play), Icon::PLAY, true),
        GameState::Preparing { .. } => (language.text(Key::Play), Icon::PLAY, true),
        GameState::Locked | GameState::Unreadable => {
            (language.text(Key::Unlock), Icon::LOCK_OPEN, true)
        }
        GameState::Failed(_) => (language.text(Key::Retry), Icon::ARROW_CLOCKWISE, false),
        GameState::Installing { .. } | GameState::Queued => {
            (language.text(Key::Cancel), Icon::X, false)
        }
        GameState::Starting => (language.text(Key::Starting), Icon::HOURGLASS, false),
        GameState::Running => (language.text(Key::Running), Icon::PLAY_CIRCLE, false),
    }
}

/// To the right of the buttons: the unlock progress, the failure or the diagnosis.
fn action_row_status(
    ui: &mut Ui,
    cx: &mut ScreenCx<'_>,
    game: &Game,
    state: &GameState,
    x: f32,
    top: f32,
) {
    let language = cx.language();
    let accent = cx.accent();
    let painter = ui.painter().clone();
    let mid = top + ACTION_ROW * 0.5;
    let running = match state {
        GameState::Installing { step, fraction } => Some((*step, *fraction, false)),
        GameState::Preparing { progress, paused } => progress.map(|(s, f)| (s, f, *paused)),
        _ => None,
    };
    match state {
        _ if running.is_some() => {
            let (step, fraction, paused) = running.expect("checked");
            let (step, fraction) = (&step, &fraction);
            // The state line above the title already names the step: the bar and its percentage
            // sit on the buttons' centre line.
            let left = x + 16.0;
            let overall = cx.model.overall(*step, *fraction);
            let block = size::HERO_PROGRESS_WIDTH;
            let percent = format!("{}%", (overall * 100.0) as u32);
            text::paint_tabular(
                &painter,
                pos2(
                    left + block + 14.0,
                    mid - ty::PROGRESS_PERCENT.line_height() * 0.5,
                ),
                ty::PROGRESS_PERCENT,
                &percent,
                white(0.85),
            );
            // The time left, from the measured speed; nothing until there is enough to tell.
            let note = if paused {
                Some(crate::screens::prepare_strings::paused_note(language).to_owned())
            } else {
                cx.model
                    .time_left(&game.id)
                    .map(|s| crate::screens::prepare_strings::time_left(s, language))
            };
            let spoken = format!(
                "{} {percent}{}",
                language.text(step.title_key()),
                note.as_deref()
                    .map(|n| format!(", {n}"))
                    .unwrap_or_default()
            );
            let status =
                Rect::from_min_size(pos2(left, mid - 12.0), vec2(block + 14.0 + 48.0, 24.0));
            ui.allocate_rect(status, Sense::hover())
                .widget_info(|| WidgetInfo::labeled(Role::Label, true, spoken.as_str()));
            if let Some(note) = note {
                let w = text::tabular_width(&painter, ty::PROGRESS_PERCENT, &percent);
                text::paint(
                    &painter,
                    pos2(
                        left + block + 14.0 + w + 14.0,
                        mid - ty::DETAIL_LARGE.line_height() * 0.5,
                    ),
                    egui::Align2::LEFT_TOP,
                    ty::DETAIL_LARGE,
                    &note,
                    color::text_subtitle(),
                );
            }
            let bar = Rect::from_min_size(
                pos2(left, mid - size::PROGRESS_HEIGHT * 0.5),
                vec2(block, size::PROGRESS_HEIGHT),
            );
            place(ui, bar, |ui| {
                if *step == crate::app::pipeline::UnlockStep::Relink && *fraction == 0.0 {
                    IndeterminateLine::new(accent).width(block).show(ui);
                } else {
                    ProgressLine::new(overall, if paused { white(0.55) } else { accent })
                        .width(block)
                        .label(language.text(step.title_key()))
                        .show(ui);
                }
            });
        }
        GameState::Preparing {
            progress: None,
            paused,
        } => {
            let note = crate::screens::prepare_strings::hero_note(*paused, language);
            let galley = wrapped(
                &painter,
                ty::DETAIL_LARGE,
                note,
                color::text_subtitle(),
                MESSAGE_WIDTH,
                2,
            );
            let top = mid - galley.size().y * 0.5;
            painter.galley(pos2(x + 16.0, top), galley, color::text_subtitle());
        }
        GameState::Failed(message) => {
            // What happened and what to do, in words; the technical message stays under it, quiet.
            let headline = wrapped(
                &painter,
                ty::ROW_TITLE,
                crate::screens::prepare_strings::failure_headline(language),
                color::error_text(),
                MESSAGE_WIDTH,
                2,
            );
            let detail = wrapped(
                &painter,
                ty::DETAIL,
                message,
                color::text_subtitle(),
                MESSAGE_WIDTH,
                2,
            );
            let gap = 4.0;
            let top = mid - (headline.size().y + gap + detail.size().y) * 0.5;
            let detail_top = top + headline.size().y + gap;
            painter.galley(pos2(x + 16.0, top), headline, color::error_text());
            let total = Rect::from_min_size(
                pos2(x + 16.0, top),
                vec2(MESSAGE_WIDTH, detail_top + detail.size().y - top),
            );
            let name = format!(
                "{} {message}",
                crate::screens::prepare_strings::failure_headline(language)
            );
            ui.allocate_rect(total, Sense::hover())
                .widget_info(|| WidgetInfo::labeled(Role::Label, true, name.as_str()));
            painter.galley(pos2(x + 16.0, detail_top), detail, color::text_subtitle());
        }
        _ => {
            if let Some(summary) = cx.model.diagnoses.get(&game.id) {
                paint_label(
                    &painter,
                    pos2(x, mid),
                    &StateLabel {
                        icon: Icon::WARNING,
                        fill: true,
                        tint: color::warning_text(),
                        text: summary.clone(),
                    },
                    ty::DETAIL_LARGE,
                    MESSAGE_WIDTH,
                    2,
                    1.0,
                );
            }
        }
    }
}

/// The glibc notice belongs to the states in which a player unlocks or starts a game; a running
/// game has started already.
fn glibc_notice_applies(state: &GameState) -> bool {
    !matches!(
        state,
        GameState::Running | GameState::Starting | GameState::Unreadable
    )
}

/// One fact of the hero: a quiet overline over its value, with space and no card around it
/// (DESIGN.md 3.7). Returns its width.
fn fact(ui: &mut Ui, at: egui::Pos2, title: &str, value: &str, language: Language) -> f32 {
    let painter = ui.painter().clone();
    let overline = text::to_upper(title, language);
    let width = text::width(&painter, ty::INFO_OVERLINE, &overline).max(text::width(
        &painter,
        ty::INFO_VALUE,
        value,
    ));
    let rect = Rect::from_min_size(at, vec2(width, facts_height()));
    let response = place(ui, rect, |ui| {
        ui.allocate_exact_size(rect.size(), Sense::hover()).1
    });
    let name = format!("{title}: {value}");
    response.widget_info(|| WidgetInfo::labeled(Role::Label, true, name.as_str()));
    text::paint(
        &painter,
        at,
        Align2::LEFT_TOP,
        ty::INFO_OVERLINE,
        &overline,
        color::text_overline(),
    );
    text::paint(
        &painter,
        pos2(at.x, at.y + ty::INFO_OVERLINE.line_height() + FACT_GAP),
        Align2::LEFT_TOP,
        ty::INFO_VALUE,
        value,
        white(0.92),
    );
    width
}

// ---------------------------------------------------------------------------------------------
// Empty library
// ---------------------------------------------------------------------------------------------

fn empty_library(ui: &mut Ui, cx: &mut ScreenCx<'_>, screen: Rect, presses: &mut Vec<Press>) {
    let language = cx.language();
    let painter = ui.painter().clone();
    let scanning = cx.model.scanning;
    let folder = cx.model.prefs.games_folder.clone();
    let folder_set = folder.is_some();
    let title = language.text(if scanning {
        Key::Scanning
    } else {
        Key::EmptyLibraryTitle
    });
    let body = language.text(Key::EmptyLibraryText);

    let icon_size = 54.0;
    let rows = [
        icon_size,
        ty::EMPTY_TITLE.line_height(),
        ty::ROW_TITLE.line_height().max(17.0 * 1.35),
    ];
    let path_height = folder
        .as_ref()
        .map_or(0.0, |_| ty::DETAIL.line_height() + 18.0);
    let total =
        rows.iter().sum::<f32>() + 18.0 * 2.0 + path_height + 18.0 + 10.0 + size::BUTTON_MIN_HEIGHT;
    let region_top = screen.min.y + pad::HOME_TOP + size::ICON_BUTTON;
    let mut y = region_top + (screen.max.y - region_top - total) * 0.5;
    let cx_mid = screen.center().x;

    let icon = if scanning {
        Icon::MAGNIFYING_GLASS
    } else {
        Icon::STACK
    };
    let centre = pos2(cx_mid, y + icon_size * 0.5);
    // While it scans, the glass breathes in opacity (nothing moves, nothing shifts the layout).
    let glow = if scanning && !cx.ctx.reduced_motion() {
        cx.ctx.request_repaint();
        lerp(0.5, 0.9, crate::ui::anim::breathing(cx.ctx.clock()))
    } else {
        0.8
    };
    painter.text(
        centre,
        Align2::CENTER_CENTER,
        icon.glyph(),
        icon.font(icon_size, false),
        white(glow),
    );
    y += icon_size + 18.0;
    text::paint(
        &painter,
        pos2(cx_mid, y),
        Align2::CENTER_TOP,
        ty::EMPTY_TITLE,
        title,
        Color32::WHITE,
    );
    y += ty::EMPTY_TITLE.line_height() + 18.0;
    // While the scan runs the advice to add dumps would contradict the title.
    if !scanning {
        let body_galley = crate::screens::wrapped_centered(
            &painter,
            theme::TextStyle::new(17.0, Weight::Regular),
            body,
            color::text_empty(),
            (screen.width() - 2.0 * pad::HOME_X).min(720.0),
            3,
        );
        let body_height = body_galley.size().y;
        painter.galley(pos2(cx_mid, y), body_galley, color::text_empty());
        y += body_height.max(rows[2]);
    }
    y += 18.0;
    if let Some(folder) = folder {
        let shown = text::middle_truncate(
            &painter,
            ty::DETAIL,
            &folder.to_string_lossy(),
            screen.width() * 0.6,
        );
        text::paint(
            &painter,
            pos2(cx_mid, y),
            Align2::CENTER_TOP,
            ty::DETAIL,
            &shown,
            color::text_detail(),
        );
        y += ty::DETAIL.line_height() + 18.0;
    }
    y += 10.0;
    let focused = cx.model.overlay.is_none();
    let button_title = language.text(if folder_set {
        Key::ChangeFolder
    } else {
        Key::PickFolder
    });
    let button_width = 260.0;
    let rect = Rect::from_min_size(
        pos2(cx_mid - button_width * 0.5, y),
        vec2(button_width, size::BUTTON_MIN_HEIGHT),
    );
    let response = ui
        .scope_builder(
            UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::top_down(Align::Center)),
            |ui| {
                ConsoleButton::new(button_title)
                    .icon(Icon::FOLDER)
                    .prominent(true)
                    .focused(focused)
                    .show(ui)
            },
        )
        .inner;
    if response.clicked() {
        presses.push(Press::ChooseFolder);
    }
}

/// The colour under the avatar's initial: the middle of the accent's gradient (accent at 100 % to
/// 55 %) over the dark of the pill.
pub(crate) fn avatar_middle(accent: Color32) -> Color32 {
    theme::mix(Color32::from_rgb(28, 30, 36), accent, 0.775)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_profile_is_named_by_what_it_shows() {
        let english = Language::English;
        // Signed out there is no name to show, only the way in.
        assert_eq!(profile_label(None, false, false, 0, english), "Sign In");
        assert_eq!(
            profile_label(None, false, false, 0, Language::Turkish),
            "Giriş Yap"
        );
        assert_eq!(
            profile_label(Some("Mira"), false, false, 0, english),
            "Mira"
        );
        assert_eq!(
            profile_label(Some("Mira"), true, false, 3, english),
            "Mira, Online, 3 unread"
        );
        assert_eq!(
            profile_label(Some("Mira"), true, true, 3, english),
            "Mira, Appearing offline, 3 unread"
        );
    }

    #[test]
    fn the_friends_button_is_named_by_how_many_are_online() {
        let name = |n: i64| SocialText::FriendsOnline.format(Language::English, &[Arg::Int(n)]);
        assert_eq!(name(3), "Friends, 3 online");
        assert_eq!(
            SocialText::FriendsOnline.format(Language::Turkish, &[Arg::Int(3)]),
            "Arkadaşlar, 3 çevrimiçi"
        );
    }
}
