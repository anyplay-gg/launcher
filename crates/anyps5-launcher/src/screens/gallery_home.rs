//! The real home screen in the gallery (`--gallery=home...`): the top bar, the tile row and the
//! hero painted by `screens::home` over invented games, so a change to the controls can be seen
//! (and compared before and after) on the surface the player sees. `--hover=x,y` puts the pointer
//! there, so a hover state can be shot too.
//!
//! ```text
//! home  home-profile  home-library  home-friends  home-settings
//! home-play  home-trophies  home-more  home-signedout
//! ```

use std::path::PathBuf;

use anyps5_core::paths::AppPaths;
use anyps5_core::settings::Preferences;
use egui::{Context, Ui};

use crate::app::model::{Model, SizeState};
use crate::app::textures::Textures;
use crate::screens::ScreenCx;
use crate::screens::gallery::{NAMES, OnlineLook, cover_image, fake_game, fake_trophies, sample_online};
use crate::ui::Shell;
use crate::ui::focus::HomeFocus;

/// Which control of the home screen has the focus, and whether anyone is signed in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HomeLook {
    pub focus: HomeFocus,
    pub signed_in: bool,
}

/// The home look named on the command line.
pub fn parse(name: &str) -> Option<HomeLook> {
    let focus = |focus| HomeLook {
        focus,
        signed_in: true,
    };
    Some(match name {
        "home" => focus(HomeFocus::Tiles),
        "home-profile" => focus(HomeFocus::PROFILE),
        "home-library" => focus(HomeFocus::LIBRARY),
        "home-friends" => focus(HomeFocus::FRIENDS),
        "home-settings" => focus(HomeFocus::SETTINGS),
        "home-play" => focus(HomeFocus::Actions(0)),
        "home-trophies" => focus(HomeFocus::Actions(1)),
        "home-more" => focus(HomeFocus::Actions(2)),
        "home-signedout" => HomeLook {
            focus: HomeFocus::PROFILE,
            signed_in: false,
        },
        _ => return None,
    })
}

pub struct HomeGallery {
    model: Model,
    textures: Option<Textures>,
}

impl HomeGallery {
    pub fn new(look: HomeLook) -> Self {
        let prefs = Preferences {
            onboarded: true,
            ..Preferences::first_run()
        };
        let mut model = Model::new(
            prefs,
            AppPaths::under(&std::env::temp_dir().join("anyplay-gallery-home")),
            false,
        );
        model.toolkit = Some("0.1.3-abc".into());
        model.library_loaded = true;
        let covers = covers_dir();
        model.games = (0..NAMES.len())
            .map(|i| {
                let mut game = fake_game(i);
                game.icon = Some(covers.join(format!("cover-{i}.png")));
                game
            })
            .collect();
        model.selected = 1;
        model.focus = look.focus;
        let id = model.games[1].id.clone();
        model.sizes.insert(id.clone(), SizeState::Known(7_550_000_000));
        model.trophies.insert(id, fake_trophies());
        if look.signed_in {
            model.online = sample_online(OnlineLook::Friends);
        }
        Self {
            model,
            textures: None,
        }
    }

    pub fn ui(&mut self, shell: &Shell, ui: &mut Ui) {
        let ctx: Context = ui.ctx().clone();
        let textures = self.textures.get_or_insert_with(|| Textures::new(&ctx));
        textures.poll();
        let mut cx = ScreenCx {
            model: &mut self.model,
            shell,
            textures,
            ctx: &ctx,
        };
        crate::screens::home::show(ui, &mut cx);
    }
}

/// Writes the invented covers once and returns their folder.
fn covers_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("anyplay-gallery-covers");
    let _ = std::fs::create_dir_all(&dir);
    for i in 0..NAMES.len() {
        let path = dir.join(format!("cover-{i}.png"));
        if path.exists() {
            continue;
        }
        let image = cover_image(i, 256, 256);
        let bytes: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
        let _ = image::save_buffer(&path, &bytes, 256, 256, image::ColorType::Rgba8);
    }
    dir
}
