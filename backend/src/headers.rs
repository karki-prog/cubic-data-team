//! Resolve spreadsheet columns from the header row, never from A/B/C.
//!
//! Staff insert columns. A letter that used to be POC can become something else.
//! Every connected tab should go through here: find the header row, match field
//! names, then use the resulting index (and A1 letter only as a Sheets API
//! address, computed from that index).

use serde_json::Value;

#[derive(Clone, Debug)]
pub struct SheetHeaders {
    /// Lowercased, whitespace-collapsed header cells. Index 0 = column A.
    pub names: Vec<String>,
    /// 1-based sheet row that held these headers.
    pub row_1: i32,
}

pub mod col {
    pub const CANDIDATE: &[&str] = &["candidate name", "candidate", "full name"];
    pub const STAGE: &[&str] = &["interview stage", "stage"];
    pub const LOCATION: &[&str] = &["marketing location", "location"];
    pub const PLATFORM: &[&str] = &["interview platform", "platform", "mode"];
    pub const POC: &[&str] = &["nepal poc", "poc"];
    pub const DATE: &[&str] = &["meeting date", "date"];
    pub const TIME: &[&str] = &["meeting time", "time (cst)", "time"];
    pub const DURATION: &[&str] = &["duration"];
    pub const CLIENT: &[&str] = &["client"];
    pub const VENDOR: &[&str] = &["vendor"];
    pub const PANEL: &[&str] = &["panel"];
    pub const STATUS: &[&str] = &["status"];
    pub const NOTE: &[&str] = &["special note", "note", "comments", "comment"];
    pub const RESUME: &[&str] = &["resume link", "resume"];
    pub const JD: &[&str] = &["job description link", "job description", "jd"];
    pub const FOLDER: &[&str] = &["drive folder link", "drive folder", "folder"];
    pub const EMAIL: &[&str] = &[
        "email (personal)",
        "mail ( personal)",
        "mail (personal)",
        "candidate email",
        "email",
        "mail",
    ];
    pub const TECH: &[&str] = &["tech"];
    pub const VISA: &[&str] = &["visa"];
    pub const SUPPORT: &[&str] = &["support"];
    pub const TIMESTAMP: &[&str] = &["submitted", "timestamp", "submitted at"];
    pub const APPLY_LINK: &[&str] = &["apply link", "apply url", "job link", "url", "link"];
    pub const ADDED_BY: &[&str] = &["added by", "addedby", "poc"];
    pub const JOB_SITE: &[&str] = &["job site", "site"];
    pub const COMPANY: &[&str] = &["company", "company name", "do not apply"];
    pub const REASON: &[&str] = &["reason", "notes", "note"];
    pub const FEEDBACK: &[&str] = &["feedback"];
    pub const APPLY_NAME: &[&str] = &["candidate name", "candidate", "name"];
    pub const APPLY_MAIL: &[&str] = &["mail ( personal)", "mail (personal)", "mail", "email"];
}

fn norm(text: &str) -> String {
    text.trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn tokens(text: &str) -> Vec<&str> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect()
}

impl SheetHeaders {
    pub fn parse(row: &[String]) -> Self {
        Self {
            names: row.iter().map(|h| norm(h)).collect(),
            row_1: 1,
        }
    }

    /// First rows of a tab. Picks the row that matches the most known fields.
    pub fn detect(preview: &[Vec<String>]) -> Self {
        const HINTS: &[&[&str]] = &[
            col::CANDIDATE,
            col::CLIENT,
            col::POC,
            col::DATE,
            col::TIME,
            col::EMAIL,
            col::STATUS,
            col::APPLY_LINK,
            col::ADDED_BY,
            col::COMPANY,
            col::APPLY_NAME,
            col::DURATION,
        ];
        let mut best_i = 0usize;
        let mut best_score = -1i32;
        let scan = preview.len().min(8);
        for i in 0..scan {
            let h = Self::parse(preview.get(i).map(Vec::as_slice).unwrap_or(&[]));
            let score = HINTS.iter().filter(|aliases| h.idx(aliases).is_some()).count() as i32;
            if score > best_score {
                best_score = score;
                best_i = i;
            }
        }
        let mut headers = Self::parse(preview.get(best_i).map(Vec::as_slice).unwrap_or(&[]));
        headers.row_1 = best_i as i32 + 1;
        headers
    }

    pub fn idx(&self, aliases: &[&str]) -> Option<usize> {
        let aliases: Vec<String> = aliases.iter().map(|a| norm(a)).filter(|a| !a.is_empty()).collect();
        for a in &aliases {
            if let Some(i) = self.names.iter().position(|h| h == a) {
                return Some(i);
            }
        }
        for a in &aliases {
            if a.len() < 4 {
                continue;
            }
            if let Some(i) = self.names.iter().position(|h| h.starts_with(a) || h.contains(a)) {
                return Some(i);
            }
        }
        for a in &aliases {
            let want = tokens(a);
            if want.is_empty() {
                continue;
            }
            if let Some(i) = self.names.iter().position(|h| {
                let have = tokens(h);
                want.iter().all(|t| have.contains(t))
            }) {
                return Some(i);
            }
        }
        None
    }

    pub fn must(&self, aliases: &[&str]) -> anyhow::Result<usize> {
        self.idx(aliases).ok_or_else(|| {
            anyhow::anyhow!(
                "Header row {} has no column matching {:?}. Headers: {:?}",
                self.row_1,
                aliases,
                self.names.iter().filter(|h| !h.is_empty()).collect::<Vec<_>>()
            )
        })
    }

    pub fn get<'a>(&self, row: &'a [String], aliases: &[&str]) -> &'a str {
        self.idx(aliases)
            .and_then(|i| row.get(i))
            .map(|s| s.trim())
            .unwrap_or("")
    }

    pub fn get_owned(&self, row: &[String], aliases: &[&str]) -> String {
        self.get(row, aliases).to_string()
    }

    /// Pair a value with a header index. Missing title → no write (never guess A/B/C).
    pub fn pair(&self, aliases: &[&str], value: Value) -> Option<(usize, Value)> {
        self.idx(aliases).map(|i| (i, value))
    }

    /// Last named header (0-based). Used only to size a fetch window, not to map fields.
    pub fn last_named_idx(&self) -> usize {
        self.names
            .iter()
            .rposition(|h| !h.is_empty())
            .unwrap_or(0)
    }

    /// 0-based column index → A1 letter.
    pub fn letter(idx: usize) -> String {
        a1_col(idx)
    }

    pub fn range_row(&self, sheet: &str, row: i32, start: usize, end: usize) -> String {
        let (a, b) = if start <= end { (start, end) } else { (end, start) };
        format!("'{sheet}'!{}{row}:{}{row}", a1_col(a), a1_col(b))
    }

    /// Empty columns immediately to the right of the last named header, used for
    /// hidden URL backups when those backups have no header of their own.
    pub fn hidden_run(&self, count: usize) -> Vec<usize> {
        let last = self
            .names
            .iter()
            .rposition(|h| !h.is_empty())
            .map(|i| i + 1)
            .unwrap_or(self.names.len());
        (last..last + count).collect()
    }
}

pub fn a1_col(index0: usize) -> String {
    let mut n = (index0 + 1) as i32;
    let mut out = String::new();
    while n > 0 {
        let rem = (n - 1) % 26;
        out.insert(0, (b'A' + rem as u8) as char);
        n = (n - 1) / 26;
    }
    out
}

pub fn sparse_row(pairs: &[(usize, Value)]) -> (usize, usize, Vec<Value>) {
    let Some(min) = pairs.iter().map(|(i, _)| *i).min() else {
        return (0, 0, Vec::new());
    };
    let max = pairs.iter().map(|(i, _)| *i).max().unwrap_or(min);
    let mut row = vec![Value::String(String::new()); max - min + 1];
    for (i, v) in pairs {
        row[i - min] = v.clone();
    }
    (min, max, row)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters() {
        assert_eq!(a1_col(0), "A");
        assert_eq!(a1_col(1), "B");
        assert_eq!(a1_col(25), "Z");
        assert_eq!(a1_col(26), "AA");
    }

    #[test]
    fn finds_moved_poc() {
        let h = SheetHeaders::parse(&[
            "Full Name".into(),
            "Marketing Location".into(),
            "Nepal POC".into(),
            "Email (Personal)".into(),
        ]);
        assert_eq!(h.idx(col::CANDIDATE), Some(0));
        assert_eq!(h.idx(col::POC), Some(2));
        assert_eq!(h.idx(col::EMAIL), Some(3));
        assert_eq!(h.idx(col::LOCATION), Some(1));
    }

    #[test]
    fn missing_header_does_not_guess_a_letter() {
        let h = SheetHeaders::parse(&["Candidate".into(), "Date".into()]);
        assert!(h.idx(col::POC).is_none());
        assert!(h.pair(col::POC, Value::String("Saksham".into())).is_none());
    }

    #[test]
    fn time_does_not_eat_timestamp() {
        let h = SheetHeaders::parse(&["Timestamp".into(), "Meeting Time".into()]);
        assert_eq!(h.idx(col::TIME), Some(1));
        assert_eq!(h.idx(col::TIMESTAMP), Some(0));
    }
}
