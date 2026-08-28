//! What is running, for how long, and what it costs — plus a guarded way to
//! stop it. Everything comes from `ps`, so there is no sampling daemon and no
//! extra dependency.
//!
//! Note on `%cpu`: macOS reports the *average* over the process's lifetime, not
//! an instantaneous reading. That is the right number for "this thing has been
//! burning a core for three days" and the wrong one for "what spiked just now".

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{anyhow, bail, Result};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Proc {
    pub pid: i32,
    pub ppid: i32,
    pub cpu: f32,
    /// Resident memory in bytes.
    pub rss: u64,
    /// Wall-clock seconds since the process started.
    pub uptime: u64,
    pub uptime_human: String,
    pub user: String,
    /// Executable name.
    pub name: String,
    /// Full command line, for the tooltip.
    pub command: String,
    /// Executable path, when the command line starts with one.
    pub path: Option<PathBuf>,
    /// True if stopping this would take the desktop, or something the OS owns,
    /// with it. Protected processes are never offered for termination.
    pub protected: bool,
}

/// Processes whose death breaks the session or the OS. Never offered for kill.
const CRITICAL: &[&str] = &[
    "launchd", "kernel_task", "WindowServer", "loginwindow", "SystemUIServer", "Finder", "Dock",
    "coreaudiod", "opendirectoryd", "securityd", "syslogd", "distnoted", "notifyd", "mds",
    "mds_stores", "configd", "powerd", "hidd", "diskarbitrationd", "logd", "UserEventAgent",
];

/// Anything shipped and managed by macOS. Killing these achieves nothing —
/// launchd restarts them — and can wedge the session in the meantime.
fn is_system_path(p: &std::path::Path) -> bool {
    let s = p.to_string_lossy();
    ["/System/", "/usr/libexec/", "/usr/sbin/", "/sbin/", "/usr/bin/", "/Library/Apple/"]
        .iter()
        .any(|prefix| s.starts_with(prefix))
}

pub fn list() -> Result<Vec<Proc>> {
    // Two calls rather than one: `comm` and `args` both contain spaces, so a
    // single row cannot be split reliably. Joining on pid always can.
    let stats = ps(&["-Ao", "pid=,ppid=,pcpu=,rss=,etime=,user=,comm="])?;
    let args = ps(&["-Ao", "pid=,args="])?;

    let cmdlines: HashMap<i32, String> = args
        .lines()
        .filter_map(|l| {
            let l = l.trim_start();
            let (pid, rest) = l.split_once(char::is_whitespace)?;
            Some((pid.parse().ok()?, rest.trim().to_string()))
        })
        .collect();

    let me = whoami();
    let mut procs: Vec<Proc> = stats.lines().filter_map(|l| parse_line(l, &me, &cmdlines)).collect();
    procs.sort_by(|a, b| b.cpu.partial_cmp(&a.cpu).unwrap_or(std::cmp::Ordering::Equal));
    Ok(procs)
}

fn ps(args: &[&str]) -> Result<String> {
    let out = Command::new("ps").args(args).output()?;
    if !out.status.success() {
        bail!("ps failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn whoami() -> String {
    std::env::var("USER").unwrap_or_default()
}

/// Six fixed numeric/word columns, then `comm` — which may itself contain
/// spaces, so it is whatever remains after the user column.
fn parse_line(line: &str, me: &str, cmdlines: &HashMap<i32, String>) -> Option<Proc> {
    let mut it = line.split_whitespace();
    let pid: i32 = it.next()?.parse().ok()?;
    let ppid: i32 = it.next()?.parse().ok()?;
    let cpu: f32 = it.next()?.parse().ok()?;
    let rss_kb: u64 = it.next()?.parse().ok()?;
    let etime = it.next()?;
    let user = it.next()?.to_string();
    let exe = it.collect::<Vec<_>>().join(" ");
    let command = cmdlines.get(&pid).cloned().unwrap_or_else(|| exe.clone());
    let path = exe.starts_with('/').then(|| PathBuf::from(&exe));
    let name = path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| exe.clone());

    let uptime = parse_etime(etime);
    Some(Proc {
        pid,
        ppid,
        cpu,
        rss: rss_kb * 1024,
        uptime,
        uptime_human: human_duration(uptime),
        protected: pid <= 1
            || user != me
            || CRITICAL.contains(&name.as_str())
            || path.as_deref().is_some_and(is_system_path),
        user,
        name,
        command,
        path,
    })
}

/// `ps` etime: `[[dd-]hh:]mm:ss`.
fn parse_etime(s: &str) -> u64 {
    let (days, rest) = match s.split_once('-') {
        Some((d, r)) => (d.parse::<u64>().unwrap_or(0), r),
        None => (0, s),
    };
    let parts: Vec<u64> = rest.split(':').map(|p| p.parse().unwrap_or(0)).collect();
    let hms = match parts.as_slice() {
        [h, m, s] => h * 3600 + m * 60 + s,
        [m, s] => m * 60 + s,
        [s] => *s,
        _ => 0,
    };
    days * 86_400 + hms
}

pub fn human_duration(secs: u64) -> String {
    let (d, h, m) = (secs / 86_400, (secs % 86_400) / 3600, (secs % 3600) / 60);
    match (d, h) {
        (0, 0) => format!("{m}m"),
        (0, _) => format!("{h}h {m}m"),
        _ => format!("{d}d {h}h"),
    }
}

/// Stop a process. SIGTERM first — `force` escalates to SIGKILL, which gives the
/// process no chance to flush anything it was writing.
pub fn kill(pid: i32, force: bool) -> Result<()> {
    let procs = list()?;
    let p = procs.iter().find(|p| p.pid == pid).ok_or_else(|| anyhow!("no process with pid {pid}"))?;
    if p.protected {
        bail!("{} (pid {pid}) is protected: it belongs to {} or the system", p.name, p.user);
    }
    let sig = if force { "-KILL" } else { "-TERM" };
    let out = Command::new("kill").args([sig, &pid.to_string()]).output()?;
    if !out.status.success() {
        bail!("kill {sig} {pid}: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ps_durations() {
        assert_eq!(parse_etime("05:30"), 330);
        assert_eq!(parse_etime("48:44"), 2924);
        assert_eq!(parse_etime("01:02:03"), 3723);
        assert_eq!(parse_etime("01-00:33:50"), 86_400 + 2030);
        assert_eq!(human_duration(86_400 + 2030), "1d 0h");
        assert_eq!(human_duration(3723), "1h 2m");
    }

    #[test]
    fn command_lines_survive_arguments_and_spaces() {
        let procs = list().unwrap();
        // This very test binary was launched with arguments; its command line
        // must contain them, and must not be a mangled half of anything.
        let me = procs.iter().find(|p| p.pid == std::process::id() as i32).unwrap();
        assert!(me.command.contains("diskwise"), "got {:?}", me.command);
        assert!(me.path.as_ref().is_some_and(|p| p.is_absolute()), "got {:?}", me.path);
        assert!(!me.name.contains(' '), "name should be a file name, got {:?}", me.name);

        // Chrome-style paths with spaces must keep their real basename.
        let spaced = procs.iter().find(|p| p.command.contains(".app/Contents/MacOS/"));
        if let Some(p) = spaced {
            assert!(p.path.as_ref().unwrap().is_absolute());
        }
    }

    #[test]
    fn lists_real_processes_and_protects_the_critical_ones() {
        let procs = list().unwrap();
        assert!(procs.len() > 20, "a mac always has more processes than this");

        let launchd = procs.iter().find(|p| p.pid == 1).expect("pid 1 must be listed");
        assert!(launchd.protected);
        assert!(kill(1, false).is_err(), "killing launchd must be refused");

        // System daemons are off limits even though they run as us.
        let spotlight = procs.iter().find(|p| p.name.starts_with("spotlight"));
        if let Some(p) = spotlight {
            assert!(p.protected, "{} runs from {:?} and must be protected", p.name, p.path);
        }

        // This test process is ours, running, and not critical.
        let me = procs.iter().find(|p| p.pid == std::process::id() as i32).expect("self must be listed");
        assert!(!me.protected);
        assert!(me.uptime < 86_400);
    }
}
