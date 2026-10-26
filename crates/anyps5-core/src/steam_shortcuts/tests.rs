use super::*;
use tempfile::TempDir;

fn text(key: &str, value: &str) -> Vec<u8> {
    let mut v = vec![1];
    v.extend_from_slice(key.as_bytes());
    v.push(0);
    v.extend_from_slice(value.as_bytes());
    v.push(0);
    v
}

fn number(key: &str, value: u32) -> Vec<u8> {
    let mut v = vec![2];
    v.extend_from_slice(key.as_bytes());
    v.push(0);
    v.extend_from_slice(&value.to_le_bytes());
    v
}

/// A file as Steam writes it: one entry of the player's own, with fields this module never sets.
fn players_file() -> Vec<u8> {
    let mut v = b"\0shortcuts\0\0".to_vec();
    v.extend_from_slice(b"0\0");
    v.extend(number("appid", 0xDEAD_BEEF));
    v.extend(text("AppName", "Some Tool"));
    v.extend(text("exe", "\"C:\\Tools\\tool.exe\""));
    v.extend(text("StartDir", "\"C:\\Tools\\\""));
    v.extend(text("LaunchOptions", "--fast"));
    v.extend(number("IsHidden", 0));
    v.extend(number("LastPlayTime", 1_700_000_000));
    v.extend(b"\0tags\0");
    v.extend(text("0", "Favorites"));
    v.extend([8, 8, 8, 8]);
    v
}

struct Fake {
    dir: TempDir,
}

impl Fake {
    fn new(users: &[&str]) -> Fake {
        let dir = TempDir::new().unwrap();
        for u in users {
            fs::create_dir_all(dir.path().join("userdata").join(u).join("config")).unwrap();
        }
        Fake { dir }
    }
    fn place(&self) -> Place {
        Place {
            root: self.dir.path().to_path_buf(),
            flatpak: false,
        }
    }
    fn config(&self, user: &str) -> PathBuf {
        self.dir.path().join("userdata").join(user).join("config")
    }
    fn tree(&self, user: &str) -> Vec<(String, vdf::Node)> {
        let bytes = fs::read(self.config(user).join("shortcuts.vdf")).unwrap();
        vdf::parse(&bytes).ok().unwrap()
    }
}

fn launcher() -> Shortcut {
    Shortcut {
        name: "AnyPlay Launcher".into(),
        exe: PathBuf::from("C:/Games/AnyPlay/anyplay-launcher.exe"),
        arguments: String::new(),
        art: Art {
            portrait: Some(b"p".to_vec()),
            wide: Some(b"w".to_vec()),
            hero: Some(b"h".to_vec()),
            logo: Some(b"l".to_vec()),
            icon: Some(b"i".to_vec()),
        },
    }
}

fn game(title_id: &str, name: &str) -> Shortcut {
    Shortcut {
        name: name.into(),
        arguments: format!("anyps5://game/{title_id}"),
        art: Art::default(),
        ..launcher()
    }
}

fn shortcuts_of(tree: &[(String, vdf::Node)]) -> &[(String, vdf::Node)] {
    match &tree.iter().find(|(k, _)| k == "shortcuts").unwrap().1 {
        vdf::Node::Map(m) => m,
        _ => panic!("shortcuts is a map"),
    }
}

fn fields_of(node: &vdf::Node) -> &[(String, vdf::Node)] {
    match node {
        vdf::Node::Map(m) => m,
        _ => panic!("an entry is a map"),
    }
}

fn never_running(_: &Place) -> bool {
    false
}

#[test]
fn the_app_id_is_the_crc32_of_exe_and_name_with_the_high_bit() {
    // CRC-32 of "123456789" is the standard check value 0xCBF43926.
    assert_eq!(app_id("12345", "6789"), 0xCBF4_3926 | 0x8000_0000);
    assert!(app_id("\"a\"", "b") & 0x8000_0000 != 0);
    assert_ne!(app_id("\"a\"", "b"), app_id("\"a\"", "c"));
}

#[test]
fn a_file_survives_reading_and_writing_byte_for_byte() {
    let bytes = players_file();
    let tree = vdf::parse(&bytes).ok().unwrap();
    assert_eq!(vdf::write(&tree), bytes);
    let empty = b"\0shortcuts\0\x08\x08".to_vec();
    assert_eq!(vdf::write(&vdf::parse(&empty).ok().unwrap()), empty);
}

#[test]
fn damaged_files_are_refused() {
    let good = players_file();
    let trailing = [good.clone(), vec![0]].concat();
    for bad in [
        &good[..good.len() - 1],
        &trailing[..],
        b"\0shortcuts\0\x07oops\0",
        b"",
    ] {
        assert!(vdf::parse(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn the_launcher_lands_in_every_users_library_with_its_art() {
    let steam = Fake::new(&["0", "1234", "5678"]);
    let outcome = install_into(&[steam.place()], &[launcher()], never_running).unwrap();
    assert_eq!(
        outcome,
        Outcome {
            users: 2,
            added: 2,
            updated: 0
        }
    );
    assert!(
        !steam.config("0").join("shortcuts.vdf").exists(),
        "userdata/0 is no account"
    );
    let want = app_id(
        "\"C:/Games/AnyPlay/anyplay-launcher.exe\"",
        "AnyPlay Launcher",
    );
    for user in ["1234", "5678"] {
        let tree = steam.tree(user);
        let list = shortcuts_of(&tree);
        assert_eq!(list.len(), 1);
        let fields = fields_of(&list[0].1);
        assert_eq!(vdf::number_of(fields, "appid"), Some(want));
        assert_eq!(vdf::text_of(fields, "AppName"), Some("AnyPlay Launcher"));
        assert_eq!(
            vdf::text_of(fields, "Exe"),
            Some("\"C:/Games/AnyPlay/anyplay-launcher.exe\"")
        );
        assert_eq!(
            vdf::text_of(fields, "StartDir"),
            Some("\"C:/Games/AnyPlay\"")
        );
        let grid = steam.config(user).join("grid");
        for (name, bytes) in [
            (format!("{want}p.png"), "p"),
            (format!("{want}.png"), "w"),
            (format!("{want}_hero.png"), "h"),
            (format!("{want}_logo.png"), "l"),
            (format!("{want}_icon.png"), "i"),
        ] {
            assert_eq!(
                fs::read(grid.join(&name)).unwrap(),
                bytes.as_bytes(),
                "{name}"
            );
        }
        assert_eq!(
            vdf::text_of(fields, "icon").map(PathBuf::from),
            Some(grid.join(format!("{want}_icon.png")))
        );
    }
}

#[test]
fn pressing_twice_updates_and_never_duplicates() {
    let steam = Fake::new(&["1234"]);
    install_into(&[steam.place()], &[launcher()], never_running).unwrap();
    let again = install_into(&[steam.place()], &[launcher()], never_running).unwrap();
    assert_eq!((again.added, again.updated), (0, 1));
    assert_eq!(shortcuts_of(&steam.tree("1234")).len(), 1);

    // The player renamed the entry and the launcher moved (an update put it elsewhere): the
    // entry is still found, by its arguments and its exe or its name, and not added again.
    let mut tree = steam.tree("1234");
    let list = vdf::child_map(&mut tree, "shortcuts");
    let vdf::Node::Map(fields) = &mut list[0].1 else {
        panic!()
    };
    vdf::set_text(fields, "AppName", "My Launcher");
    fs::write(
        steam.config("1234").join("shortcuts.vdf"),
        vdf::write(&tree),
    )
    .unwrap();
    let mut other = launcher();
    other.exe = PathBuf::from("C:/Games/AnyPlay/Anyplay-Launcher.EXE");
    let third = install_into(&[steam.place()], &[other], never_running).unwrap();
    assert_eq!((third.added, third.updated), (0, 1));
    let tree = steam.tree("1234");
    let list = shortcuts_of(&tree);
    assert_eq!(list.len(), 1);
    let fields = fields_of(&list[0].1);
    assert_eq!(vdf::text_of(fields, "AppName"), Some("My Launcher"));
    assert_eq!(
        vdf::text_of(fields, "Exe"),
        Some("\"C:/Games/AnyPlay/Anyplay-Launcher.EXE\"")
    );
}

#[test]
fn games_get_their_own_entries_by_their_arguments() {
    let steam = Fake::new(&["1234"]);
    let all = [
        launcher(),
        game("PPSA00001", "First Game"),
        game("PPSA00002", "Second Game"),
    ];
    let outcome = install_into(&[steam.place()], &all, never_running).unwrap();
    assert_eq!(outcome.added, 3);
    // The same exe starts all three; only the arguments tell them apart.
    let outcome = install_into(&[steam.place()], &all[1..], never_running).unwrap();
    assert_eq!((outcome.added, outcome.updated), (0, 2));
    let tree = steam.tree("1234");
    let keys: Vec<&str> = shortcuts_of(&tree)
        .iter()
        .map(|(k, _)| k.as_str())
        .collect();
    assert_eq!(keys, ["0", "1", "2"]);
}

#[test]
fn the_players_own_entries_and_fields_are_kept_and_the_file_is_backed_up() {
    let steam = Fake::new(&["1234"]);
    let original = players_file();
    fs::write(steam.config("1234").join("shortcuts.vdf"), &original).unwrap();
    install_into(&[steam.place()], &[launcher()], never_running).unwrap();
    let tree = steam.tree("1234");
    let list = shortcuts_of(&tree);
    assert_eq!(list.len(), 2);
    let first = fields_of(&list[0].1);
    assert_eq!(vdf::number_of(first, "LastPlayTime"), Some(1_700_000_000));
    assert_eq!(vdf::number_of(first, "appid"), Some(0xDEAD_BEEF));
    assert_eq!(list[1].0, "1");
    let backup = steam.config("1234").join("shortcuts.vdf.anyplay-backup");
    assert_eq!(fs::read(&backup).unwrap(), original);
    // A second run does not replace the backup of the player's file.
    install_into(&[steam.place()], &[launcher()], never_running).unwrap();
    assert_eq!(fs::read(&backup).unwrap(), original);
}

#[test]
fn nothing_is_written_while_steam_runs() {
    let steam = Fake::new(&["1234"]);
    let result = install_into(&[steam.place()], &[launcher()], |_| true);
    assert!(matches!(result, Err(Error::SteamRunning)));
    assert!(!steam.config("1234").join("shortcuts.vdf").exists());
    assert!(!steam.config("1234").join("grid").exists());
}

#[test]
fn a_file_in_an_unknown_form_is_left_alone() {
    let steam = Fake::new(&["1234"]);
    let garbage = b"\0shortcuts\0\x07oops\0".to_vec();
    let file = steam.config("1234").join("shortcuts.vdf");
    fs::write(&file, &garbage).unwrap();
    let result = install_into(&[steam.place()], &[launcher()], never_running);
    assert!(matches!(result, Err(Error::Unreadable(_))));
    assert_eq!(fs::read(&file).unwrap(), garbage);
}

#[test]
fn no_steam_and_no_user_say_so() {
    assert!(matches!(
        install_into(&[], &[launcher()], never_running),
        Err(Error::NoSteam)
    ));
    let steam = Fake::new(&["0"]);
    assert!(matches!(
        install_into(&[steam.place()], &[launcher()], never_running),
        Err(Error::NoUsers)
    ));
}

#[test]
fn a_user_that_never_had_a_config_folder_gets_one() {
    let steam = Fake::new(&[]);
    fs::create_dir_all(steam.dir.path().join("userdata/42")).unwrap();
    install_into(&[steam.place()], &[launcher()], never_running).unwrap();
    assert_eq!(shortcuts_of(&steam.tree("42")).len(), 1);
}

#[test]
fn a_jpeg_left_by_a_removed_entry_does_not_hide_our_png() {
    let steam = Fake::new(&["1234"]);
    let id = app_id(
        "\"C:/Games/AnyPlay/anyplay-launcher.exe\"",
        "AnyPlay Launcher",
    );
    let grid = steam.config("1234").join("grid");
    fs::create_dir_all(&grid).unwrap();
    let jpg = grid.join(format!("{id}_hero.jpg"));
    fs::write(&jpg, b"x").unwrap();
    install_into(&[steam.place()], &[launcher()], never_running).unwrap();
    assert!(!jpg.exists());
    assert_eq!(fs::read(grid.join(format!("{id}_hero.png"))).unwrap(), b"h");
}

#[test]
fn pictures_the_player_chose_stay_when_the_entry_is_updated() {
    let steam = Fake::new(&["1234"]);
    install_into(&[steam.place()], &[launcher()], never_running).unwrap();
    let id = app_id(
        "\"C:/Games/AnyPlay/anyplay-launcher.exe\"",
        "AnyPlay Launcher",
    );
    let grid = steam.config("1234").join("grid");
    fs::write(grid.join(format!("{id}_hero.png")), b"mine").unwrap();
    fs::write(grid.join(format!("{id}p.jpg")), b"mine too").unwrap();
    fs::remove_file(grid.join(format!("{id}_logo.png"))).unwrap();
    install_into(&[steam.place()], &[launcher()], never_running).unwrap();
    assert_eq!(
        fs::read(grid.join(format!("{id}_hero.png"))).unwrap(),
        b"mine"
    );
    assert!(grid.join(format!("{id}p.jpg")).exists());
    assert_eq!(fs::read(grid.join(format!("{id}_logo.png"))).unwrap(), b"l");
}

#[test]
fn an_empty_file_counts_as_none() {
    let steam = Fake::new(&["1234"]);
    fs::write(steam.config("1234").join("shortcuts.vdf"), b"").unwrap();
    install_into(&[steam.place()], &[launcher()], never_running).unwrap();
    assert_eq!(shortcuts_of(&steam.tree("1234")).len(), 1);
}

#[test]
fn places_are_found_once_each() {
    let home = TempDir::new().unwrap();
    let real = home.path().join(".local/share/Steam");
    fs::create_dir_all(real.join("userdata")).unwrap();
    let flatpak = home
        .path()
        .join(".var/app/com.valvesoftware.Steam/.local/share/Steam");
    fs::create_dir_all(flatpak.join("userdata")).unwrap();
    let found = places(Some(home.path()), Some(real.clone()));
    assert_eq!(
        found,
        vec![
            Place {
                root: real,
                flatpak: false
            },
            Place {
                root: flatpak,
                flatpak: true
            }
        ]
    );
    assert!(places(Some(&home.path().join("nobody")), None).is_empty());
}

#[test]
fn a_game_entry_starts_the_launcher_with_a_link_the_launcher_understands() {
    let shortcut = game("PPSA01234", "A Game");
    assert!(crate::links::parse(&shortcut.arguments).is_ok());
}
