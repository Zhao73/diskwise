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
    /// Newest mtime anywhere in this subtree, seconds since the epoch. Used to
    /// honour retention windows so live data is never archived out from under you.
    pub newest: i64,
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

/// Everything the walker threads accumulate, behind one lock. Aggregating as
/// we go rather than collecting per-directory results and folding them at the
/// end keeps peak memory to roughly the size of the finished index.
#[derive(Default)]
struct Agg {
    dirs: HashMap<PathBuf, DirEntry>,
    big_files: Vec<FileEntry>,
    /// Inodes of multiply-linked files, so the same blocks are counted once.
    seen_links: HashSet<(u64, u64)>,
    denied: usize,
    scanned_files: u64,
}

impl Agg {
    /// Fold one directory's own files in, and roll the bytes up to every
    /// ancestor as far as the scan root.
    fn absorb(&mut self, path: PathBuf, own: u64, files: u64, newest: i64, root: &Path) {
        let e = self.dirs.entry(path.clone()).or_default();
        e.own += own;
        e.files += files;
        e.newest = e.newest.max(newest);
        self.scanned_files += files;

        let mut cur: Option<&Path> = Some(path.as_path());
        while let Some(p) = cur {
            // Look up before inserting: nearly every ancestor already exists,
            // and `entry()` would allocate an owned key for each of them. Over a
            // few hundred thousand directories that is millions of throwaway
            // allocations, and it shows up as both time and peak memory.
            match self.dirs.get_mut(p) {
                Some(anc) => {
                    anc.total += own;
                    anc.newest = anc.newest.max(newest);
                }
                None => {
                    self.dirs.insert(
                        p.to_path_buf(),
                        DirEntry {
                            total: own,
                            own: 0,
                            files: 0,
                            newest,
                        },
                    );
                }
            }
            if p == root {
                break;
            }
            cur = p.parent();
        }
    }
}

// ponytail: jwalk allocates a DirEntry per file, so peak RSS tracks the file
// count at roughly 280 bytes each — about 850 MB for a 3M-file home directory,
// released when the scan ends. The finished index is only ~90 MB of that. If
// that ceiling ever matters, the fix is a hand-rolled readdir loop that stats
// in place instead of materialising an entry per file.
pub fn scan_with(root: &Path, big_file_threshold: u64) -> Scan {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let agg: Arc<Mutex<Agg>> = Arc::new(Mutex::new(Agg::default()));
    agg.lock()
        .unwrap()
        .dirs
        .insert(root.clone(), DirEntry::default());

    // All the stat() calls happen here, on the walker's thread pool. Doing them
    // in the consuming loop instead pins the whole scan to one core.
    let sink = Arc::clone(&agg);
    let scan_root = root.clone();
    let walk = jwalk::WalkDirGeneric::<((), ())>::new(&root)
        .follow_links(false)
        .skip_hidden(false)
        .parallelism(jwalk::Parallelism::RayonDefaultPool {
            busy_timeout: std::time::Duration::from_secs(1),
        })
        .process_read_dir(move |_depth, path, _state, children| {
            let mut own = 0u64;
            let mut files = 0u64;
            let mut newest = 0i64;
            let mut denied = 0usize;
            let mut bigs: Vec<FileEntry> = Vec::new();
            let mut linked: Vec<(u64, u64, u64)> = Vec::new();

            for child in children.iter_mut() {
                let child = match child {
                    Ok(c) => c,
                    Err(_) => {
                        denied += 1;
                        continue;
                    }
                };
                if !child.file_type.is_file() {
                    continue;
                }
                let md = match child.metadata() {
                    Ok(m) => m,
                    Err(_) => {
                        denied += 1;
                        continue;
                    }
                };
                let bytes = disk_bytes(&md);
                own += bytes;
                files += 1;
                newest = newest.max(md.mtime());
                if md.nlink() > 1 {
                    linked.push((md.dev(), md.ino(), bytes));
                }
                if bytes >= big_file_threshold {
                    bigs.push(FileEntry {
                        path: path.join(child.file_name()),
                        size: bytes,
                        mtime: md.mtime(),
                    });
                }
            }
            // Files are fully accounted for; only directories need walking.
            children.retain(|c| c.as_ref().map(|c| c.file_type.is_dir()).unwrap_or(false));

            let mut agg = sink.lock().unwrap();
            for (dev, ino, bytes) in linked {
                // ponytail: whichever thread arrives first owns the blocks. Which
                // directory that is isn't deterministic; the total is.
                if !agg.seen_links.insert((dev, ino)) {
                    own = own.saturating_sub(bytes);
                }
            }
            agg.denied += denied;
            agg.big_files.append(&mut bigs);
            agg.absorb(path.to_path_buf(), own, files, newest, &scan_root);
        });

    // Draining the iterator is what drives the walk.
    let mut denied = 0usize;
    for entry in walk {
        if entry.is_err() {
            denied += 1;
        }
    }

    let mut agg = std::mem::take(&mut *agg.lock().unwrap());
    agg.big_files.sort_by(|a, b| b.size.cmp(&a.size));
    Scan {
        root,
        dirs: agg.dirs,
        big_files: agg.big_files,
        denied: denied + agg.denied,
        scanned_files: agg.scanned_files,
    }
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
        out.push(FileEntry {
            path: entry.path(),
            size: disk_bytes(&md),
            mtime: md.mtime(),
        });
    }
    out.sort_by(|a, b| b.size.cmp(&a.size));
    Ok(out)
}

impl Scan {
    /// A placeholder for a root that has not been scanned yet.
    pub fn empty(root: PathBuf) -> Scan {
        Scan {
            root,
            ..Default::default()
        }
    }

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
        assert_eq!(
            s.dirs[&root.join("a/b/c")].own,
            s.dirs[&root.join("a/b/c")].total
        );
        assert_eq!(s.dirs[&root.join("a")].own, 0);
        assert_eq!(s.big_files.len(), 1);
        assert_eq!(s.big_files[0].path, root.join("a/b/c/f.bin"));
        assert_eq!(list_files(&root.join("a/b/c")).unwrap().len(), 1);
        assert!(list_files(&root.join("a")).unwrap().is_empty());

        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
