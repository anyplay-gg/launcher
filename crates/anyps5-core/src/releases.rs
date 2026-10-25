//! What the toolkit's repository has published: the newest tag, the last releases to pin one of,
//! and whether the releases between two tags changed how games are converted.
//!
//! The newest tag comes from the repository's Pages file `release.json` (no API quota), with the
//! GitHub API as the fallback. Both answer conditional requests, so an unchanged answer costs
//! one small reply. Nothing is installed here; the caller decides.

use std::cmp::Ordering;
use std::time::Duration;

use serde::Deserialize;

use crate::compat::release_tag;
use crate::feed::{self, Request};
use crate::notes::{self, ReleaseNotes};
use crate::paths::AppPaths;
use crate::platform::Os;
use crate::toolkit::{
    API_BASE, ReleaseInfo, ToolkitError, compare_tags, parse_release, parse_releases,
};

/// How many releases the "pin a version" choice offers.
pub const RECENT: usize = 10;

/// Where to ask. [`Endpoints::github`] is the real thing; tests point both at a local server.
#[derive(Clone, Debug)]
pub struct Endpoints {
    api: String,
    /// The Pages site of the repository; only the default repository's is trusted.
    pages: Option<String>,
}

impl Endpoints {
    pub fn github(repo: &str) -> Endpoints {
        let pages =
            (repo == "boykopovar/AnyPS5").then(|| "https://boykopovar.github.io/AnyPS5".to_owned());
        Endpoints {
            api: API_BASE.to_owned(),
            pages,
        }
    }

    #[cfg(test)]
    fn local(base: &str, pages: bool) -> Endpoints {
        Endpoints {
            api: base.to_owned(),
            pages: pages.then(|| format!("{base}/pages")),
        }
    }
}

#[derive(Deserialize)]
struct PagesRelease {
    tag: String,
    platforms: Option<PagesPlatforms>,
}

#[derive(Deserialize)]
struct PagesPlatforms {
    windows: Option<PagesAsset>,
    linux: Option<PagesAsset>,
    macos: Option<PagesAsset>,
}

#[derive(Deserialize)]
struct PagesAsset {
    sha256: Option<String>,
}

fn feeds(paths: &AppPaths) -> std::path::PathBuf {
    paths.cache_dir().join("Feeds")
}

fn pages_release(paths: &AppPaths, endpoints: &Endpoints) -> Result<PagesRelease, ToolkitError> {
    let base = endpoints.pages.as_deref().ok_or(ToolkitError::NoRelease)?;
    let document = feed::fetch(
        &feeds(paths),
        &Request {
            url: &format!("{base}/release.json"),
            name: "release.json",
            accept: "application/json",
            max_age: Duration::ZERO,
            max_bytes: 64 * 1024,
        },
    )?;
    serde_json::from_str(&document.body)
        .map_err(|e| ToolkitError::Network(format!("bad release data: {e}")))
}

fn api_json(
    paths: &AppPaths,
    endpoints: &Endpoints,
    repo: &str,
    path: &str,
    name: &str,
    max_age: Duration,
) -> Result<String, ToolkitError> {
    feed::fetch(
        &feeds(paths),
        &Request {
            url: &format!("{}/repos/{repo}/{path}", endpoints.api),
            name,
            accept: "application/vnd.github+json",
            max_age,
            max_bytes: 4 * 1024 * 1024,
        },
    )
    .map(|d| d.body)
}

/// The newest release tag, without a leading `v`.
pub fn newest_tag(
    paths: &AppPaths,
    endpoints: &Endpoints,
    repo: &str,
) -> Result<String, ToolkitError> {
    if let Ok(release) = pages_release(paths, endpoints) {
        return Ok(release.tag.trim_start_matches('v').to_owned());
    }
    let json = api_json(
        paths,
        endpoints,
        repo,
        "releases/latest",
        "latest.json",
        Duration::ZERO,
    )?;
    Ok(parse_release(&json)?.plain_tag().to_owned())
}

/// Whether the Pages file and the API disagree about the libraries of `release`. The API is the
/// release itself and wins; a mismatch is only worth a log line.
pub fn libs_digest_disagrees(
    paths: &AppPaths,
    endpoints: &Endpoints,
    release: &ReleaseInfo,
) -> bool {
    let Ok(pages) = pages_release(paths, endpoints) else {
        return false;
    };
    let from_pages = pages
        .platforms
        .and_then(|p| match Os::current() {
            Os::Windows => p.windows,
            Os::Linux => p.linux,
            Os::Macos => p.macos,
        })
        .and_then(|asset| asset.sha256);
    pages.tag.trim_start_matches('v') == release.plain_tag()
        && matches!((&from_pages, &release.libs.sha256), (Some(a), Some(b)) if !a.eq_ignore_ascii_case(b))
}

/// How long a stored list of releases counts as current without asking.
const LIST_AGE: Duration = Duration::from_secs(6 * 3600);

/// The last [`RECENT`] releases that have a Windows toolkit, newest first. `max_age` is how old
/// the stored list may be before the server is asked (with the ETag) again.
pub fn recent(
    paths: &AppPaths,
    endpoints: &Endpoints,
    repo: &str,
    max_age: Duration,
) -> Result<Vec<ReleaseInfo>, ToolkitError> {
    let json = api_json(
        paths,
        endpoints,
        repo,
        &format!("releases?per_page={RECENT}"),
        "recent.json",
        max_age,
    )?;
    parse_releases(&json)
}

/// The release `tag` (with or without `v`), from the last ones or, for an older one, asked for by
/// name. Only the configured repository is asked.
pub fn by_tag(
    paths: &AppPaths,
    endpoints: &Endpoints,
    repo: &str,
    tag: &str,
) -> Result<ReleaseInfo, ToolkitError> {
    let wanted = tag.trim().trim_start_matches('v');
    if let Some(found) = recent(paths, endpoints, repo, Duration::ZERO)?
        .into_iter()
        .find(|r| r.plain_tag() == wanted)
    {
        return Ok(found);
    }
    // Only a dotted-number tag is put into the address.
    if crate::toolkit::pin_tag(wanted).as_deref() != Some(wanted) {
        return Err(ToolkitError::NoRelease);
    }
    let json = api_json(
        paths,
        endpoints,
        repo,
        &format!("releases/tags/v{wanted}"),
        &format!("tag-{wanted}.json"),
        Duration::ZERO,
    )?;
    let release = parse_release(&json)?;
    if release.plain_tag() == wanted {
        Ok(release)
    } else {
        Err(ToolkitError::NoRelease)
    }
}

/// One release's notes, as the "What's new" panel shows them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Published {
    /// Without a leading `v`.
    pub tag: String,
    /// `YYYY-MM-DD`, empty if unknown.
    pub date: String,
    pub notes: ReleaseNotes,
}

/// The notes of release `tag` (among the last [`RECENT`]); `None` if it is not listed or has no
/// notes. Asks the server, so a release published minutes ago is found.
pub fn published_notes(
    paths: &AppPaths,
    endpoints: &Endpoints,
    repo: &str,
    tag: &str,
) -> Result<Option<Published>, ToolkitError> {
    let wanted = tag.trim_start_matches('v');
    let Some(release) = recent(paths, endpoints, repo, Duration::ZERO)?
        .into_iter()
        .find(|r| r.plain_tag() == wanted)
    else {
        return Ok(None);
    };
    Ok(notes::fetch(paths, &release)?.map(|notes| Published {
        tag: wanted.to_owned(),
        date: release.published.chars().take(10).collect(),
        notes,
    }))
}

/// Whether a release after `from` up to and including `to` changed how games are converted
/// (a change scoped `relinker`). `Ok(false)` also covers releases whose notes could not be had
/// from the network or cache: the caller then gives the softer hint.
pub fn conversion_changed(
    paths: &AppPaths,
    endpoints: &Endpoints,
    repo: &str,
    from: &str,
    to: &str,
) -> Result<bool, ToolkitError> {
    let (Some(from), Some(to)) = (release_tag(from), release_tag(to)) else {
        return Ok(false);
    };
    for release in recent(paths, endpoints, repo, LIST_AGE)? {
        let tag = release.plain_tag();
        let between = compare_tags(tag, &from) == Some(Ordering::Greater)
            && compare_tags(tag, &to).is_some_and(Ordering::is_le);
        if between
            && let Ok(Some(notes)) = notes::fetch(paths, &release)
            && notes.changes_conversion()
        {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// Serves the routes (path to body) that `routes` builds from the server's own base URL;
    /// unknown paths answer 404.
    fn serve(
        routes: impl FnOnce(&str) -> Vec<(&'static str, String)>,
    ) -> (String, Arc<Mutex<Vec<String>>>) {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("bind");
        let base = format!("http://{}", server.server_addr());
        let routes = routes(&base);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            for request in server.incoming_requests() {
                let url = request.url().to_owned();
                log.lock().unwrap().push(url.clone());
                let path = url.split('?').next().unwrap_or("");
                let response = match routes.iter().find(|(p, _)| *p == path) {
                    Some((_, body)) => tiny_http::Response::from_string(body.clone()),
                    None => tiny_http::Response::from_string("").with_status_code(404),
                };
                let _ = request.respond(response);
            }
        });
        (base, seen)
    }

    fn release_json(tag: &str, base: &str) -> String {
        format!(
            r#"{{"tag_name":"{tag}","published_at":"2026-10-20T10:00:00Z","html_url":"",
            "assets":[
              {{"name":"{}","browser_download_url":"{base}/r","size":1}},
              {{"name":"{}","browser_download_url":"{base}/p","size":1,"digest":"sha256:{}"}},
              {{"name":"release-notes.md","browser_download_url":"{base}/notes/{tag}","size":1,"digest":"sha256:{}"}}]}}"#,
            Os::current()
                .relinker_asset(tag)
                .unwrap_or_else(|| "unrelated-platform-asset".into()),
            Os::current().libs_asset(tag),
            "a".repeat(64),
            "b".repeat(64)
        )
    }

    fn paths() -> (tempfile::TempDir, AppPaths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(dir.path());
        (dir, paths)
    }

    #[test]
    fn the_pages_file_answers_without_the_api() {
        let (_dir, paths) = paths();
        let (base, seen) = serve(|_| vec![("/pages/release.json", r#"{"tag":"v0.1.4"}"#.into())]);
        let tag = newest_tag(&paths, &Endpoints::local(&base, true), "x/y").unwrap();
        assert_eq!(tag, "0.1.4");
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn the_api_answers_when_the_pages_file_does_not() {
        let (_dir, paths) = paths();
        let routes = vec![(
            "/repos/x/y/releases/latest",
            release_json("v0.1.5", "http://h"),
        )];
        let (base, _) = serve(|_| routes);
        let tag = newest_tag(&paths, &Endpoints::local(&base, true), "x/y").unwrap();
        assert_eq!(tag, "0.1.5", "the missing Pages file falls back to the API");
        let other = newest_tag(&paths, &Endpoints::local(&base, false), "x/y").unwrap();
        assert_eq!(
            other, "0.1.5",
            "a repository without trusted Pages uses the API"
        );
    }

    #[test]
    fn a_digest_mismatch_between_pages_and_api_is_seen() {
        let (_dir, paths) = paths();
        let json = format!(
            r#"{{"tag":"v0.1.4","platforms":{{"{}":{{"sha256":"ffff"}}}}}}"#,
            Os::current().platform_key()
        );
        let (base, _) = serve(|_| vec![("/pages/release.json", json.clone())]);
        let release = parse_release(&release_json("v0.1.4", "http://h")).unwrap();
        assert!(libs_digest_disagrees(
            &paths,
            &Endpoints::local(&base, true),
            &release
        ));
        let mut same = release.clone();
        same.libs.sha256 = Some("ffff".into());
        assert!(!libs_digest_disagrees(
            &paths,
            &Endpoints::local(&base, true),
            &same
        ));
    }

    #[test]
    fn the_relinker_scope_between_two_tags_is_found() {
        let (_dir, paths) = paths();
        let line = |scope: &str| {
            format!(
                "## What's Changed
* feat({scope}): x by @a in https://github.com/boykopovar/AnyPS5/pull/9
"
            )
        };
        // Releases 0.1.1 (relinker change), 0.1.2 and 0.1.3 (none).
        let (base, _) = serve(|base| {
            let list = format!(
                "[{},{},{}]",
                release_json("v0.1.3", base),
                release_json("v0.1.2", base),
                release_json("v0.1.1", base)
            );
            vec![
                ("/repos/x/y/releases", list),
                ("/notes/v0.1.1", line("relinker")),
                ("/notes/v0.1.2", line("agc")),
                ("/notes/v0.1.3", line("agc")),
            ]
        });
        let endpoints = Endpoints::local(&base, false);
        assert!(conversion_changed(&paths, &endpoints, "x/y", "0.1.0", "0.1.3").unwrap());
        assert!(!conversion_changed(&paths, &endpoints, "x/y", "0.1.1", "0.1.3").unwrap());
        assert!(
            !conversion_changed(&paths, &endpoints, "x/y", "local-abc", "0.1.3").unwrap(),
            "a local build has no tag to compare"
        );
        let published = published_notes(&paths, &endpoints, "x/y", "v0.1.1")
            .unwrap()
            .unwrap();
        assert_eq!(
            (published.tag.as_str(), published.date.as_str()),
            ("0.1.1", "2026-10-20")
        );
        assert_eq!(published.notes.changes.len(), 1);
        assert!(
            published_notes(&paths, &endpoints, "x/y", "v9.9.9")
                .unwrap()
                .is_none()
        );
    }
}
