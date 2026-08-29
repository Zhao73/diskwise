//! Working out what a directory actually is, from what is in it.
//!
//! Most folders announce themselves: a package.json has a name, a Cargo.toml
//! has a description, a git config has a remote. Reading those is instant, free
//! and — unlike a model — cannot be wrong about it. Only what stays unexplained
//! is worth spending an agent's quota on, and even then the evidence gathered
//! here is what makes that answer specific instead of generic.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::scan::Scan;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Evidence {
    pub path: PathBuf,
    /// "Node project", "Rust crate", "Git repository", … when a marker says so.
    pub kind: Option<String>,
    /// The project's own name, which is often not the folder name.
    pub name: Option<String>,
    /// One line from a description field or a README.
    pub summary: Option<String>,
    /// Human-readable facts, each traceable to a file.
    pub markers: Vec<String>,
    /// Largest sub-directories, for context.
    pub children: Vec<String>,
    /// Most common file extensions at the top level.
    pub extensions: Vec<String>,
}

impl Evidence {
    /// A factual one-liner, or None when the evidence is too thin to say
    /// anything a person could not see from the folder name alone.
    pub fn describe(&self) -> Option<String> {
        self.describe_in("en")
    }

    /// The same sentence with its fixed vocabulary translated. Names and
    /// descriptions stay verbatim — they are the project's own words, and
    /// translating them would be putting words in someone's mouth.
    pub fn describe_in(&self, lang: &str) -> Option<String> {
        let kind = self.kind.as_deref()?;
        let kind = if lang == "zh" { kind_zh(kind) } else { kind };
        let mut s = match (
            &self.name,
            self.name.as_deref() == self.basename().as_deref(),
        ) {
            (Some(n), false) => format!("{kind}「{n}」"),
            _ => kind.to_string(),
        };
        if let Some(sum) = &self.summary {
            s.push_str(" — ");
            s.push_str(sum);
        }
        Some(s)
    }

    fn basename(&self) -> Option<String> {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
    }
}

/// The vocabulary this module can produce is small and fixed, so translating it
/// is a lookup rather than a call to anything.
fn kind_zh(kind: &str) -> &str {
    match kind {
        "Next.js app" => "Next.js 应用",
        "Expo app" => "Expo 应用",
        "React Native app" => "React Native 应用",
        "MCP server" => "MCP 服务",
        "React app" => "React 应用",
        "Node server" => "Node 服务",
        "Node project" => "Node 项目",
        "Rust crate" => "Rust crate",
        "Python project" => "Python 项目",
        "Go module" => "Go 模块",
        "Xcode project" => "Xcode 项目",
        "Swift package" => "Swift 包",
        "Git repository" => "Git 仓库",
        "Project folder" => "项目文件夹",
        other => other,
    }
}

/// Read at most this much of any marker file. These are metadata, not data.
const MAX_READ: usize = 16 * 1024;

fn read_head(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; MAX_READ];
    let n = f.read(&mut buf).ok()?;
    buf.truncate(n);
    String::from_utf8(buf).ok()
}

pub fn gather(path: &Path, scan: &Scan) -> Evidence {
    let mut e = Evidence {
        path: path.to_path_buf(),
        ..Default::default()
    };
    node(path, &mut e);
    rust(path, &mut e);
    python(path, &mut e);
    go(path, &mut e);
    swift(path, &mut e);
    git(path, &mut e);
    readme(path, &mut e);
    context(path, scan, &mut e);
    e
}

fn set_kind(e: &mut Evidence, kind: &str) {
    if e.kind.is_none() {
        e.kind = Some(kind.into());
    }
}

fn node(path: &Path, e: &mut Evidence) {
    let Some(raw) = read_head(&path.join("package.json")) else {
        return;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return;
    };
    // The framework is the useful part: "Node project" says less than "Next.js app".
    let deps: Vec<&str> = ["dependencies", "devDependencies"]
        .iter()
        .filter_map(|k| v[k].as_object())
        .flat_map(|o| o.keys().map(String::as_str))
        .collect();
    let kind = if deps.contains(&"next") {
        "Next.js app"
    } else if deps.contains(&"expo") {
        "Expo app"
    } else if deps.contains(&"react-native") {
        "React Native app"
    } else if deps.contains(&"@modelcontextprotocol/sdk") {
        "MCP server"
    } else if deps.contains(&"react") {
        "React app"
    } else if deps.contains(&"express") || deps.contains(&"fastify") {
        "Node server"
    } else {
        "Node project"
    };
    set_kind(e, kind);
    if let Some(n) = v["name"].as_str() {
        e.name.get_or_insert_with(|| n.to_string());
        e.markers.push(format!("package.json · {n}"));
    }
    if let Some(d) = v["description"].as_str().filter(|d| !d.is_empty()) {
        e.summary.get_or_insert_with(|| d.to_string());
    }
}

fn rust(path: &Path, e: &mut Evidence) {
    let Some(raw) = read_head(&path.join("Cargo.toml")) else {
        return;
    };
    set_kind(e, "Rust crate");
    if let Some(n) = toml_str(&raw, "name") {
        e.name.get_or_insert_with(|| n.clone());
        e.markers.push(format!("Cargo.toml · {n}"));
    }
    if let Some(d) = toml_str(&raw, "description") {
        e.summary.get_or_insert(d);
    }
}

fn python(path: &Path, e: &mut Evidence) {
    if let Some(raw) = read_head(&path.join("pyproject.toml")) {
        set_kind(e, "Python project");
        if let Some(n) = toml_str(&raw, "name") {
            e.name.get_or_insert_with(|| n.clone());
            e.markers.push(format!("pyproject.toml · {n}"));
        }
        if let Some(d) = toml_str(&raw, "description") {
            e.summary.get_or_insert(d);
        }
        return;
    }
    if path.join("requirements.txt").exists() {
        set_kind(e, "Python project");
        e.markers.push("requirements.txt".into());
    }
}

fn go(path: &Path, e: &mut Evidence) {
    let Some(raw) = read_head(&path.join("go.mod")) else {
        return;
    };
    set_kind(e, "Go module");
    if let Some(m) = raw.lines().find_map(|l| l.strip_prefix("module ")) {
        let m = m.trim().to_string();
        e.markers.push(format!("go.mod · {m}"));
        e.name.get_or_insert(m);
    }
}

fn swift(path: &Path, e: &mut Evidence) {
    let xcodeproj = std::fs::read_dir(path).ok().and_then(|d| {
        d.flatten()
            .map(|x| x.file_name().to_string_lossy().into_owned())
            .find(|n| n.ends_with(".xcodeproj") || n.ends_with(".xcworkspace"))
    });
    if let Some(proj) = xcodeproj {
        set_kind(e, "Xcode project");
        e.name
            .get_or_insert_with(|| proj.split('.').next().unwrap_or(&proj).to_string());
        e.markers.push(proj);
    } else if path.join("Package.swift").exists() {
        set_kind(e, "Swift package");
        e.markers.push("Package.swift".into());
    }
}

fn git(path: &Path, e: &mut Evidence) {
    let Some(cfg) = read_head(&path.join(".git/config")) else {
        return;
    };
    set_kind(e, "Git repository");
    if let Some(url) = cfg.lines().find_map(|l| l.trim().strip_prefix("url = ")) {
        e.markers.push(format!("git remote · {}", url.trim()));
    } else {
        e.markers.push("git repository (no remote)".into());
    }
}

/// The first real sentence of a README, which is usually the whole answer.
fn readme(path: &Path, e: &mut Evidence) {
    if e.summary.is_some() {
        return;
    }
    for name in ["README.md", "readme.md", "README", "CLAUDE.md"] {
        let Some(raw) = read_head(&path.join(name)) else {
            continue;
        };
        let line = raw
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with("![") && !l.starts_with("<!--"))
            .map(|l| l.trim_start_matches('#').trim())
            .find(|l| l.len() > 12);
        if let Some(l) = line {
            e.summary = Some(truncate(l, 160));
            e.markers.push(format!("{name} · first line"));
            set_kind(e, "Project folder");
            return;
        }
    }
}

/// Largest children and the extensions in play — the raw material an agent
/// needs when the markers came up empty.
fn context(path: &Path, scan: &Scan, e: &mut Evidence) {
    e.children = scan
        .children(path)
        .into_iter()
        .take(6)
        .map(|(p, d)| {
            format!(
                "{}/ {}",
                p.file_name().unwrap_or_default().to_string_lossy(),
                humansize::format_size(d.total, humansize::DECIMAL)
            )
        })
        .collect();

    let mut counts: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    if let Ok(files) = crate::scan::list_files(path) {
        for f in files.iter().take(400) {
            let ext = f
                .path
                .extension()
                .map(|x| x.to_string_lossy().to_lowercase())
                .unwrap_or_else(|| "(none)".into());
            *counts.entry(ext).or_default() += 1;
        }
    }
    let mut top: Vec<_> = counts.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1));
    e.extensions = top
        .into_iter()
        .take(5)
        .map(|(x, n)| format!(".{x} ×{n}"))
        .collect();
}

fn toml_str(raw: &str, key: &str) -> Option<String> {
    raw.lines()
        .map(str::trim)
        .find_map(|l| l.strip_prefix(key)?.trim().strip_prefix('='))
        .map(|v| v.trim().trim_matches('"').to_string())
        .filter(|v| !v.is_empty() && !v.starts_with('{'))
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    s.chars().take(n).collect::<String>() + "…"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("diskwise-id-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn a_node_project_names_its_framework_not_just_node() {
        let d = fixture("node");
        std::fs::write(
            d.join("package.json"),
            r#"{"name":"work-radar","dependencies":{"next":"15","react":"19"}}"#,
        )
        .unwrap();
        let e = gather(&d, &crate::scan::scan(&d));
        assert_eq!(e.kind.as_deref(), Some("Next.js app"));
        assert_eq!(e.name.as_deref(), Some("work-radar"));
        // The name differs from the folder, so it belongs in the description.
        assert!(
            e.describe().unwrap().contains("work-radar"),
            "{:?}",
            e.describe()
        );
        // The fixed vocabulary translates; the project's own name does not.
        let zh = e.describe_in("zh").unwrap();
        assert!(zh.contains("应用") && zh.contains("work-radar"), "{zh}");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn a_readme_answers_when_no_manifest_does() {
        let d = fixture("readme");
        std::fs::write(
            d.join("README.md"),
            "# Title\n\nA tool for measuring card centering.\n",
        )
        .unwrap();
        let e = gather(&d, &crate::scan::scan(&d));
        assert_eq!(
            e.summary.as_deref(),
            Some("A tool for measuring card centering.")
        );
        std::fs::remove_dir_all(&d).ok();
    }

    /// A folder with nothing to go on must say so rather than invent a story.
    #[test]
    fn an_unexplained_folder_is_marked_thin_but_still_carries_context() {
        let d = fixture("thin");
        std::fs::write(d.join("a.pptx"), "x").unwrap();
        std::fs::write(d.join("b.pptx"), "x").unwrap();
        let e = gather(&d, &crate::scan::scan(&d));
        assert!(e.describe().is_none());
        assert!(
            e.extensions.iter().any(|x| x.starts_with(".pptx")),
            "{:?}",
            e.extensions
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn git_remotes_are_read_from_config() {
        let d = fixture("git");
        std::fs::create_dir_all(d.join(".git")).unwrap();
        std::fs::write(
            d.join(".git/config"),
            "[remote \"origin\"]\n\turl = https://github.com/Zhao73/diskwise\n",
        )
        .unwrap();
        let e = gather(&d, &crate::scan::scan(&d));
        assert_eq!(e.kind.as_deref(), Some("Git repository"));
        assert!(
            e.markers.iter().any(|m| m.contains("Zhao73/diskwise")),
            "{:?}",
            e.markers
        );
        std::fs::remove_dir_all(&d).ok();
    }
}
