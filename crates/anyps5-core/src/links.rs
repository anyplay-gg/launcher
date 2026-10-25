//! Deep links: `anyps5://...` addresses that a website, a compatibility list or a message can use
//! to open a game, its unlock or a settings page in the launcher (docs/DESIGN.md 9.5).
//!
//! ```text
//! anyps5://game/<TITLEID>            show the game
//! anyps5://game/<TITLEID>/unlock     show the game and ask for the unlock
//! anyps5://settings[/<page>]         open the settings, optionally on a page
//! anyps5://open?path=<dump folder>   show the dump in the library
//! anyps5://join/<code>               ask the network for the invitation a join code stands for
//! ```
//!
//! Every address may carry the version as its first segment (`anyps5://v1/game/<TITLEID>`), which
//! is the form websites should use; the short form is the same link. Another version is refused.
//!
//! A [`Link`] only says *where to go*. It never starts a game and never unlocks anything by
//! itself: the launcher shows the place and the player presses the button. [`parse`] is strict
//! because the text comes from anywhere: unknown actions, extra segments, unknown query keys,
//! overlong input and paths that are not plain absolute folders are all errors.

use std::fmt;
use std::path::PathBuf;

/// The URL scheme of the launcher.
pub const SCHEME: &str = "anyps5";
/// Longest accepted link or argument, in bytes.
pub const MAX_LEN: usize = 2048;
/// Longest accepted folder path after decoding, in bytes.
pub const MAX_PATH_LEN: usize = 1024;

/// Where a deep link leads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    /// Show the game with this title ID (upper case).
    Game { title_id: String },
    /// Show the game and ask for its unlock.
    Unlock { title_id: String },
    /// Open the settings, on a page when one is named.
    Settings(Option<SettingsPage>),
    /// Show the dump in this folder.
    Open { path: PathBuf },
    /// Ask the network for the invitation this join code stands for (lower case). The launcher
    /// shows the invitation like any other; it never joins by itself.
    Join { code: String },
}

/// The pages of the settings a link can name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsPage {
    Appearance,
    Library,
    Language,
    Behavior,
    Toolkit,
    Advanced,
    About,
}

impl SettingsPage {
    fn from_name(name: &str) -> Option<SettingsPage> {
        Some(match name {
            "appearance" => SettingsPage::Appearance,
            "library" => SettingsPage::Library,
            "language" => SettingsPage::Language,
            "behavior" | "behaviour" => SettingsPage::Behavior,
            "toolkit" => SettingsPage::Toolkit,
            "advanced" => SettingsPage::Advanced,
            "about" => SettingsPage::About,
            _ => return None,
        })
    }
}

/// Why a text is not a deep link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkError {
    Empty,
    TooLong,
    /// The text does not start with the scheme (and is not a folder path).
    WrongScheme,
    UnknownAction,
    /// The first segment names a version this launcher does not know.
    UnknownVersion,
    /// More segments, or a query, than the action takes.
    Unexpected,
    BadTitleId,
    BadSettingsPage,
    MissingPath,
    /// A join code is ten characters of the join alphabet.
    BadJoinCode,
    /// A broken `%` escape, or bytes that are not UTF-8.
    BadEncoding,
    /// The path is not a plain absolute folder path; the reason is for logs.
    BadPath(&'static str),
}

impl fmt::Display for LinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LinkError::Empty => f.write_str("empty link"),
            LinkError::TooLong => f.write_str("link too long"),
            LinkError::WrongScheme => f.write_str("not an anyps5 link"),
            LinkError::UnknownAction => f.write_str("unknown action"),
            LinkError::UnknownVersion => f.write_str("unknown link version"),
            LinkError::Unexpected => f.write_str("unexpected parts after the action"),
            LinkError::BadTitleId => f.write_str("not a title ID"),
            LinkError::BadSettingsPage => f.write_str("unknown settings page"),
            LinkError::MissingPath => f.write_str("the link has no folder path"),
            LinkError::BadJoinCode => f.write_str("not a join code"),
            LinkError::BadEncoding => f.write_str("broken percent encoding"),
            LinkError::BadPath(reason) => write!(f, "refused folder path: {reason}"),
        }
    }
}

impl std::error::Error for LinkError {}

/// Parses `anyps5://...`.
pub fn parse(text: &str) -> Result<Link, LinkError> {
    parse_scheme(text, SCHEME)
}

/// Like [`parse`] for another scheme name (the tests register `anyps5-test`).
pub fn parse_scheme(text: &str, scheme: &str) -> Result<Link, LinkError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(LinkError::Empty);
    }
    if text.len() > MAX_LEN {
        return Err(LinkError::TooLong);
    }
    let rest = strip_scheme(text, scheme).ok_or(LinkError::WrongScheme)?;
    // What breaks out of a quoted command line, or is not text at all, is no link.
    if text
        .chars()
        .any(|c| c.is_control() || matches!(c, '"' | '`' | '^'))
    {
        return Err(LinkError::Unexpected);
    }
    let rest = rest.split('#').next().unwrap_or_default();
    let (path, query) = match rest.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (rest, None),
    };
    // Browsers add a closing slash to an address without a path.
    let path = path.strip_suffix('/').unwrap_or(path);
    let mut segments = path.split('/').peekable();
    match segments.peek() {
        Some(first) if first.eq_ignore_ascii_case("v1") => {
            segments.next();
        }
        Some(first) if is_version(first) => return Err(LinkError::UnknownVersion),
        _ => {}
    }
    let action = segments.next().unwrap_or_default().to_ascii_lowercase();
    let args: Vec<&str> = segments.collect();
    match (action.as_str(), args.as_slice(), query) {
        ("game", [id], None) => Ok(Link::Game {
            title_id: title_id(id)?,
        }),
        ("game", [id, verb], None) if verb.eq_ignore_ascii_case("unlock") => Ok(Link::Unlock {
            title_id: title_id(id)?,
        }),
        ("game", [_, _], None) => Err(LinkError::UnknownAction),
        ("settings", [], None) => Ok(Link::Settings(None)),
        ("settings", [page], None) => SettingsPage::from_name(&page.to_ascii_lowercase())
            .map(|page| Link::Settings(Some(page)))
            .ok_or(LinkError::BadSettingsPage),
        ("open", [], Some(query)) => Ok(Link::Open {
            path: open_path(query)?,
        }),
        ("open", [], None) => Err(LinkError::MissingPath),
        ("join", [code], None) => Ok(Link::Join {
            code: join_code(code)?,
        }),
        ("game" | "settings" | "open" | "join", _, _) => Err(LinkError::Unexpected),
        _ => Err(LinkError::UnknownAction),
    }
}

/// Parses one command-line argument: a link, or a folder path as Explorer passes it for the
/// "Open with AnyPS5" entry of a folder.
pub fn parse_argument(argument: &str, scheme: &str) -> Result<Link, LinkError> {
    let argument = argument.trim();
    if argument.len() > MAX_LEN {
        return Err(LinkError::TooLong);
    }
    match parse_scheme(argument, scheme) {
        Err(LinkError::WrongScheme) => Ok(Link::Open {
            path: clean_path(argument)?,
        }),
        other => other,
    }
}

/// What follows `<scheme>://`, the scheme compared without case.
fn strip_scheme<'a>(text: &'a str, scheme: &str) -> Option<&'a str> {
    let head = text.get(..scheme.len())?;
    if !head.eq_ignore_ascii_case(scheme) {
        return None;
    }
    text[scheme.len()..].strip_prefix("://")
}

/// `v` and digits: a version segment.
fn is_version(text: &str) -> bool {
    text.len() > 1
        && text[..1].eq_ignore_ascii_case("v")
        && text[1..].bytes().all(|b| b.is_ascii_digit())
}

/// A console title ID: `PPSA` or `CUSA` and five digits (`PPSA02100`).
fn title_id(text: &str) -> Result<String, LinkError> {
    let bytes = text.as_bytes();
    let valid = bytes.len() == 9
        && (bytes[..4].eq_ignore_ascii_case(b"PPSA") || bytes[..4].eq_ignore_ascii_case(b"CUSA"))
        && bytes[4..].iter().all(u8::is_ascii_digit);
    if valid {
        Ok(text.to_ascii_uppercase())
    } else {
        Err(LinkError::BadTitleId)
    }
}

/// A join code: ten characters of the network's alphabet (lower case letters and digits without
/// `0`, `1`, `l` and `o`), written in either case. Anything else is not a code, and a code is
/// never shortened, extended or searched for in a longer text.
fn join_code(text: &str) -> Result<String, LinkError> {
    use crate::online::api::{JOIN_CODE_ALPHABET, JOIN_CODE_LEN};
    let code = text.to_ascii_lowercase();
    let valid = code.len() == JOIN_CODE_LEN && code.chars().all(|c| JOIN_CODE_ALPHABET.contains(c));
    if valid {
        Ok(code)
    } else {
        Err(LinkError::BadJoinCode)
    }
}

/// The value of `path` in `path=<folder>`, the only key `open` takes.
fn open_path(query: &str) -> Result<PathBuf, LinkError> {
    let mut found = None;
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        if key != "path" || found.is_some() {
            return Err(LinkError::Unexpected);
        }
        found = Some(decode(value)?);
    }
    clean_path(&found.ok_or(LinkError::MissingPath)?)
}

/// Undoes `%XX` escapes. `+` stays a plus.
fn decode(text: &str) -> Result<String, LinkError> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3).ok_or(LinkError::BadEncoding)?;
            let hex = std::str::from_utf8(hex).map_err(|_| LinkError::BadEncoding)?;
            out.push(u8::from_str_radix(hex, 16).map_err(|_| LinkError::BadEncoding)?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| LinkError::BadEncoding)
}

/// Windows file names that are devices, whatever their extension.
fn is_reserved_name(component: &str) -> bool {
    let stem = component.split('.').next().unwrap_or_default();
    let stem = stem.to_ascii_uppercase();
    matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|p| {
        stem.strip_prefix(p)
            .is_some_and(|n| n.len() == 1 && n.as_bytes()[0].is_ascii_digit())
    })
}

/// A plain absolute folder path, or the reason it is not one. Whether the folder exists is for the
/// caller to find out; this only refuses what no link should ever carry: relative paths, `..`,
/// network and device paths, stream names (`file:stream`) and names Windows rewrites.
pub fn clean_path(path: &str) -> Result<PathBuf, LinkError> {
    if path.is_empty() {
        return Err(LinkError::MissingPath);
    }
    if path.len() > MAX_PATH_LEN {
        return Err(LinkError::BadPath("too long"));
    }
    if path
        .chars()
        .any(|c| c.is_control() || matches!(c, '<' | '>' | '"' | '|' | '?' | '*'))
    {
        return Err(LinkError::BadPath("forbidden character"));
    }
    let windows = cfg!(windows);
    // The root: a drive letter on Windows, a slash elsewhere.
    let after_root = if windows {
        let bytes = path.as_bytes();
        let drive = bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/');
        if !drive {
            return Err(LinkError::BadPath("not a drive path"));
        }
        &path[3..]
    } else {
        path.strip_prefix('/')
            .ok_or(LinkError::BadPath("not absolute"))?
    };
    // One closing separator is fine.
    let body = after_root
        .strip_suffix('/')
        .or_else(|| after_root.strip_suffix('\\').filter(|_| windows))
        .unwrap_or(after_root);
    if !body.is_empty() {
        for component in body.split(|c| c == '/' || (windows && c == '\\')) {
            if component.is_empty() {
                return Err(LinkError::BadPath("empty part"));
            }
            if component == "." || component == ".." {
                return Err(LinkError::BadPath("dot part"));
            }
            if windows {
                if component.contains(':') {
                    return Err(LinkError::BadPath("colon in a name"));
                }
                if component.ends_with(['.', ' ']) {
                    return Err(LinkError::BadPath("name ends in a dot or space"));
                }
                if is_reserved_name(component) {
                    return Err(LinkError::BadPath("device name"));
                }
            }
        }
    }
    Ok(PathBuf::from(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(id: &str) -> Link {
        Link::Game {
            title_id: id.to_owned(),
        }
    }

    #[test]
    fn game_links() {
        assert_eq!(parse("anyps5://game/PPSA16064"), Ok(game("PPSA16064")));
        assert_eq!(parse("anyps5://game/PPSA16064/"), Ok(game("PPSA16064")));
        assert_eq!(parse("ANYPS5://Game/ppsa16064"), Ok(game("PPSA16064")));
        assert_eq!(
            parse("  anyps5://game/CUSA00001#top "),
            Ok(game("CUSA00001"))
        );
    }

    #[test]
    fn versioned_links() {
        assert_eq!(parse("anyps5://v1/game/PPSA16064"), Ok(game("PPSA16064")));
        assert_eq!(
            parse("anyps5://V1/settings/about"),
            Ok(Link::Settings(Some(SettingsPage::About)))
        );
        assert_eq!(
            parse("anyps5://v2/game/PPSA16064"),
            Err(LinkError::UnknownVersion)
        );
        assert_eq!(parse("anyps5://v1"), Err(LinkError::UnknownAction));
    }

    #[test]
    fn no_quotes_or_control_characters() {
        for link in [
            "anyps5://game/PPSA16064\" --demo",
            "anyps5://game/PPSA16064`",
            "anyps5://game/PPSA16064^",
            "anyps5://game/PPSA16064%00",
            "anyps5://game/PPSA16064
x",
            "anyps5://settings ",
        ] {
            assert!(parse(link).is_err(), "{link:?}");
        }
    }

    fn join(code: &str) -> Link {
        Link::Join {
            code: code.to_owned(),
        }
    }

    #[test]
    fn join_links() {
        assert_eq!(parse("anyps5://v1/join/k3x9m2pqra"), Ok(join("k3x9m2pqra")));
        assert_eq!(parse("anyps5://join/k3x9m2pqra/"), Ok(join("k3x9m2pqra")));
        assert_eq!(
            parse("ANYPS5://V1/JOIN/K3X9M2PQRA#x"),
            Ok(join("k3x9m2pqra")),
            "the code is compared without case and kept in lower case"
        );
        for code in [
            "k3x9m2pqr",   // nine
            "k3x9m2pqrab", // eleven
            "k3x9m2pqr0",  // 0, 1, l and o are not in the alphabet
            "k3x9m2pqr1",
            "k3x9m2pqrl",
            "k3x9m2pqro",
            "k3x9m2pqr-",
            "k3x9m2pqré",
        ] {
            assert_eq!(
                parse(&format!("anyps5://v1/join/{code}")),
                Err(LinkError::BadJoinCode),
                "{code:?}"
            );
        }
    }

    #[test]
    fn a_join_link_takes_one_code_and_nothing_else() {
        assert_eq!(parse("anyps5://v1/join"), Err(LinkError::Unexpected));
        assert_eq!(parse("anyps5://v1/join/"), Err(LinkError::Unexpected));
        for link in [
            "anyps5://v1/join/k3x9m2pqra/play",
            "anyps5://v1/join/k3x9m2pqra?game=PPSA16064",
            "anyps5://v1/join/k3x9m2pqra/k3x9m2pqra",
            "anyps5://v1/join/k3x9m2pqra%20",
            "anyps5://v1/join/k3x9m2pqra\"",
        ] {
            assert!(parse(link).is_err(), "{link:?}");
        }
        assert_eq!(
            parse("anyps5://v2/join/k3x9m2pqra"),
            Err(LinkError::UnknownVersion)
        );
    }

    #[test]
    fn the_join_alphabet_is_the_networks() {
        // The parser reads the alphabet from the network's API crate; a code the cloud can issue
        // is a code the launcher accepts.
        for c in crate::online::api::JOIN_CODE_ALPHABET.chars() {
            let code = c.to_string().repeat(crate::online::api::JOIN_CODE_LEN);
            assert_eq!(parse(&format!("anyps5://v1/join/{code}")), Ok(join(&code)));
        }
        assert_eq!(crate::online::api::JOIN_CODE_LEN, 10);
    }

    #[test]
    fn unlock_links() {
        let expected = Link::Unlock {
            title_id: "PPSA02100".to_owned(),
        };
        assert_eq!(
            parse("anyps5://game/PPSA02100/unlock"),
            Ok(expected.clone())
        );
        assert_eq!(parse("anyps5://game/PPSA02100/UNLOCK/"), Ok(expected));
        assert_eq!(
            parse("anyps5://game/PPSA02100/play"),
            Err(LinkError::UnknownAction)
        );
        assert_eq!(
            parse("anyps5://game/PPSA02100/unlock/now"),
            Err(LinkError::Unexpected)
        );
        assert_eq!(
            parse("anyps5://game/PPSA02100/unlock?force=1"),
            Err(LinkError::Unexpected)
        );
    }

    #[test]
    fn bad_title_ids() {
        for id in [
            "SLUS00001",
            "PPSB16064",
            "PPSA1606",
            "PPSA160644",
            "PPS16064",
            "12345ABCD",
            "PPSA1606é",
            "PPSA 1606",
        ] {
            assert_eq!(
                parse(&format!("anyps5://game/{id}")),
                Err(LinkError::BadTitleId),
                "{id:?}"
            );
        }
        assert_eq!(parse("anyps5://game"), Err(LinkError::Unexpected));
        assert_eq!(parse("anyps5://game/"), Err(LinkError::Unexpected));
    }

    #[test]
    fn settings_links() {
        assert_eq!(parse("anyps5://settings"), Ok(Link::Settings(None)));
        assert_eq!(parse("anyps5://settings/"), Ok(Link::Settings(None)));
        assert_eq!(
            parse("anyps5://settings/behavior"),
            Ok(Link::Settings(Some(SettingsPage::Behavior)))
        );
        assert_eq!(
            parse("anyps5://settings/Behaviour"),
            Ok(Link::Settings(Some(SettingsPage::Behavior)))
        );
        assert_eq!(
            parse("anyps5://settings/toolkit"),
            Ok(Link::Settings(Some(SettingsPage::Toolkit)))
        );
        assert_eq!(
            parse("anyps5://settings/wallet"),
            Err(LinkError::BadSettingsPage)
        );
        assert_eq!(
            parse("anyps5://settings/about/more"),
            Err(LinkError::Unexpected)
        );
    }

    #[test]
    fn refuses_what_it_does_not_know() {
        assert_eq!(parse(""), Err(LinkError::Empty));
        assert_eq!(parse("   "), Err(LinkError::Empty));
        assert_eq!(parse("https://game/PPSA16064"), Err(LinkError::WrongScheme));
        assert_eq!(parse("anyps5:game/PPSA16064"), Err(LinkError::WrongScheme));
        assert_eq!(
            parse("anyps5s://game/PPSA16064"),
            Err(LinkError::WrongScheme)
        );
        assert_eq!(parse("anyps5://"), Err(LinkError::UnknownAction));
        assert_eq!(
            parse("anyps5://play/PPSA16064"),
            Err(LinkError::UnknownAction)
        );
        assert_eq!(
            parse("anyps5://delete/PPSA16064"),
            Err(LinkError::UnknownAction)
        );
        assert_eq!(parse("anyps5://uninstall"), Err(LinkError::UnknownAction));
        assert_eq!(
            parse("anyps5://game/PPSA16064?autostart=1"),
            Err(LinkError::Unexpected)
        );
    }

    #[test]
    fn overlong_input() {
        let long = format!("anyps5://open?path={}", "a".repeat(MAX_LEN));
        assert_eq!(parse(&long), Err(LinkError::TooLong));
        let at_limit = format!("anyps5://game/PPSA16064#{}", "a".repeat(MAX_LEN));
        assert_eq!(parse(&at_limit), Err(LinkError::TooLong));
    }

    #[test]
    fn another_scheme_name() {
        assert_eq!(
            parse_scheme("anyps5-test://game/PPSA16064", "anyps5-test"),
            Ok(game("PPSA16064"))
        );
        assert_eq!(
            parse_scheme("anyps5://game/PPSA16064", "anyps5-test"),
            Err(LinkError::WrongScheme)
        );
        assert_eq!(
            parse("anyps5-test://game/PPSA16064"),
            Err(LinkError::WrongScheme)
        );
    }

    #[test]
    fn percent_decoding() {
        assert_eq!(decode("a%20b%2Fc"), Ok("a b/c".to_owned()));
        assert_eq!(decode("a+b"), Ok("a+b".to_owned()));
        assert_eq!(decode("%C3%A4"), Ok("ä".to_owned()));
        assert_eq!(decode("100%"), Err(LinkError::BadEncoding));
        assert_eq!(decode("%zz"), Err(LinkError::BadEncoding));
        assert_eq!(decode("%C3"), Err(LinkError::BadEncoding));
    }

    #[cfg(windows)]
    mod windows_paths {
        use super::*;

        fn open(path: &str) -> Result<Link, LinkError> {
            parse(&format!("anyps5://open?path={path}"))
        }

        #[test]
        fn plain_drive_paths() {
            let expected = Link::Open {
                path: PathBuf::from(r"D:\Games\Demon Souls"),
            };
            assert_eq!(open(r"D:\Games\Demon%20Souls"), Ok(expected.clone()));
            assert_eq!(open("D%3A%5CGames%5CDemon%20Souls"), Ok(expected.clone()));
            assert_eq!(
                open("D:/Games/Demon%20Souls/"),
                Ok(Link::Open {
                    path: PathBuf::from("D:/Games/Demon Souls/")
                })
            );
            assert_eq!(
                open(r"C:\"),
                Ok(Link::Open {
                    path: PathBuf::from(r"C:\")
                })
            );
        }

        #[test]
        fn refuses_tricks() {
            let cases = [
                (r"D:\Games\..\Windows", "dot part"),
                (r"D:\Games\.\x", "dot part"),
                ("D:%5CGames%5C..%5C..", "dot part"),
                (r"Games\x", "not a drive path"),
                (r"\Games\x", "not a drive path"),
                (r"\\server\share\x", "not a drive path"),
                (r"\\.\C:\x", "not a drive path"),
                (r"C:\Games\x:stream", "colon in a name"),
                (r"C:\Games\NUL", "device name"),
                (r"C:\Games\com1.txt", "device name"),
                (r"C:\Games\x.", "name ends in a dot or space"),
                ("C:%5CGames%5Cx%20", "name ends in a dot or space"),
                (r"C:\Games\\x", "empty part"),
                (r"C:\Games\*", "forbidden character"),
                ("C:%5CGames%00x", "forbidden character"),
                ("C:%5CGames%0Ax", "forbidden character"),
                (r"relative", "not a drive path"),
            ];
            for (path, reason) in cases {
                assert_eq!(open(path), Err(LinkError::BadPath(reason)), "{path}");
            }
        }

        #[test]
        fn query_shapes() {
            assert_eq!(parse("anyps5://open"), Err(LinkError::MissingPath));
            assert_eq!(parse("anyps5://open?"), Err(LinkError::MissingPath));
            assert_eq!(parse("anyps5://open?path="), Err(LinkError::MissingPath));
            assert_eq!(
                parse(r"anyps5://open?path=C:\a&path=C:\b"),
                Err(LinkError::Unexpected)
            );
            assert_eq!(
                parse(r"anyps5://open?path=C:\a&run=1"),
                Err(LinkError::Unexpected)
            );
            assert_eq!(
                parse(r"anyps5://open/x?path=C:\a"),
                Err(LinkError::Unexpected)
            );
            assert_eq!(parse("anyps5://open?path=%E9"), Err(LinkError::BadEncoding));
        }

        #[test]
        fn arguments() {
            let dump = Link::Open {
                path: PathBuf::from(r"E:\Dumps\Hollow Knight"),
            };
            assert_eq!(parse_argument(r"E:\Dumps\Hollow Knight", SCHEME), Ok(dump));
            assert_eq!(
                parse_argument("anyps5://settings", SCHEME),
                Ok(Link::Settings(None))
            );
            assert_eq!(
                parse_argument("--gallery=home", SCHEME),
                Err(LinkError::BadPath("not a drive path"))
            );
            assert_eq!(
                parse_argument(r"\\server\share", SCHEME),
                Err(LinkError::BadPath("not a drive path"))
            );
        }
    }

    #[cfg(not(windows))]
    mod unix_paths {
        use super::*;

        #[test]
        fn plain_and_refused() {
            assert_eq!(
                parse("anyps5://open?path=/home/mira/Games/Demon%20Souls"),
                Ok(Link::Open {
                    path: PathBuf::from("/home/mira/Games/Demon Souls")
                })
            );
            assert_eq!(
                parse("anyps5://open?path=/home/mira/../root"),
                Err(LinkError::BadPath("dot part"))
            );
            assert_eq!(
                parse("anyps5://open?path=games/x"),
                Err(LinkError::BadPath("not absolute"))
            );
        }
    }
}
