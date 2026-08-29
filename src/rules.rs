//! The semantic layer: which directory is what, and whether it can be reclaimed.
//! Rules live in `rules/default.toml` as data so contributors can add coverage
//! without touching Rust.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};

const DEFAULT_RULES: &str = include_str!("../rules/default.toml");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Suggest {
    /// Irreplaceable but compressible — tar.zst it.
    Archive,
    /// Regenerable — move to the Trash.
    Trash,
    /// Big enough to matter, but only the user can judge.
    Review,
    /// Never touch, at any privilege level.
    Never,
}

impl Suggest {
    pub fn as_str(&self) -> &'static str {
        match self {
            Suggest::Archive => "archive",
            Suggest::Trash => "trash",
            Suggest::Review => "review",
            Suggest::Never => "never",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Rule {
    pub id: String,
    #[serde(rename = "match")]
    pub patterns: Vec<String>,
    pub category: String,
    pub regenerable: bool,
    pub suggest: Suggest,
    pub note: String,
    /// Simplified Chinese note. Optional so a contributor can add a rule
    /// without being able to write one.
    #[serde(default)]
    pub note_zh: Option<String>,
    #[serde(default)]
    pub retain_days: Option<u32>,
    /// Names a built-in inspector that can look inside this path when the
    /// filesystem alone cannot explain it.
    #[serde(default)]
    pub inspect: Option<String>,
}

#[derive(Deserialize)]
struct RuleFile {
    rule: Vec<Rule>,
}

pub struct Rules {
    rules: Vec<Rule>,
    sets: Vec<GlobSet>,
}

/// What a rule says about one specific path.
#[derive(Debug, Clone, Serialize)]
pub struct Verdict {
    pub rule_id: String,
    pub category: String,
    pub regenerable: bool,
    pub suggest: String,
    pub note: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note_zh: Option<String>,
    pub retain_days: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inspect: Option<String>,
}

impl Rules {
    pub fn load_default() -> Result<Self> {
        Self::from_toml(DEFAULT_RULES)
    }

    pub fn from_toml(src: &str) -> Result<Self> {
        let parsed: RuleFile = toml::from_str(src).context("parsing rule file")?;
        let home = home_dir();
        let mut sets = Vec::with_capacity(parsed.rule.len());
        for rule in &parsed.rule {
            let mut b = GlobSetBuilder::new();
            for pat in &rule.patterns {
                let expanded = expand_home(pat, &home);
                b.add(
                    Glob::new(&expanded)
                        .with_context(|| format!("bad glob in rule {}: {pat}", rule.id))?,
                );
                // A rule that names a directory also owns everything inside it.
                if !expanded.ends_with("**") {
                    b.add(Glob::new(&format!("{expanded}/**"))?);
                }
            }
            sets.push(b.build()?);
        }
        Ok(Rules {
            rules: parsed.rule,
            sets,
        })
    }

    /// The most specific verdict for a path. `protected` always wins so a bad
    /// rule ordering can never expose credentials to a cleanup plan.
    pub fn classify(&self, path: &Path) -> Option<Verdict> {
        let mut best: Option<&Rule> = None;
        for (rule, set) in self.rules.iter().zip(&self.sets) {
            if !set.is_match(path) {
                continue;
            }
            if rule.suggest == Suggest::Never {
                return Some(verdict(rule));
            }
            // Prefer the rule whose longest pattern is most specific.
            let better = match best {
                None => true,
                Some(cur) => specificity(rule) > specificity(cur),
            };
            if better {
                best = Some(rule);
            }
        }
        best.map(verdict)
    }

    pub fn all(&self) -> &[Rule] {
        &self.rules
    }
}

fn verdict(r: &Rule) -> Verdict {
    Verdict {
        rule_id: r.id.clone(),
        category: r.category.clone(),
        regenerable: r.regenerable,
        suggest: r.suggest.as_str().to_string(),
        note: r.note.clone(),
        note_zh: r.note_zh.clone(),
        retain_days: r.retain_days,
        inspect: r.inspect.clone(),
    }
}

fn specificity(r: &Rule) -> usize {
    r.patterns.iter().map(|p| p.len()).max().unwrap_or(0)
}

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

pub fn expand_home(pat: &str, home: &Path) -> String {
    match pat.strip_prefix("~/") {
        Some(rest) => format!("{}/{}", home.display(), rest),
        None => pat.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> Rules {
        Rules::load_default().unwrap()
    }

    #[test]
    fn classifies_agent_and_build_dirs() {
        let r = rules();
        let home = home_dir();
        let v = r.classify(&home.join(".codex/sessions")).unwrap();
        assert_eq!(v.rule_id, "codex-sessions");
        assert_eq!(v.suggest, "archive");

        // Files inside a matched directory inherit its rule.
        let v = r
            .classify(&home.join(".codex/sessions/2026/07/09/rollout.jsonl"))
            .unwrap();
        assert_eq!(v.rule_id, "codex-sessions");

        let v = r
            .classify(Path::new("/Users/x/App/thing/node_modules"))
            .unwrap();
        assert_eq!(v.suggest, "trash");
        assert!(v.regenerable);

        assert!(r.classify(Path::new("/Users/x/App/thing/src")).is_none());
    }

    #[test]
    fn protected_paths_are_never_reclaimable() {
        let r = rules();
        let home = home_dir();
        for p in [".ssh", ".gnupg", "Library/Keychains"] {
            let v = r
                .classify(&home.join(p))
                .unwrap_or_else(|| panic!("{p} unclassified"));
            assert_eq!(v.suggest, "never", "{p} must be protected");
        }
        // A .git dir inside an otherwise-reclaimable tree still wins.
        let v = r
            .classify(Path::new("/Users/x/App/thing/node_modules/.git"))
            .unwrap();
        assert_eq!(v.suggest, "never");
    }
}
