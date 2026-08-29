//! Looking inside things macOS shows as one opaque lump.
//!
//! A Docker disk image really is a single 23 GB file; a simulator directory is
//! a pile of UUIDs. Neither can be understood by walking the filesystem, so
//! diskwise asks the tool that owns them.
//!
//! Rules name an inspector by id, and ids resolve to commands defined *here*.
//! A rule file can never introduce a command of its own.

use std::path::Path;
use std::process::Command;

use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Inspection {
    pub kind: String,
    /// False when the owning tool isn't running or installed. The space is
    /// still occupied either way, which is the point worth making.
    pub available: bool,
    pub note: String,
    pub rows: Vec<Row>,
}

#[derive(Debug, Serialize)]
pub struct Row {
    pub label: String,
    pub size: String,
    pub detail: String,
}

pub fn run(kind: &str, path: &Path) -> Result<Inspection> {
    match kind {
        "docker" => Ok(docker()),
        "simulators" => Ok(simulators(path)),
        other => Ok(Inspection {
            kind: other.into(),
            available: false,
            note: format!("no inspector named {other}"),
            rows: vec![],
        }),
    }
}

fn out(cmd: &mut Command) -> Option<String> {
    let o = cmd.output().ok()?;
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

/// `docker system df` is the only thing that can say what is inside the disk
/// image — and it needs the daemon running to say it.
fn docker() -> Inspection {
    let Some(text) = out(Command::new("docker").args(["system", "df", "--format", "json"])) else {
        return Inspection {
            kind: "docker".into(),
            available: false,
            note: "Docker isn't running, so nothing can read inside the disk image. \
                   The file occupies its space regardless — start Docker Desktop to see \
                   the breakdown, or if you no longer use Docker, uninstalling it \
                   reclaims the whole image."
                .into(),
            rows: vec![],
        };
    };

    let mut rows = Vec::new();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
        if s("Type").is_empty() {
            continue;
        }
        rows.push(Row {
            label: s("Type"),
            size: s("Size"),
            detail: format!(
                "{} total, {} active, {} reclaimable",
                s("TotalCount"),
                s("Active"),
                s("Reclaimable")
            ),
        });
    }
    Inspection {
        kind: "docker".into(),
        available: true,
        note: "Reclaim with `docker system prune -a --volumes`. On macOS the disk image \
               does not shrink by itself afterwards — use Docker Desktop's \
               Troubleshoot → Clean / Purge data, or delete and recreate the VM."
            .into(),
        rows,
    }
}

/// Simulator directories are named by UUID. `simctl` knows which device each is.
fn simulators(path: &Path) -> Inspection {
    let Some(text) = out(Command::new("xcrun").args(["simctl", "list", "devices", "--json"]))
    else {
        return Inspection {
            kind: "simulators".into(),
            available: false,
            note: "Xcode command line tools are not available, so device UUIDs cannot be \
                   resolved to names."
                .into(),
            rows: vec![],
        };
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Inspection {
            kind: "simulators".into(),
            available: false,
            note: "simctl returned something unexpected".into(),
            rows: vec![],
        };
    };

    let mut rows = Vec::new();
    if let Some(runtimes) = v["devices"].as_object() {
        for (runtime, devices) in runtimes {
            for d in devices.as_array().unwrap_or(&vec![]) {
                let udid = d["udid"].as_str().unwrap_or_default();
                let dir = path.join(udid);
                if !dir.exists() {
                    continue;
                }
                let size = crate::scan::scan(&dir).total();
                if size < 50 << 20 {
                    continue;
                }
                rows.push(Row {
                    label: d["name"].as_str().unwrap_or(udid).to_string(),
                    size: humansize::format_size(size, humansize::DECIMAL),
                    detail: format!(
                        "{} · {}",
                        runtime.rsplit('.').next().unwrap_or(runtime),
                        if d["isAvailable"].as_bool().unwrap_or(false) {
                            "available"
                        } else {
                            "unavailable"
                        }
                    ),
                });
            }
        }
    }
    rows.sort_by(|a, b| b.size.len().cmp(&a.size.len()).then(b.size.cmp(&a.size)));
    Inspection {
        kind: "simulators".into(),
        available: true,
        note: "`xcrun simctl delete unavailable` removes devices for runtimes you no longer \
               have. Deleting a device also deletes the apps and data inside it."
            .into(),
        rows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An inspector whose tool isn't running must still explain the situation
    /// rather than look like a failure.
    #[test]
    fn a_missing_tool_is_reported_not_hidden() {
        let i = run("nonesuch", Path::new("/tmp")).unwrap();
        assert!(!i.available);
        assert!(i.note.contains("nonesuch"));

        let d = docker();
        assert_eq!(d.kind, "docker");
        // Either the daemon answered, or the note says why it could not.
        assert!(d.available || d.note.contains("Docker isn't running"));
    }
}
