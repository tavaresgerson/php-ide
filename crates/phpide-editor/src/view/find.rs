//! Find and replace in the current file (literal or regex).

use phpide_core::{Change, ChangeSet, Revision};
use regex::{Regex, RegexBuilder};
use ropey::Rope;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FindOptions {
    pub regex: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
}

#[derive(Debug, Default)]
pub struct FindState {
    pub open: bool,
    pub replace_open: bool,
    pub query: String,
    pub replacement: String,
    pub options: FindOptions,
    /// Matches as char ranges, for `computed_for`.
    pub matches: Vec<(usize, usize)>,
    pub current: Option<usize>,
    pub error: Option<String>,
    computed_for: Option<(Revision, String, FindOptions)>,
    pub(crate) focus_query: bool,
}

fn build_regex(query: &str, options: &FindOptions) -> Result<Regex, String> {
    let mut pattern = if options.regex {
        query.to_string()
    } else {
        regex::escape(query)
    };
    if options.whole_word {
        pattern = format!(r"\b(?:{pattern})\b");
    }
    RegexBuilder::new(&pattern)
        .case_insensitive(!options.case_sensitive)
        .multi_line(true)
        .build()
        .map_err(|e| {
            e.to_string()
                .lines()
                .last()
                .unwrap_or("invalid pattern")
                .to_string()
        })
}

/// All matches of `query` as char ranges (empty matches skipped).
pub fn find_all(
    rope: &Rope,
    query: &str,
    options: &FindOptions,
) -> Result<Vec<(usize, usize)>, String> {
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let regex = build_regex(query, options)?;
    let text = rope.to_string();
    Ok(regex
        .find_iter(&text)
        .filter(|m| !m.is_empty())
        .take(100_000)
        .map(|m| (rope.byte_to_char(m.start()), rope.byte_to_char(m.end())))
        .collect())
}

impl FindState {
    /// Recomputes matches when the text, query or options changed.
    pub fn refresh(&mut self, rope: &Rope, revision: Revision) {
        let key = (revision, self.query.clone(), self.options.clone());
        if self.computed_for.as_ref() == Some(&key) {
            return;
        }
        match find_all(rope, &self.query, &self.options) {
            Ok(matches) => {
                self.matches = matches;
                self.error = None;
            }
            Err(error) => {
                self.matches.clear();
                self.error = Some(error);
            }
        }
        self.current = self
            .current
            .filter(|i| *i < self.matches.len())
            .or((!self.matches.is_empty()).then_some(0));
        self.computed_for = Some(key);
    }

    /// The match after (or before) `pos`, wrapping around.
    pub fn step(&mut self, pos: usize, forward: bool) -> Option<(usize, usize)> {
        if self.matches.is_empty() {
            return None;
        }
        let index = if forward {
            self.matches
                .iter()
                .position(|(s, _)| *s >= pos)
                .unwrap_or(0)
        } else {
            self.matches
                .iter()
                .rposition(|(s, _)| *s < pos)
                .unwrap_or(self.matches.len() - 1)
        };
        self.current = Some(index);
        Some(self.matches[index])
    }

    /// Replacement text for one match (regex groups like `$1` expanded).
    fn expand(&self, rope: &Rope, (start, end): (usize, usize)) -> Option<String> {
        let found = rope.slice(start..end).to_string();
        if !self.options.regex {
            return Some(self.replacement.clone());
        }
        let regex = build_regex(&self.query, &self.options).ok()?;
        let captures = regex.captures(&found)?;
        let mut out = String::new();
        captures.expand(&self.replacement, &mut out);
        Some(out)
    }

    /// Change set replacing one match (the current) or all of them.
    pub fn replacement_changes(&self, rope: &Rope, all: bool) -> ChangeSet {
        let targets: Vec<(usize, usize)> = if all {
            self.matches.clone()
        } else {
            self.current
                .and_then(|i| self.matches.get(i).copied())
                .into_iter()
                .collect()
        };
        ChangeSet::new(
            targets
                .into_iter()
                .filter_map(|m| self.expand(rope, m).map(|text| Change::new(m.0, m.1, text)))
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_regex_case_and_word() {
        let rope = Rope::from_str("Foo foo food\n$foo->bar();");
        let mut options = FindOptions::default();
        assert_eq!(find_all(&rope, "foo", &options).unwrap().len(), 4);
        options.case_sensitive = true;
        assert_eq!(find_all(&rope, "foo", &options).unwrap().len(), 3);
        options.whole_word = true;
        assert_eq!(find_all(&rope, "foo", &options).unwrap().len(), 2);
        let options = FindOptions {
            regex: true,
            ..Default::default()
        };
        assert_eq!(find_all(&rope, r"\$\w+", &options).unwrap(), vec![(13, 17)]);
        assert!(find_all(&rope, "(", &options).is_err());
        assert_eq!(
            find_all(&rope, "(", &FindOptions::default()).unwrap().len(),
            1
        );
    }

    #[test]
    fn stepping_wraps_and_replace_expands_groups() {
        let rope = Rope::from_str("a1 b2 c3");
        let mut find = FindState {
            query: r"([a-z])(\d)".into(),
            replacement: "$2$1".into(),
            options: FindOptions {
                regex: true,
                ..Default::default()
            },
            ..Default::default()
        };
        find.refresh(&rope, 1);
        assert_eq!(find.matches.len(), 3);
        assert_eq!(find.step(4, true), Some((6, 8)));
        assert_eq!(find.step(7, true), Some((0, 2)), "wraps");
        assert_eq!(find.step(0, false), Some((6, 8)));
        let changes = find.replacement_changes(&rope, true);
        let mut doc = phpide_core::Document::new("a1 b2 c3");
        doc.apply(&changes);
        assert_eq!(doc.text(), "1a 2b 3c");
    }
}
