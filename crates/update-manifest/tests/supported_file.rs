//! The repository's `MIN_SUPPORTED_VERSION` file is read by the release and the weekly renewal. A typo
//! there would stop both (or, worse, be read as "off"), so it is checked on every pull request.

use anyplay_update_manifest::{Version, parse_supported_file};

#[test]
fn the_supported_version_file_is_readable_and_not_above_this_version() {
    let file = concat!(env!("CARGO_MANIFEST_DIR"), "/../../MIN_SUPPORTED_VERSION");
    let text =
        std::fs::read_to_string(file).expect("MIN_SUPPORTED_VERSION exists at the repository root");
    let parsed =
        parse_supported_file(&text).expect("MIN_SUPPORTED_VERSION is empty or one plain version");
    if let Some(minimum) = parsed {
        let this = Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
        assert!(
            Version::parse(&minimum).unwrap() <= this,
            "MIN_SUPPORTED_VERSION {minimum} is above the version of this source ({}): \
             raise it only after that release is out",
            env!("CARGO_PKG_VERSION")
        );
    }
}
