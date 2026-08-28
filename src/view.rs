//! The one place that turns a raw scan into ranked, classified, filtered rows.
//! CLI, web UI and MCP all go through here so they can never disagree.

use std::collections::HashSet;
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
        s.big_files.iter().map(|f| file_row(rules, &f.path, f.size)).collect()
    } else if let Some(dir) = &q.dir {
        // A directory listing mixes folders with the loose files beside them —
        // a 600MB sqlite file matters as much as a folder does.
        let mut rows: Vec<Row> = s.children(dir).into_iter().map(|(p, e)| dir_row(rules, p, e.total, e.files)).collect();
        rows.extend(
            s.big_files
                .iter()
                .filter(|f| f.path.parent() == Some(dir.as_path()))
                .map(|f| file_row(rules, &f.path, f.size)),
        );
        rows
    } else {
        s.ranked()
            .into_iter()
            .filter(|(p, _)| p != &s.root)
            .map(|(p, e)| dir_row(rules, p, e.total, e.files))
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
        // Without this a deep tree prints the same bytes at every level.
        out = drop_nested(out);
    }
    out.sort_by(|a, b| b.size.cmp(&a.size));
    if q.limit > 0 {
        out.truncate(q.limit);
    }
    out
}

/// Remove rows whose parent is also in the list, so each byte is shown once.
fn drop_nested(rows: Vec<Row>) -> Vec<Row> {
    let shown: HashSet<&Path> = rows.iter().map(|r| r.path.as_path()).collect();
    let keep: Vec<bool> = rows.iter().map(|r| !r.path.parent().is_some_and(|p| shown.contains(p))).collect();
    rows.into_iter().zip(keep).filter(|(_, k)| *k).map(|(r, _)| r).collect()
}

fn dir_row(rules: &Rules, path: PathBuf, size: u64, files: u64) -> Row {
    Row {
        name: basename(&path),
        verdict: rules.classify(&path),
        human: format_size(size, DECIMAL),
        path,
        size,
        is_dir: true,
        files,
    }
}

fn file_row(rules: &Rules, path: &Path, size: u64) -> Row {
    Row {
        name: basename(path),
        verdict: rules.classify(path),
        human: format_size(size, DECIMAL),
        path: path.to_path_buf(),
        size,
        is_dir: false,
        files: 1,
    }
}

fn basename(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
}

/// Bytes the rules say are safely reclaimable (trash or archive) in these rows.
pub fn reclaimable(rows: &[Row]) -> u64 {
    rows.iter()
        .filter(|r| r.verdict.as_ref().is_some_and(|v| matches!(v.suggest.as_str(), "trash" | "archive")))
        .map(|r| r.size)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_rows_are_collapsed_to_their_top_ancestor() {
        let rules = Rules::load_default().unwrap();
        let mk = |p: &str| dir_row(&rules, PathBuf::from(p), 10, 1);
        let kept = drop_nested(vec![mk("/a"), mk("/a/b"), mk("/a/b/c"), mk("/z")]);
        let names: Vec<_> = kept.iter().map(|r| r.path.display().to_string()).collect();
        assert_eq!(names, vec!["/a", "/z"]);
    }
}

/// Rows for an explicit list of files (used by the live directory listing).
pub fn file_rows(rules: &Rules, files: &[crate::scan::FileEntry]) -> Vec<Row> {
    files.iter().map(|f| file_row(rules, &f.path, f.size)).collect()
}
