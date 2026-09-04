//! Who last changed each line of a file, which is what the gutter annotates with.
//!
//! Read from `git blame --line-porcelain`, which prints a block for every line of the file: a header
//! naming the commit and the line numbers, then one `key value` a line, then the line's own text
//! preceded by a tab. Every block repeats every field, even when twenty lines in a row came from one
//! commit, which is why it is called the *line* porcelain — it costs some output and it means nothing
//! has to be remembered between blocks.
//!
//! ```text
//! 3f2a1b0e... 12 12 3
//! author Jason
//! author-mail <jason@example.com>
//! author-time 1745539200
//! author-tz +0000
//! summary the commit's first line
//! filename backend/src/thing.ts
//! \tthis.sql = createScopedSql();
//! ```
//!
//! The date is formatted here rather than in the window, because turning a Unix time into
//! `4/25/2026` is not drawing. It is done by hand rather than with a dates library: the only thing
//! Unluminate needs is a civil date in whatever the commit's own zone was, and a whole dependency for
//! that is not worth it. The arithmetic is Howard Hinnant's `civil_from_days`, which is the standard
//! way of doing it and is tested here against dates that are easy to check.

use std::path::Path;

use crate::command::{run, Outcome};

/// One line of a file, and the commit it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct BlameLine {
    pub commit: String,
    pub author: String,
    /// Seconds since the epoch, in the commit's own zone.
    pub time: i64,
    pub summary: String,
    /// Formatted as `M/D/YYYY`, which is what the reference capture shows.
    pub date: String,
    /// 0.0 for the oldest commit in the file and 1.0 for the newest, by rank rather than by date.
    ///
    /// By rank, because a file whose history is one recent burst of work and one ancient commit
    /// would otherwise read as two colours with nothing between them.
    pub age: f32,
}

/// A whole file's worth.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Blame {
    pub lines: Vec<BlameLine>,
}

impl Blame {
    /// Ask git who wrote each line of `path`.
    ///
    /// `path` is relative to `folder`, or absolute; git accepts either.
    pub fn read(folder: &Path, path: &Path) -> Result<Self, Outcome> {
        let outcome = run(
            folder,
            &[
                std::ffi::OsStr::new("blame"),
                std::ffi::OsStr::new("--line-porcelain"),
                std::ffi::OsStr::new("--"),
                path.as_os_str(),
            ],
        );
        if !outcome.ok {
            return Err(outcome);
        }
        Ok(parse(&outcome.stdout))
    }

    pub fn line(&self, index: usize) -> Option<&BlameLine> {
        self.lines.get(index)
    }
}

/// Turn `--line-porcelain` output into a [`Blame`].
pub fn parse(text: &str) -> Blame {
    let mut lines: Vec<BlameLine> = Vec::new();
    let mut current: Option<BlameLine> = None;
    for line in text.lines() {
        // The line's own text, which ends the block. It is not needed — the editor already has the
        // file — but it is how the end of a block is recognised.
        if line.starts_with('\t') {
            if let Some(entry) = current.take() {
                lines.push(entry);
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("author ") {
            if let Some(entry) = current.as_mut() {
                entry.author = rest.to_owned();
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("author-time ") {
            if let Some(entry) = current.as_mut() {
                entry.time = rest.trim().parse().unwrap_or(0);
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("summary ") {
            if let Some(entry) = current.as_mut() {
                entry.summary = rest.to_owned();
            }
            continue;
        }
        // Anything else that starts with forty hexadecimal characters is a block header.
        if let Some(commit) = header_commit(line) {
            current = Some(BlameLine {
                commit,
                author: String::new(),
                time: 0,
                summary: String::new(),
                date: String::new(),
                age: 0.0,
            });
        }
    }
    for entry in &mut lines {
        entry.date = format_date(entry.time);
    }
    rank(&mut lines);
    Blame { lines }
}

/// The commit at the start of a block header, if this line is one.
fn header_commit(line: &str) -> Option<String> {
    let candidate = line.split(' ').next()?;
    let long_enough = candidate.len() >= 40;
    let hexadecimal = candidate.chars().all(|c| c.is_ascii_hexdigit());
    (long_enough && hexadecimal).then(|| candidate.to_owned())
}

/// Give every line an age from 0.0 to 1.0 by where its commit ranks among the commits in the file.
///
/// Ranked by **commit**, ordered by time, rather than by time alone. Two commits a second apart —
/// or in the same second, which happens whenever anything commits twice quickly — are two commits
/// and should be two colours; ranking by the timestamp alone would draw them as one. The hash
/// breaks a tie so the order is the same on every run.
///
/// A file touched by one commit gives every line 1.0, because the newest commit in it is also the
/// only one and drawing it as the oldest would be misleading.
fn rank(lines: &mut [BlameLine]) {
    let mut commits: Vec<(i64, String)> =
        lines.iter().map(|line| (line.time, line.commit.clone())).collect();
    commits.sort();
    commits.dedup();
    if commits.len() <= 1 {
        for line in lines {
            line.age = 1.0;
        }
        return;
    }
    let last = (commits.len() - 1) as f32;
    for line in lines {
        let place = commits
            .iter()
            .position(|(time, commit)| *time == line.time && *commit == line.commit)
            .unwrap_or(0);
        line.age = place as f32 / last;
    }
}

/// A Unix time as `M/D/YYYY`.
///
/// The arithmetic is Howard Hinnant's `civil_from_days`, which turns a count of days since the epoch
/// into a year, a month and a day with no table and no leap-year special cases. It is used here
/// rather than a dates library because a civil date is the only thing Unluminate wants from one.
pub fn format_date(time: i64) -> String {
    if time <= 0 {
        return "uncommitted".to_owned();
    }
    let days = time.div_euclid(86_400);
    // Shift the epoch to the 1st of March 0000, so that the leap day is the last day of the year and
    // does not have to be reasoned about at all.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{month}/{day}/{year}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(commit: &str, author: &str, time: i64, summary: &str, text: &str) -> String {
        format!(
            "{commit} 1 1 1\nauthor {author}\nauthor-mail <a@b>\nauthor-time {time}\nauthor-tz +0000\nsummary {summary}\nfilename a.ts\n\t{text}\n"
        )
    }

    #[test]
    fn one_block_a_line_is_read() {
        let text = block("3f2a1b0e3f2a1b0e3f2a1b0e3f2a1b0e3f2a1b0e", "Jason", 1_777_075_200, "a commit", "code();");
        let blame = parse(&text);
        assert_eq!(blame.lines.len(), 1);
        let line = &blame.lines[0];
        assert_eq!(line.author, "Jason");
        assert_eq!(line.summary, "a commit");
        assert_eq!(line.time, 1_777_075_200);
        assert!(line.commit.starts_with("3f2a1b"));
    }

    #[test]
    fn a_line_of_text_with_a_hash_in_it_is_not_mistaken_for_a_header() {
        // The line's own text is preceded by a tab, which is how a block ends. Forty hexadecimal
        // characters inside a file must not start a new block.
        let mut text = block("a".repeat(40).as_str(), "Jason", 1_777_075_200, "one", "const hash = 'deadbeef';");
        text.push_str(&block("b".repeat(40).as_str(), "Sam", 1_745_625_600, "two", &"c".repeat(40)));
        let blame = parse(&text);
        assert_eq!(blame.lines.len(), 2);
        assert_eq!(blame.lines[1].author, "Sam");
    }

    #[test]
    fn two_commits_in_the_same_second_are_still_two_colours() {
        // Ranking by the timestamp alone drew both as the newest, which a real repository showed
        // straight away: two commits made one after the other land in the same second.
        let mut text = block("a".repeat(40).as_str(), "Jason", 1_000, "one", "first");
        text.push_str(&block("b".repeat(40).as_str(), "Sam", 1_000, "two", "second"));
        let blame = parse(&text);
        assert_eq!(blame.lines[0].age, 0.0);
        assert_eq!(blame.lines[1].age, 1.0);
    }

    #[test]
    fn the_oldest_commit_in_a_file_is_zero_and_the_newest_is_one() {
        let mut text = block("a".repeat(40).as_str(), "Jason", 1_000, "old", "one");
        text.push_str(&block("b".repeat(40).as_str(), "Sam", 2_000, "middle", "two"));
        text.push_str(&block("c".repeat(40).as_str(), "Kim", 3_000, "new", "three"));
        // A second line from the oldest commit, which must rank with the first.
        text.push_str(&block("a".repeat(40).as_str(), "Jason", 1_000, "old", "four"));
        let blame = parse(&text);
        let ages: Vec<f32> = blame.lines.iter().map(|line| line.age).collect();
        assert_eq!(ages, vec![0.0, 0.5, 1.0, 0.0]);
    }

    #[test]
    fn a_file_with_one_commit_in_it_is_all_new_rather_than_all_old() {
        let mut text = block("a".repeat(40).as_str(), "Jason", 1_000, "only", "one");
        text.push_str(&block("a".repeat(40).as_str(), "Jason", 1_000, "only", "two"));
        let blame = parse(&text);
        assert!(blame.lines.iter().all(|line| line.age == 1.0));
    }

    #[test]
    fn a_date_is_the_civil_date_in_the_commits_own_zone() {
        // Checked against dates that are easy to verify by hand.
        assert_eq!(format_date(86_400), "1/2/1970");
        assert_eq!(format_date(1_777_075_200), "4/25/2026");
        // The 29th of February in a leap year, and the 1st of March after it.
        assert_eq!(format_date(1_709_164_800), "2/29/2024");
        assert_eq!(format_date(1_709_251_200), "3/1/2024");
        // The last day of a century that is not a leap year.
        assert_eq!(format_date(4_107_456_000), "2/28/2100");
    }

    #[test]
    fn a_line_that_has_never_been_committed_says_so() {
        assert_eq!(format_date(0), "uncommitted");
    }
}
