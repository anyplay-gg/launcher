//! The Noto substitutes for the console's system fonts, embedded in the binary
//! (`assets/fonts/noto/`, SIL OFL 1.1; sources and checksums in its README). The logic that
//! writes them out and fetches the CJK set is `anyps5_core::fonts`.

use anyps5_core::fonts::Bundle;

/// Bump the version whenever a file in `assets/fonts/noto/` changes: it names the folder in the
/// data directory.
pub static BUNDLE: Bundle<'static> = Bundle {
    version: "1",
    files: &[
        (
            "NotoSans-Light.ttf",
            include_bytes!("../assets/fonts/noto/NotoSans-Light.ttf"),
        ),
        (
            "NotoSans-Regular.ttf",
            include_bytes!("../assets/fonts/noto/NotoSans-Regular.ttf"),
        ),
        (
            "NotoSans-Medium.ttf",
            include_bytes!("../assets/fonts/noto/NotoSans-Medium.ttf"),
        ),
        (
            "NotoSans-Bold.ttf",
            include_bytes!("../assets/fonts/noto/NotoSans-Bold.ttf"),
        ),
        (
            "NotoSans-LightItalic.ttf",
            include_bytes!("../assets/fonts/noto/NotoSans-LightItalic.ttf"),
        ),
        (
            "NotoSans-Italic.ttf",
            include_bytes!("../assets/fonts/noto/NotoSans-Italic.ttf"),
        ),
        (
            "NotoSans-MediumItalic.ttf",
            include_bytes!("../assets/fonts/noto/NotoSans-MediumItalic.ttf"),
        ),
        (
            "NotoSans-BoldItalic.ttf",
            include_bytes!("../assets/fonts/noto/NotoSans-BoldItalic.ttf"),
        ),
        (
            "NotoSansMono-Light.ttf",
            include_bytes!("../assets/fonts/noto/NotoSansMono-Light.ttf"),
        ),
        (
            "NotoSansMono-Regular.ttf",
            include_bytes!("../assets/fonts/noto/NotoSansMono-Regular.ttf"),
        ),
        (
            "NotoSansMono-Medium.ttf",
            include_bytes!("../assets/fonts/noto/NotoSansMono-Medium.ttf"),
        ),
        (
            "NotoSansMono-Bold.ttf",
            include_bytes!("../assets/fonts/noto/NotoSansMono-Bold.ttf"),
        ),
        (
            "NotoSansThai-Light.ttf",
            include_bytes!("../assets/fonts/noto/NotoSansThai-Light.ttf"),
        ),
        (
            "NotoSansThai-Regular.ttf",
            include_bytes!("../assets/fonts/noto/NotoSansThai-Regular.ttf"),
        ),
        (
            "NotoSansThai-Medium.ttf",
            include_bytes!("../assets/fonts/noto/NotoSansThai-Medium.ttf"),
        ),
        (
            "NotoSansThai-Bold.ttf",
            include_bytes!("../assets/fonts/noto/NotoSansThai-Bold.ttf"),
        ),
        (
            "NotoSansArabic-Light.ttf",
            include_bytes!("../assets/fonts/noto/NotoSansArabic-Light.ttf"),
        ),
        (
            "NotoSansArabic-Regular.ttf",
            include_bytes!("../assets/fonts/noto/NotoSansArabic-Regular.ttf"),
        ),
        (
            "NotoSansArabic-Medium.ttf",
            include_bytes!("../assets/fonts/noto/NotoSansArabic-Medium.ttf"),
        ),
        (
            "NotoSansArabic-Bold.ttf",
            include_bytes!("../assets/fonts/noto/NotoSansArabic-Bold.ttf"),
        ),
        ("OFL.txt", include_bytes!("../assets/fonts/noto/OFL.txt")),
    ],
};

#[cfg(test)]
mod tests {
    use super::*;
    use anyps5_core::fonts::{BUNDLED_FILES, LICENCE_FILE};

    #[test]
    fn the_bundle_holds_every_file_the_engine_reads_and_the_licence() {
        for name in BUNDLED_FILES.iter().chain([&LICENCE_FILE]) {
            let (_, data) = BUNDLE
                .files
                .iter()
                .find(|(n, _)| n == name)
                .unwrap_or_else(|| panic!("{name} is not embedded"));
            assert!(data.len() > 1000, "{name} is empty");
        }
        assert_eq!(BUNDLE.files.len(), BUNDLED_FILES.len() + 1);
    }

    #[test]
    fn the_fonts_are_truetype() {
        for (name, data) in BUNDLE.files.iter().filter(|(n, _)| n.ends_with(".ttf")) {
            assert_eq!(&data[..4], &[0, 1, 0, 0], "{name} is not a TrueType file");
        }
    }
}
