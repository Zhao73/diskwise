//! Parallel disk scan. Sizes are *allocated blocks*, not apparent size, so the
//! numbers line up with `du` on APFS (clones and sparse files lie otherwise).

use std::collections::{HashMap, HashSet};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Scan {
    pub root: PathBuf,
    /// Every directory that contains anything, with its recursive total.
    pub dirs: HashMap<PathBuf, DirEntry>,
    /// Individual files at or above `big_file_threshold`, largest first.
    /// Small files are only counted, not listed — drill into a directory with
    /// `list_files` when you actually need them.
    pub big_files: Vec<FileEntry>,
    pub denied: usize,
    pub scanned_files: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: PathBuf,
    pub size: u64,
    /// Seconds since the unix epoch.
    pub mtime: i64,
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
pub struct DirEntry {
    /// Recursive bytes on disk, including all descendants.
    pub total: u64,
    /// Bytes of files sitting directly in this directory.
    pub own: u64,
    pub files: u64,
}

/// Bytes actually allocated on disk. `st_blocks` is always 512-byte units per
/// POSIX, regardless of the filesystem's own block size.
fn disk_bytes(md: &std::fs::Metadata) -> u64 {
    md.blocks() * 512
}

/// Files smaller than this are counted but not individually indexed.
pub const BIG_FILE_THRESHOLD: u64 = 1 << 20;

pub fn scan(root: &Path) -> Scan {
    scan_with(root, BIG_FILE_THRESHOLD)
}

/// One directory's worth of work, produced on a walker thread.
#[derive(Default)]
struct DirWork {
    path: PathBuf,
    own: u64,
    files: u64,
    bigs: Vec<FileEntry>,
    /// (dev, ino, bytes) for files with more than one link, so the roll-up can
    /// subtract the duplicates instead of counting the same blocks twice.
    linked: Vec<(u64, u64, u64)>,
    denied: usize,
}

pub fn scan_with(root: &Path, big_file_threshold: u64) -> Scan {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let collected: Arc<Mutex<Vec<DirWork>>> = Arc::new(Mutex::new(Vec::new()));

    // All the stat() calls happen here, on the walker's thread pool. Doing them
    // in the consuming loop instead pins the whole scan to one core.
    let sink = Arc::clone(&collected);
    let walk = jwalk::WalkDirGeneric::<((), ())>::new(&root)
        .follow_links(false)
        .skip_hidden(false)
        .parallelism(jwalk::Parallelism::RayonDefaultPool {
            busy_timeout: std::time::Duration::from_secs(1),
        })
        .process_read_dir(move |_depth, path, _state, children| {
            let mut work = DirWork { path: path.to_path_buf(), ..Default::default() };
            for child in children.iter_mut() {
                let child = match child {
                    Ok(c) => c,
                    Err(_) => {
                        work.denied += 1;
                        continue;
                    }
                };
                if !child.file_type.is_file() {
                    continue;
                }
                let md = match child.metadata() {
                    Ok(m) => m,
                    Err(_) => {
                        work.denied += 1;
                        continue;
                    }
                };
                let bytes = disk_bytes(&md);
                work.own += bytes;
                work.files += 1;
                if md.nlink() > 1 {
                    work.linked.push((md.dev(), md.ino(), bytes));
                }
                if bytes >= big_file_threshold {
                    work.bigs.push(FileEntry {
                        path: path.join(child.file_name()),
                        size: bytes,
                        mtime: md.mtime(),
                    });
                }
            }
            // Files are fully accounted for; only directories need to be walked.
            children.retain(|c| c.as_ref().map(|c| c.file_type.is_dir()).unwrap_or(false));
            sink.lock().unwrap().push(work);
        });

    // Draining the iterator is what drives the walk.
    let mut denied = 0usize;
    for entry in walk {
        if entry.is_err() {
            denied += 1;
        }
    }

    let works = std::mem::take(&mut *collected.lock().unwrap());
    let mut dirs: HashMap<PathBuf, DirEntry> = HashMap::new();
    let mut big_files: Vec<FileEntry> = Vec::new();
    let mut seen_inodes: HashSet<(u64, u64)> = HashSet::new();
    let mut scanned_files = 0u64;

    for w in &works {
        denied += w.denied;
        scanned_files += w.files;
        let mut own = w.own;
        for (dev, ino, bytes) in &w.linked {
            // ponytail: whichever thread got there first owns the blocks; which
            // directory that is isn't deterministic, but the total is.
            if !seen_inodes.insert((*dev, *ino)) {
                own = own.saturating_sub(*bytes);
            }
        }
        let e = dirs.entry(w.path.clone()).or_default();
        e.own += own;
        e.files += w.files;
        // Roll the bytes up through every ancestor, stopping at the scan root.
        let mut cur: Option<&Path> = Some(w.path.as_path());
        while let Some(p) = cur {
            dirs.entry(p.to_path_buf()).or_default().total += own;
            if p == root {
                break;
            }
            cur = p.parent();
        }
    }
    for w in works {
        big_files.extend(w.bigs);
    }
    big_files.sort_by(|a, b| b.size.cmp(&a.size));

    Scan { root, dirs, big_files, denied, scanned_files }
}

/// Every file directly inside `dir`, largest first. Read live, so it also works
/// for the small files the index deliberately skips.
pub fn list_files(dir: &Path) -> std::io::Result<Vec<FileEntry>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let md = match entry.metadata() {
            Ok(m) if m.is_file() => m,
            _ => continue,
        };
        out.push(FileEntry { path: entry.path(), size: disk_bytes(&md), mtime: md.mtime() });
    }
    out.sort_by(|a, b| b.size.cmp(&a.size));
    Ok(out)
}

impl Scan {
    pub fn total(&self) -> u64 {
        self.dirs.get(&self.root).map(|d| d.total).unwrap_or(0)
    }

    /// Direct children of `dir`, largest first.
    pub fn children(&self, dir: &Path) -> Vec<(PathBuf, DirEntry)> {
        let mut out: Vec<_> = self
            .dirs
            .iter()
            .filter(|(p, _)| p.parent() == Some(dir))
            .map(|(p, e)| (p.clone(), *e))
            .collect();
        out.sort_by(|a, b| b.1.total.cmp(&a.1.total));
        out
    }

    /// Every directory, largest first — used to find offenders anywhere in the tree.
    pub fn ranked(&self) -> Vec<(PathBuf, DirEntry)> {
        let mut out: Vec<_> = self.dirs.iter().map(|(p, e)| (p.clone(), *e)).collect();
        out.sort_by(|a, b| b.1.total.cmp(&a.1.total));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolls_sizes_up_to_ancestors() {
        let tmp = std::env::temp_dir().join(format!("diskwise-test-{}", std::process::id()));
        let deep = tmp.join("a/b/c");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("f.bin"), vec![7u8; 2_000_000]).unwrap();

        let s = scan(&tmp);
        let root = tmp.canonicalize().unwrap();
        assert_eq!(s.scanned_files, 1);
        // Same bytes visible at every level of the chain.
        assert!(s.total() >= 2_000_000);
        assert_eq!(s.dirs[&root].total, s.dirs[&root.join("a/b/c")].total);
        assert_eq!(s.dirs[&root.join("a/b/c")].own, s.dirs[&root.join("a/b/c")].total);
        assert_eq!(s.dirs[&root.join("a")].own, 0);
        assert_eq!(s.big_files.len(), 1);
        assert_eq!(s.big_files[0].path, root.join("a/b/c/f.bin"));
        assert_eq!(list_files(&root.join("a/b/c")).unwrap().len(), 1);
        assert!(list_files(&root.join("a")).unwrap().is_empty());

        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
