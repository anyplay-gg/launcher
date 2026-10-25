//! The profile page (docs/DESIGN.md 17, "Profile"): a banner with the avatar, the handle and the
//! text of an account, up to three favourite games, and rows for the trophies, the most played
//! games and the linked accounts. The player's own profile adds Edit Profile, which opens the
//! account web; nothing is edited in the launcher.
//!
//! The cloud sends no banner picture. The banner takes its colour from the avatar (computed once
//! from its pixels, see `anyps5_core::online::banner_colour`) and, without an avatar, a calm
//! colour from the launcher's accent. A part the cloud leaves out is not shown: for a stranger a
//! missing part tells nothing, and a placeholder would tell it was hidden.

use anyps5_core::online::api::{AccountId, ProfileLink, ProfilePage};
use anyps5_core::online::{self, Purpose};
use egui::{Color32, Context, Rect, Sense, Ui, pos2, vec2};

use super::feed::paint_avatar;
use super::local::{Avatar, Local};
use super::{OnlineAction, OnlineData, OnlinePanel, Page, Row, trophy_row};
use crate::screens::online_feed_strings::FeedText as F;
use crate::screens::online_strings::OnlineText as T;
use crate::screens::rows;
use crate::screens::side::{self, SideDrawn, SideView};
use crate::ui::Shell;
use crate::ui::chrome::{PanelRow, RowControl, TileImage, TileStyle, paint_tile};
use crate::ui::components::corner;
use crate::ui::focus::RowKind;
use crate::ui::i18n::{Arg, Language};
use crate::ui::icons::Icon;
use crate::ui::input::Command;
use crate::ui::sound::Cue;
use crate::ui::theme::{TextStyle, Weight, black, color, mix, radius, white};

/// The most favourite games and most played games a profile shows.
const FAVOURITES: usize = 3;
const MOST_PLAYED: usize = 3;

const PAD: f32 = 16.0;
const AVATAR: f32 = 72.0;
const HANDLE: TextStyle = TextStyle::new(24.0, Weight::SemiBold);
const ABOUT: TextStyle = TextStyle::new(14.0, Weight::Regular);
const COVER: f32 = 64.0;
const SHELF_GAP: f32 = 12.0;
const GAME_NAME: TextStyle = TextStyle::new(13.0, Weight::Medium);

/// The banner colour for an avatar, or a calm one of the theme's accent without a usable one.
pub fn banner_for(avatar: Option<Avatar>, accent: Color32) -> [u8; 3] {
    avatar.and_then(|a| a.banner).unwrap_or_else(|| {
        let calm = mix(accent, black(1.0), 0.62);
        online::readable_banner([calm.r(), calm.g(), calm.b()])
    })
}

/// Playtime in words: "35 min", "4 h 20 min", "12 h". From ten hours on, the minutes are noise.
pub fn playtime_text(minutes: u64, language: Language) -> String {
    let (hours, rest) = online::hours_minutes(minutes);
    let number = |n: u64| Arg::Int(i64::try_from(n).unwrap_or(i64::MAX));
    if hours == 0 {
        F::Minutes.format(language, &[number(rest)])
    } else if hours >= 10 || rest == 0 {
        F::Hours.format(language, &[number(hours)])
    } else {
        F::HoursMinutes.format(language, &[number(hours), number(rest)])
    }
}

/// The name of a linked account's provider.
fn provider_name(link: &ProfileLink, language: Language) -> String {
    match link.provider.as_str() {
        "discord" => F::Discord.text(language).to_owned(),
        "steam" => F::Steam.text(language).to_owned(),
        other => other.to_owned(),
    }
}

fn provider_icon(link: &ProfileLink) -> Icon {
    match link.provider.as_str() {
        "discord" => Icon::DISCORD_LOGO,
        "steam" => Icon::STEAM_LOGO,
        _ => Icon::LINK,
    }
}

/// A favourite game as the header draws it.
struct Favourite {
    name: String,
    icon: Option<TileImage>,
    owned: bool,
    title_id: String,
}

fn favourites(page: &ProfilePage, local: &Local) -> Vec<Favourite> {
    page.profile
        .favourites
        .iter()
        .flatten()
        .take(FAVOURITES)
        .map(|title_id| {
            let game = local.games.get(title_id);
            Favourite {
                name: game.map_or_else(|| title_id.clone(), |g| g.name.clone()),
                icon: game.and_then(|g| g.icon).map(|texture| TileImage {
                    texture,
                    aspect: 1.0,
                }),
                owned: game.is_some(),
                title_id: title_id.clone(),
            }
        })
        .collect()
}

impl OnlinePanel {
    /// The page's title, subtitle and rows.
    pub(super) fn profile_rows(
        &self,
        id: AccountId,
        data: &OnlineData<'_>,
        rows: &mut Vec<Row>,
    ) -> (String, Option<String>) {
        let language = data.language;
        let online = data.online;
        let me = online.account.as_ref().map(|a| a.id);
        let own = me == Some(id);
        let entry = online.profiles.get(&id);
        let page = entry.and_then(|e| e.page.as_ref());
        let handle = page
            .map(|p| p.profile.account.handle.clone())
            .or_else(|| online.known(id).map(|a| a.handle.clone()))
            .or_else(|| {
                own.then(|| online.account.as_ref().map(|a| a.handle.clone()))
                    .flatten()
            })
            .unwrap_or_default();
        let subtitle = if own {
            Some(F::OwnSubtitle.text(language).to_owned())
        } else {
            online.friend(id).map(|f| super::presence_text(f, language))
        };
        let title = F::Profile.text(language).to_owned();
        let Some(page) = page else {
            if entry.is_some_and(|e| e.failed) {
                rows.push(Row::info(
                    "profileState",
                    F::ProfileFailed.text(language),
                    None,
                ));
                rows.push(
                    Row::command("profileRetry", F::TryAgain.text(language))
                        .icon(Icon::ARROWS_CLOCKWISE),
                );
            } else {
                rows.push(Row::info(
                    "profileState",
                    F::ProfileLoading.text(language),
                    None,
                ));
            }
            return (title, subtitle.or(Some(handle)));
        };
        let played = page
            .profile
            .playtime
            .as_deref()
            .map(|p| online::most_played(p, MOST_PLAYED))
            .unwrap_or_default();
        let earned = page
            .trophies
            .as_ref()
            .is_some_and(|c| c.platinum + c.gold + c.silver + c.bronze > 0);
        let empty = !earned
            && played.is_empty()
            && page.links.is_empty()
            && page.profile.about.is_none()
            && page.profile.favourites.as_ref().is_none_or(Vec::is_empty);
        // The player's own profile leads with its one action, under the banner where the eye
        // lands, so the page opens on it with the banner in view.
        if own {
            let waiting = online.is_waiting(|p| matches!(p, Purpose::AccountPage));
            rows.push(
                Row::command("editProfile", F::EditProfile.text(language))
                    .icon(Icon::GLOBE)
                    .detail(if waiting {
                        T::Sending.text(language)
                    } else {
                        F::EditProfileDetail.text(language)
                    }),
            );
            if empty {
                rows.push(Row::info("ownEmpty", F::OwnEmpty.text(language), None));
            }
        }
        // The trophies: the player may know they are hidden, as a friend's gamer card says.
        let mut trophies = match &page.trophies {
            Some(count) => trophy_row(Some(count), language),
            None => Row::info("trophies", F::TrophiesPrivate.text(language), None),
        };
        trophies.group_start = own;
        rows.push(trophies);
        if !played.is_empty() {
            rows.push(Row::caption("playedCaption", F::MostPlayed.text(language)).group_start());
            for title in played {
                let name = self
                    .local
                    .game_name(&title.title_id)
                    .map_or_else(|| title.title_id.clone(), str::to_owned);
                rows.push(
                    Row::info("played", name, Some(playtime_text(title.minutes, language)))
                        .icon(Icon::GAME_CONTROLLER),
                );
            }
        }
        if !page.links.is_empty() {
            rows.push(Row::caption("linksCaption", F::LinkedAccounts.text(language)).group_start());
            for link in &page.links {
                let name = link
                    .name
                    .clone()
                    .unwrap_or_else(|| F::Linked.text(language).to_owned());
                rows.push(
                    Row::info("link", provider_name(link, language), Some(name))
                        .icon(provider_icon(link)),
                );
            }
        }
        (title, subtitle)
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

    /// A command on the profile page. Up and down walk every row, the facts too: nothing on the
    /// page takes the focus but Edit Profile, and a pad has no other way to scroll the rest into
    /// view. A fact under the focus shows no highlight; the list follows it. `true` leaves the page.
    pub(super) fn profile_command(
        &mut self,
        shell: &mut Shell,
        command: Command,
        data: &OnlineData<'_>,
        actions: &mut Vec<OnlineAction>,
    ) -> bool {
        let rows = self.rows(data).2;
        let last = rows.len().saturating_sub(1);
        let now = self.side.nav.row.min(last);
        match command {
            Command::Up | Command::Down => {
                let next = if command == Command::Up {
                    now.saturating_sub(1)
                } else {
                    (now + 1).min(last)
                };
                if next != now {
                    shell.play(Cue::Move);
                }
                self.side.nav.row = next;
            }
            Command::Select => {
                if let Some(row) = rows.get(now)
                    && row.kind() != RowKind::Info
                {
                    shell.play(Cue::Select);
                    self.run(row.id, data, actions);
                }
            }
            Command::Back | Command::Options => return true,
            Command::Left | Command::Right => {}
        }
        false
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
                    self.go(Page::Profile(me), data);
                }
            }
            ("feed", _) => self.go(Page::Feed, data),
            ("viewProfile", Page::Friend(id)) => {
                let id = *id;
                self.go(Page::Profile(id), data);
            }
            ("editProfile", Page::Profile(_)) => actions.push(OnlineAction::ManageAccount),
            ("profileRetry", Page::Profile(id)) => actions.push(OnlineAction::Profile(*id)),
            ("feedRetry", Page::Feed) => actions.push(OnlineAction::FeedRefresh),
            _ => return false,
        }
        true
    }

    /// Draws the page: the banner and the favourites, then the rows.
    pub(super) fn profile_draw(
        &self,
        ctx: &Context,
        view: &SideView<'_>,
        rows: &mut [Row],
        id: AccountId,
        data: &OnlineData<'_>,
    ) -> Option<SideDrawn> {
        let online = data.online;
        let local = &self.local;
        let page = online.profiles.get(&id).and_then(|e| e.page.as_ref());
        let language = data.language;
        let accent = data.accent;
        let open = self.side.is_open();
        side::draw_with_header(ctx, open, view, rows, |ui| {
            let Some(page) = page else {
                return;
            };
            let avatar = local.avatar(page.profile.avatar.as_deref());
            let banner = banner_for(avatar, accent);
            let header = Header {
                handle: &page.profile.account.handle,
                about: page.profile.about.as_deref(),
                account: id,
                avatar,
                banner,
                favourites: favourites(page, local),
                language,
                accent,
            };
            paint_header(ui, &header);
        })
    }
}

struct Header<'a> {
    handle: &'a str,
    about: Option<&'a str>,
    account: AccountId,
    avatar: Option<Avatar>,
    banner: [u8; 3],
    favourites: Vec<Favourite>,
    language: Language,
    accent: Color32,
}

fn paint_header(ui: &mut Ui, header: &Header<'_>) {
    let painter = ui.painter().clone();
    let width = ui.available_width();
    let [r, g, b] = header.banner;
    let banner = Color32::from_rgb(r, g, b);

    // The banner: avatar, handle, text.
    let text_x = PAD + AVATAR + PAD;
    let text_width = width - text_x - PAD;
    let handle = {
        let mut job = egui::text::LayoutJob::simple(
            header.handle.to_owned(),
            HANDLE.font(),
            Color32::PLACEHOLDER,
            text_width,
        );
        job.wrap.max_rows = 1;
        job.sections[0].format.line_height = Some(HANDLE.line_height());
        painter.layout_job(job)
    };
    let about = header.about.map(|about| {
        let mut job = egui::text::LayoutJob::simple(
            about.to_owned(),
            ABOUT.font(),
            Color32::PLACEHOLDER,
            text_width,
        );
        job.wrap.max_rows = 3;
        job.sections[0].format.line_height = Some(1.4 * ABOUT.size);
        painter.layout_job(job)
    });
    let words = handle.size().y + about.as_ref().map_or(0.0, |a| 6.0 + a.size().y);
    let height = 2.0 * PAD + words.max(AVATAR);
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Label,
            true,
            F::BannerName.format(header.language, &[Arg::Str(header.handle)]),
        )
    });
    if ui.is_rect_visible(rect) {
        painter.rect_filled(rect, corner(radius::CARD), banner);
        // A light edge keeps a dark banner apart from the glass behind it.
        painter.rect_stroke(
            rect,
            corner(radius::CARD),
            egui::Stroke::new(1.0, white(0.10)),
            egui::StrokeKind::Inside,
        );
        let avatar_rect = Rect::from_min_size(
            pos2(rect.min.x + PAD, rect.center().y - AVATAR * 0.5),
            vec2(AVATAR, AVATAR),
        );
        paint_avatar(
            &painter,
            avatar_rect,
            header.avatar,
            header.account,
            header.handle,
        );
        let x = rect.min.x + text_x;
        // The words sit centred against the avatar when they are short.
        let mut y = rect.center().y - words * 0.5;
        let handle_height = handle.size().y;
        painter.galley(pos2(x, y), handle, Color32::WHITE);
        y += handle_height + 6.0;
        if let Some(about) = about {
            painter.galley(pos2(x, y), about, white(online::BANNER_TEXT_ALPHA));
        }
    }

    if header.favourites.is_empty() {
        ui.add_space(4.0);
        return;
    }
    PanelRow::new(F::Favourites.text(header.language), RowControl::Caption)
        .language(header.language)
        .show(ui);
    paint_shelf(ui, header);
    ui.add_space(4.0);
}

/// Up to three game cards side by side: the cover over the name.
fn paint_shelf(ui: &mut Ui, header: &Header<'_>) {
    let painter = ui.painter().clone();
    let width = ui.available_width();
    let count = FAVOURITES as f32;
    let cell = (width - SHELF_GAP * (count - 1.0)) / count;
    let name_height = GAME_NAME.line_height() * 2.0;
    let height = PAD * 0.75 + COVER + 8.0 + name_height + PAD * 0.75;
    let (strip, strip_response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    if !ui.is_rect_visible(strip) {
        return;
    }
    for (n, game) in header.favourites.iter().enumerate() {
        let card = Rect::from_min_size(
            pos2(strip.min.x + n as f32 * (cell + SHELF_GAP), strip.min.y),
            vec2(cell, height),
        );
        painter.rect_filled(card, corner(radius::ROW), color::surface_row());
        let cover = Rect::from_center_size(
            pos2(card.center().x, card.min.y + PAD * 0.75 + COVER * 0.5),
            vec2(COVER, COVER),
        );
        let mut style = TileStyle::new(header.accent);
        style.image = game.icon;
        // A game the player does not have shows no art, only that it is not theirs.
        style.opacity = if game.owned { 1.0 } else { 0.55 };
        paint_tile(&painter, cover, &style);
        let ink = if game.owned {
            Color32::WHITE
        } else {
            white(0.62)
        };
        let galley = {
            let mut job = egui::text::LayoutJob::simple(
                game.name.clone(),
                GAME_NAME.font(),
                Color32::PLACEHOLDER,
                cell - 2.0 * 10.0,
            );
            job.wrap.max_rows = 2;
            job.halign = egui::Align::Center;
            job.sections[0].format.line_height = Some(GAME_NAME.line_height());
            painter.layout_job(job)
        };
        let top = card.min.y + PAD * 0.75 + COVER + 8.0;
        // A centred galley is placed by its middle.
        painter.galley(pos2(card.center().x, top), galley, ink);
        let name = if game.owned {
            game.name.clone()
        } else {
            F::NotInLibrary.format(header.language, &[Arg::Str(&game.title_id)])
        };
        let response = ui.interact(
            card,
            strip_response.id.with(("favourite", n)),
            Sense::hover(),
        );
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, name.clone()));
    }
}

#[cfg(test)]
mod tests {
    use anyps5_core::online::contrast_with_white;

    use crate::ui::theme;

    use super::*;

    #[test]
    fn playtime_is_told_in_the_unit_that_fits() {
        let en = Language::English;
        assert_eq!(playtime_text(35, en), "35 min");
        assert_eq!(playtime_text(260, en), "4 h 20 min");
        assert_eq!(playtime_text(240, en), "4 h");
        assert_eq!(playtime_text(60 * 12 + 30, en), "12 h");
        assert_eq!(playtime_text(60 * 12 + 30, Language::Turkish), "12 sa");
    }

    #[test]
    fn the_banner_without_an_avatar_is_calm_and_readable() {
        for accent in theme::Accent::ALL {
            let [r, g, b] = banner_for(None, accent.color());
            assert!(
                contrast_with_white([r, g, b]) >= online::BANNER_CONTRAST,
                "{accent:?}"
            );
        }
    }

    #[test]
    fn the_banner_takes_the_avatars_colour_when_it_has_one() {
        let avatar = Avatar {
            texture: egui::TextureId::default(),
            banner: Some([20, 60, 70]),
        };
        assert_eq!(banner_for(Some(avatar), Color32::RED), [20, 60, 70]);
        let grey = Avatar {
            banner: None,
            ..avatar
        };
        assert_ne!(banner_for(Some(grey), Color32::RED), [20, 60, 70]);
    }

    #[test]
    fn providers_are_named_and_unknown_ones_kept() {
        let link = |p: &str| ProfileLink {
            provider: p.into(),
            name: None,
        };
        assert_eq!(
            provider_name(&link("discord"), Language::English),
            "Discord"
        );
        assert_eq!(provider_name(&link("steam"), Language::English), "Steam");
        assert_eq!(provider_name(&link("gog"), Language::English), "gog");
        assert_eq!(provider_icon(&link("discord")), Icon::DISCORD_LOGO);
        assert_eq!(provider_icon(&link("steam")), Icon::STEAM_LOGO);
    }
}
