//! First start (docs/DESIGN.md 5.15): language, theme, games folder and, with a cloud, signing in.
//!
//! A left-aligned column on a left-to-right black gradient over the live theme backdrop. The
//! language and theme choices apply while the focus moves, so the page re-renders in that
//! language and the backdrop switches theme under the cards.

use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{Align2, Id, Painter, Rect, Role, Sense, Shape, Ui, WidgetInfo, pos2, vec2};

use crate::screens::legal::{Legal, consent_line};
use crate::screens::onboarding_strings::OnboardingText as OT;
use crate::screens::online_strings::OnlineText;
use crate::screens::{ScreenCx, place, wrapped};
use crate::ui::anim::{AnimExt, Motion, lerp};
use crate::ui::background::{Background, SLOT_CARDS, STILL_TIME_CARD};
use crate::ui::chrome::{self, language_card, onboarding_dots};
use crate::ui::components::{
    ConsoleButton, hint_bar, hint_bar_size, paint_focus_glow, paint_focus_stroke, scale_rect,
};
use crate::ui::glass::{GlassStyle, paint_glass};
use crate::ui::i18n::{Arg, Key, Language};
use crate::ui::icons::Icon;
use crate::ui::input;
use crate::ui::text;
use crate::ui::theme::{self, Theme, black, color, radius, size, space, ty};

/// Steps slide in from this far to the side.
const STEP_SLIDE: f32 = 40.0;
/// The gradient behind the column: black at the leading edge, lighter at the trailing edge.
const GRADIENT: (f32, f32) = (0.55, 0.1);
/// The widest a subtitle runs, and the status card of the folder step.
const SUBTITLE_WIDTH: f32 = 720.0;
const STATUS_WIDTH: f32 = 640.0;
const CONTINUE_WIDTH: f32 = 180.0;
/// The radius of the accent dot after a theme's name.
const ACCENT_DOT: f32 = 6.0;
/// The state glyph of the folder card.
const STATUS_GLYPH: f32 = 24.0;
/// The step's content stands this far below the dots, so the block sits a little lower than
/// the top-left corner (one offset for every step: the title never jumps).
const CONTENT_GAP: f32 = space::XXXL + space::L;

/// What a press on the onboarding asks the model to do.
enum Press {
    Language(usize),
    Theme(usize),
    Folder,
    /// Continue or Start on the folder step, and Not Now when no folder is set.
    Start,
    /// Continue on the language and theme steps.
    Continue,
    /// Sign In, Create Account or Not Now on the online step.
    Online(usize),
    /// A link of the sentence under them: the Terms of Service or the Privacy Policy.
    Legal(Legal),
    Back,
}

/// Paints the onboarding and applies its presses.
pub fn show(ui: &mut Ui, cx: &mut ScreenCx<'_>) {
    let screen = cx.ctx.content_rect();
    let painter = ui.painter().clone();
    paint_gradient(&painter, screen);

    let language = cx.language();
    let step = cx.model.onboarding.step;
    let steps = cx.model.onboarding_steps();
    let (pad_x, pad_y) = theme::pad::ONBOARDING;
    let left = screen.min.x + pad_x;
    let top = screen.min.y + pad_y;
    let mut presses = Vec::new();

    // Dots, and the same progress in words.
    place(
        ui,
        Rect::from_min_size(pos2(left, top), vec2(200.0, 5.0)),
        |ui| {
            ui.spacing_mut().interact_size.y = 0.0;
            onboarding_dots(ui, steps, step)
        },
    );
    let progress = OT::StepOf.format(
        language,
        &[Arg::Int(step as i64 + 1), Arg::Int(steps as i64)],
    );
    let dots_width = chrome::onboarding_dots_width(steps);
    let label = text::paint(
        &painter,
        pos2(left + dots_width + space::L, top + 2.5),
        Align2::LEFT_CENTER,
        ty::DETAIL,
        &progress,
        color::text_note(),
    );
    ui.interact(
        label.expand(space::XS),
        Id::unique("onboarding-progress"),
        Sense::hover(),
    )
    .widget_info(|| WidgetInfo::labeled(Role::Label, true, progress.as_str()));

    // The steps slide and fade as the step number animates.
    let animated = cx
        .ctx
        .animate_to("onboarding-step", step as f32, Motion::Layout);
    let content_top = top + 5.0 + CONTENT_GAP;
    for index in 0..steps {
        let d = animated - index as f32;
        if d.abs() >= 1.0 {
            continue;
        }
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(screen));
        child.set_opacity((1.0 - d.abs()) * ui.painter().opacity());
        let slide = if cx.ctx.reduced_motion() {
            0.0
        } else {
            STEP_SLIDE
        };
        let origin = pos2(left - slide * d, content_top);
        match index {
            0 => language_step(&mut child, cx, origin, language, &mut presses),
            1 => theme_step(&mut child, cx, origin, language, &mut presses),
            2 => folder_step(&mut child, cx, origin, language, &mut presses),
            _ => online_step(&mut child, cx, origin, language, &mut presses),
        }
    }

    // Bottom row: back button on the left, hints on the right.
    let bottom = screen.max.y - pad_y;
    if step > 0 {
        let rect = Rect::from_min_size(
            pos2(left, bottom - size::BUTTON_MIN_HEIGHT),
            vec2(200.0, size::BUTTON_MIN_HEIGHT),
        );
        let response = place(ui, rect, |ui| {
            ConsoleButton::new(language.text(Key::Back))
                .icon(Icon::CARET_LEFT)
                .show(ui)
        });
        if response.clicked() {
            presses.push(Press::Back);
        }
    }
    let (hints_width, hints_height) = hint_bar_size(&painter, language, false);
    let rect = Rect::from_min_size(
        pos2(
            screen.max.x - pad_x - hints_width,
            bottom - size::BUTTON_MIN_HEIGHT * 0.5 - hints_height * 0.5,
        ),
        vec2(hints_width, hints_height),
    );
    place(ui, rect, |ui| hint_bar(ui, language, false));

    for press in presses {
        match press {
            Press::Language(i) => cx.model.tap_language(i),
            Press::Theme(i) => cx.model.tap_theme(i),
            Press::Folder => {
                cx.model.set_onboarding_choice(0);
                cx.model.request_folder_dialog();
            }
            Press::Start => cx.model.onboarding_start(),
            Press::Continue => cx.model.onboarding_confirm(),
            Press::Online(i) => cx.model.tap_online(i),
            Press::Legal(page) => page.open(),
            Press::Back => cx.model.onboarding_back(),
        }
    }
}

/// Black 0.55 at the leading edge to 0.1 at the trailing edge.
fn paint_gradient(painter: &Painter, rect: Rect) {
    let mut mesh = Mesh::default();
    let (from, to) = (black(GRADIENT.0), black(GRADIENT.1));
    for (pos, colour) in [
        (rect.left_top(), from),
        (rect.left_bottom(), from),
        (rect.right_top(), to),
        (rect.right_bottom(), to),
    ] {
        mesh.vertices.push(Vertex {
            pos,
            uv: WHITE_UV,
            color: colour,
        });
    }
    mesh.indices.extend_from_slice(&[0, 1, 2, 1, 3, 2]);
    painter.add(Shape::mesh(mesh));
}

/// Title and subtitle of a step; returns the y at which the step's content starts.
fn step_title(painter: &Painter, origin: egui::Pos2, title: &str, subtitle: &str) -> f32 {
    text::paint(
        painter,
        origin,
        Align2::LEFT_TOP,
        ty::DISPLAY,
        title,
        color::text(),
    );
    let sub_y = origin.y + ty::DISPLAY.line_height() + space::M;
    let galley = wrapped(
        painter,
        ty::CATEGORY,
        subtitle,
        color::text_detail(),
        SUBTITLE_WIDTH,
        4,
    );
    let height = galley.size().y;
    painter.galley(pos2(origin.x, sub_y), galley, color::text_detail());
    sub_y + height + space::XXXL
}

fn language_step(
    ui: &mut Ui,
    cx: &mut ScreenCx<'_>,
    origin: egui::Pos2,
    language: Language,
    presses: &mut Vec<Press>,
) {
    let painter = ui.painter().clone();
    let mut y = step_title(
        &painter,
        origin,
        language.text(Key::Welcome),
        language.text(Key::WelcomeSubtitle),
    );
    text::paint(
        &painter,
        pos2(origin.x, y),
        Align2::LEFT_TOP,
        ty::OVERLINE,
        &text::to_upper(language.text(Key::ChooseLanguage), language),
        color::text_detail(),
    );
    y += ty::OVERLINE.line_height() + space::L;
    let choice = cx.model.onboarding_choice();
    let on_step = cx.model.onboarding.step == 0;
    for (index, card_language) in Language::ALL.iter().enumerate() {
        let rect = Rect::from_min_size(
            pos2(
                origin.x + index as f32 * (size::LANGUAGE_CARD.0 + space::XL),
                y,
            ),
            vec2(size::LANGUAGE_CARD.0, size::LANGUAGE_CARD.1),
        );
        let response = place(ui, rect, |ui| {
            language_card(ui, *card_language, on_step && choice == index, None)
        });
        if on_step && response.clicked() {
            presses.push(Press::Language(index));
        }
    }
    continue_button(
        ui,
        pos2(origin.x, y + size::LANGUAGE_CARD.1 + space::XXL),
        on_step,
        language,
        presses,
    );
}

/// The one primary action of a step that has no button of its own. A tap on it does what Enter
/// does; the keys and the pad keep the focus on the cards.
fn continue_button(
    ui: &mut Ui,
    at: egui::Pos2,
    on_step: bool,
    language: Language,
    presses: &mut Vec<Press>,
) {
    let row = Rect::from_min_size(
        at,
        vec2(
            CONTINUE_WIDTH + 2.0 * size::BUTTON_PADDING,
            size::BUTTON_MIN_HEIGHT,
        ),
    );
    let response = place(ui, row, |ui| {
        ConsoleButton::new(language.text(Key::ContinueAction))
            .icon(Icon::ARROW_RIGHT)
            .prominent(true)
            .min_width(CONTINUE_WIDTH)
            .show(ui)
    });
    if on_step && response.clicked() {
        presses.push(Press::Continue);
    }
}

fn theme_step(
    ui: &mut Ui,
    cx: &mut ScreenCx<'_>,
    origin: egui::Pos2,
    language: Language,
    presses: &mut Vec<Press>,
) {
    let ctx = cx.ctx.clone();
    let painter = ui.painter().clone();
    let y = step_title(
        &painter,
        origin,
        language.text(Key::ChooseTheme),
        language.text(Key::ChooseThemeSubtitle),
    );
    let choice = cx.model.onboarding_choice();
    let on_step = cx.model.onboarding.step == 1;
    let opacity = painter.opacity();
    let (width, preview_height) = size::THEME_CARD_PREVIEW;
    let name_style = ty::THEME_CARD_NAME_FOCUSED;
    let column_height = preview_height + space::M + name_style.line_height();
    for (index, theme) in Theme::ALL.iter().enumerate() {
        let focused = on_step && choice == index;
        let t = ctx.choice_t(("onboarding-theme", index), focused);
        // Unfocused cards keep their size, so they share the left edge with the title.
        let factor = lerp(1.0, 1.03, t);
        let column = scale_rect(
            Rect::from_min_size(
                pos2(origin.x + index as f32 * (width + space::XXL), y),
                vec2(width, column_height),
            ),
            factor,
        );
        let preview =
            Rect::from_min_size(column.min, vec2(width * factor, preview_height * factor));
        let card_radius = radius::THEME_CARD * factor;
        // The focused card is live, the others hold a still frame.
        let time = if focused {
            STILL_TIME_CARD + ctx.clock() as f32
        } else {
            STILL_TIME_CARD
        };
        let mut layer = Background::new(SLOT_CARDS + theme.index(), *theme, time);
        layer.radius = card_radius;
        layer.alpha = lerp(0.72, 1.0, t) * opacity;
        paint_focus_glow(&painter, preview, card_radius, size::FOCUS_RING_LARGE, t);
        layer.paint(&painter, preview);
        paint_focus_stroke(&painter, preview, card_radius, size::FOCUS_RING_LARGE, t);
        if focused {
            ctx.request_repaint();
        }

        // The names sit on one baseline under the resting cards, so the focus only grows the card.
        let resting_left = origin.x + index as f32 * (width + space::XXL);
        let name_y = y + preview_height + space::M + name_style.line_height() * 0.5;
        let style = if focused {
            ty::THEME_CARD_NAME_FOCUSED
        } else {
            ty::THEME_CARD_NAME
        };
        let faded = theme::fade(color::text(), lerp(0.72, 1.0, t));
        let name = text::paint(
            &painter,
            pos2(resting_left, name_y),
            Align2::LEFT_CENTER,
            style,
            theme.title(language),
            faded,
        );
        // The accent the theme brings, right after its name.
        painter.circle_filled(
            pos2(name.max.x + space::M + ACCENT_DOT, name_y),
            ACCENT_DOT,
            theme::fade(theme.default_accent().color(), lerp(0.72, 1.0, t)),
        );

        if on_step {
            let response = ui.interact(
                column,
                Id::unique(("onboarding-theme-card", index)),
                Sense::click(),
            );
            let label = theme.title(language);
            response.widget_info(|| WidgetInfo::labeled(Role::Button, true, label));
            crate::ui::a11y::follow(&ctx, response.id, focused);
            if input::hovered(&ctx, &response) {
                ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if response.clicked() {
                presses.push(Press::Theme(index));
            }
        }
    }
    continue_button(
        ui,
        pos2(origin.x, y + column_height + space::XXL),
        on_step,
        language,
        presses,
    );
}

fn folder_step(
    ui: &mut Ui,
    cx: &mut ScreenCx<'_>,
    origin: egui::Pos2,
    language: Language,
    presses: &mut Vec<Press>,
) {
    let ctx = cx.ctx.clone();
    let painter = ui.painter().clone();
    let mut y = step_title(
        &painter,
        origin,
        language.text(Key::ChooseFolder),
        language.text(Key::ChooseFolderSubtitle),
    );
    let on_step = cx.model.onboarding.step == 2;
    let choice = cx.model.onboarding_choice();
    let folder = cx.model.prefs.games_folder.clone();
    let accent = cx.accent();

    // Buttons: the primary first. Without a folder it is Choose Folder, and Not Now goes on
    // without one; with a folder it is Continue (Start on the last step) and Change Folder.
    let row = Rect::from_min_size(pos2(origin.x, y), vec2(800.0, size::BUTTON_MIN_HEIGHT));
    let has_folder = folder.is_some();
    let last = cx.model.onboarding_steps() <= 3;
    place(ui, row, |ui| {
        ui.spacing_mut().item_spacing.x = size::CONTROL_GAP;
        let (first_label, first_icon) = if has_folder {
            (
                language.text(if last {
                    Key::Start
                } else {
                    Key::ContinueAction
                }),
                Icon::ARROW_RIGHT,
            )
        } else {
            (language.text(Key::PickFolder), Icon::FOLDER)
        };
        let first = ConsoleButton::new(first_label)
            .icon(first_icon)
            .prominent(true)
            .min_width(CONTINUE_WIDTH)
            .focused(on_step && choice == 0)
            .show(ui);
        if on_step && first.clicked() {
            presses.push(if has_folder {
                Press::Start
            } else {
                Press::Folder
            });
        }
        let (second_label, second_icon) = if has_folder {
            (language.text(Key::ChangeFolder), Icon::FOLDER)
        } else {
            (OT::NotNow.text(language), Icon::ARROW_RIGHT)
        };
        let second = ConsoleButton::new(second_label)
            .icon(second_icon)
            .min_width(CONTINUE_WIDTH)
            .focused(on_step && choice == 1)
            .show(ui);
        if on_step && second.clicked() {
            presses.push(if has_folder {
                Press::Folder
            } else {
                Press::Start
            });
        }
    });
    y += size::BUTTON_MIN_HEIGHT + space::XL;

    let Some(folder) = folder else { return };
    // The status card.
    let scanning = cx.model.scanning;
    let found = cx.model.games.len();

    let status_line = ty::ROW_TITLE.line_height();
    let path_line = ty::DETAIL.line_height();
    let content_height = status_line + space::XS + path_line;
    let card = Rect::from_min_size(
        pos2(origin.x, y),
        vec2(STATUS_WIDTH, content_height + 2.0 * space::XL),
    );
    paint_glass(&ctx, &painter, card, GlassStyle::glass(radius::CARD, 0.38));
    let (icon, fill, tint) = if scanning {
        (Icon::MAGNIFYING_GLASS, false, accent)
    } else if found == 0 {
        (Icon::FOLDER_SIMPLE_DASHED, false, color::text_detail())
    } else {
        (Icon::CHECK_CIRCLE, true, accent)
    };
    let glyph = STATUS_GLYPH;
    painter.text(
        pos2(card.min.x + space::XL + glyph * 0.5, card.center().y),
        Align2::CENTER_CENTER,
        icon.glyph(),
        icon.font(glyph, fill),
        tint,
    );
    let text_x = card.min.x + space::XL + glyph + space::M;
    let status = if scanning {
        language.text(Key::Scanning).to_owned()
    } else if found == 0 {
        language.text(Key::NoGamesFound).to_owned()
    } else {
        cx.model.games_found_text()
    };
    let text_top = card.center().y - content_height * 0.5;
    text::paint(
        &painter,
        pos2(text_x, text_top),
        Align2::LEFT_TOP,
        ty::ROW_TITLE,
        &status,
        color::text(),
    );
    let shown = text::middle_truncate(
        &painter,
        ty::DETAIL,
        &folder.to_string_lossy(),
        card.max.x - space::XL - text_x,
    );
    text::paint(
        &painter,
        pos2(text_x, text_top + status_line + space::XS),
        Align2::LEFT_TOP,
        ty::DETAIL,
        &shown,
        color::text_detail(),
    );
    y = card.max.y + space::XL;

    // The first eight games.
    for (index, game) in cx.model.games.clone().iter().take(8).enumerate() {
        let side = size::ONBOARDING_TILE;
        let rect = Rect::from_min_size(
            pos2(origin.x + index as f32 * (side + space::M), y),
            vec2(side, side),
        );
        let style = crate::screens::home::tile_style(cx, game, side, accent);
        chrome::paint_tile(&painter, rect, &style);
    }
}

/// The last step, only with a cloud: Sign In (prominent), Create Account and Not Now in one row,
/// and a line under them that says where signing in waits later.
fn online_step(
    ui: &mut Ui,
    cx: &mut ScreenCx<'_>,
    origin: egui::Pos2,
    language: Language,
    presses: &mut Vec<Press>,
) {
    let painter = ui.painter().clone();
    let y = step_title(
        &painter,
        origin,
        OT::OnlineTitle.text(language),
        OT::OnlineSubtitle.text(language),
    );
    let on_step = cx.model.onboarding.step == 3;
    let choice = cx.model.onboarding_choice();

    let row = Rect::from_min_size(pos2(origin.x, y), vec2(800.0, size::BUTTON_MIN_HEIGHT));
    place(ui, row, |ui| {
        ui.spacing_mut().item_spacing.x = size::CONTROL_GAP;
        let buttons = [
            (OnlineText::SignIn.text(language), Icon::SIGN_IN, true),
            (
                OnlineText::CreateAccount.text(language),
                Icon::USER_PLUS,
                false,
            ),
            (OT::NotNow.text(language), Icon::ARROW_RIGHT, false),
        ];
        for (index, (label, icon, prominent)) in buttons.into_iter().enumerate() {
            let response = ConsoleButton::new(label)
                .icon(icon)
                .prominent(prominent)
                .min_width(CONTINUE_WIDTH)
                .focused(on_step && choice == index)
                .show(ui);
            if on_step && response.clicked() {
                presses.push(Press::Online(index));
            }
        }
    });
    // What creating an account means agreeing to, under the buttons that lead there.
    let consent_top = y + size::BUTTON_MIN_HEIGHT + space::L;
    let (page, consent_height) = consent_line(
        ui,
        pos2(origin.x, consent_top),
        SUBTITLE_WIDTH,
        language,
        on_step,
    );
    if let Some(page) = page {
        presses.push(Press::Legal(page));
    }
    text::paint(
        &painter,
        pos2(origin.x, consent_top + consent_height + space::M),
        Align2::LEFT_TOP,
        ty::DETAIL,
        OT::OnlineLater.text(language),
        color::text_note(),
    );
}
