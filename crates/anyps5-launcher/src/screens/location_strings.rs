//! The words of the unlocked games location and of moving the unlocked games (docs/DESIGN.md 5.7).
//! English and Turkish like the rest.

use std::path::Path;

use anyps5_core::location::Refusal;

use crate::ui::i18n::{Key, Language, byte_text};

fn pick(language: Language, english: &'static str, turkish: &'static str) -> &'static str {
    match language {
        Language::English => english,
        Language::Turkish => turkish,
    }
}

/// The title of the setting row.
pub fn title(language: Language) -> &'static str {
    pick(language, "Unlocked Games Location", "Kilidi Açılan Oyunların Konumu")
}

/// The title of the folder dialog.
pub fn dialog(language: Language) -> &'static str {
    pick(
        language,
        "Choose a Folder for Unlocked Games",
        "Kilidi Açılan Oyunlar İçin Klasör Seç",
    )
}

/// "Use Default Location": back to the games folder.
pub fn use_default(language: Language) -> &'static str {
    pick(language, "Use Default Location", "Varsayılan Konumu Kullan")
}

/// Where the default is, in the detail of the row.
fn default_place(language: Language) -> &'static str {
    pick(language, "Next to your games", "Oyunlarının yanında")
}

/// What the row shows: the place and what is free there, or that it is not there.
pub fn detail(
    custom: Option<&Path>,
    free: Option<u64>,
    missing: bool,
    language: Language,
) -> String {
    let place = custom.map_or_else(
        || default_place(language).to_owned(),
        |folder| folder.display().to_string(),
    );
    if missing {
        return format!("{place} \u{b7} {}", pick(language, "Not found", "Bulunamadı"));
    }
    match free {
        Some(free) => format!(
            "{place} \u{b7} {} {}",
            byte_text(free.min(i64::MAX as u64) as i64, language),
            pick(language, "free", "boş")
        ),
        None => place,
    }
}

/// The note under the row when the copies sit next to the dumps.
pub fn note_default(language: Language) -> &'static str {
    language.text(Key::InstalledSize)
}

/// The note under the row for another folder.
pub fn note_custom(language: Language) -> &'static str {
    pick(
        language,
        "Unlocked games are copied in full into a .AnyPS5 folder here. Your dumps are never changed.",
        "Kilidi açılan oyunlar buradaki .AnyPS5 klasörüne tam kopya olarak yazılır. Dökümlerin hiç değişmez.",
    )
}

/// The note under the row for a network drive.
pub fn note_network(language: Language) -> &'static str {
    pick(
        language,
        "This is a network location. Copying is slower, and games run over the network.",
        "Bu bir ağ konumu. Kopyalama daha yavaştır ve oyunlar ağ üzerinden çalışır.",
    )
}

/// The note under the row when the folder or its drive is gone.
pub fn note_missing(language: Language) -> &'static str {
    pick(
        language,
        "This folder is not available, so its games show as locked. Connect the drive, or choose another folder.",
        "Bu klasör kullanılamıyor, bu yüzden oyunları kilitli görünür. Sürücüyü tak ya da başka bir klasör seç.",
    )
}

/// The row while the copies move: "Moving 3 of 7 · 42%".
pub fn moving(done: usize, total: usize, percent: u32, language: Language) -> String {
    match language {
        Language::English => format!("Moving {} of {total} \u{b7} {percent}%", (done + 1).min(total)),
        Language::Turkish => format!("{} / {total} taşınıyor \u{b7} %{percent}", (done + 1).min(total)),
    }
}

/// The note under the row while the copies move.
pub fn note_moving(language: Language) -> &'static str {
    pick(
        language,
        "Unlocking and playing wait until the move is done. Saves move with their game.",
        "Taşıma bitene kadar kilit açma ve oynama bekler. Kayıtlar oyunlarıyla birlikte taşınır.",
    )
}

pub fn cancel_move(language: Language) -> &'static str {
    pick(language, "Cancel Move", "Taşımayı İptal Et")
}

pub fn continue_move(language: Language) -> &'static str {
    pick(language, "Continue Moving", "Taşımaya Devam Et")
}

pub fn leave(language: Language) -> &'static str {
    pick(language, "Leave Games Where They Are", "Oyunları Olduğu Yerde Bırak")
}

pub fn leave_detail(language: Language) -> &'static str {
    pick(
        language,
        "They stay in the old location, unused. You unlock them again after that.",
        "Eski konumda kullanılmadan kalırlar. Sonra yeniden kilidini açarsın.",
    )
}

/// The note under the row when a move stopped before it was done.
pub fn note_left(count: usize, from: &Path, language: Language) -> String {
    let from = from.display();
    match (language, count) {
        (Language::English, 0) => format!(
            "Some unlocked games are still in {from}. Unlocking and playing wait until they are moved."
        ),
        (Language::English, 1) => format!(
            "1 unlocked game is still in {from}. Unlocking and playing wait until it is moved."
        ),
        (Language::English, n) => format!(
            "{n} unlocked games are still in {from}. Unlocking and playing wait until they are moved."
        ),
        (Language::Turkish, 0) => format!(
            "Kilidi açılan bazı oyunlar hâlâ {from} içinde. Taşınana kadar kilit açma ve oynama bekler."
        ),
        (Language::Turkish, n) => format!(
            "Kilidi açılan {n} oyun hâlâ {from} içinde. Taşınana kadar kilit açma ve oynama bekler."
        ),
    }
}

/// A toast: why the picked folder cannot be used.
pub fn refusal(why: &Refusal, language: Language) -> &'static str {
    match why {
        Refusal::NotAbsolute | Refusal::NotAFolder => pick(
            language,
            "That folder is not there. Choose another one.",
            "Bu klasör yok. Başka birini seç.",
        ),
        Refusal::InsideGamesFolder => pick(
            language,
            "Choose a folder outside your games folder. Your dumps are never written to.",
            "Oyun klasörünün dışında bir klasör seç. Dökümlere hiçbir şey yazılmaz.",
        ),
        Refusal::InsideStore => pick(
            language,
            "Choose a folder outside the .AnyPS5 folders.",
            ".AnyPS5 klasörlerinin dışında bir klasör seç.",
        ),
        Refusal::NotWritable(_) => pick(
            language,
            "Cannot write to that folder. Choose another one.",
            "Bu klasöre yazılamıyor. Başka birini seç.",
        ),
    }
}

/// A toast: the location cannot change right now.
pub fn busy_playing(language: Language) -> &'static str {
    pick(
        language,
        "Close the running game first.",
        "Önce çalışan oyunu kapat.",
    )
}

pub fn busy_unlocking(language: Language) -> &'static str {
    pick(
        language,
        "Wait until unlocking is done.",
        "Kilit açma bitene kadar bekle.",
    )
}

pub fn busy_moving(language: Language) -> &'static str {
    pick(
        language,
        "Finish or cancel the move first.",
        "Önce taşımayı bitir ya da iptal et.",
    )
}

/// A toast: the games moved.
pub fn moved(language: Language) -> &'static str {
    pick(language, "Unlocked games moved.", "Kilidi açılan oyunlar taşındı.")
}

/// A toast: the move stopped with games left in the old location.
pub fn stopped(left: usize, language: Language) -> String {
    match language {
        Language::English => format!(
            "{left} {} left in the old location. Continue the move in Settings.",
            if left == 1 { "game was" } else { "games were" }
        ),
        Language::Turkish => format!(
            "{left} oyun eski konumunda kaldı. Taşımaya Ayarlar'dan devam et."
        ),
    }
}

/// A toast: the new drive is full.
pub fn no_space(needed: u64, free: u64, language: Language) -> String {
    let (needed, free) = (
        byte_text(needed.min(i64::MAX as u64) as i64, language),
        byte_text(free.min(i64::MAX as u64) as i64, language),
    );
    match language {
        Language::English => format!(
            "The new drive is full: the next game needs {needed}, {free} free. Free some space, then continue."
        ),
        Language::Turkish => format!(
            "Yeni sürücü dolu: sıradaki oyun {needed} istiyor, {free} boş. Yer açıp devam et."
        ),
    }
}

/// The failure of a play or unlock command while games are still being moved.
pub fn waits_for_move(language: Language) -> &'static str {
    pick(
        language,
        "Your unlocked games are being moved. Try again when that is done.",
        "Kilidi açılan oyunların taşınıyor. Bitince yeniden dene.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_row_names_the_place_and_the_free_space() {
        let en = Language::English;
        assert_eq!(
            detail(None, Some(3_000_000_000), false, en),
            "Next to your games \u{b7} 3.00 GB free"
        );
        assert_eq!(
            detail(Some(Path::new("E:\\Unlocked")), None, true, en),
            "E:\\Unlocked \u{b7} Not found"
        );
        assert_eq!(detail(Some(Path::new("E:\\U")), None, false, en), "E:\\U");
    }

    #[test]
    fn counts_agree_with_their_nouns() {
        let from = Path::new("D:\\Old");
        assert!(note_left(1, from, Language::English).starts_with("1 unlocked game is "));
        assert!(note_left(0, from, Language::English).starts_with("Some unlocked games"));
        assert!(note_left(3, from, Language::English).starts_with("3 unlocked games are "));
        assert!(stopped(1, Language::English).starts_with("1 game was "));
        assert_eq!(moving(0, 7, 4, Language::English), "Moving 1 of 7 \u{b7} 4%");
        assert_eq!(moving(7, 7, 100, Language::English), "Moving 7 of 7 \u{b7} 100%");
    }
}
