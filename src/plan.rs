//! Turning "free up 50G" into a concrete, reviewable list of actions.

use std::path::PathBuf;

use crate::actions::{new_plan_id, Action, Plan, PlanItem};
use crate::policy::Guard;
use crate::rules::Rules;
use crate::scan::Scan;
use crate::view::{self, Query};

#[derive(Debug, Clone, Default)]
pub struct PlanOptions {
    /// Stop once this many bytes are accounted for. 0 = take everything eligible.
    pub target: u64,
    /// Restrict to one category, e.g. "build".
    pub category: Option<String>,
    /// Ignore anything smaller than this.
    pub min: u64,
    /// Include `archive` candidates, not just regenerable `trash` ones.
    pub include_archives: bool,
}

/// Build a cleanup plan, cheapest-regret first: regenerable caches before
/// anything irreplaceable, biggest win first within each tier.
pub fn build(s: &Scan, rules: &Rules, guard: &Guard, opts: &PlanOptions) -> Plan {
    let rows = view::rows(
        s,
        rules,
        &Query {
            min: opts.min.max(1 << 20),
            category: opts.category.clone(),
            limit: 0,
            keep_nested: true,
            ..Default::default()
        },
    );

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let mut candidates: Vec<(u8, PlanItem)> = Vec::new();
    for r in rows {
        let Some(v) = r.verdict else { continue };
        if !r.is_dir || guard.is_protected(&r.path) {
            continue;
        }
        let (tier, action) = match v.suggest.as_str() {
            "trash" => (0u8, Action::Trash),
            "archive" if opts.include_archives => (1, Action::Archive),
            _ => continue,
        };
        let reason = format!("{}: {}", v.rule_id, v.note);
        match v.retain_days {
            // No retention window: the whole directory is fair game.
            None => candidates.push((
                tier,
                PlanItem {
                    path: r.path,
                    size: r.size,
                    action,
                    reason,
                    older_than: None,
                },
            )),
            Some(days) => {
                let cutoff = now - (days as i64) * 86_400;
                for part in settled_parts(s, &r.path, cutoff) {
                    candidates.push((
                        tier,
                        PlanItem {
                            path: part.path,
                            size: part.size,
                            action,
                            reason: format!(
                                "{reason} (only what has been untouched for {days}+ days)"
                            ),
                            older_than: part.older_than,
                        },
                    ));
                }
            }
        }
    }
    candidates.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.size.cmp(&a.1.size)));

    let mut items = Vec::new();
    let mut total = 0u64;
    for (_, item) in candidates {
        // Never list a path already covered by an ancestor in the plan.
        if items
            .iter()
            .any(|i: &PlanItem| item.path.starts_with(&i.path))
        {
            continue;
        }
        total += item.size;
        items.push(item);
        if opts.target > 0 && total >= opts.target {
            break;
        }
    }

    Plan {
        id: new_plan_id(),
        created: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        items,
    }
}

/// A reclaimable piece of a directory tree: either a whole settled sub-tree, or
/// the stale files inside a directory that also holds live data.
pub struct Part {
    pub path: PathBuf,
    pub size: u64,
    pub older_than: Option<i64>,
}

/// Split `dir` into the largest pieces that have seen no writes since `cutoff`.
/// Whole sub-trees are preferred, so a date-partitioned tree yields months and
/// days rather than thousands of files; a directory that mixes live and stale
/// files yields a file-filtered piece instead of being skipped entirely.
fn settled_parts(s: &Scan, dir: &std::path::Path, cutoff: i64) -> Vec<Part> {
    const MAX_DEPTH: usize = 6;
    let mut out = Vec::new();
    let mut queue = vec![(dir.to_path_buf(), 0usize)];
    while let Some((path, depth)) = queue.pop() {
        let Some(entry) = s.dirs.get(&path) else {
            continue;
        };
        if entry.total == 0 {
            continue;
        }
        if entry.newest < cutoff {
            out.push(Part {
                path,
                size: entry.total,
                older_than: None,
            });
            continue;
        }
        let children = s.children(&path);
        if depth < MAX_DEPTH {
            for (child, _) in &children {
                queue.push((child.clone(), depth + 1));
            }
        }
        // Loose files beside those sub-directories: take the stale ones.
        if entry.own > 0 {
            let stale: u64 = crate::scan::list_files(&path)
                .unwrap_or_default()
                .iter()
                .filter(|f| f.mtime < cutoff)
                .map(|f| f.size)
                .sum();
            if stale > 0 {
                out.push(Part {
                    path,
                    size: stale,
                    older_than: Some(cutoff),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::Policy;

    /// A plan must reach for regenerable caches before anything irreplaceable,
    /// and must never nest one item inside another.
    #[test]
    fn orders_by_regret_and_never_nests() {
        let tmp = std::env::temp_dir().join(format!("diskwise-plan-{}", std::process::id()));
        let nm = tmp.join("proj/node_modules");
        std::fs::create_dir_all(nm.join("deep/inner")).unwrap();
        std::fs::write(nm.join("big.bin"), vec![0u8; 3_000_000]).unwrap();
        std::fs::write(nm.join("deep/inner/x.bin"), vec![0u8; 2_000_000]).unwrap();

        let s = crate::scan::scan(&tmp);
        let rules = Rules::load_default().unwrap();
        let guard = Guard::new(Policy::default()).unwrap();
        let plan = build(
            &s,
            &rules,
            &guard,
            &PlanOptions {
                min: 1 << 20,
                ..Default::default()
            },
        );

        assert_eq!(
            plan.items.len(),
            1,
            "only the node_modules root, not its children"
        );
        assert!(plan.items[0].path.ends_with("node_modules"));
        assert_eq!(plan.items[0].action, Action::Trash);

        std::fs::remove_dir_all(&tmp).ok();
    }

    /// A retention window must protect recent data even when the rule matches
    /// the whole tree — archiving a session you are still in is data loss.
    #[test]
    fn retention_window_spares_recently_touched_subtrees() {
        let tmp = std::env::temp_dir().join(format!("diskwise-retain-{}", std::process::id()));
        let old = tmp.join("2020/01");
        let fresh = tmp.join("2026/08");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&fresh).unwrap();
        std::fs::write(old.join("a.bin"), vec![0u8; 2_000_000]).unwrap();
        std::fs::write(fresh.join("b.bin"), vec![0u8; 2_000_000]).unwrap();
        // Backdate the old branch by a year.
        let year_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(365 * 86_400);
        filetime_set(&old.join("a.bin"), year_ago);

        let s = crate::scan::scan(&tmp);
        let cutoff = now_secs() - 30 * 86_400;
        let parts = settled_parts(&s, &s.root, cutoff);
        let paths: Vec<String> = parts.iter().map(|p| p.path.display().to_string()).collect();

        assert_eq!(parts.len(), 1, "only the settled branch: {paths:?}");
        assert!(
            paths[0].ends_with("2020"),
            "takes the whole settled branch, not leaf by leaf: {paths:?}"
        );

        std::fs::remove_dir_all(&tmp).ok();
    }

    /// A directory holding both live and stale files should still give up the
    /// stale half, rather than being skipped because one file is recent.
    #[test]
    fn mixed_directories_yield_a_file_filtered_part() {
        let tmp = std::env::temp_dir().join(format!("diskwise-mixed-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("old.bin"), vec![0u8; 3_000_000]).unwrap();
        std::fs::write(tmp.join("live.bin"), vec![0u8; 1_000_000]).unwrap();
        let year_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(365 * 86_400);
        filetime_set(&tmp.join("old.bin"), year_ago);

        let s = crate::scan::scan(&tmp);
        let cutoff = now_secs() - 30 * 86_400;
        let parts = settled_parts(&s, &s.root, cutoff);

        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].older_than, Some(cutoff));
        assert!(
            parts[0].size >= 3_000_000 && parts[0].size < 4_000_000,
            "only the stale file"
        );

        std::fs::remove_dir_all(&tmp).ok();
    }

    fn now_secs() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }

    /// `filetime` would be another dependency for one line of test setup.
    fn filetime_set(path: &std::path::Path, when: std::time::SystemTime) {
        let f = std::fs::File::options().write(true).open(path).unwrap();
        f.set_times(
            std::fs::FileTimes::new()
                .set_accessed(when)
                .set_modified(when),
        )
        .unwrap();
    }
}
