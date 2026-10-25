//! Invented people for the gallery scenes of the profile screen and the feed (`--gallery=profile-own`
//! and so on). Names, pictures, games and trophies are made up; the title IDs are not real ones.

use anyps5_core::online::api::{
    Account, AccountId, Count, Feed, FeedEvent, FeedItem, Grade, Profile, ProfileLink, ProfilePage,
    Reaction, ReactionCounts, TitlePlaytime,
};
use anyps5_core::online::{self, Failure, Online, ProfileEntry};
use egui::{Color32, ColorImage, Context, TextureHandle, TextureOptions};

use crate::screens::gallery::{OnlineLook, ProfileLook};
use crate::screens::online::{Avatar, Local, OwnedGame};

/// The moment the invented items are told from (see `gallery::SAMPLE_NOW`).
const NOW: u64 = 1_792_670_400;

/// Avatar hashes of the invented players (not real hashes: nothing is fetched here).
const MIRA: &str = "aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11aa11";
const LUMEN: &str = "bb22bb22bb22bb22bb22bb22bb22bb22bb22bb22bb22bb22bb22bb22bb22bb22";
const JUNO: &str = "cc33cc33cc33cc33cc33cc33cc33cc33cc33cc33cc33cc33cc33cc33cc33cc33";

/// The invented games: title ID, and the index of the gallery's cover and name.
const OWNED: [(&str, usize); 3] = [("GAME00001", 1), ("GAME00002", 4), ("GAME00003", 2)];

fn who(id: u64, handle: &str) -> Account {
    Account {
        id: AccountId(id),
        handle: handle.into(),
    }
}

fn counts(nice: u32, gg: u32, wow: u32, same: u32) -> ReactionCounts {
    ReactionCounts {
        nice,
        gg,
        wow,
        same,
    }
}

fn item(
    id: u64,
    account: Account,
    ago: u64,
    event: FeedEvent,
    reactions: ReactionCounts,
    mine: Option<Reaction>,
) -> FeedItem {
    FeedItem {
        id,
        account,
        at: NOW - ago,
        event,
        reactions,
        mine,
    }
}

/// The invented items, newest first.
fn items() -> Vec<FeedItem> {
    let mut items = raw_items();
    items.sort_by_key(|i| std::cmp::Reverse(i.at));
    items
}

fn raw_items() -> Vec<FeedItem> {
    let title = |t: &str| t.to_owned();
    vec![
        item(
            9,
            who(2, "Lumen"),
            4 * 60,
            FeedEvent::Trophy {
                title_id: title("GAME00001"),
                number: 4,
                grade: Grade::Gold,
                rarity: Some(2.3),
            },
            counts(3, 1, 0, 0),
            None,
        ),
        item(
            8,
            who(5, "Rook"),
            2 * 3_600,
            FeedEvent::Friend {
                with: who(9, "Nox"),
            },
            counts(0, 0, 1, 0),
            Some(Reaction::Wow),
        ),
        item(
            7,
            who(1, "Mira"),
            5 * 3_600,
            FeedEvent::FirstPlayed {
                title_id: title("GAME00003"),
            },
            counts(2, 0, 0, 0),
            None,
        ),
        item(
            6,
            who(2, "Lumen"),
            30 * 3_600,
            FeedEvent::Party {
                members: vec![who(2, "Lumen"), who(9, "Nox"), who(4, "Kit")],
            },
            counts(0, 4, 0, 1),
            None,
        ),
        item(
            5,
            who(3, "Juno"),
            3 * 86_400,
            FeedEvent::Trophy {
                title_id: title("GAME00004"),
                number: 1,
                grade: Grade::Platinum,
                rarity: Some(0.6),
            },
            counts(0, 0, 0, 0),
            None,
        ),
        item(
            4,
            who(4, "Kit"),
            4 * 86_400,
            FeedEvent::FirstPlayed {
                title_id: title("GAME00005"),
            },
            counts(0, 0, 0, 2),
            None,
        ),
        item(
            10,
            who(1, "Mira"),
            26 * 3_600,
            FeedEvent::Friend {
                with: who(11, "Sol"),
            },
            counts(0, 0, 1, 0),
            Some(Reaction::Nice),
        ),
        item(
            11,
            who(2, "Lumen"),
            2 * 86_400,
            FeedEvent::FirstPlayed {
                title_id: title("GAME00002"),
            },
            counts(1, 0, 0, 0),
            None,
        ),
        item(
            3,
            who(2, "Lumen"),
            5 * 86_400,
            FeedEvent::Trophy {
                title_id: title("GAME00003"),
                number: 9,
                grade: Grade::Silver,
                rarity: Some(18.4),
            },
            counts(1, 0, 0, 0),
            None,
        ),
    ]
}

fn profile(account: Account) -> Profile {
    Profile {
        account,
        about: None,
        favourites: None,
        avatar: None,
        playtime: None,
    }
}

fn played(title_id: &str, minutes: u64) -> TitlePlaytime {
    TitlePlaytime {
        title_id: title_id.into(),
        minutes,
    }
}

/// The profiles and the feed the looks show, over the invented friends of the gallery.
pub fn sample(state: &mut Online, look: OnlineLook) {
    use OnlineLook as L;
    state.avatars.insert(AccountId(1), Some(MIRA.into()));
    state.avatars.insert(AccountId(2), Some(LUMEN.into()));
    state.avatars.insert(AccountId(3), Some(JUNO.into()));
    match look {
        L::Feed | L::FeedReaction => {
            state.feed.merge_newest(Feed {
                items: items(),
                next: Some(3),
            });
        }
        L::FeedEmpty => state.feed.merge_newest(Feed::default()),
        L::FeedLoading => {}
        L::FeedError => state.feed.error = Some(Failure::new(Failure::OFFLINE, "unreachable")),
        _ => {}
    }
}

/// The profile screen's looks over the invented friends: the profile pages that are there, and the
/// feed.
pub fn sample_profile(state: &mut Online, look: ProfileLook) {
    use ProfileLook as L;
    let entry = |page: ProfilePage| ProfileEntry {
        page: Some(page),
        failed: false,
    };
    state.feed.merge_newest(Feed {
        items: items(),
        next: None,
    });
    let page = |account: Account, build: &dyn Fn(&mut Profile)| {
        let mut p = profile(account);
        build(&mut p);
        p
    };
    match look {
        L::Own | L::OwnFocus | L::OwnEnd => {
            let p = page(who(1, "Mira"), &|p| {
                p.about = Some(
                    "Co-op on weekends, trophy hunting when it rains. Always up for a party."
                        .into(),
                );
                p.avatar = Some(MIRA.into());
            });
            state.profiles.insert(
                AccountId(1),
                entry(ProfilePage {
                    profile: p,
                    trophies: Some(Count {
                        bronze: 30,
                        silver: 9,
                        gold: 4,
                        platinum: 1,
                    }),
                    links: vec![ProfileLink {
                        provider: "discord".into(),
                        name: Some("mira.plays".into()),
                    }],
                }),
            );
        }
        L::New => {
            state.profiles.insert(
                AccountId(1),
                entry(ProfilePage {
                    profile: profile(who(1, "Mira")),
                    trophies: Some(Count::default()),
                    links: Vec::new(),
                }),
            );
            state.avatars.insert(AccountId(1), None);
            state.feed.merge_newest(Feed::default());
        }
        L::Friend => {
            let p = page(who(2, "Lumen"), &|p| {
                p.about = Some("Speedruns, soft launches and strong opinions about co-op.".into());
                p.favourites = Some(vec!["GAME00002".into(), "GAME00005".into()]);
                p.avatar = Some(LUMEN.into());
                p.playtime = Some(vec![
                    played("GAME00001", 3_840),
                    played("GAME00002", 1_385),
                    played("GAME00005", 210),
                    played("GAME00003", 35),
                ]);
            });
            state.profiles.insert(
                AccountId(2),
                entry(ProfilePage {
                    profile: p,
                    trophies: Some(Count {
                        bronze: 30,
                        silver: 9,
                        gold: 4,
                        platinum: 1,
                    }),
                    links: Vec::new(),
                }),
            );
        }
        L::FriendHidden => {
            // Nothing of theirs in the feed either.
            state.feed.items.retain(|i| i.account.id != AccountId(3));
            let p = page(who(3, "Juno"), &|p| {
                p.about = Some("Mostly here for the party chat.".into());
                p.favourites = Some(vec!["GAME00005".into(), "GAME00006".into()]);
                p.avatar = Some(JUNO.into());
            });
            state.profiles.insert(
                AccountId(3),
                entry(ProfilePage {
                    profile: p,
                    trophies: None,
                    links: Vec::new(),
                }),
            );
        }
        L::Request => {
            let p = page(who(6, "Vale"), &|p| {
                p.about = Some("We met in the Tidebreaker lobby.".into());
            });
            state.profiles.insert(
                AccountId(6),
                entry(ProfilePage {
                    profile: p,
                    trophies: None,
                    links: Vec::new(),
                }),
            );
        }
        L::NewFriend => {
            state.profiles.insert(
                AccountId(4),
                entry(ProfilePage {
                    profile: profile(who(4, "Kit")),
                    trophies: None,
                    links: Vec::new(),
                }),
            );
        }
        L::Private => {
            state.profiles.insert(
                AccountId(12),
                entry(ProfilePage {
                    profile: profile(who(12, "Tarn")),
                    trophies: None,
                    links: Vec::new(),
                }),
            );
        }
        L::Loading => {}
        L::Error => {
            state.profiles.insert(
                AccountId(2),
                ProfileEntry {
                    page: None,
                    failed: true,
                },
            );
        }
        L::Offline => {
            state.cloud = Some(online::Cloud::Offline);
        }
    }
}

/// A picture of invented origin: a gradient, a head and shoulders in lighter tints.
pub fn avatar_image(base: [u8; 3]) -> ColorImage {
    const SIZE: usize = 96;
    let mut rgba = Vec::with_capacity(SIZE * SIZE * 4);
    let lift = |c: [u8; 3], t: f32| {
        c.map(|v| (f32::from(v) + (255.0 - f32::from(v)) * t).min(255.0) as u8)
    };
    let head = lift(base, 0.6);
    let shoulders = lift(base, 0.42);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (fx, fy) = (x as f32 / SIZE as f32, y as f32 / SIZE as f32);
            let shade = 1.0 - 0.45 * fy;
            let mut px = base.map(|v| (f32::from(v) * shade) as u8);
            if (fx - 0.5).powi(2) + (fy - 0.36).powi(2) < 0.17f32.powi(2) {
                px = head;
            } else if ((fx - 0.5) / 0.42).powi(2) + ((fy - 1.0) / 0.38).powi(2) < 1.0 {
                px = shoulders;
            }
            rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
        }
    }
    ColorImage::from_rgba_unmultiplied([SIZE, SIZE], &rgba)
}

/// The three invented avatars, uploaded.
pub fn avatar_textures(ctx: &Context) -> Vec<TextureHandle> {
    [[24, 170, 160], [236, 132, 36], [124, 84, 214]]
        .into_iter()
        .enumerate()
        .map(|(n, base)| {
            ctx.load_texture(
                format!("avatar-{n}"),
                avatar_image(base),
                TextureOptions::LINEAR,
            )
        })
        .collect()
}

/// What the page takes from "this PC": three of the gallery's games, a trophy name and the
/// avatars with the banner colour computed from their pixels, as the launcher does.
pub fn local(covers: &[TextureHandle], avatars: &[TextureHandle], names: &[&str]) -> Local {
    let mut local = Local::default();
    for (title, index) in OWNED {
        local.games.insert(
            title.into(),
            OwnedGame {
                name: names[index].into(),
                icon: covers.get(index).map(TextureHandle::id),
            },
        );
    }
    local
        .trophy_names
        .insert(("GAME00001".into(), 4), "First Light".into());
    for (hash, texture) in [MIRA, LUMEN, JUNO].into_iter().zip(avatars) {
        let image = avatar_image(match hash {
            MIRA => [24, 170, 160],
            LUMEN => [236, 132, 36],
            _ => [124, 84, 214],
        });
        let rgba: Vec<u8> = image
            .pixels
            .iter()
            .flat_map(|p: &Color32| p.to_array())
            .collect();
        local.avatars.insert(
            hash.into(),
            Avatar {
                texture: texture.id(),
                banner: online::banner_colour(&rgba),
            },
        );
    }
    local
}
