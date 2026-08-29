//! Getting the UI in front of someone — a browser window for the human, a PNG
//! for an agent that can look at images.

use std::io::Read;
use std::net::{Ipv4Addr, SocketAddrV4, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};

pub const DEFAULT_PORT: u16 = 7373;

/// Chromium-family browsers that support `--headless --screenshot`, in the
/// order we would rather use them.
const BROWSERS: &[&str] = &[
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
];

pub fn is_listening(port: u16) -> bool {
    TcpStream::connect_timeout(
        &SocketAddrV4::new(Ipv4Addr::LOCALHOST, port).into(),
        Duration::from_millis(300),
    )
    .is_ok()
}

/// Whether a server had to be started, and its pid if so.
pub struct Server {
    pub url: String,
    pub started: Option<u32>,
}

/// Make sure a diskwise UI is answering on `port`, starting a detached one if
/// not. Safe to call repeatedly: an already-running server is left alone.
pub fn ensure_server(port: u16, root: Option<&Path>) -> Result<Server> {
    let url = format!("http://127.0.0.1:{port}");
    if is_listening(port) {
        return Ok(Server { url, started: None });
    }

    let exe = std::env::current_exe()?;
    let mut cmd = Command::new(exe);
    cmd.arg("ui");
    if let Some(r) = root {
        cmd.arg(r);
    }
    cmd.args(["--port", &port.to_string(), "--no-open"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let child = cmd
        .spawn()
        .map_err(|e| anyhow!("starting the UI server: {e}"))?;

    // The server binds before it finishes scanning, so this is a short wait.
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if is_listening(port) {
            return Ok(Server {
                url,
                started: Some(child.id()),
            });
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    bail!("the UI server did not come up on port {port}")
}

/// Build the URL for a particular view, so a link reproduces exactly what
/// someone is looking at.
pub fn view_url(base: &str, view: &str, params: &[(&str, String)]) -> String {
    let query: Vec<String> = params
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| format!("{k}={}", urlencode(v)))
        .collect();
    let q = if query.is_empty() {
        String::new()
    } else {
        format!("?{}", query.join("&"))
    };
    let hash = if view == "processes" {
        "#processes"
    } else {
        ""
    };
    format!("{base}/{q}{hash}")
}

/// Minimal percent-encoding: paths contain slashes, spaces and non-ASCII.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn open_in_browser(url: &str) -> Result<()> {
    Command::new("open")
        .arg(url)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

pub fn find_browser() -> Option<PathBuf> {
    BROWSERS.iter().map(PathBuf::from).find(|p| p.exists())
}

/// Render a page to PNG with headless Chrome. Returns the raw bytes.
pub fn screenshot(url: &str, width: u32, height: u32) -> Result<Vec<u8>> {
    let browser = find_browser().ok_or_else(|| {
        anyhow!("no Chromium-family browser found; install Google Chrome to capture screenshots")
    })?;
    let out = std::env::temp_dir().join(format!("diskwise-shot-{}.png", std::process::id()));
    let _ = std::fs::remove_file(&out);

    let status = Command::new(&browser)
        .args([
            "--headless",
            "--disable-gpu",
            "--hide-scrollbars",
            "--virtual-time-budget=9000",
            &format!("--window-size={width},{height}"),
            &format!("--screenshot={}", out.display()),
            url,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !status.success() && !out.exists() {
        bail!("{} could not render {url}", browser.display());
    }

    let mut bytes = Vec::new();
    std::fs::File::open(&out)?.read_to_end(&mut bytes)?;
    let _ = std::fs::remove_file(&out);
    if bytes.is_empty() {
        bail!("the screenshot came back empty");
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_urls_reproduce_a_view() {
        let base = "http://127.0.0.1:7373";
        assert_eq!(view_url(base, "disk", &[]), "http://127.0.0.1:7373/");
        assert_eq!(
            view_url(
                base,
                "processes",
                &[("days", "3".into()), ("find", "".into())]
            ),
            "http://127.0.0.1:7373/?days=3#processes"
        );
        // Spaces and non-ASCII in a path must survive the round trip.
        let u = view_url(base, "disk", &[("dir", "/Users/a b/项目".into())]);
        assert!(u.contains("/Users/a%20b/"), "got {u}");
        assert!(!u.contains('项'), "got {u}");
    }
}
