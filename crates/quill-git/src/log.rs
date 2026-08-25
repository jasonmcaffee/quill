//! The commits, for the history window and for the list of recent commit messages.
//!
//! Read from `git log` with a `--format` of our own, using the two control characters git suggests
//! for the job: `%x1f` between the fields of one commit and `%x1e` between commits. Neither can
//! appear in a name, an address or a subject, so nothing has to be escaped and a subject with a
//! newline in it — which a `-m` message spanning lines produces — does not split a commit in two.

use std::path::Path;

use crate::blame::format_date;
use crate::command::{run, Outcome};

/// One commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub hash: String,
    /// The short hash git itself would print.
    pub short: String,
    pub author: String,
    pub email: String,
    /// Seconds since the epoch.
    pub time: i64,
    pub date: String,
    /// The first line of the message.
    pub subject: String,
    /// Everything after the first line, which is often empty.
    pub body: String,
    /// The branches and tags pointing at this commit, as git spells them.
    pub refs: String,
}

/// The separators. Unit separator between fields, record separator between commits.
const FIELD: char = '\u{1f}';
const RECORD: char = '\u{1e}';
const FORMAT: &str = "%H\u{1f}%h\u{1f}%an\u{1f}%ae\u{1f}%at\u{1f}%s\u{1f}%b\u{1f}%D\u{1e}";

/// The commits reachable from HEAD, newest first.
///
/// `path` limits it to the commits that touched one file, which is what `Show History` on a file
/// asks for. `limit` is there because a repository can hold a hundred thousand commits and a window
/// shows a few dozen.
pub fn read(folder: &Path, path: Option<&Path>, limit: usize) -> Result<Vec<Commit>, Outcome> {
    let mut arguments: Vec<std::ffi::OsString> = vec![
        "log".into(),
        format!("--format={FORMAT}").into(),
        format!("-n{limit}").into(),
    ];
    if let Some(path) = path {
        arguments.push("--follow".into());
        arguments.push("--".into());
        arguments.push(path.into());
    }
    let outcome = run(folder, &arguments);
    if !outcome.ok {
        // A repository with no commits in it yet is not a failure worth reporting as one: there is
        // simply no history, which is exactly what an empty list says.
        if outcome.stderr.contains("does not have any commits yet") {
            return Ok(Vec::new());
        }
        return Err(outcome);
    }
    Ok(parse(&outcome.stdout))
}

/// The messages of the last few commits, which the commit panel offers under its clock button.
pub fn recent_messages(folder: &Path, limit: usize) -> Vec<String> {
    read(folder, None, limit)
        .unwrap_or_default()
        .into_iter()
        .map(|commit| {
            if commit.body.trim().is_empty() {
                commit.subject
            } else {
                format!("{}\n\n{}", commit.subject, commit.body.trim())
            }
        })
        .collect()
}

/// Turn the formatted log into commits.
pub fn parse(text: &str) -> Vec<Commit> {
    text.split(RECORD)
        .map(str::trim_start_matches_newline)
        .filter(|record| !record.trim().is_empty())
        .filter_map(|record| {
            let fields: Vec<&str> = record.split(FIELD).collect();
            if fields.len() < 8 {
                return None;
            }
            let time: i64 = fields[4].trim().parse().unwrap_or(0);
            Some(Commit {
                hash: fields[0].to_owned(),
                short: fields[1].to_owned(),
                author: fields[2].to_owned(),
                email: fields[3].to_owned(),
                time,
                date: format_date(time),
                subject: fields[5].to_owned(),
                body: fields[6].to_owned(),
                refs: fields[7].trim().to_owned(),
            })
        })
        .collect()
}

/// `split` leaves the newline git puts after each record at the start of the next one.
trait TrimNewline {
    fn trim_start_matches_newline(&self) -> &str;
}

impl TrimNewline for str {
    fn trim_start_matches_newline(&self) -> &str {
        self.trim_start_matches(['\n', '\r'])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(hash: &str, author: &str, time: i64, subject: &str, body: &str, refs: &str) -> String {
        format!("{hash}\u{1f}{}\u{1f}{author}\u{1f}a@b\u{1f}{time}\u{1f}{subject}\u{1f}{body}\u{1f}{refs}\u{1e}\n", &hash[..7])
    }

    #[test]
    fn a_commit_is_read_field_by_field() {
        let text = record("3f2a1b0e9c", "Jason", 1_777_075_200, "some work.", "", "HEAD -> master");
        let commits = parse(&text);
        assert_eq!(commits.len(), 1);
        let commit = &commits[0];
        assert_eq!(commit.short, "3f2a1b0");
        assert_eq!(commit.author, "Jason");
        assert_eq!(commit.subject, "some work.");
        assert_eq!(commit.date, "4/25/2026");
        assert_eq!(commit.refs, "HEAD -> master");
    }

    #[test]
    fn a_message_that_spans_lines_stays_one_commit() {
        // This is why the records are separated by a control character rather than by a newline.
        let text = record(
            "aaaaaaaaaa",
            "Jason",
            1_777_075_200,
            "task-1649: line numbers",
            "- the gutter\n- the blame column\n",
            "",
        ) + &record("bbbbbbbbbb", "Sam", 1_777_161_600, "a second", "", "");
        let commits = parse(&text);
        assert_eq!(commits.len(), 2, "a body with newlines in it does not split a commit in two");
        assert!(commits[0].body.contains("the blame column"));
        assert_eq!(commits[1].author, "Sam");
    }

    #[test]
    fn nothing_at_all_is_no_commits_rather_than_one_empty_one() {
        assert!(parse("").is_empty());
        assert!(parse("\n").is_empty());
    }

    #[test]
    fn a_record_missing_fields_is_skipped_rather_than_read_wrongly() {
        assert!(parse("only\u{1f}two\u{1e}").is_empty());
    }
}
