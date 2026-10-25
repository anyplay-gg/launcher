//! Run reports on the launcher's side ("How did it run?", CONTEXT.md): what players found per
//! title, sending the player's own answer, and the friends who play a title right now.

use std::collections::{BTreeMap, BTreeSet};

use super::{Command, Online, Purpose, api};

/// What players found, by title and the game version asked for first (empty: any version).
#[derive(Debug, Default)]
pub struct Runs {
    summaries: BTreeMap<(String, String), api::RunSummary>,
    /// The questions sent since this sign-in, so a game page asks the cloud once.
    asked: BTreeSet<(String, String)>,
}

fn key(title_id: &str, version: Option<&str>) -> (String, String) {
    (title_id.to_owned(), version.unwrap_or_default().to_owned())
}

impl Runs {
    /// Keeps the cloud's answer to a question.
    pub fn keep(&mut self, title_id: &str, version: Option<&str>, summary: api::RunSummary) {
        self.summaries.insert(key(title_id, version), summary);
    }

    /// The most reported verdict and how many players reported, once enough did. Prefers the
    /// player's own game version, as far as the cloud said it has enough reports for it.
    pub fn verdict(&self, title_id: &str, version: &str) -> Option<(api::Verdict, u32)> {
        let summary = self.summaries.get(&key(title_id, Some(version)))?;
        if summary.reports < api::RUN_REPORTS_MIN {
            return None;
        }
        Some((summary.verdict?, summary.reports))
    }
}

impl Online {
    /// Asks what players found for the title, with the player's game version first. Once per
    /// sign-in, unless `again`.
    pub fn request_runs(&mut self, title_id: &str, version: &str, again: bool) {
        if !self.is_online() || !api::is_title_id(title_id) || !api::is_run_version(version) {
            return;
        }
        let asked = key(title_id, Some(version));
        if !self.runs.asked.insert(asked) && !again {
            return;
        }
        self.send(
            Purpose::RunSummary {
                title_id: title_id.to_owned(),
                version: Some(version.to_owned()),
            },
            Command::RunReports {
                title_id: title_id.to_owned(),
                version: Some(version.to_owned()),
            },
        );
    }

    /// Sends the player's answer. Nothing is sent, and false comes back, when the player is not
    /// signed in or a version is not one the cloud takes.
    pub fn send_run_report(
        &mut self,
        title_id: &str,
        version: &str,
        toolkit: &str,
        verdict: api::Verdict,
    ) -> bool {
        if !self.is_signed_in()
            || !api::is_title_id(title_id)
            || !api::is_run_version(version)
            || !api::is_run_version(toolkit)
        {
            return false;
        }
        self.send(
            Purpose::RunReport,
            Command::RunReport {
                title_id: title_id.to_owned(),
                version: version.to_owned(),
                toolkit: toolkit.to_owned(),
                verdict,
            },
        );
        true
    }

    /// The handles of the friends who play the title now, by name.
    pub fn friends_playing(&self, title_id: &str) -> Vec<&str> {
        let mut names: Vec<&str> = self
            .friends
            .friends
            .iter()
            .filter(|f| {
                f.presence
                    .activity
                    .as_ref()
                    .is_some_and(|a| a.title_id == title_id)
            })
            .map(|f| f.account.handle.as_str())
            .collect();
        names.sort_by_key(|n| n.to_lowercase());
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use api::{Account, AccountId, Activity, Friend, Presence, RunSummary, Verdict, VerdictCounts};

    fn friend(id: u64, handle: &str, title: Option<&str>) -> Friend {
        Friend {
            account: Account {
                id: AccountId(id),
                handle: handle.into(),
            },
            since: 1,
            presence: Presence {
                online: true,
                activity: title.map(|t| Activity {
                    title_id: t.into(),
                    title: "Game".into(),
                    version: "1".into(),
                    session: None,
                }),
                revision: 1,
            },
        }
    }

    fn summary(reports: u32, verdict: Option<Verdict>) -> RunSummary {
        RunSummary {
            title_id: "GAME00001".into(),
            version: None,
            reports,
            verdict,
            counts: VerdictCounts::default(),
        }
    }

    #[test]
    fn friends_playing_names_those_in_the_title_by_name() {
        let mut online = Online::default();
        online.friends.friends = vec![
            friend(1, "sam", Some("GAME00001")),
            friend(2, "Mira", Some("GAME00001")),
            friend(3, "Lu", Some("GAME00002")),
            friend(4, "Ed", None),
        ];
        assert_eq!(online.friends_playing("GAME00001"), ["Mira", "sam"]);
        assert!(online.friends_playing("GAME00009").is_empty());
    }

    #[test]
    fn a_verdict_shows_only_with_enough_reports() {
        let mut runs = Runs::default();
        assert_eq!(runs.verdict("GAME00001", "1.0"), None);
        runs.keep("GAME00001", Some("1.0"), summary(2, None));
        assert_eq!(runs.verdict("GAME00001", "1.0"), None);
        runs.keep(
            "GAME00001",
            Some("1.0"),
            summary(12, Some(Verdict::Playable)),
        );
        assert_eq!(
            runs.verdict("GAME00001", "1.0"),
            Some((Verdict::Playable, 12))
        );
        assert_eq!(runs.verdict("GAME00001", "2.0"), None);
    }

    #[test]
    fn nothing_is_sent_signed_out() {
        let mut online = Online::default();
        assert!(!online.send_run_report("GAME00001", "1.0", "0.2.0", Verdict::Menus));
        online.request_runs("GAME00001", "1.0", false);
        assert!(online.take_outbox().is_empty());
    }
}
