//! Texts the Windows port adds to the 212 keys of the original (`ui::i18n`): the toolkit rows of the
//! settings, the CPU conversion choice and the host-CPU diagnosis. English and Turkish like the rest.

use crate::ui::i18n::{Arg, Language, substitute};

/// A Windows-only text. The generated table of the original stays untouched.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Extra {
    /// Settings category.
    Toolkit,
    UpdateCheck,
    UpdateCheckDetail,
    CheckNow,
    /// Value while the check runs.
    Checking,
    UpToDate,
    /// `%@` = the new version.
    UpdateAvailable,
    CheckFailed,
    LastChecked,
    NeverChecked,
    ToolkitMissing,
    ToolkitInstall,
    ToolkitInstallUpdate,
    ToolkitChooseFolder,
    CpuConversion,
    CpuConversionDetail,
    CpuAuto,
    CpuNone,
    CpuIntel,
    /// `%@` = the missing CPU features.
    DiagnosisHostCpu,
    /// Linux: the user cannot open `/dev/udmabuf`.
    DiagnosisNoUdmabuf,
    SaveReport,
    /// Toast while the toolkit downloads.
    ToolkitDownloading,
    /// Toast: `%@` = the installed version.
    ToolkitReady,
    ToolkitFailed,
    /// Toast: the picked folder has no relinker or libraries.
    ToolkitFolderInvalid,
    /// Settings row: the home screen music toggle.
    HomeMusic,
    HomeMusicDetail,
    /// Settings row: the running game on Discord.
    DiscordPresence,
    DiscordPresenceDetail,
    /// Settings row: hidden trophies in the trophy list, and the action on one hidden trophy.
    ShowHiddenTrophies,
    ShowHiddenTrophiesDetail,
    TrophyShow,
    /// The log box before the game wrote anything.
    EmptyLog,
    /// The button of the removal confirmation (its title already says what is removed).
    Remove,
    /// Controls row: the keyboard layout and its choices.
    Layout,
    LayoutLauncher,
    LayoutMouse,
    /// The PC keys the game's own files name.
    LayoutGame,
    /// The layout after the user moved keys.
    LayoutCustom,
    /// `%@` = how many buttons the game's files give a PC key.
    LayoutGameDetail,
    /// About row value and toast: `%@` = the new launcher version.
    LauncherReady,
    /// Toast when a launcher update is downloaded and checked.
    LauncherToast,
    RestartNow,
    /// About row value: `%@` = the new version, but the folder cannot be written to.
    LauncherReadOnly,
    /// Toast when "Restart Now" or the swap on exit fails.
    LauncherUpdateFailed,
    /// Settings row: the launcher answers anyps5:// links and folders.
    Links,
    LinksDetail,
    LinksDetailOther,
    LinksFailed,
    /// Toast: a link or folder the launcher cannot use.
    LinkInvalid,
    /// Toast: `%@` = the title ID.
    LinkNoGame,
    /// Toast: the folder of a link or of the folder menu holds no game.
    LinkNotADump,
    /// Toast: `%@` = the game name.
    LinkOutside,
    /// Toast: `%@` = the game name.
    LinkUnlock,
    /// Toast: `%@` = the game name.
    LinkUnlocked,
    /// Settings row: which toolkit to follow.
    ChannelTitle,
    ChannelStable,
    ChannelPinned,
    ChannelLocal,
    ChannelStableDetail,
    ChannelPinnedDetail,
    ChannelLocalDetail,
    /// Settings row: the release a pinned channel stays on.
    PinnedVersion,
    /// Settings row and panel title.
    WhatsNew,
    /// Detail of Check Now: `%@` = the time.
    LastCheckedOn,
    /// Detail of the toolkit row.
    FitNewer,
    /// Detail of the toolkit row.
    FitOlder,
    /// Toast: `%@` = the release.
    ToolkitAvailable,
    /// Value of a turned-off feature: `%@` = the first release that has it.
    NeedsToolkit,
    /// Value of a turned-off feature: `%@` = the last release without it.
    NeedsNewerThan,
    /// Header of the notes: release, date.
    NotesHeader,
    /// `%@` = how many.
    ChangesCount,
    ChangeOne,
    NotesHighlights,
    NotesOther,
    NotesEverything,
    NotesLoading,
    NotesUnavailable,
    /// Button: the release page on GitHub.
    NotesOpenRelease,
    /// Game sheet: `%@` = the version it was unlocked with, then the installed one.
    UnlockedWith,
    ConversionChanged,
    UnlockOptional,
    UnlockEarlierBuild,
    /// Game sheet row and panel subtitle.
    RecTitle,
    /// Detail of the row: the game does not start without it.
    RecNeeded,
    /// Detail of the row when there is only a warning.
    RecWorthALook,
    /// The one button of the panel.
    RecApply,
    /// Value before a setting is changed.
    RecDefault,
    RecOn,
    RecOff,
    /// Setting name.
    RecPlayGo,
    RecPlayGoInstalled,
    RecPlayGoAbsent,
    /// Setting name.
    RecImageBase,
    /// Setting name.
    RecModuleFolder,
    /// Setting name.
    RecExecutable,
    /// Setting name.
    RecTextureCache,
    /// Setting name.
    RecShaderCache,
    /// Hint under the changes.
    RecUnlockHint,
    /// Toast.
    RecApplied,
    /// Toast.
    RecAppliedUnlock,
    /// Facts line of the home screen: the label over the time played.
    Played,
    /// Time played, under a minute.
    PlayedUnderMinute,
    /// `%@` = minutes.
    PlayedMinutes,
    /// `%@` = hours.
    PlayedHours,
    /// `%@` = hours, then minutes.
    PlayedHoursMinutes,
    /// Game sheet row and panel subtitle.
    SavesTitle,
    /// Detail of the row: no backup yet.
    SavesNone,
    /// Detail of the row.
    SavesOne,
    /// Detail of the row: `%@` = how many.
    SavesCount,
    /// Command.
    SavesBackUp,
    /// Detail of the command and its toast when there is nothing to back up.
    SavesNothing,
    /// Note of the panel without backups.
    SavesEmpty,
    /// Section caption over the list.
    SavesCaption,
    /// Note under the list: `%@` = how many are kept.
    SavesKept,
    /// Why a backup was made.
    SavesReasonManual,
    /// Why a backup was made.
    SavesReasonUnlock,
    /// Why a backup was made.
    SavesReasonRestore,
    /// Why a backup was made.
    SavesReasonUninstall,
    /// Why a backup was made.
    SavesReasonPlayed,
    /// Toast.
    SavesBackedUp,
    /// Command and title of the question.
    SavesRestore,
    /// The question: `%@` = date of the backup.
    SavesRestoreAsk,
    /// Toast.
    SavesRestored,
    /// Note while the game runs, and the toast of a refusal.
    SavesRunning,
    /// Toast.
    SavesFailed,
    /// Toast.
    SavesRestoreFailed,
    /// Toast.
    SavesCorrupt,
    /// Added to the removal question.
    UninstallBacksUp,
    /// Toast: the removal did not start.
    UninstallNeedsBackup,
    /// Toast: the restore stopped part way.
    SavesRestoreIncomplete,
    /// Toast: the restore stopped part way, nothing had to be kept.
    SavesRestoreIncompleteNone,
    /// Toast: a backup or restore is still under way.
    SavesBusy,
}

impl Extra {
    pub const fn english(self) -> &'static str {
        match self {
            Extra::Toolkit => "Toolkit",
            Extra::UpdateCheck => "Check for Updates",
            Extra::UpdateCheckDetail => {
                "Looks for a newer toolkit when the launcher starts. Only the version number is requested."
            }
            Extra::CheckNow => "Check Now",
            Extra::Checking => "Checking…",
            Extra::UpToDate => "Up to date",
            Extra::UpdateAvailable => "Update available: %@",
            Extra::CheckFailed => "Could not check",
            Extra::LastChecked => "Last Checked",
            Extra::NeverChecked => "Never",
            Extra::ToolkitMissing => "Not installed",
            Extra::ToolkitInstall => "Download the Toolkit",
            Extra::ToolkitInstallUpdate => "Install the Update",
            Extra::ToolkitChooseFolder => "Use a Toolkit Folder",
            Extra::CpuConversion => "CPU Conversion",
            Extra::CpuConversionDetail => {
                "How games are prepared for your processor. Automatic suits most PCs."
            }
            Extra::CpuAuto => "Automatic",
            Extra::CpuNone => "None",
            Extra::CpuIntel => "Intel",
            Extra::DiagnosisHostCpu => "This CPU lacks %@, which the game needs.",
            Extra::DiagnosisNoUdmabuf => {
                "The game cannot open /dev/udmabuf. Add your user to the kvm group and log in again."
            }
            Extra::SaveReport => "Save Report",
            Extra::ToolkitDownloading => "Downloading the toolkit…",
            Extra::ToolkitReady => "Toolkit %@ is ready",
            Extra::ToolkitFailed => {
                "Could not install the toolkit. Check your connection and try again."
            }
            Extra::HomeMusic => "Home Screen Music",
            Extra::HomeMusicDetail => {
                "Plays quiet ambient music on the home screen. It fades out while a game runs."
            }
            Extra::DiscordPresence => "Show Game on Discord",
            Extra::DiscordPresenceDetail => {
                "With Discord open, your profile shows the game and for how long."
            }
            Extra::ShowHiddenTrophies => "Always Show Hidden Trophies",
            Extra::ShowHiddenTrophiesDetail => {
                "Hidden trophies show their name and description before you earn them. Only you see this."
            }
            Extra::TrophyShow => "Show",
            Extra::EmptyLog => "No log yet. Play the game once and it appears here.",
            Extra::Remove => "Remove",
            Extra::ToolkitFolderInvalid => {
                "This folder is not a toolkit. Pick the folder that holds the relinker and libs."
            }
            Extra::Layout => "Layout",
            Extra::LayoutLauncher => "Launcher",
            Extra::LayoutMouse => "Mouse and Keyboard",
            Extra::LayoutGame => "From the Game",
            Extra::LayoutCustom => "Custom",
            Extra::LayoutGameDetail => "Found the game's own PC keys for %@ buttons.",
            Extra::LauncherReady => "%@ ready",
            Extra::LauncherToast => {
                "Launcher %@ is ready. It installs when you close the launcher."
            }
            Extra::RestartNow => "Restart Now",
            Extra::LauncherReadOnly => "%@ available, but this folder is read-only",
            Extra::LauncherUpdateFailed => {
                "Could not install the launcher update. Download it from GitHub instead."
            }
            Extra::Links => "Open AnyPlay Links with This Launcher",
            Extra::LinksDetail => {
                "Links on websites and in messages open here, and folders get an \"Open with AnyPlay Launcher\" entry."
            }
            Extra::LinksDetailOther => {
                "Another copy of the launcher opens them now. Turn this on to use this one."
            }
            Extra::LinksFailed => "Could not change the link settings. Try again.",
            Extra::LinkInvalid => "This is not a valid AnyPlay link.",
            Extra::LinkNoGame => "%@ is not in your library. Check the games folder in Settings.",
            Extra::LinkNotADump => {
                "This folder is not a game dump. Open the folder that holds eboot.bin."
            }
            Extra::LinkOutside => "%@ is outside your games folder. Move it there to add it.",
            Extra::LinkUnlock => "Select Unlock to prepare %@.",
            Extra::LinkUnlocked => "%@ is already unlocked.",
            Extra::ChannelTitle => "Channel",
            Extra::ChannelStable => "Stable",
            Extra::ChannelPinned => "Pinned",
            Extra::ChannelLocal => "Local Build",
            Extra::ChannelStableDetail => "Follows the newest release.",
            Extra::ChannelPinnedDetail => "Stays on one release. No update is offered.",
            Extra::ChannelLocalDetail => "Uses a folder you built or picked. No update is offered.",
            Extra::PinnedVersion => "Version",
            Extra::WhatsNew => "What's New",
            Extra::LastCheckedOn => "Last checked %@",
            Extra::FitNewer => "Newer than this launcher was tested with. It should still work.",
            Extra::FitOlder => "An older toolkit: some features are turned off.",
            Extra::ToolkitAvailable => "Toolkit %@ is available",
            Extra::NeedsToolkit => "Needs toolkit %@",
            Extra::NeedsNewerThan => "Needs a build newer than %@",
            Extra::NotesHeader => "%@, released %@",
            Extra::ChangesCount => "%@ changes",
            Extra::ChangeOne => "1 change",
            Extra::NotesHighlights => "Highlights",
            Extra::NotesOther => "Other areas",
            Extra::NotesEverything => "Everything",
            Extra::NotesLoading => "Loading the notes…",
            Extra::NotesUnavailable => "The notes could not be loaded.",
            Extra::NotesOpenRelease => "Open on GitHub",
            Extra::UnlockedWith => "Unlocked with %@. %@ is installed.",
            Extra::ConversionChanged => "This update changes how games are converted.",
            Extra::UnlockOptional => "Unlocking again is optional.",
            Extra::UnlockEarlierBuild => "Unlocked with an earlier build of this toolkit.",
            Extra::RecTitle => "Recommended for This Game",
            Extra::RecNeeded => "Needed to start",
            Extra::RecWorthALook => "Worth a look",
            Extra::RecApply => "Apply",
            Extra::RecDefault => "Default",
            Extra::RecOn => "On",
            Extra::RecOff => "Off",
            Extra::RecPlayGo => "Missing PlayGo Data",
            Extra::RecPlayGoInstalled => "Report as installed",
            Extra::RecPlayGoAbsent => "Report as absent",
            Extra::RecImageBase => "Image Base",
            Extra::RecModuleFolder => "Module Folder",
            Extra::RecExecutable => "Executable",
            Extra::RecTextureCache => "Texture Cache",
            Extra::RecShaderCache => "Shader Cache",
            Extra::RecUnlockHint => "After you apply this, unlock the game again.",
            Extra::RecApplied => "Applied",
            Extra::RecAppliedUnlock => "Saved. Unlock the game again to use it.",
            Extra::Played => "Played",
            Extra::PlayedUnderMinute => "Under 1 min",
            Extra::PlayedMinutes => "%@ min",
            Extra::PlayedHours => "%@ h",
            Extra::PlayedHoursMinutes => "%@ h %@ min",
            Extra::SavesTitle => "Save Data",
            Extra::SavesNone => "No backups yet",
            Extra::SavesOne => "1 backup",
            Extra::SavesCount => "%@ backups",
            Extra::SavesBackUp => "Back Up Now",
            Extra::SavesNothing => "No save data to back up yet",
            Extra::SavesEmpty => {
                "No backups yet. Back Up Now keeps a copy of your saves and trophies."
            }
            Extra::SavesCaption => "Backups",
            Extra::SavesKept => "Up to %@ backups are kept. Backups made after playing go first.",
            Extra::SavesReasonManual => "Manual",
            Extra::SavesReasonUnlock => "Before unlocking again",
            Extra::SavesReasonRestore => "Before a restore",
            Extra::SavesReasonUninstall => "Before uninstall",
            Extra::SavesReasonPlayed => "After playing",
            Extra::SavesBackedUp => "Saves backed up",
            Extra::SavesRestore => "Restore",
            Extra::SavesRestoreAsk => {
                "This replaces the saves and trophies the game has now with the backup from %@. The current ones are backed up first, so you can undo it."
            }
            Extra::SavesRestored => "Saves restored",
            Extra::SavesRunning => "Close the game to restore a backup.",
            Extra::SavesFailed => {
                "Could not back up the saves. Check the free disk space and try again."
            }
            Extra::SavesRestoreFailed => {
                "Could not restore the backup. Your saves were not changed."
            }
            Extra::SavesCorrupt => {
                "This backup is damaged. Your saves were not changed. Pick another backup."
            }
            Extra::UninstallBacksUp => "Your saves and trophies are backed up first.",
            Extra::UninstallNeedsBackup => {
                "Could not back up the saves, so the game was not removed. Check the free disk space and try again."
            }
            Extra::SavesRestoreIncomplete => {
                "The restore did not finish. The saves from before are in the newest backup, Before a restore."
            }
            Extra::SavesRestoreIncompleteNone => "The restore did not finish. Try it again.",
            Extra::SavesBusy => "Still working on the saves. Try again in a moment.",
        }
    }

    pub const fn turkish(self) -> &'static str {
        match self {
            Extra::Toolkit => "Araç seti",
            Extra::UpdateCheck => "Güncellemeleri denetle",
            Extra::UpdateCheckDetail => {
                "Başlatıcı açılırken daha yeni bir araç seti arar. Yalnızca sürüm numarası istenir."
            }
            Extra::CheckNow => "Şimdi denetle",
            Extra::Checking => "Denetleniyor…",
            Extra::UpToDate => "Güncel",
            Extra::UpdateAvailable => "Güncelleme var: %@",
            Extra::CheckFailed => "Denetlenemedi",
            Extra::LastChecked => "Son denetleme",
            Extra::NeverChecked => "Hiç",
            Extra::ToolkitMissing => "Kurulu değil",
            Extra::ToolkitInstall => "Araç setini indir",
            Extra::ToolkitInstallUpdate => "Güncellemeyi kur",
            Extra::ToolkitChooseFolder => "Araç seti klasörü kullan",
            Extra::CpuConversion => "CPU dönüştürme",
            Extra::CpuConversionDetail => {
                "Oyunların işlemcine göre nasıl hazırlanacağı. Otomatik çoğu bilgisayara uyar."
            }
            Extra::CpuAuto => "Otomatik",
            Extra::CpuNone => "Yok",
            Extra::CpuIntel => "Intel",
            Extra::DiagnosisHostCpu => "Bu işlemcide oyunun ihtiyaç duyduğu %@ yok.",
            Extra::DiagnosisNoUdmabuf => {
                "Oyun /dev/udmabuf dosyasını açamıyor. Kullanıcını kvm grubuna ekleyip yeniden oturum aç."
            }
            Extra::SaveReport => "Raporu kaydet",
            Extra::ToolkitDownloading => "Araç seti indiriliyor…",
            Extra::ToolkitReady => "Araç seti %@ hazır",
            Extra::ToolkitFailed => "Araç seti kurulamadı. Bağlantını denetleyip yeniden dene.",
            Extra::HomeMusic => "Ana Ekran Müziği",
            Extra::HomeMusicDetail => {
                "Ana ekranda sakin bir ortam müziği çalar. Oyun çalışırken susar."
            }
            Extra::DiscordPresence => "Oyunu Discord'da Göster",
            Extra::DiscordPresenceDetail => {
                "Discord açıksa profilinde oyunun adı ve süresi görünür."
            }
            Extra::ShowHiddenTrophies => "Gizli Kupaları Her Zaman Göster",
            Extra::ShowHiddenTrophiesDetail => {
                "Gizli kupaların adı ve açıklaması kazanmadan önce de görünür. Bunu yalnızca sen görürsün."
            }
            Extra::TrophyShow => "Göster",
            Extra::EmptyLog => "Henüz günlük yok. Oyunu bir kez oynadığında burada görünür.",
            Extra::Remove => "Kaldır",
            Extra::ToolkitFolderInvalid => {
                "Bu klasör bir araç seti değil. relinker ve libs içeren klasörü seç."
            }
            Extra::Layout => "Düzen",
            Extra::LayoutLauncher => "Başlatıcı",
            Extra::LayoutMouse => "Fare ve klavye",
            Extra::LayoutGame => "Oyundan",
            Extra::LayoutCustom => "Özel",
            Extra::LayoutGameDetail => "Oyunun kendi PC tuşları %@ düğme için bulundu.",
            Extra::LauncherReady => "%@ hazır",
            Extra::LauncherToast => "Başlatıcı %@ hazır. Başlatıcıyı kapattığında kurulur.",
            Extra::RestartNow => "Şimdi yeniden başlat",
            Extra::LauncherReadOnly => "%@ var, ama bu klasör salt okunur",
            Extra::LauncherUpdateFailed => {
                "Başlatıcı güncellemesi kurulamadı. Bunun yerine GitHub'dan indir."
            }
            Extra::Links => "AnyPlay bağlantılarını bu başlatıcıyla aç",
            Extra::LinksDetail => {
                "Web sitelerindeki ve mesajlardaki bağlantılar burada açılır, klasörlere \"AnyPlay Launcher ile aç\" girişi eklenir."
            }
            Extra::LinksDetailOther => {
                "Şu an bunları başka bir başlatıcı kopyası açıyor. Bunu kullanmak için aç."
            }
            Extra::LinksFailed => "Bağlantı ayarları değiştirilemedi. Yeniden dene.",
            Extra::LinkInvalid => "Bu geçerli bir AnyPlay bağlantısı değil.",
            Extra::LinkNoGame => "%@ kitaplığında yok. Ayarlar'daki oyun klasörünü denetle.",
            Extra::LinkNotADump => "Bu klasör bir oyun dökümü değil. eboot.bin içeren klasörü aç.",
            Extra::LinkOutside => "%@ oyun klasörünün dışında. Eklemek için oraya taşı.",
            Extra::LinkUnlock => "%@ oyununu hazırlamak için Kilidi Aç'ı seç.",
            Extra::LinkUnlocked => "%@ zaten açık.",
            Extra::ChannelTitle => "Kanal",
            Extra::ChannelStable => "Kararlı",
            Extra::ChannelPinned => "Sabit",
            Extra::ChannelLocal => "Yerel yapı",
            Extra::ChannelStableDetail => "En yeni sürümü izler.",
            Extra::ChannelPinnedDetail => "Tek bir sürümde kalır. Güncelleme önerilmez.",
            Extra::ChannelLocalDetail => {
                "Kendi yaptığın ya da seçtiğin klasörü kullanır. Güncelleme önerilmez."
            }
            Extra::PinnedVersion => "Sürüm",
            Extra::WhatsNew => "Yenilikler",
            Extra::LastCheckedOn => "Son denetleme: %@",
            Extra::FitNewer => {
                "Bu başlatıcının denendiğinden daha yeni. Yine de çalışması beklenir."
            }
            Extra::FitOlder => "Eski bir araç seti: bazı özellikler kapalı.",
            Extra::ToolkitAvailable => "Araç seti %@ indirilebilir",
            Extra::NeedsToolkit => "Araç seti %@ gerekir",
            Extra::NeedsNewerThan => "%@ sürümünden yeni bir yapı gerekir",
            Extra::NotesHeader => "%@, yayın tarihi %@",
            Extra::ChangesCount => "%@ değişiklik",
            Extra::ChangeOne => "1 değişiklik",
            Extra::NotesHighlights => "Öne çıkanlar",
            Extra::NotesOther => "Diğer alanlar",
            Extra::NotesEverything => "Tümü",
            Extra::NotesLoading => "Notlar yükleniyor…",
            Extra::NotesUnavailable => "Notlar yüklenemedi.",
            Extra::NotesOpenRelease => "GitHub'da aç",
            Extra::UnlockedWith => "%@ ile açıldı. %@ kurulu.",
            Extra::ConversionChanged => {
                "Bu güncelleme oyunların dönüştürülme şeklini değiştiriyor."
            }
            Extra::UnlockOptional => "Yeniden açmak isteğe bağlı.",
            Extra::UnlockEarlierBuild => "Bu araç setinin önceki bir yapısıyla açıldı.",
            Extra::RecTitle => "Bu oyun için önerilenler",
            Extra::RecNeeded => "Başlamak için gerekli",
            Extra::RecWorthALook => "Göz atmaya değer",
            Extra::RecApply => "Uygula",
            Extra::RecDefault => "Varsayılan",
            Extra::RecOn => "Açık",
            Extra::RecOff => "Kapalı",
            Extra::RecPlayGo => "Eksik PlayGo verisi",
            Extra::RecPlayGoInstalled => "Kurulu say",
            Extra::RecPlayGoAbsent => "Yok say",
            Extra::RecImageBase => "Yükleme adresi",
            Extra::RecModuleFolder => "Modül klasörü",
            Extra::RecExecutable => "Çalıştırılabilir dosya",
            Extra::RecTextureCache => "Doku önbelleği",
            Extra::RecShaderCache => "Gölgelendirici önbelleği",
            Extra::RecUnlockHint => "Bunun geçerli olması için oyunun kilidini yeniden aç.",
            Extra::RecApplied => "Uygulandı",
            Extra::RecAppliedUnlock => "Kaydedildi. Kullanmak için oyunun kilidini yeniden aç.",
            Extra::Played => "Oynama süresi",
            Extra::PlayedUnderMinute => "1 dk altı",
            Extra::PlayedMinutes => "%@ dk",
            Extra::PlayedHours => "%@ sa",
            Extra::PlayedHoursMinutes => "%@ sa %@ dk",
            Extra::SavesTitle => "Kayıt Verileri",
            Extra::SavesNone => "Henüz yedek yok",
            Extra::SavesOne => "1 yedek",
            Extra::SavesCount => "%@ yedek",
            Extra::SavesBackUp => "Şimdi Yedekle",
            Extra::SavesNothing => "Yedeklenecek kayıt verisi henüz yok",
            Extra::SavesEmpty => {
                "Henüz yedek yok. Şimdi Yedekle, kayıtlarının ve kupalarının bir kopyasını saklar."
            }
            Extra::SavesCaption => "Yedekler",
            Extra::SavesKept => {
                "En fazla %@ yedek saklanır. Oynadıktan sonra alınanlar önce silinir."
            }
            Extra::SavesReasonManual => "Elle",
            Extra::SavesReasonUnlock => "Kilidi yeniden açmadan önce",
            Extra::SavesReasonRestore => "Geri yüklemeden önce",
            Extra::SavesReasonUninstall => "Kaldırmadan önce",
            Extra::SavesReasonPlayed => "Oynadıktan sonra",
            Extra::SavesBackedUp => "Kayıtlar yedeklendi",
            Extra::SavesRestore => "Geri Yükle",
            Extra::SavesRestoreAsk => {
                "Oyunun şu anki kayıtları ve kupaları, %@ tarihli yedekle değiştirilir. Şu anki hâli önce yedeklenir, böylece geri alabilirsin."
            }
            Extra::SavesRestored => "Kayıtlar geri yüklendi",
            Extra::SavesRunning => "Yedeği geri yüklemek için oyunu kapat.",
            Extra::SavesFailed => {
                "Kayıtlar yedeklenemedi. Boş disk alanını kontrol edip yeniden dene."
            }
            Extra::SavesRestoreFailed => "Yedek geri yüklenemedi. Kayıtların değişmedi.",
            Extra::SavesCorrupt => "Bu yedek hasarlı. Kayıtların değişmedi. Başka bir yedek seç.",
            Extra::UninstallBacksUp => "Kayıtların ve kupaların önce yedeklenir.",
            Extra::UninstallNeedsBackup => {
                "Kayıtlar yedeklenemedi, bu yüzden oyun kaldırılmadı. Boş disk alanını kontrol edip yeniden dene."
            }
            Extra::SavesRestoreIncomplete => {
                "Geri yükleme tamamlanmadı. Önceki kayıtlar en yeni yedekte, \"Geri yüklemeden önce\"."
            }
            Extra::SavesRestoreIncompleteNone => "Geri yükleme tamamlanmadı. Yeniden dene.",
            Extra::SavesBusy => "Kayıtlar üzerinde hâlâ çalışılıyor. Birazdan yeniden dene.",
        }
    }

    pub const fn text(self, language: Language) -> &'static str {
        match language {
            Language::Turkish => self.turkish(),
            Language::English => self.english(),
        }
    }

    pub fn format(self, language: Language, args: &[Arg<'_>]) -> String {
        substitute(self.text(language), args)
    }

    pub const ALL: [Extra; 134] = [
        Extra::Toolkit,
        Extra::UpdateCheck,
        Extra::UpdateCheckDetail,
        Extra::CheckNow,
        Extra::Checking,
        Extra::UpToDate,
        Extra::UpdateAvailable,
        Extra::CheckFailed,
        Extra::LastChecked,
        Extra::NeverChecked,
        Extra::ToolkitMissing,
        Extra::ToolkitInstall,
        Extra::ToolkitInstallUpdate,
        Extra::ToolkitChooseFolder,
        Extra::CpuConversion,
        Extra::CpuConversionDetail,
        Extra::CpuAuto,
        Extra::CpuNone,
        Extra::CpuIntel,
        Extra::DiagnosisHostCpu,
        Extra::DiagnosisNoUdmabuf,
        Extra::SaveReport,
        Extra::ToolkitDownloading,
        Extra::ToolkitReady,
        Extra::ToolkitFailed,
        Extra::ToolkitFolderInvalid,
        Extra::HomeMusic,
        Extra::HomeMusicDetail,
        Extra::DiscordPresence,
        Extra::DiscordPresenceDetail,
        Extra::ShowHiddenTrophies,
        Extra::ShowHiddenTrophiesDetail,
        Extra::TrophyShow,
        Extra::EmptyLog,
        Extra::Remove,
        Extra::Layout,
        Extra::LayoutLauncher,
        Extra::LayoutMouse,
        Extra::LayoutGame,
        Extra::LayoutCustom,
        Extra::LayoutGameDetail,
        Extra::LauncherReady,
        Extra::LauncherToast,
        Extra::RestartNow,
        Extra::LauncherReadOnly,
        Extra::LauncherUpdateFailed,
        Extra::Links,
        Extra::LinksDetail,
        Extra::LinksDetailOther,
        Extra::LinksFailed,
        Extra::LinkInvalid,
        Extra::LinkNoGame,
        Extra::LinkNotADump,
        Extra::LinkOutside,
        Extra::LinkUnlock,
        Extra::LinkUnlocked,
        Extra::ChannelTitle,
        Extra::ChannelStable,
        Extra::ChannelPinned,
        Extra::ChannelLocal,
        Extra::ChannelStableDetail,
        Extra::ChannelPinnedDetail,
        Extra::ChannelLocalDetail,
        Extra::PinnedVersion,
        Extra::WhatsNew,
        Extra::LastCheckedOn,
        Extra::FitNewer,
        Extra::FitOlder,
        Extra::ToolkitAvailable,
        Extra::NeedsToolkit,
        Extra::NeedsNewerThan,
        Extra::NotesHeader,
        Extra::ChangesCount,
        Extra::ChangeOne,
        Extra::NotesHighlights,
        Extra::NotesOther,
        Extra::NotesEverything,
        Extra::NotesLoading,
        Extra::NotesUnavailable,
        Extra::NotesOpenRelease,
        Extra::UnlockedWith,
        Extra::ConversionChanged,
        Extra::UnlockOptional,
        Extra::UnlockEarlierBuild,
        Extra::RecTitle,
        Extra::RecNeeded,
        Extra::RecWorthALook,
        Extra::RecApply,
        Extra::RecDefault,
        Extra::RecOn,
        Extra::RecOff,
        Extra::RecPlayGo,
        Extra::RecPlayGoInstalled,
        Extra::RecPlayGoAbsent,
        Extra::RecImageBase,
        Extra::RecModuleFolder,
        Extra::RecExecutable,
        Extra::RecTextureCache,
        Extra::RecShaderCache,
        Extra::RecUnlockHint,
        Extra::RecApplied,
        Extra::RecAppliedUnlock,
        Extra::Played,
        Extra::PlayedUnderMinute,
        Extra::PlayedMinutes,
        Extra::PlayedHours,
        Extra::PlayedHoursMinutes,
        Extra::SavesTitle,
        Extra::SavesNone,
        Extra::SavesOne,
        Extra::SavesCount,
        Extra::SavesBackUp,
        Extra::SavesNothing,
        Extra::SavesEmpty,
        Extra::SavesCaption,
        Extra::SavesKept,
        Extra::SavesReasonManual,
        Extra::SavesReasonUnlock,
        Extra::SavesReasonRestore,
        Extra::SavesReasonUninstall,
        Extra::SavesReasonPlayed,
        Extra::SavesBackedUp,
        Extra::SavesRestore,
        Extra::SavesRestoreAsk,
        Extra::SavesRestored,
        Extra::SavesRunning,
        Extra::SavesFailed,
        Extra::SavesRestoreFailed,
        Extra::SavesCorrupt,
        Extra::UninstallBacksUp,
        Extra::UninstallNeedsBackup,
        Extra::SavesRestoreIncomplete,
        Extra::SavesRestoreIncompleteNone,
        Extra::SavesBusy,
    ];
}

/// The time played as the home screen shows it: under a minute, whole minutes below an hour, hours
/// and minutes below ten hours, then whole hours (rounded down, so it never claims more than was
/// played).
pub fn playtime_text(seconds: u64, language: Language) -> String {
    let minutes = seconds / 60;
    if minutes == 0 {
        return Extra::PlayedUnderMinute.text(language).to_owned();
    }
    let (hours, rest) = (minutes / 60, minutes % 60);
    match (hours, rest) {
        (0, minutes) => Extra::PlayedMinutes.format(language, &[Arg::Int(minutes as i64)]),
        (hours, 0) => Extra::PlayedHours.format(language, &[Arg::Int(hours as i64)]),
        (hours, rest) if hours < 10 => Extra::PlayedHoursMinutes
            .format(language, &[Arg::Int(hours as i64), Arg::Int(rest as i64)]),
        (hours, _) => Extra::PlayedHours.format(language, &[Arg::Int(hours as i64)]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_text_names_the_old_product() {
        for extra in Extra::ALL {
            for language in Language::ALL {
                let text = extra.text(language);
                assert!(!text.contains("AnyPS5"), "{extra:?} {language:?}: {text}");
            }
        }
    }

    #[test]
    fn every_text_exists_in_both_languages() {
        for extra in Extra::ALL {
            for language in Language::ALL {
                assert!(!extra.text(language).is_empty(), "{extra:?} {language:?}");
            }
        }
    }

    #[test]
    fn playtime_reads_calmly_in_both_languages() {
        let en = |s| playtime_text(s, Language::English);
        assert_eq!(en(0), "Under 1 min");
        assert_eq!(en(59), "Under 1 min");
        assert_eq!(en(60), "1 min");
        assert_eq!(en(45 * 60 + 59), "45 min");
        assert_eq!(en(3600), "1 h");
        assert_eq!(en(3 * 3600 + 20 * 60), "3 h 20 min");
        assert_eq!(en(9 * 3600 + 59 * 60 + 59), "9 h 59 min");
        // From ten hours the minutes are noise.
        assert_eq!(en(10 * 3600 + 59 * 60), "10 h");
        assert_eq!(en(12 * 3600 + 34 * 60), "12 h");
        assert_eq!(en(112 * 3600 + 1800), "112 h");
        assert_eq!(
            playtime_text(3 * 3600 + 20 * 60, Language::Turkish),
            "3 sa 20 dk"
        );
        assert_eq!(playtime_text(45 * 60, Language::Turkish), "45 dk");
        assert_eq!(playtime_text(5, Language::Turkish), "1 dk altı");
    }

    #[test]
    fn placeholders_match_between_languages() {
        for extra in Extra::ALL {
            assert_eq!(
                extra.english().matches("%@").count(),
                extra.turkish().matches("%@").count(),
                "{extra:?}"
            );
        }
    }
}
