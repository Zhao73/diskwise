//! The one place that turns a raw scan into ranked, classified, filtered rows.
//! CLI, web UI and MCP all go through here so they can never disagree.

use std::path::{Path, PathBuf};

use humansize::{format_size, DECIMAL};
use serde::Serialize;

use crate::rules::{Rules, Verdict};
use crate::scan::Scan;

#[derive(Debug, Clone, Serialize)]
pub struct Row {
    pub path: PathBuf,
    /// Just the basename, for display.
    pub name: String,
    pub size: u64,
    pub human: String,
    pub is_dir: bool,
    /// Recursive file count, for directories.
    pub files: u64,
    /// Newest mtime in this subtree, seconds since the epoch. Shown for entries
    /// no rule recognises: "1,204 files, last written in March" is at least a
    /// fact, where a guess about what the folder is for would not be.
    pub newest: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<Verdict>,
}

#[derive(Debug, Clone, Default)]
pub struct Query {
    /// `None` ranks the whole tree; `Some(dir)` lists that directory's children.
    pub dir: Option<PathBuf>,
    /// List individual files rather than directories.
    pub files_only: bool,
    pub min: u64,
    pub category: Option<String>,
    /// Substring match on the path.
    pub contains: Option<String>,
    /// Keep directories whose parent is also listed. Display collapses them so
    /// each byte is shown once; planning needs them all.
    pub keep_nested: bool,
    pub limit: usize,
}

pub fn rows(s: &Scan, rules: &Rules, q: &Query) -> Vec<Row> {
    let mut out: Vec<Row> = if q.files_only {
        s.big_files
            .iter()
            .map(|f| file_row(rules, &f.path, f.size, f.mtime))
            .collect()
    } else if let Some(dir) = &q.dir {
        // A directory listing mixes folders with the loose files beside them —
        // a 600MB sqlite file matters as much as a folder does.
        let mut rows: Vec<Row> = s
            .children(dir)
            .into_iter()
            .map(|(p, e)| dir_row(rules, p, e.total, e.files, e.newest))
            .collect();
        rows.extend(
            s.big_files
                .iter()
                .filter(|f| f.path.parent() == Some(dir.as_path()))
                .map(|f| file_row(rules, &f.path, f.size, f.mtime)),
        );
        rows
    } else {
        s.ranked()
            .into_iter()
            .filter(|(p, _)| p != &s.root)
            .map(|(p, e)| dir_row(rules, p, e.total, e.files, e.newest))
            .collect()
    };

    if q.min > 0 {
        out.retain(|r| r.size >= q.min);
    }
    if let Some(cat) = &q.category {
        out.retain(|r| r.verdict.as_ref().is_some_and(|v| &v.category == cat));
    }
    if let Some(needle) = &q.contains {
        let needle = needle.to_lowercase();
        out.retain(|r| r.path.to_string_lossy().to_lowercase().contains(&needle));
    }
    if q.dir.is_none() && !q.files_only && !q.keep_nested {
        out.sort_by(by_size_then_depth);
        out = keep_informative(out);
    }
    out.sort_by(by_size_then_depth);
    if q.limit > 0 {
        out.truncate(q.limit);
    }
    out
}

/// Largest first; on a tie the shallower path wins. A directory holding one
/// child of the same size would otherwise sort arbitrarily, and the collapse
/// below would keep whichever of the two it happened to see first.
fn by_size_then_depth(a: &Row, b: &Row) -> std::cmp::Ordering {
    b.size.cmp(&a.size).then_with(|| {
        a.path
            .components()
            .count()
            .cmp(&b.path.components().count())
    })
}

/// Collapse a nested row into its ancestor only when it adds nothing: same
/// rule, or no rule at all. `~/.codex` and `~/.codex/sessions` both earn a row
/// — one is the folder you recognise, the other is the thing with a verdict —
/// but the two hundred day-directories underneath `sessions` do not.
fn keep_informative(rows: Vec<Row>) -> Vec<Row> {
    let mut kept: Vec<Row> = Vec::with_capacity(rows.len());
    for row in rows {
        // Rows arrive largest-first, so any ancestor has already been decided.
        let ancestor = kept
            .iter()
            .rev()
            .find(|k| row.path.starts_with(&k.path) && row.path != k.path);
        let informative = match ancestor {
            None => true,
            Some(a) => rule_of(&row).is_some() && rule_of(&row) != rule_of(a),
        };
        if informative {
            kept.push(row);
        }
    }
    kept
}

fn rule_of(r: &Row) -> Option<&str> {
    r.verdict.as_ref().map(|v| v.rule_id.as_str())
}

fn dir_row(rules: &Rules, path: PathBuf, size: u64, files: u64, newest: i64) -> Row {
    Row {
        name: basename(&path),
        verdict: rules.classify(&path),
        human: format_size(size, DECIMAL),
        path,
        size,
        is_dir: true,
        files,
        newest,
    }
}

fn file_row(rules: &Rules, path: &Path, size: u64, mtime: i64) -> Row {
    Row {
        name: basename(path),
        verdict: rules.classify(path),
        human: format_size(size, DECIMAL),
        path: path.to_path_buf(),
        size,
        is_dir: false,
        files: 1,
        newest: mtime,
    }
}

fn basename(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
}

/// Bytes the rules say are safely reclaimable (trash or archive) in these rows.
/// Rows nested inside another counted row are skipped, so the total is a real
/// number rather than the same gigabytes added twice.
pub fn reclaimable(rows: &[Row]) -> u64 {
    let mut counted: Vec<&Path> = Vec::new();
    let mut total = 0;
    for r in rows {
        let claimable = r
            .verdict
            .as_ref()
            .is_some_and(|v| matches!(v.suggest.as_str(), "trash" | "archive"));
        if !claimable || counted.iter().any(|c| r.path.starts_with(c)) {
            continue;
        }
        counted.push(&r.path);
        total += r.size;
    }
    total
}

/// A compact, rule-annotated digest of a scan — small enough to be cheap to
/// send to an agent, specific enough to answer questions about.
pub fn digest(s: &Scan, rules: &Rules) -> String {
    let rows = rows(
        s,
        rules,
        &Query {
            min: 100 << 20,
            limit: 40,
            ..Default::default()
        },
    );
    let mut out = format!(
        "root: {}\ntotal: {}\nfiles: {}\nreclaimable by rule: {}\n\n",
        s.root.display(),
        format_size(s.total(), DECIMAL),
        s.scanned_files,
        format_size(reclaimable(&rows), DECIMAL)
    );
    for r in &rows {
        let v = r
            .verdict
            .as_ref()
            .map(|v| format!("{} / {} — {}", v.category, v.suggest, v.note))
            .unwrap_or_else(|| "no rule (probably your own data)".into());
        out.push_str(&format!("{:>10}  {}  [{}]\n", r.human, r.path.display(), v));
    }
    out
}

/// Rows for an explicit list of files (used by the live directory listing).
pub fn file_rows(rules: &Rules, files: &[crate::scan::FileEntry]) -> Vec<Row> {
    files
        .iter()
        .map(|f| file_row(rules, &f.path, f.size, f.mtime))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_rows_collapse_unless_they_carry_a_new_verdict() {
        let rules = Rules::load_default().unwrap();
        let home = crate::rules::home_dir();
        let mk = |p: PathBuf, size: u64| dir_row(&rules, p, size, 1, 0);

        // Unclassified chain: only the outermost survives.
        let kept = keep_informative(vec![
            mk(PathBuf::from("/a"), 30),
            mk(PathBuf::from("/a/b"), 20),
            mk(PathBuf::from("/a/b/c"), 10),
        ]);
        assert_eq!(kept.len(), 1);

        // ~/.codex has no rule, ~/.codex/sessions does — both are worth a row,
        // and the day directories underneath it are not.
        let kept = keep_informative(vec![
            mk(home.join(".codex"), 40),
            mk(home.join(".codex/sessions"), 30),
            mk(home.join(".codex/sessions/2026/07"), 20),
            mk(home.join(".codex/plugins"), 10),
        ]);
        let names: Vec<String> = kept.iter().map(|r| r.name.clone()).collect();
        assert_eq!(names, vec![".codex", "sessions", "plugins"]);
    }

    /// A directory whose single child is the same size must not appear twice.
    #[test]
    fn equal_sized_parent_and_child_collapse_to_the_parent() {
        let rules = Rules::load_default().unwrap();
        let mut rows = vec![
            dir_row(&rules, PathBuf::from("/a/b/versions"), 400, 1, 0),
            dir_row(&rules, PathBuf::from("/a/b"), 400, 1, 0),
        ];
        rows.sort_by(by_size_then_depth);
        let kept = keep_informative(rows);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].path, PathBuf::from("/a/b"));
    }

    #[test]
    fn reclaimable_never_counts_the_same_bytes_twice() {
        let rules = Rules::load_default().unwrap();
        let home = crate::rules::home_dir();
        // plugins is `trash`; a child of it must not add to the total again.
        let rows = vec![
            dir_row(&rules, home.join(".codex/plugins"), 1_000, 1, 0),
            dir_row(&rules, home.join(".codex/plugins/inner"), 400, 1, 0),
        ];
        assert_eq!(reclaimable(&rows), 1_000);
    }
}
