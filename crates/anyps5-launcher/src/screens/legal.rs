//! The two pages a player agrees to by creating an account (docs/DESIGN.md 17): the Terms of
//! Service and the Privacy Policy. The sentence that names them is laid out here for the places
//! that have no row list (the onboarding's online step); the Friends screen shows them as rows.

use egui::{
    Align2, Color32, CursorIcon, Painter, Pos2, Rect, Role, Sense, Stroke, Ui, WidgetInfo, pos2,
    vec2,
};

use crate::screens::about_strings::{AboutText, PRIVACY, TERMS};
use crate::ui::i18n::Language;
use crate::ui::text;
use crate::ui::theme::{color, ty};

/// One of the two pages. The launcher opens no other address for them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Legal {
    Terms,
    Privacy,
}

impl Legal {
    pub const fn url(self) -> &'static str {
        match self {
            Legal::Terms => TERMS,
            Legal::Privacy => PRIVACY,
        }
    }

    pub const fn name(self, language: Language) -> &'static str {
        match self {
            Legal::Terms => AboutText::Terms.text(language),
            Legal::Privacy => AboutText::Privacy.text(language),
        }
    }

    /// The sentence as plain text, the names where the links stand: for a list of rows.
    pub fn sentence(language: Language) -> String {
        AboutText::Consent
            .text(language)
            .replace("{terms}", Legal::Terms.name(language))
            .replace("{privacy}", Legal::Privacy.name(language))
    }

    /// Opens the page in the system browser.
    pub fn open(self) {
        if let Err(error) = anyps5_core::platform::open_url(self.url()) {
            tracing::warn!("cannot open {}: {error}", self.url());
        }
    }
}

/// A word of the sentence, and the page it belongs to when it is part of a link's name.
struct Word {
    text: String,
    /// What touches a link's name without a space (a full stop): beside it, not part of it.
    tail: String,
    page: Option<Legal>,
}

/// The sentence in words: the template's text as it is, the placeholders replaced by the pages'
/// names. A name of several words is several words of the same link, and what touches the name
/// without a space (a suffix, a full stop) is the last word's tail.
fn words(language: Language) -> Vec<Word> {
    let mut out = Vec::new();
    let mut rest = AboutText::Consent.text(language);
    while !rest.is_empty() {
        let next = [("{terms}", Legal::Terms), ("{privacy}", Legal::Privacy)]
            .into_iter()
            .filter_map(|(tag, page)| rest.find(tag).map(|at| (at, tag, page)))
            .min_by_key(|(at, _, _)| *at);
        let (plain, link) = match next {
            Some((at, tag, page)) => (&rest[..at], Some((tag, page))),
            None => (rest, None),
        };
        out.extend(plain.split_whitespace().map(|w| Word {
            text: w.to_owned(),
            tail: String::new(),
            page: None,
        }));
        let Some((tag, page)) = link else { break };
        rest = &rest[plain.len() + tag.len()..];
        let mut parts = page.name(language).split_whitespace().peekable();
        while let Some(part) = parts.next() {
            let mut tail = String::new();
            if parts.peek().is_none() {
                tail = rest
                    .chars()
                    .take_while(|c| !c.is_whitespace() && *c != '{')
                    .collect();
                rest = &rest[tail.len()..];
            }
            out.push(Word {
                text: part.to_owned(),
                tail,
                page: Some(page),
            });
        }
    }
    out
}

/// Draws "By creating an account you agree to the Terms of Service and the Privacy Policy." with
/// the two names as links, wrapped at `width` from `origin`. Returns the page that was clicked and
/// the height the sentence takes.
pub fn consent_line(
    ui: &mut Ui,
    origin: Pos2,
    width: f32,
    language: Language,
    active: bool,
) -> (Option<Legal>, f32) {
    let painter = ui.painter().clone();
    let style = ty::DETAIL;
    let line = style.line_height();
    let space = text::width(&painter, style, " ") + 1.5;
    let mut clicked = None;
    let (mut x, mut y) = (0.0, 0.0);
    let words = words(language);
    for (i, word) in words.iter().enumerate() {
        let w = text::width(&painter, style, &word.text);
        let tail = if word.tail.is_empty() {
            0.0
        } else {
            text::width(&painter, style, &word.tail)
        };
        if x > 0.0 && x + w + tail > width {
            x = 0.0;
            y += line;
        }
        let at = pos2(origin.x + x, origin.y + y);
        match word.page {
            None => paint_word(&painter, at, &word.text, color::text_detail(), None),
            Some(page) => {
                // The gap before the next word of the same link is underlined too.
                let joined = words.get(i + 1).is_some_and(|n| n.page == Some(page));
                let rect = Rect::from_min_size(at, vec2(w, line));
                let sense = if active {
                    Sense::click()
                } else {
                    Sense::hover()
                };
                let response = ui.interact(rect, ui.scope_id().with(("legal", i)), sense);
                response.widget_info(|| {
                    WidgetInfo::labeled(Role::Link, true, page.name(language).to_owned())
                });
                let hot = active && response.hovered();
                if hot {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }
                let ink = if hot {
                    color::text()
                } else {
                    color::text_label()
                };
                let underline = w + if joined { space } else { 0.0 };
                paint_word(&painter, at, &word.text, ink, Some(underline));
                if tail > 0.0 {
                    let after = pos2(at.x + w, at.y);
                    paint_word(&painter, after, &word.tail, color::text_detail(), None);
                }
                if active && response.clicked() {
                    clicked = Some(page);
                }
            }
        }
        x += w + tail + space;
    }
    (clicked, y + line)
}

fn paint_word(painter: &Painter, at: Pos2, word: &str, ink: Color32, underline: Option<f32>) {
    let rect = text::paint(painter, at, Align2::LEFT_TOP, ty::DETAIL, word, ink);
    if let Some(length) = underline {
        let y = rect.max.y - 2.0;
        painter.line_segment(
            [pos2(rect.min.x, y), pos2(rect.min.x + length, y)],
            Stroke::new(1.0, ink),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sentence_names_both_pages_in_both_languages() {
        for language in Language::ALL {
            let sentence = words(language);
            for page in [Legal::Terms, Legal::Privacy] {
                let linked = sentence.iter().filter(|w| w.page == Some(page)).count();
                let name = page.name(language).split_whitespace().count();
                assert_eq!(linked, name, "{page:?} {language:?}");
            }
        }
    }

    #[test]
    fn the_english_sentence_reads_as_written() {
        let line: Vec<_> = words(Language::English)
            .into_iter()
            .map(|w| format!("{}{}", w.text, w.tail))
            .collect();
        assert_eq!(
            line.join(" "),
            "By creating an account you agree to the Terms of Service and the Privacy Policy."
        );
    }

    #[test]
    fn the_turkish_sentence_keeps_what_touches_a_name() {
        let line: Vec<_> = words(Language::Turkish)
            .into_iter()
            .map(|w| format!("{}{}", w.text, w.tail))
            .collect();
        assert_eq!(
            line.join(" "),
            "Hesap oluşturarak Hizmet Şartları ve Gizlilik Politikası metinlerini kabul edersin."
        );
    }

    #[test]
    fn the_pages_are_the_about_pages() {
        assert_eq!(Legal::Terms.url(), "https://anyplaynet.com/terms");
        assert_eq!(Legal::Privacy.url(), "https://anyplaynet.com/privacy");
    }
}
