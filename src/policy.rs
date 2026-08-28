//! What is allowed to happen without a human saying yes.
//!
//! Two layers, and they are not the same thing:
//!   * `FORBIDDEN` is hard-coded and no configuration can switch it off.
//!   * `Policy` is the user's own dial, from read-only up to fully automatic.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};

use crate::rules::{expand_home, home_dir};

/// Paths diskwise refuses to modify no matter what the config says. Losing any
/// of these is unrecoverable in a way that no amount of disk space justifies.
const FORBIDDEN: &[&str] = &[
    "~/.ssh/**",
    "~/.gnupg/**",
    "~/Library/Keychains/**",
    "~/Library/Mobile Documents/**",
    "~/Library/Application Support/MobileSync/**",
    "**/.git/**",
    "/System/**",
    "/Library/**",
    "/usr/**",
    "/bin/**",
    "/sbin/**",
    "/etc/**",
    "/private/var/db/**",
    "/Volumes/**",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Analysis only. Nothing is ever modified, even with confirmation.
    Readonly,
    /// The default: plans can be built freely, applying them needs a human.
    #[default]
    Confirm,
    /// Apply without asking, within `auto_allow` and `max_auto_delete_gb`.
    Auto,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Policy {
    pub default: Mode,
    /// Paths an agent may act on without confirmation (only when mode = auto).
    pub auto_allow: Vec<String>,
    /// Extra user-defined paths to protect, on top of `FORBIDDEN`.
    pub never: Vec<String>,
    /// Ceiling on a single unattended run.
    pub max_auto_delete_gb: f64,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            default: Mode::Confirm,
            auto_allow: vec![],
            never: vec![],
            max_auto_delete_gb: 5.0,
        }
    }
}

pub struct Guard {
    pub policy: Policy,
    forbidden: GlobSet,
    auto_allow: GlobSet,
}

/// Why an action was refused, in words a user or an agent can act on.
#[derive(Debug, PartialEq)]
pub enum Denial {
    /// Hard-coded or user-configured protection. Not overridable.
    Protected(String),
    /// Allowed, but a human has to say yes first.
    NeedsConfirmation,
    /// Policy is read-only.
    ReadOnly,
    /// Over the unattended size ceiling.
    TooBig { limit_gb: f64 },
}

impl std::fmt::Display for Denial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Denial::Protected(p) => {
                write!(f, "{p} is protected and can never be modified by diskwise")
            }
            Denial::NeedsConfirmation => {
                write!(f, "needs confirmation: run `diskwise confirm <plan-id>`")
            }
            Denial::ReadOnly => write!(f, "policy is set to readonly; nothing will be modified"),
            Denial::TooBig { limit_gb } => {
                write!(
                    f,
                    "over the unattended ceiling of {limit_gb} GB; needs confirmation"
                )
            }
        }
    }
}

pub fn policy_path() -> PathBuf {
    home_dir().join(".diskwise/policy.toml")
}

impl Guard {
    pub fn load() -> Result<Self> {
        let policy = match std::fs::read_to_string(policy_path()) {
            Ok(src) => toml::from_str(&src).context("parsing ~/.diskwise/policy.toml")?,
            Err(_) => Policy::default(),
        };
        Self::new(policy)
    }

    pub fn new(policy: Policy) -> Result<Self> {
        let home = home_dir();
        let mut forbidden = GlobSetBuilder::new();
        for pat in FORBIDDEN
            .iter()
            .map(|s| s.to_string())
            .chain(policy.never.iter().cloned())
        {
            let e = expand_home(&pat, &home);
            forbidden.add(Glob::new(&e)?);
            // `~/.ssh/**` should also protect `~/.ssh` itself.
            if let Some(base) = e.strip_suffix("/**") {
                forbidden.add(Glob::new(base)?);
            }
        }
        let mut allow = GlobSetBuilder::new();
        for pat in &policy.auto_allow {
            let e = expand_home(pat, &home);
            allow.add(Glob::new(&e)?);
            if !e.ends_with("**") {
                allow.add(Glob::new(&format!("{e}/**"))?);
            }
        }
        Ok(Guard {
            policy,
            forbidden: forbidden.build()?,
            auto_allow: allow.build()?,
        })
    }

    /// True if this path may never be touched. Checked on every ancestor too,
    /// so a path *inside* a protected directory is protected as well.
    pub fn is_protected(&self, path: &Path) -> bool {
        if self.forbidden.is_match(path) {
            return true;
        }
        path.ancestors().skip(1).any(|a| self.forbidden.is_match(a))
    }

    /// May this path be modified at all (given a human has confirmed)?
    pub fn check(&self, path: &Path) -> Result<(), Denial> {
        if self.is_protected(path) {
            return Err(Denial::Protected(path.display().to_string()));
        }
        if self.policy.default == Mode::Readonly {
            return Err(Denial::ReadOnly);
        }
        Ok(())
    }

    /// May this run proceed *without* asking a human?
    pub fn check_unattended(&self, paths: &[PathBuf], bytes: u64) -> Result<(), Denial> {
        for p in paths {
            self.check(p)?;
        }
        if self.policy.default != Mode::Auto {
            return Err(Denial::NeedsConfirmation);
        }
        let limit = (self.policy.max_auto_delete_gb * 1e9) as u64;
        if bytes > limit {
            return Err(Denial::TooBig {
                limit_gb: self.policy.max_auto_delete_gb,
            });
        }
        if !paths.iter().all(|p| self.auto_allow.is_match(p)) {
            return Err(Denial::NeedsConfirmation);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guard(p: Policy) -> Guard {
        Guard::new(p).unwrap()
    }

    #[test]
    fn protected_paths_survive_any_configuration() {
        // The most permissive config a user could possibly write.
        let g = guard(Policy {
            default: Mode::Auto,
            auto_allow: vec!["/**".into(), "~/**".into()],
            never: vec![],
            max_auto_delete_gb: 1e9,
        });
        let home = home_dir();
        for p in [
            home.join(".ssh"),
            home.join(".ssh/id_ed25519"),
            home.join("Library/Keychains/login.keychain-db"),
            home.join("App/thing/.git/objects/ab/cdef"),
            PathBuf::from("/System/Library/Foo"),
            home.join("Library/Mobile Documents/com~apple~CloudDocs/x.txt"),
        ] {
            assert!(g.is_protected(&p), "{} must stay protected", p.display());
            assert!(matches!(g.check(&p), Err(Denial::Protected(_))));
        }
    }

    #[test]
    fn readonly_blocks_everything_and_confirm_is_the_default() {
        let home = home_dir();
        let target = home.join("App/x/node_modules");

        let ro = guard(Policy {
            default: Mode::Readonly,
            ..Default::default()
        });
        assert_eq!(ro.check(&target), Err(Denial::ReadOnly));

        let dflt = guard(Policy::default());
        assert!(dflt.check(&target).is_ok(), "confirmed actions are allowed");
        assert_eq!(
            dflt.check_unattended(std::slice::from_ref(&target), 1),
            Err(Denial::NeedsConfirmation),
            "default policy must never act unattended"
        );
    }

    #[test]
    fn auto_mode_respects_allowlist_and_size_ceiling() {
        let home = home_dir();
        let g = guard(Policy {
            default: Mode::Auto,
            auto_allow: vec!["~/App/**/node_modules".into()],
            never: vec![],
            max_auto_delete_gb: 5.0,
        });
        let allowed = home.join("App/x/node_modules");
        let elsewhere = home.join("Documents/taxes");

        assert!(g
            .check_unattended(std::slice::from_ref(&allowed), 1_000_000)
            .is_ok());
        assert_eq!(
            g.check_unattended(&[elsewhere], 1_000),
            Err(Denial::NeedsConfirmation)
        );
        assert_eq!(
            g.check_unattended(&[allowed], 6_000_000_000),
            Err(Denial::TooBig { limit_gb: 5.0 })
        );
    }
}
