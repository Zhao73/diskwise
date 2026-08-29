//! Explaining every large folder, in the background, without you waiting.
//!
//! The moment a scan finishes this starts working: evidence first (instant and
//! free), then one batched agent call for whatever is left. Answers land in a
//! cache on disk and stream into the page as they arrive — nothing blocks, and
//! nothing is asked twice.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::identify::{self, Evidence};
use crate::rules::{home_dir, Rules};
use crate::scan::Scan;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Annotation {
    pub text: String,
    /// Simplified Chinese, produced in the same call rather than a second one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_zh: Option<String>,
    /// "evidence" when a file said so, "agent" when a model inferred it.
    pub source: String,
    /// Facts behind the claim, so it can be checked rather than believed.
    #[serde(default)]
    pub markers: Vec<String>,
    /// Size and mtime when this was written; a material change re-asks.
    pub fingerprint: String,
}

#[derive(Default, Serialize, Deserialize)]
pub struct Store {
    #[serde(default)]
    pub items: HashMap<PathBuf, Annotation>,
}

pub fn cache_path() -> PathBuf {
    home_dir().join(".diskwise/annotations.json")
}

impl Store {
    pub fn load() -> Store {
        std::fs::File::open(cache_path())
            .ok()
            .and_then(|f| serde_json::from_reader(std::io::BufReader::new(f)).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let p = cache_path();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(f) = std::fs::File::create(&p) {
            let _ = serde_json::to_writer(std::io::BufWriter::new(f), self);
        }
    }

    fn is_current(&self, path: &Path, fingerprint: &str) -> bool {
        self.items
            .get(path)
            .is_some_and(|a| a.fingerprint == fingerprint)
    }
}

/// Live progress, so the page can show that work is happening.
#[derive(Default)]
pub struct Progress {
    pub running: AtomicBool,
    pub done: AtomicUsize,
    pub total: AtomicUsize,
    /// Set when the agent call failed, so the UI can say why rather than
    /// silently showing nothing.
    pub error: Mutex<Option<String>>,
}

#[derive(Serialize)]
pub struct Status {
    pub running: bool,
    pub done: usize,
    pub total: usize,
    pub error: Option<String>,
}

impl Progress {
    pub fn status(&self) -> Status {
        Status {
            running: self.running.load(Ordering::Relaxed),
            done: self.done.load(Ordering::Relaxed),
            total: self.total.load(Ordering::Relaxed),
            error: self.error.lock().unwrap().clone(),
        }
    }
}

fn fingerprint(s: &Scan, path: &Path) -> String {
    s.dirs
        .get(path)
        .map(|d| format!("{}:{}", d.total, d.newest))
        .unwrap_or_default()
}

/// Directories worth explaining: big, real, and not already named by a rule.
fn candidates(s: &Scan, rules: &Rules, limit: usize) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = s
        .children(&s.root)
        .into_iter()
        .chain(s.ranked().into_iter().take(300))
        .filter(|(p, d)| d.total > 300 << 20 && p != &s.root && rules.classify(p).is_none())
        .map(|(p, _)| p)
        .collect();
    out.sort();
    out.dedup();
    // Largest first, so the folders someone is actually looking at land first.
    out.sort_by_key(|p| std::cmp::Reverse(s.dirs.get(p).map(|d| d.total).unwrap_or(0)));
    out.truncate(limit);
    out
}

/// Kick off annotation in the background. Returns immediately.
pub fn spawn(
    scan: Arc<Scan>,
    rules: Arc<Rules>,
    store: Arc<Mutex<Store>>,
    progress: Arc<Progress>,
    agent: Option<crate::ask::Agent>,
) {
    if progress.running.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        *progress.error.lock().unwrap() = None;
        let paths = candidates(&scan, &rules, 60);
        progress.total.store(paths.len(), Ordering::Relaxed);
        progress.done.store(0, Ordering::Relaxed);

        // Pass one: free. Anything a file can answer is answered now.
        let mut unexplained: Vec<Evidence> = Vec::new();
        for path in paths {
            let fp = fingerprint(&scan, &path);
            if store.lock().unwrap().is_current(&path, &fp) {
                progress.done.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            let ev = identify::gather(&path, &scan);
            match ev.describe() {
                Some(text) => {
                    let a = Annotation {
                        text,
                        text_zh: ev.describe_in("zh"),
                        source: "evidence".into(),
                        markers: ev.markers.clone(),
                        fingerprint: fp,
                    };
                    store.lock().unwrap().items.insert(path.clone(), a);
                    progress.done.fetch_add(1, Ordering::Relaxed);
                }
                None => unexplained.push(ev),
            }
        }
        store.lock().unwrap().save();

        // Pass two: only what is left, batched, on the agent's quota.
        if let Some(agent) = agent {
            for batch in unexplained.chunks(12) {
                match ask_batch(agent, batch, &scan) {
                    Ok(answers) => {
                        let mut st = store.lock().unwrap();
                        for ev in batch {
                            if let Some(pair) = answers.get(&ev.path) {
                                st.items.insert(
                                    ev.path.clone(),
                                    Annotation {
                                        text: pair.0.clone(),
                                        text_zh: pair.1.clone(),
                                        source: "agent".into(),
                                        markers: ev.markers.clone(),
                                        fingerprint: fingerprint(&scan, &ev.path),
                                    },
                                );
                            }
                        }
                        st.save();
                    }
                    Err(e) => {
                        *progress.error.lock().unwrap() = Some(e.to_string());
                        break;
                    }
                }
                progress.done.fetch_add(batch.len(), Ordering::Relaxed);
            }
        } else if !unexplained.is_empty() {
            *progress.error.lock().unwrap() =
                Some("no agent CLI found, so folders without a manifest stay unexplained".into());
        }
        progress.running.store(false, Ordering::SeqCst);
    });
}

/// One agent call for a dozen folders. Asking per-folder would be twelve times
/// the latency and twelve times the fixed prompt cost for the same answer.
fn ask_batch(
    agent: crate::ask::Agent,
    batch: &[Evidence],
    scan: &Scan,
) -> Result<HashMap<PathBuf, (String, Option<String>)>> {
    let mut ctx = String::new();
    for ev in batch {
        let size = scan
            .dirs
            .get(&ev.path)
            .map(|d| humansize::format_size(d.total, humansize::DECIMAL))
            .unwrap_or_default();
        ctx.push_str(&format!("PATH: {}\nSIZE: {size}\n", ev.path.display()));
        if !ev.children.is_empty() {
            ctx.push_str(&format!("CONTAINS: {}\n", ev.children.join(", ")));
        }
        if !ev.extensions.is_empty() {
            ctx.push_str(&format!("FILE TYPES: {}\n", ev.extensions.join(", ")));
        }
        if !ev.markers.is_empty() {
            ctx.push_str(&format!("MARKERS: {}\n", ev.markers.join("; ")));
        }
        ctx.push('\n');
    }

    let prompt = format!(
        "For each PATH below, write one short sentence saying what that folder is and what it \
         is for, based only on the evidence given. Be concrete — name the project or the kind \
         of work, not the obvious. If the evidence does not support a claim, say \"unclear\" \
         rather than guessing.\n\n\
         Give each sentence twice: \"en\" in English and \"zh\" in Simplified Chinese. Both \
         in one pass — do not translate names of projects, files or products.\n\n\
         Reply with nothing but a JSON object of the form \
         {{\"<path>\": {{\"en\": \"…\", \"zh\": \"…\"}}}}. No prose, no code fences.\n\n{ctx}"
    );

    let answer = crate::ask::ask(agent, &prompt, "")?;
    let json = extract_json(&answer.text);
    let map: HashMap<String, HashMap<String, String>> = serde_json::from_str(&json)
        .map_err(|e| anyhow::anyhow!("the agent did not return usable JSON ({e})"))?;
    Ok(map
        .into_iter()
        .filter_map(|(k, v)| {
            let en = v.get("en")?.trim().to_string();
            if en.is_empty() || en.to_lowercase() == "unclear" {
                return None;
            }
            let zh = v
                .get("zh")
                .map(|z| z.trim().to_string())
                .filter(|z| !z.is_empty());
            Some((PathBuf::from(k), (en, zh)))
        })
        .collect())
}

/// Models wrap JSON in prose or fences however much you ask them not to.
fn extract_json(s: &str) -> String {
    let s = s.trim();
    let body = s
        .strip_prefix("```json")
        .or_else(|| s.strip_prefix("```"))
        .unwrap_or(s);
    let body = body.strip_suffix("```").unwrap_or(body);
    match (body.find('{'), body.rfind('}')) {
        (Some(a), Some(b)) if b > a => body[a..=b].to_string(),
        _ => body.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_survives_fences_and_chatter() {
        let want = r#"{"/a":{"en":"a thing","zh":"一个东西"}}"#;
        for wrapped in [
            want.to_string(),
            format!("```json\n{want}\n```"),
            format!("Here you go:\n```\n{want}\n```\nHope that helps."),
        ] {
            let got = extract_json(&wrapped);
            let v: HashMap<String, HashMap<String, String>> = serde_json::from_str(&got)
                .unwrap_or_else(|e| panic!("{wrapped:?} -> {got:?}: {e}"));
            assert_eq!(v["/a"]["en"], "a thing");
            assert_eq!(v["/a"]["zh"], "一个东西");
        }
    }

    /// Rule-classified and small directories are not worth anyone's quota.
    #[test]
    fn candidates_skip_what_the_rules_already_explain() {
        let tmp = std::env::temp_dir().join(format!("diskwise-cand-{}", std::process::id()));
        let nm = tmp.join("proj/node_modules");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::create_dir_all(tmp.join("mystery")).unwrap();
        std::fs::write(nm.join("big.bin"), vec![0u8; 400 << 20]).unwrap();
        std::fs::write(tmp.join("mystery/big.bin"), vec![0u8; 400 << 20]).unwrap();

        let s = crate::scan::scan(&tmp);
        let rules = Rules::load_default().unwrap();
        let c = candidates(&s, &rules, 20);
        let names: Vec<String> = c.iter().map(|p| p.display().to_string()).collect();

        assert!(names.iter().any(|n| n.ends_with("mystery")), "{names:?}");
        assert!(
            !names.iter().any(|n| n.ends_with("node_modules")),
            "{names:?}"
        );
        std::fs::remove_dir_all(&tmp).ok();
    }
}
