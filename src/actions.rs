//! Doing the thing. Every destructive path here goes through `policy::Guard`
//! first, and archives are verified before the original is given up.

use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::policy::Guard;
use crate::rules::home_dir;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    /// tar.zst it somewhere safe, verify, then trash the original.
    Archive,
    /// Move to the macOS Trash. Recoverable until the Trash is emptied.
    Trash,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanItem {
    pub path: PathBuf,
    pub size: u64,
    pub action: Action,
    pub reason: String,
    /// When set, only files last modified before this unix timestamp are taken;
    /// anything newer stays exactly where it is. Lets a directory that mixes
    /// live and stale data still be partly reclaimed.
    #[serde(default)]
    pub older_than: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: String,
    pub created: u64,
    pub items: Vec<PlanItem>,
}

impl Plan {
    pub fn total(&self) -> u64 {
        self.items.iter().map(|i| i.size).sum()
    }

    pub fn paths(&self) -> Vec<PathBuf> {
        self.items.iter().map(|i| i.path.clone()).collect()
    }

    pub fn save(&self) -> Result<PathBuf> {
        let dir = home_dir().join(".diskwise/plans");
        std::fs::create_dir_all(&dir)?;
        let p = dir.join(format!("{}.json", self.id));
        std::fs::write(&p, serde_json::to_vec_pretty(self)?)?;
        Ok(p)
    }

    pub fn load(id: &str) -> Result<Plan> {
        let p = home_dir()
            .join(".diskwise/plans")
            .join(format!("{id}.json"));
        let raw = std::fs::read(&p).with_context(|| format!("no such plan: {id}"))?;
        Ok(serde_json::from_slice(&raw)?)
    }
}

/// Outcome of applying one item, so a partial failure still reports honestly.
#[derive(Debug, Serialize)]
pub struct Outcome {
    pub path: PathBuf,
    pub action: Action,
    pub freed: u64,
    pub archive: Option<PathBuf>,
    pub error: Option<String>,
}

pub fn apply(plan: &Plan, guard: &Guard) -> Vec<Outcome> {
    plan.items
        .iter()
        .map(|item| {
            let mut out = Outcome {
                path: item.path.clone(),
                action: item.action,
                freed: 0,
                archive: None,
                error: None,
            };
            let res = match guard.check(&item.path) {
                Err(d) => Err(anyhow!("{d}")),
                Ok(()) => match item.action {
                    Action::Trash if item.older_than.is_none() => trash(&item.path).map(|_| None),
                    Action::Trash => {
                        trash_older_than(&item.path, item.older_than.unwrap()).map(|_| None)
                    }
                    Action::Archive => archive_filtered(&item.path, item.older_than).map(Some),
                },
            };
            match res {
                Ok(archive) => {
                    out.freed = item.size;
                    out.archive = archive;
                }
                Err(e) => out.error = Some(e.to_string()),
            }
            out
        })
        .collect()
}

pub fn trash(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!("{} no longer exists", path.display());
    }
    trash::delete(path).with_context(|| format!("moving {} to the Trash", path.display()))?;
    Ok(())
}

pub fn archives_dir() -> PathBuf {
    home_dir().join(".diskwise/archives")
}

/// tar + zstd a directory, verify every entry reads back, then trash the source.
/// The verify step is not optional — an archive nobody has read is not a backup.
pub fn archive(src: &Path) -> Result<PathBuf> {
    archive_filtered(src, None)
}

/// As `archive`, but when `older_than` is set only files modified before that
/// timestamp are taken, and only those files are released afterwards.
pub fn archive_filtered(src: &Path, older_than: Option<i64>) -> Result<PathBuf> {
    if !src.is_dir() {
        bail!(
            "{} is not a directory; archiving is for directory trees",
            src.display()
        );
    }
    let dir = archives_dir();
    std::fs::create_dir_all(&dir)?;
    let name = format!("{}-{}", slug(src), stamp(now()));
    let out = dir.join(format!("{name}.tar.zst"));
    if out.exists() {
        bail!("{} already exists", out.display());
    }

    let base = PathBuf::from(src.file_name().ok_or_else(|| anyhow!("bad source path"))?);
    let taken: Vec<PathBuf> = match older_than {
        None => vec![],
        Some(cutoff) => {
            let files = stale_files(src, cutoff);
            if files.is_empty() {
                bail!(
                    "nothing in {} is older than the retention window",
                    src.display()
                );
            }
            files
        }
    };

    let file = std::fs::File::create(&out)?;
    let enc = zstd::Encoder::new(file, 10)?;
    let mut tar = tar::Builder::new(enc);
    tar.follow_symlinks(false);
    let mut build = || -> Result<()> {
        if older_than.is_none() {
            tar.append_dir_all(&base, src)?;
        } else {
            for f in &taken {
                let rel = base.join(f.strip_prefix(src)?);
                tar.append_path_with_name(f, rel)?;
            }
        }
        Ok(())
    };
    let built = build();
    let finished = tar.into_inner().and_then(|e| e.finish());
    built.with_context(|| format!("archiving {}", src.display()))?;
    finished?.sync_all()?;

    let (entries, bytes) = verify(&out).with_context(|| format!("verifying {}", out.display()))?;
    let manifest = Manifest {
        source: src.to_path_buf(),
        created: now(),
        entries,
        uncompressed: bytes,
        compressed: std::fs::metadata(&out)?.len(),
    };
    std::fs::write(
        dir.join(format!("{name}.index.json")),
        serde_json::to_vec_pretty(&manifest)?,
    )?;

    // Only now is the original expendable.
    match older_than {
        None => trash(src)?,
        Some(_) => release(&taken, src)?,
    }
    Ok(out)
}

/// Every file under `dir` last modified before `cutoff`.
fn stale_files(dir: &Path, cutoff: i64) -> Vec<PathBuf> {
    use std::os::unix::fs::MetadataExt;
    jwalk::WalkDir::new(dir)
        .follow_links(false)
        .skip_hidden(false)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| {
            let md = e.metadata().ok()?;
            (md.mtime() < cutoff).then(|| e.path())
        })
        .collect()
}

/// Move a specific set of files to the Trash together, by staging them in one
/// directory first — 300 individual Trash calls is slow and can half-fail.
fn release(files: &[PathBuf], src: &Path) -> Result<()> {
    let staging = home_dir().join(format!(".diskwise/staging-{}", now()));
    for f in files {
        let rel = f.strip_prefix(src)?;
        let dest = staging.join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(f, &dest)
            .with_context(|| format!("staging {} for the Trash", f.display()))?;
    }
    trash(&staging)
}

/// Trash only the files under `dir` older than `cutoff`, leaving the rest.
pub fn trash_older_than(dir: &Path, cutoff: i64) -> Result<()> {
    let files = stale_files(dir, cutoff);
    if files.is_empty() {
        bail!(
            "nothing in {} is older than the retention window",
            dir.display()
        );
    }
    release(&files, dir)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub source: PathBuf,
    pub created: u64,
    pub entries: u64,
    pub uncompressed: u64,
    pub compressed: u64,
}

/// Read the whole archive back, decompressing every byte. Returns (entries, bytes).
fn verify(archive: &Path) -> Result<(u64, u64)> {
    let file = std::fs::File::open(archive)?;
    let dec = zstd::Decoder::new(file)?;
    let mut tar = tar::Archive::new(dec);
    let mut entries = 0u64;
    let mut bytes = 0u64;
    let mut sink = [0u8; 64 * 1024];
    for entry in tar.entries()? {
        let mut entry = entry?;
        entries += 1;
        loop {
            let n = entry.read(&mut sink)?;
            if n == 0 {
                break;
            }
            bytes += n as u64;
        }
    }
    if entries == 0 {
        bail!("archive is empty");
    }
    Ok((entries, bytes))
}

/// Unpack an archive back into `dest` (defaults to the original location's parent).
pub fn restore(archive: &Path, dest: Option<&Path>) -> Result<PathBuf> {
    let manifest_path = archive.with_extension("").with_extension("index.json");
    let dest = match dest {
        Some(d) => d.to_path_buf(),
        None => {
            let raw = std::fs::read(&manifest_path).with_context(|| {
                format!("need --to: no manifest at {}", manifest_path.display())
            })?;
            let m: Manifest = serde_json::from_slice(&raw)?;
            m.source
                .parent()
                .map(Path::to_path_buf)
                .ok_or_else(|| anyhow!("bad manifest source"))?
        }
    };
    std::fs::create_dir_all(&dest)?;
    let file = std::fs::File::open(archive)?;
    let mut tar = tar::Archive::new(zstd::Decoder::new(file)?);
    tar.unpack(&dest)?;
    Ok(dest)
}

pub fn list_archives() -> Result<Vec<(PathBuf, Option<Manifest>)>> {
    let dir = archives_dir();
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    for e in std::fs::read_dir(&dir)? {
        let p = e?.path();
        if p.extension().is_some_and(|x| x == "zst") {
            let m = std::fs::read(p.with_extension("").with_extension("index.json"))
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok());
            out.push((p, m));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

pub fn new_plan_id() -> String {
    format!("{}", now())
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A readable archive name: the path relative to home, last few components,
/// e.g. `~/.codex/sessions/2026/06/09` -> `codex-sessions-2026-06-09`.
fn slug(p: &Path) -> String {
    let rel = p.strip_prefix(home_dir()).unwrap_or(p);
    let parts: Vec<String> = rel
        .components()
        .map(|c| {
            c.as_os_str()
                .to_string_lossy()
                .trim_start_matches('.')
                .to_string()
        })
        .filter(|s| !s.is_empty())
        .collect();
    let tail = parts[parts.len().saturating_sub(5)..].join("-");
    tail.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '.' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

#[cfg(test)]
mod slug_tests {
    use super::*;

    #[test]
    fn slug_reads_like_the_path_it_came_from() {
        assert_eq!(
            slug(&home_dir().join(".codex/sessions/2026/06/09")),
            "codex-sessions-2026-06-09"
        );
        assert_eq!(
            slug(&home_dir().join("App/proj/node_modules")),
            "App-proj-node_modules"
        );
    }
}

/// `YYYYMMDD-HHMM` in UTC, so archive names sort chronologically.
/// Days-to-civil-date is Howard Hinnant's algorithm; it is exact for all dates.
fn stamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let tod = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}",
        tod / 3600,
        (tod % 3600) / 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamp_matches_known_dates() {
        assert_eq!(stamp(0), "19700101-0000");
        assert_eq!(stamp(1_609_459_200), "20210101-0000"); // 2021-01-01T00:00Z
        assert_eq!(stamp(1_583_020_800), "20200301-0000"); // day after a leap day
        assert_eq!(stamp(1_756_425_600 + 3661), "20250829-0101");
    }

    #[test]
    fn archive_verifies_before_it_trashes_and_restores_byte_for_byte() {
        let tmp = std::env::temp_dir().join(format!("diskwise-arch-{}", std::process::id()));
        let src = tmp.join("payload");
        std::fs::create_dir_all(src.join("nested")).unwrap();
        std::fs::write(src.join("a.txt"), "hello ".repeat(5000)).unwrap();
        std::fs::write(src.join("nested/b.bin"), vec![3u8; 200_000]).unwrap();

        let archive = super::archive(&src).unwrap();
        assert!(archive.exists());
        assert!(
            !src.exists(),
            "source is only given up after a successful verify"
        );

        let back = tmp.join("restored");
        restore(&archive, Some(&back)).unwrap();
        assert_eq!(
            std::fs::read(back.join("payload/nested/b.bin")).unwrap(),
            vec![3u8; 200_000]
        );
        assert_eq!(
            std::fs::read_to_string(back.join("payload/a.txt")).unwrap(),
            "hello ".repeat(5000)
        );

        std::fs::remove_file(&archive).ok();
        std::fs::remove_file(archive.with_extension("").with_extension("index.json")).ok();
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn apply_refuses_protected_paths_even_when_the_plan_asks() {
        let guard = Guard::load().unwrap();
        let plan = Plan {
            id: "test".into(),
            created: 0,
            items: vec![PlanItem {
                path: home_dir().join(".ssh"),
                size: 1,
                action: Action::Trash,
                reason: "a malicious or buggy plan".into(),
                older_than: None,
            }],
        };
        let out = apply(&plan, &guard);
        assert_eq!(out[0].freed, 0);
        assert!(out[0].error.as_ref().unwrap().contains("protected"));
        assert!(home_dir().join(".ssh").exists(), "must still be there");
    }
}
