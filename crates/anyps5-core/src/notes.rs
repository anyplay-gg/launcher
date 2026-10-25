//! Release notes of a toolkit release: the `release-notes.md` asset parsed into changes.
//!
//! Upstream's notes are GitHub's generated list: one bullet per merged pull request,
//! `* <type>(<scope>): <subject> by @<user> in <pull request URL>`. A bullet that does not have
//! that shape is kept as a plain line, so a change of format loses nothing. [`fetch`] downloads
//! the asset once per release and digest and keeps it on disk.

use std::sync::OnceLock;
use std::time::Duration;

use regex::Regex;

use crate::feed::{self, Request};
use crate::paths::AppPaths;
use crate::toolkit::{Asset, ReleaseInfo, ToolkitError};

/// The repository the pull request numbers belong to.
const PULL_URL: &str = "https://github.com/boykopovar/AnyPS5/pull";
/// `release-notes.md` of v0.1.3 is 230 KB; a few times that is still sane.
const MAX_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// `feat`, `fix`, `perf`, ...; empty for a line that did not parse.
    pub kind: String,
    /// The area (`agc`, `relinker`, `libSceXxx`).
    pub scope: Option<String>,
    pub subject: String,
    pub pull: Option<u64>,
}

impl Change {
    pub fn url(&self) -> Option<String> {
        self.pull.map(|n| format!("{PULL_URL}/{n}"))
    }

    /// A new feature or a speed-up: what the highlights list.
    pub fn is_highlight(&self) -> bool {
        self.kind == "feat" || self.kind == "perf"
    }
}

/// The highlights of one area.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group<'a> {
    pub scope: &'a str,
    pub changes: Vec<&'a Change>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReleaseNotes {
    pub changes: Vec<Change>,
}

/// The most lines of a launcher's notes kept.
pub const LAUNCHER_NOTES_MAX: usize = 60;

/// Text of one Markdown line without its marks: bold, italics, code and `[text](address)` links
/// (the text stays).
fn plain(line: &str) -> String {
    static LINK: OnceLock<Regex> = OnceLock::new();
    let link = LINK.get_or_init(|| Regex::new(r"\[([^\]]*)\]\([^)]*\)").expect("link pattern"));
    let text = link.replace_all(line, "$1");
    let text: String = text
        .replace("**", "")
        .replace("__", "")
        .chars()
        .filter(|c| *c != '`')
        .collect();
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn bullet() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^\* (?P<type>[a-z]+)(?:\((?P<scope>[^)]+)\))?!?: (?P<subject>.+?) by @\S+ in https://github\.com/boykopovar/AnyPS5/pull/(?P<pr>\d+)$",
        )
        .expect("bullet pattern")
    })
}

impl ReleaseNotes {
    /// Reads the "What's Changed" section. Other sections (new contributors, the changelog link)
    /// are not changes.
    pub fn parse(text: &str) -> ReleaseNotes {
        let mut changes = Vec::new();
        let mut in_changes = false;
        for line in text.lines() {
            let line = line.trim_end();
            if let Some(heading) = line.strip_prefix("## ") {
                in_changes = heading.trim().eq_ignore_ascii_case("What's Changed");
                continue;
            }
            if !in_changes || !line.starts_with("* ") {
                continue;
            }
            changes.push(match bullet().captures(line) {
                Some(c) => Change {
                    kind: c["type"].to_owned(),
                    scope: c.name("scope").map(|m| m.as_str().to_owned()),
                    subject: c["subject"].to_owned(),
                    pull: c["pr"].parse().ok(),
                },
                None => Change {
                    kind: String::new(),
                    scope: None,
                    subject: line[2..].trim().to_owned(),
                    pull: None,
                },
            });
        }
        ReleaseNotes { changes }
    }

    /// `feat` and `perf` changes by area, in the order the areas first appear.
    pub fn highlights_in_order(&self) -> Vec<Group<'_>> {
        let mut groups: Vec<Group<'_>> = Vec::new();
        for change in self.changes.iter().filter(|c| c.is_highlight()) {
            let scope = change.scope.as_deref().unwrap_or("general");
            match groups.iter_mut().find(|g| g.scope == scope) {
                Some(group) => group.changes.push(change),
                None => groups.push(Group {
                    scope,
                    changes: vec![change],
                }),
            }
        }
        groups
    }

    /// `feat` and `perf` changes by area, the busiest area first.
    pub fn highlights(&self) -> Vec<Group<'_>> {
        let mut groups = self.highlights_in_order();
        groups.sort_by(|a, b| {
            b.changes
                .len()
                .cmp(&a.changes.len())
                .then(a.scope.cmp(b.scope))
        });
        groups
    }

    /// Whether the release changes how games are converted: some change is scoped `relinker...`.
    pub fn changes_conversion(&self) -> bool {
        self.changes.iter().any(|c| {
            c.scope
                .as_deref()
                .is_some_and(|s| s.starts_with("relinker"))
        })
    }

    /// The notes of a launcher release, written by hand: `###` headings with `-` bullets. Every
    /// bullet becomes a highlight under its heading, so the "What's New" panel can show them. A
    /// heading that only names the release, a changelog link and empty lines are skipped; Markdown
    /// marks (bold, code, links) are removed. At most [`LAUNCHER_NOTES_MAX`] lines are kept.
    pub fn parse_launcher(text: &str) -> ReleaseNotes {
        let mut changes = Vec::new();
        let mut scope: Option<String> = None;
        for line in text.lines() {
            let line = line.trim();
            if changes.len() >= LAUNCHER_NOTES_MAX {
                break;
            }
            if line.is_empty() || line.starts_with("---") {
                continue;
            }
            if line.starts_with('#') {
                let title = plain(line.trim_start_matches('#'));
                let release_title = title.to_ascii_lowercase().contains("launcher")
                    || title.eq_ignore_ascii_case("what's changed");
                scope = (!title.is_empty() && !release_title).then_some(title);
                continue;
            }
            let item = line
                .strip_prefix("- ")
                .or_else(|| line.strip_prefix("* "))
                .unwrap_or(line);
            let subject = plain(item);
            let link_only = subject.starts_with("http") && !subject.contains(' ');
            if subject.is_empty()
                || link_only
                || subject.to_ascii_lowercase().contains("full changelog")
            {
                continue;
            }
            changes.push(Change {
                kind: "feat".into(),
                scope: Some(scope.clone().unwrap_or_else(|| "Changes".into())),
                subject: subject.chars().take(280).collect(),
                pull: None,
            });
        }
        ReleaseNotes { changes }
    }
}

/// The notes of `release`, downloaded once per tag and digest and then read from disk. `None`
/// when the release has no notes asset. Blocking: call from a worker thread.
pub fn fetch(
    paths: &AppPaths,
    release: &ReleaseInfo,
) -> Result<Option<ReleaseNotes>, ToolkitError> {
    let Some(asset) = &release.notes else {
        return Ok(None);
    };
    fetch_asset(paths, release.plain_tag(), asset).map(Some)
}

pub(crate) fn fetch_asset(
    paths: &AppPaths,
    tag: &str,
    asset: &Asset,
) -> Result<ReleaseNotes, ToolkitError> {
    // The file name is checked data only: a tag is dotted numbers, a digest is hex.
    let safe = |s: &str| s.chars().all(|c| c.is_ascii_alphanumeric() || c == '.');
    let digest = asset.sha256.as_deref().unwrap_or("none");
    if !safe(tag) || !safe(digest) {
        return Err(ToolkitError::Network("unusable release data".into()));
    }
    let document = feed::fetch(
        &paths.cache_dir().join("Feeds"),
        &Request {
            url: &asset.url,
            name: &format!("notes-{tag}-{}.md", &digest[..digest.len().min(16)]),
            accept: "text/markdown, text/plain",
            // The asset of a published release does not change.
            max_age: Duration::from_secs(365 * 86_400),
            max_bytes: MAX_BYTES,
        },
    )?;
    Ok(ReleaseNotes::parse(&document.body))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "## What's Changed\n\
* feat(agc): add mesh shader path by @a in https://github.com/boykopovar/AnyPS5/pull/101\n\
* fix(relinker)!: keep TLS alignment by @b in https://github.com/boykopovar/AnyPS5/pull/102\n\
* perf(shader): skip dead stores by @c in https://github.com/boykopovar/AnyPS5/pull/103\n\
* feat(agc): second agc change by @a in https://github.com/boykopovar/AnyPS5/pull/104\n\
* feat: no scope by @d in https://github.com/boykopovar/AnyPS5/pull/105\n\
* A hand-written line without the usual shape\n\
\n\
## New Contributors\n\
* @zed made their first contribution in https://github.com/boykopovar/AnyPS5/pull/106\n\
\n\
**Full Changelog**: https://github.com/boykopovar/AnyPS5/compare/v0.1.2...v0.1.3\n";

    #[test]
    fn bullets_become_changes() {
        let notes = ReleaseNotes::parse(SAMPLE);
        assert_eq!(notes.changes.len(), 6, "contributors are not changes");
        let first = &notes.changes[0];
        assert_eq!(
            (first.kind.as_str(), first.scope.as_deref(), first.pull),
            ("feat", Some("agc"), Some(101))
        );
        assert_eq!(first.subject, "add mesh shader path");
        assert_eq!(
            first.url().as_deref(),
            Some("https://github.com/boykopovar/AnyPS5/pull/101")
        );
        assert_eq!(notes.changes[1].kind, "fix", "a breaking mark is allowed");
    }

    #[test]
    fn an_unparsed_line_is_kept_as_text() {
        let raw = ReleaseNotes::parse(SAMPLE).changes.pop().unwrap();
        assert_eq!(raw.kind, "");
        assert_eq!(raw.pull, None);
        assert_eq!(raw.subject, "A hand-written line without the usual shape");
    }

    #[test]
    fn highlights_group_features_and_speedups_by_area() {
        let notes = ReleaseNotes::parse(SAMPLE);
        let groups = notes.highlights();
        let names: Vec<_> = groups.iter().map(|g| (g.scope, g.changes.len())).collect();
        assert_eq!(names, [("agc", 2), ("general", 1), ("shader", 1)]);
    }

    #[test]
    fn a_relinker_scope_means_conversion_changed() {
        assert!(ReleaseNotes::parse(SAMPLE).changes_conversion());
        let only_agc = "## What's Changed\n* feat(agc): x by @a in https://github.com/boykopovar/AnyPS5/pull/1\n";
        assert!(!ReleaseNotes::parse(only_agc).changes_conversion());
    }

    #[test]
    fn a_large_release_parses_quickly() {
        let line = "* feat(libc): something by @a in https://github.com/boykopovar/AnyPS5/pull/7\n";
        let text = format!("## What's Changed\n{}", line.repeat(2_000));
        let started = std::time::Instant::now();
        assert_eq!(ReleaseNotes::parse(&text).changes.len(), 2_000);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn a_launcher_release_reads_as_highlights_under_its_headings() {
        let text = "## AnyPlay Launcher 0.2.4\n\n### Your Profile\n\n- Your **profile** button opens a [full-screen profile](https://x.test/p).\n- Edit `Profile` opens your account.\n\n### Calls\n\n* A call uses Voice Activity.\n\n**Full Changelog**: https://github.com/anyplaynet/launcher/compare/v0.2.3...v0.2.4\n";
        let notes = ReleaseNotes::parse_launcher(text);
        let seen: Vec<_> = notes
            .changes
            .iter()
            .map(|c| (c.scope.as_deref().unwrap(), c.subject.as_str()))
            .collect();
        assert_eq!(
            seen,
            [
                (
                    "Your Profile",
                    "Your profile button opens a full-screen profile."
                ),
                ("Your Profile", "Edit Profile opens your account."),
                ("Calls", "A call uses Voice Activity."),
            ]
        );
        assert!(
            notes
                .changes
                .iter()
                .all(|c| c.is_highlight() && c.pull.is_none())
        );
        assert_eq!(notes.highlights().len(), 2);
        let order: Vec<_> = notes
            .highlights_in_order()
            .iter()
            .map(|g| g.scope)
            .collect();
        assert_eq!(order, ["Your Profile", "Calls"]);
    }

    #[test]
    fn launcher_notes_without_headings_and_a_huge_body_stay_bounded() {
        let plain = ReleaseNotes::parse_launcher("Fixes the update check.\n\nSmaller window.");
        assert_eq!(plain.changes.len(), 2);
        assert_eq!(plain.changes[0].scope.as_deref(), Some("Changes"));
        let many = "- line\n".repeat(500);
        assert_eq!(
            ReleaseNotes::parse_launcher(&many).changes.len(),
            LAUNCHER_NOTES_MAX
        );
        assert!(ReleaseNotes::parse_launcher("").changes.is_empty());
    }
}
