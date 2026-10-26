//! Texts of the online area: signing in, friends, invitations and joining a friend's game.
//! English and Turkish like the rest. Row titles are in title case, notes and toasts in
//! sentence case, as everywhere in the launcher.

use crate::ui::i18n::{Arg, Language, substitute};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OnlineText {
    /// Panel title before sign-in, and the profile button's name.
    Online,
    Subtitle,
    Cloud,
    CloudNotSet,
    /// The cloud AnyPlay runs, as the cloud row names it.
    CloudOfficial,
    CloudUseOfficial,
    CloudSubtitle,
    CloudAddress,
    CloudUse,
    CloudHint,
    /// Toast: the address is not an http or https URL.
    CloudInvalid,
    Sending,
    SignInNote,
    NeedsCloud,
    SignIn,
    Handle,
    CreateAccount,
    /// Subtitle while the browser sign-in waits.
    ContinueInBrowser,
    OpenAgain,
    BrowserNote,
    /// Toast: the browser sign-in ran out of time.
    SignInTimedOut,
    ManageAccount,
    ManageAccountDetail,
    StatusOnline,
    StatusConnecting,
    StatusOffline,
    Invitations,
    Requests,
    /// The row under the first few requests; leads to all of them.
    SeeAllRequests,
    /// `%@` = how many.
    RequestsWaiting,
    /// Under the friends title, after the handle: `%@` = how many friends are online.
    FriendsOnlineCount,
    Friends,
    Sent,
    Session,
    Blocked,
    /// `%@` = the game.
    Playing,
    /// `%@` = the game.
    PlayingJoinable,
    FriendOnline,
    FriendOffline,
    NoFriends,
    AddFriend,
    AddFriendSubtitle,
    SendRequest,
    AddFriendHint,
    WantsFriend,
    Waiting,
    Accept,
    Decline,
    Withdraw,
    RemoveFriend,
    Block,
    Unblock,
    Report,
    /// `%@` = the friend.
    ReportTitle,
    Reason,
    ReasonCheating,
    ReasonHarassment,
    ReasonSpam,
    ReasonName,
    ReasonOther,
    SendReport,
    JoinGame,
    /// `%@` members of `%@`.
    JoinDetail,
    InviteToSession,
    /// `%@` = who invites.
    InvitedBy,
    LinkDirect,
    LinkRelay,
    LinkConnecting,
    You,
    Account,
    SignOut,
    /// `%@` = the game.
    PickerTitle,
    /// `%@` = how many at most.
    PickerSubtitle,
    SendInvitations,
    // Toasts
    /// `%@` = the handle.
    Welcome,
    SignedOut,
    /// `%@` = the handle.
    RequestSent,
    /// `%@` = the handle.
    NowFriends,
    /// `%@` = the handle.
    RequestArrived,
    /// `%@` = who, `%@` = the game.
    InvitationArrived,
    /// `%@` = the handle.
    BlockedToast,
    Reported,
    /// `%@` = the friend, `%@` = the game.
    Joining,
    InvitationsSent,
    // Problems
    Offline,
    CodeWrong,
    CodeExpired,
    HandleTaken,
    HandleInvalid,
    NotFound,
    RateLimited,
    Banned,
    /// `%@` = the reason the service gave.
    Failed,
    /// `%@` = the game.
    NotOwned,
    /// `%@` = the game.
    NotUnlocked,
    /// `%@` friend, `%@` their version, `%@` their AnyPS5, `%@` yours, `%@` your AnyPS5.
    VersionMismatch,
    Full,
    ServiceMissing,
    ServiceEnded,
    // Settings
    RecordNetwork,
    RecordNetworkDetail,
    RelayOnly,
    RelayOnlyNote,
    /// Linux: the OpenH264 row and its states.
    Openh264,
    Openh264Off,
    Openh264Downloading,
    Openh264Ready,
    Openh264Missing,
    /// `%@`: why the download failed.
    Openh264Failed,
    /// The share worker found a file that is not Cisco's build.
    Openh264Refused,
    /// `%@`: why the share worker could not load it.
    Openh264NotLoaded,
    /// Cisco's licence asks for this text, unchanged, where the player turns the codec on and off.
    Openh264Notice,
    // Appearing offline
    AppearOffline,
    AppearOfflineDetail,
    /// The quiet words where the player sees their own status.
    AppearingOffline,
    // Trophies
    /// A friend hides their trophies from the player.
    TrophiesHidden,
    TrophiesNone,
    TrophyAudience,
    AudienceFriends,
    AudienceEveryone,
    AudienceOnlyYou,
    TrophyAudienceNote,
}

impl OnlineText {
    pub const fn english(self) -> &'static str {
        match self {
            OnlineText::Online => "Online",
            OnlineText::Subtitle => "Play with your friends over the internet",
            OnlineText::Cloud => "Cloud",
            OnlineText::CloudNotSet => "Not set up yet",
            OnlineText::CloudOfficial => "AnyPlay",
            OnlineText::CloudUseOfficial => "Use the AnyPlay Cloud",
            OnlineText::CloudSubtitle => "The address of the cloud you sign in to",
            OnlineText::CloudAddress => "Address",
            OnlineText::CloudUse => "Use This Cloud",
            OnlineText::CloudHint => {
                "A cloud of your own, or of friends who run one. Ask whoever runs it for its address, such as https://cloud.example.org."
            }
            OnlineText::CloudInvalid => "Enter an address that starts with https://",
            OnlineText::Sending => "Sending…",
            OnlineText::SignInNote => {
                "You sign in or create your account on the cloud's page in your browser. Nothing goes online before that."
            }
            OnlineText::NeedsCloud => "Set up the cloud first.",
            OnlineText::SignIn => "Sign In",
            OnlineText::Handle => "Name",
            OnlineText::CreateAccount => "Create Account",
            OnlineText::ContinueInBrowser => "Continue in your browser",
            OnlineText::OpenAgain => "Open the Page Again",
            OnlineText::BrowserNote => "When you are done there, the launcher signs in by itself.",
            OnlineText::SignInTimedOut => "The sign-in timed out. Try again.",
            OnlineText::ManageAccount => "Manage Account",
            OnlineText::ManageAccountDetail => "E-mail, passkeys, devices and deletion",
            OnlineText::StatusOnline => "Online",
            OnlineText::StatusConnecting => "Connecting…",
            OnlineText::StatusOffline => "Offline, trying again",
            OnlineText::Invitations => "Invitations",
            OnlineText::Requests => "Friend Requests",
            OnlineText::SeeAllRequests => "See All Requests",
            OnlineText::RequestsWaiting => "%@ waiting",
            OnlineText::FriendsOnlineCount => "%@ online",
            OnlineText::Friends => "Friends",
            OnlineText::Sent => "Sent Requests",
            OnlineText::Session => "Playing Together",
            OnlineText::Blocked => "Blocked",
            OnlineText::Playing => "Playing %@",
            OnlineText::PlayingJoinable => "Playing %@ · You can join",
            OnlineText::FriendOnline => "Online",
            OnlineText::FriendOffline => "Offline",
            OnlineText::NoFriends => "No friends yet. Add one by the name they chose.",
            OnlineText::AddFriend => "Add a Friend",
            OnlineText::AddFriendSubtitle => "Enter the name of their account",
            OnlineText::SendRequest => "Send Request",
            OnlineText::AddFriendHint => {
                "Capital letters do not matter. They see the request once they are online."
            }
            OnlineText::WantsFriend => "Wants to be your friend",
            OnlineText::Waiting => "Waiting for an answer",
            OnlineText::Accept => "Accept",
            OnlineText::Decline => "Decline",
            OnlineText::Withdraw => "Withdraw Request",
            OnlineText::RemoveFriend => "Remove Friend",
            OnlineText::Block => "Block",
            OnlineText::Unblock => "Unblock",
            OnlineText::Report => "Report",
            OnlineText::ReportTitle => "Report %@",
            OnlineText::Reason => "Reason",
            OnlineText::ReasonCheating => "Cheating",
            OnlineText::ReasonHarassment => "Harassment",
            OnlineText::ReasonSpam => "Spam",
            OnlineText::ReasonName => "Offensive Name",
            OnlineText::ReasonOther => "Something Else",
            OnlineText::SendReport => "Send Report",
            OnlineText::JoinGame => "Join Game",
            OnlineText::JoinDetail => "%@ of %@ players",
            OnlineText::InviteToSession => "Invite to Your Game",
            OnlineText::InvitedBy => "%@ invites you",
            OnlineText::LinkDirect => "Direct",
            OnlineText::LinkRelay => "Through a relay",
            OnlineText::LinkConnecting => "Connecting…",
            OnlineText::You => "You",
            OnlineText::Account => "Account",
            OnlineText::SignOut => "Sign Out",
            OnlineText::PickerTitle => "Invite to %@",
            OnlineText::PickerSubtitle => "Pick up to %@ friends",
            OnlineText::SendInvitations => "Send Invitations",
            OnlineText::Welcome => "Signed in as %@",
            OnlineText::SignedOut => "Signed out",
            OnlineText::RequestSent => "Request sent to %@",
            OnlineText::NowFriends => "You and %@ are friends",
            OnlineText::RequestArrived => "%@ wants to be your friend",
            OnlineText::InvitationArrived => "%@ invites you to %@",
            OnlineText::BlockedToast => "%@ is blocked",
            OnlineText::Reported => "Report sent. Thank you.",
            OnlineText::Joining => "Joining %@ in %@",
            OnlineText::InvitationsSent => "Invitations sent",
            OnlineText::Offline => {
                "The cloud cannot be reached. Check your connection and try again."
            }
            OnlineText::CodeWrong => "That code does not match. Check the latest e-mail.",
            OnlineText::CodeExpired => "The code expired. Send a new one.",
            OnlineText::HandleTaken => "That name is taken. Try another one.",
            OnlineText::HandleInvalid => {
                "Use 3 to 16 letters, digits, - or _, starting with a letter."
            }
            OnlineText::NotFound => "No account has this name.",
            OnlineText::RateLimited => "Too many tries. Wait a moment and try again.",
            OnlineText::Banned => "This account is banned.",
            OnlineText::Failed => "That did not work: %@",
            OnlineText::NotOwned => "%@ is not in your library.",
            OnlineText::NotUnlocked => "Unlock %@ first, then join.",
            OnlineText::VersionMismatch => {
                "%@ plays version %@ with AnyPS5 %@. You have %@ with AnyPS5 %@."
            }
            OnlineText::Full => "The game is full.",
            OnlineText::ServiceMissing => {
                "This installation has no network service. Install the launcher again."
            }
            OnlineText::ServiceEnded => {
                "The network service stopped. Open Online to start it again."
            }
            OnlineText::RecordNetwork => "Record Network Calls",
            OnlineText::RecordNetworkDetail => {
                "Games write their network requests to the Logs folder, to help build online support. Nothing is uploaded."
            }
            OnlineText::AppearOffline => "Appear Offline",
            OnlineText::AppearOfflineDetail => "Friends see you as offline",
            OnlineText::AppearingOffline => "Appearing offline",
            OnlineText::RelayOnly => "Hide Your IP Address",
            OnlineText::RelayOnlyNote => {
                "Other players reach you only through a relay and never see your IP address. Games can respond a little slower."
            }
            OnlineText::Openh264 => "Video for Share Screen",
            OnlineText::Openh264Off => {
                "Turn on to download Cisco's video codec. Share Screen cannot send or show video without it"
            }
            OnlineText::Openh264Downloading => "Downloading from Cisco",
            OnlineText::Openh264Ready => "Downloaded from Cisco",
            OnlineText::Openh264Missing => "Not downloaded yet",
            OnlineText::Openh264Failed => "The download failed: %@",
            OnlineText::Openh264Refused => {
                "The file is not Cisco's build. Turn it off and on to download it again."
            }
            OnlineText::Openh264NotLoaded => "It did not load: %@",
            OnlineText::Openh264Notice => "OpenH264 Video Codec provided by Cisco Systems, Inc.",
            OnlineText::TrophiesHidden => "Hidden",
            OnlineText::TrophiesNone => "None yet",
            OnlineText::TrophyAudience => "Who Sees Your Trophies",
            OnlineText::AudienceFriends => "Friends",
            OnlineText::AudienceEveryone => "Everyone",
            OnlineText::AudienceOnlyYou => "Only You",
            OnlineText::TrophyAudienceNote => {
                "The players you choose see how many trophies you have of each grade. Only each trophy's number, grade and date leave this PC."
            }
        }
    }

    pub const fn turkish(self) -> &'static str {
        match self {
            OnlineText::Online => "Çevrimiçi",
            OnlineText::Subtitle => "Arkadaşlarınla internet üzerinden oyna",
            OnlineText::Cloud => "Bulut",
            OnlineText::CloudNotSet => "Henüz ayarlanmadı",
            OnlineText::CloudOfficial => "AnyPlay",
            OnlineText::CloudUseOfficial => "AnyPlay Bulutunu Kullan",
            OnlineText::CloudSubtitle => "Giriş yaptığın bulutun adresi",
            OnlineText::CloudAddress => "Adres",
            OnlineText::CloudUse => "Bu Bulutu Kullan",
            OnlineText::CloudHint => {
                "Kendi bulutun ya da bir tane işleten arkadaşlarının bulutu. Adresi işleten kişiden iste, örneğin https://cloud.example.org."
            }
            OnlineText::CloudInvalid => "https:// ile başlayan bir adres gir",
            OnlineText::Sending => "Gönderiliyor…",
            OnlineText::SignInNote => {
                "Giriş yapma ve hesap oluşturma tarayıcında, bulutun kendi sayfasında olur. Bundan önce hiçbir şey çevrimiçi olmaz."
            }
            OnlineText::NeedsCloud => "Önce bulutu ayarla.",
            OnlineText::SignIn => "Giriş Yap",
            OnlineText::Handle => "Ad",
            OnlineText::CreateAccount => "Hesap Oluştur",
            OnlineText::ContinueInBrowser => "Tarayıcında devam et",
            OnlineText::OpenAgain => "Sayfayı Yeniden Aç",
            OnlineText::BrowserNote => "Orada işin bitince başlatıcı kendiliğinden giriş yapar.",
            OnlineText::SignInTimedOut => "Girişin süresi doldu. Yeniden dene.",
            OnlineText::ManageAccount => "Hesabı Yönet",
            OnlineText::ManageAccountDetail => "E-posta, geçiş anahtarları, cihazlar ve silme",
            OnlineText::StatusOnline => "Çevrimiçi",
            OnlineText::StatusConnecting => "Bağlanıyor…",
            OnlineText::StatusOffline => "Çevrimdışı, yeniden deneniyor",
            OnlineText::Invitations => "Davetler",
            OnlineText::Requests => "Arkadaşlık İstekleri",
            OnlineText::SeeAllRequests => "Tüm İstekleri Gör",
            OnlineText::RequestsWaiting => "%@ bekliyor",
            OnlineText::FriendsOnlineCount => "%@ çevrimiçi",
            OnlineText::Friends => "Arkadaşlar",
            OnlineText::Sent => "Gönderilen İstekler",
            OnlineText::Session => "Birlikte Oynananlar",
            OnlineText::Blocked => "Engellenenler",
            OnlineText::Playing => "%@ oynuyor",
            OnlineText::PlayingJoinable => "%@ oynuyor · Katılabilirsin",
            OnlineText::FriendOnline => "Çevrimiçi",
            OnlineText::FriendOffline => "Çevrimdışı",
            OnlineText::NoFriends => "Henüz arkadaşın yok. Seçtikleri adla birini ekle.",
            OnlineText::AddFriend => "Arkadaş Ekle",
            OnlineText::AddFriendSubtitle => "Hesabının adını gir",
            OnlineText::SendRequest => "İstek Gönder",
            OnlineText::AddFriendHint => {
                "Büyük harfler fark etmez. İsteği çevrimiçi olduklarında görürler."
            }
            OnlineText::WantsFriend => "Arkadaşın olmak istiyor",
            OnlineText::Waiting => "Yanıt bekleniyor",
            OnlineText::Accept => "Kabul Et",
            OnlineText::Decline => "Reddet",
            OnlineText::Withdraw => "İsteği Geri Çek",
            OnlineText::RemoveFriend => "Arkadaşlıktan Çıkar",
            OnlineText::Block => "Engelle",
            OnlineText::Unblock => "Engeli Kaldır",
            OnlineText::Report => "Şikâyet Et",
            OnlineText::ReportTitle => "%@ Kişisini Şikâyet Et",
            OnlineText::Reason => "Neden",
            OnlineText::ReasonCheating => "Hile",
            OnlineText::ReasonHarassment => "Taciz",
            OnlineText::ReasonSpam => "Spam",
            OnlineText::ReasonName => "Uygunsuz Ad",
            OnlineText::ReasonOther => "Başka Bir Şey",
            OnlineText::SendReport => "Şikâyeti Gönder",
            OnlineText::JoinGame => "Oyuna Katıl",
            OnlineText::JoinDetail => "%@ / %@ oyuncu",
            OnlineText::InviteToSession => "Oyununa Davet Et",
            OnlineText::InvitedBy => "%@ seni davet ediyor",
            OnlineText::LinkDirect => "Doğrudan",
            OnlineText::LinkRelay => "Aktarıcı üzerinden",
            OnlineText::LinkConnecting => "Bağlanıyor…",
            OnlineText::You => "Sen",
            OnlineText::Account => "Hesap",
            OnlineText::SignOut => "Çıkış Yap",
            OnlineText::PickerTitle => "%@ Oyununa Davet Et",
            OnlineText::PickerSubtitle => "En fazla %@ arkadaş seç",
            OnlineText::SendInvitations => "Davetleri Gönder",
            OnlineText::Welcome => "%@ olarak giriş yapıldı",
            OnlineText::SignedOut => "Çıkış yapıldı",
            OnlineText::RequestSent => "%@ kişisine istek gönderildi",
            OnlineText::NowFriends => "%@ ile artık arkadaşsınız",
            OnlineText::RequestArrived => "%@ arkadaşın olmak istiyor",
            OnlineText::InvitationArrived => "%@ seni %@ oyununa davet ediyor",
            OnlineText::BlockedToast => "%@ engellendi",
            OnlineText::Reported => "Şikâyet gönderildi. Teşekkürler.",
            OnlineText::Joining => "%@ kişisinin %@ oyununa katılınıyor",
            OnlineText::InvitationsSent => "Davetler gönderildi",
            OnlineText::Offline => "Buluta ulaşılamıyor. Bağlantını kontrol edip yeniden dene.",
            OnlineText::CodeWrong => "Kod eşleşmiyor. En son e-postaya bak.",
            OnlineText::CodeExpired => "Kodun süresi doldu. Yeni bir kod gönder.",
            OnlineText::HandleTaken => "Bu ad alınmış. Başka bir ad dene.",
            OnlineText::HandleInvalid => {
                "Harfle başlayan, 3 ile 16 arası harf, rakam, - veya _ kullan."
            }
            OnlineText::NotFound => "Bu adda bir hesap yok.",
            OnlineText::RateLimited => "Çok fazla deneme. Biraz bekleyip yeniden dene.",
            OnlineText::Banned => "Bu hesap engellendi.",
            OnlineText::Failed => "Olmadı: %@",
            OnlineText::NotOwned => "%@ kütüphanende yok.",
            OnlineText::NotUnlocked => "Önce %@ oyununun kilidini aç, sonra katıl.",
            OnlineText::VersionMismatch => {
                "%@, %@ sürümünü AnyPS5 %@ ile oynuyor. Sende %@ sürümü AnyPS5 %@ ile var."
            }
            OnlineText::Full => "Oyun dolu.",
            OnlineText::ServiceMissing => "Bu kurulumda ağ hizmeti yok. Başlatıcıyı yeniden kur.",
            OnlineText::ServiceEnded => "Ağ hizmeti durdu. Yeniden başlatmak için Çevrimiçi'yi aç.",
            OnlineText::RecordNetwork => "Ağ Çağrılarını Kaydet",
            OnlineText::RecordNetworkDetail => {
                "Oyunlar ağdan ne istediklerini Logs klasörüne yazar; çevrimiçi desteğin geliştirilmesine yardım eder. Hiçbir şey yüklenmez."
            }
            OnlineText::AppearOffline => "Çevrimdışı Görün",
            OnlineText::AppearOfflineDetail => "Arkadaşların seni çevrimdışı görür",
            OnlineText::AppearingOffline => "Çevrimdışı görünüyorsun",
            OnlineText::RelayOnly => "IP Adresini Gizle",
            OnlineText::RelayOnlyNote => {
                "Diğer oyuncular sana yalnızca bir aktarıcı üzerinden ulaşır ve IP adresini hiç görmez. Oyunlar biraz daha geç tepki verebilir."
            }
            OnlineText::Openh264 => "Ekran Paylaşımında Video",
            OnlineText::Openh264Off => {
                "Cisco'nun video kodeğini indirmek için aç. Kodek olmadan ekran paylaşımı video gönderemez ve gösteremez"
            }
            OnlineText::Openh264Downloading => "Cisco'dan indiriliyor",
            OnlineText::Openh264Ready => "Cisco'dan indirildi",
            OnlineText::Openh264Missing => "Henüz indirilmedi",
            OnlineText::Openh264Failed => "İndirme başarısız oldu: %@",
            OnlineText::Openh264Refused => {
                "Dosya Cisco'nun derlemesi değil. Yeniden indirmek için kapatıp aç."
            }
            OnlineText::Openh264NotLoaded => "Yüklenemedi: %@",
            // Cisco's text stays in English in every language.
            OnlineText::Openh264Notice => "OpenH264 Video Codec provided by Cisco Systems, Inc.",
            OnlineText::TrophiesHidden => "Gizli",
            OnlineText::TrophiesNone => "Henüz yok",
            OnlineText::TrophyAudience => "Kupalarını Kimler Görür",
            OnlineText::AudienceFriends => "Arkadaşlar",
            OnlineText::AudienceEveryone => "Herkes",
            OnlineText::AudienceOnlyYou => "Yalnızca Sen",
            OnlineText::TrophyAudienceNote => {
                "Seçtiğin oyuncular her dereceden kaç kupan olduğunu görür. Bilgisayarından yalnızca her kupanın numarası, derecesi ve tarihi çıkar."
            }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: &[OnlineText] = &[
        OnlineText::Playing,
        OnlineText::PlayingJoinable,
        OnlineText::ReportTitle,
        OnlineText::JoinDetail,
        OnlineText::InvitedBy,
        OnlineText::PickerTitle,
        OnlineText::PickerSubtitle,
        OnlineText::Welcome,
        OnlineText::RequestSent,
        OnlineText::NowFriends,
        OnlineText::RequestsWaiting,
        OnlineText::FriendsOnlineCount,
        OnlineText::RequestArrived,
        OnlineText::InvitationArrived,
        OnlineText::BlockedToast,
        OnlineText::Joining,
        OnlineText::Failed,
        OnlineText::NotOwned,
        OnlineText::NotUnlocked,
        OnlineText::VersionMismatch,
        OnlineText::Openh264Failed,
        OnlineText::Openh264NotLoaded,
    ];

    #[test]
    fn placeholders_match_between_languages() {
        for text in ALL {
            let count = |s: &str| s.matches("%@").count();
            assert_eq!(count(text.english()), count(text.turkish()), "{text:?}");
        }
    }

    #[test]
    fn the_version_message_names_both_sides() {
        let text = OnlineText::VersionMismatch.format(
            Language::English,
            &[
                Arg::Str("Lumen"),
                Arg::Str("01.100"),
                Arg::Str("0.1.4"),
                Arg::Str("01.000"),
                Arg::Str("0.1.3"),
            ],
        );
        assert_eq!(
            text,
            "Lumen plays version 01.100 with AnyPS5 0.1.4. You have 01.000 with AnyPS5 0.1.3."
        );
    }
}
