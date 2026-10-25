//! What the player hears of share screen outside the panel: a toast when a member starts
//! sharing, when a stream they watch or share ends by itself, and when a command is refused
//! (docs/DESIGN.md 17, "Share screen").

use anyps5_core::online::{Failure, ScreenNotice};

use crate::app::model::Model;
use crate::app::online::failure_text;
use crate::app::overlay::Overlay;
use crate::screens::online::screen::name_of;
use crate::screens::online_screen_strings::ScreenText as X;
use crate::ui::i18n::Arg;
use crate::ui::icons::Icon;

/// A stream appeared or ended.
pub fn notice(model: &mut Model, notice: ScreenNotice) {
    let language = model.language();
    match notice {
        // The open panel already shows the new row.
        ScreenNotice::Offered { account, title } => {
            if model.overlay != Some(Overlay::Online) {
                let name = name_of(&model.online, account, language);
                let text = X::ToastOffered.format(language, &[Arg::Str(&name), Arg::Str(&title)]);
                model.toast(text, Icon::APP_WINDOW);
            }
        }
        ScreenNotice::WatchEnded { account, .. } => {
            let name = name_of(&model.online, account, language);
            let text = X::ToastWatchEnded.format(language, &[Arg::Str(&name)]);
            model.toast(text, Icon::APP_WINDOW);
        }
        ScreenNotice::ShareEnded => {
            model.toast(X::ToastShareEnded.text(language), Icon::APP_WINDOW);
        }
    }
}

/// A start, stop, watch or leave was refused: a short plain sentence, else the general text.
pub fn failed(model: &mut Model, failure: &Failure) {
    model.online.screen.forget_expectations();
    let language = model.language();
    let text = match failure.code.as_str() {
        Failure::NO_GAME => X::ErrorNoGame.text(language).to_owned(),
        "not_in_party" => X::ErrorNotInParty.text(language).to_owned(),
        "busy" => X::ErrorBusy.text(language).to_owned(),
        "no_encoder" => X::ErrorNoEncoder.text(language).to_owned(),
        "not_found" => X::ErrorNotFound.text(language).to_owned(),
        _ => failure_text(failure, language),
    };
    model.toast(text, Icon::WARNING);
}
