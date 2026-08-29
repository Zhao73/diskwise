//! End-to-end: the real binary starts a UI, and the real MCP server hands back
//! a screenshot of it as an image. These need the built executable, which is
//! why they live out here rather than in a unit test.

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_diskwise");
const PORT: u16 = 7411;

fn listening(port: u16) -> bool {
    TcpStream::connect_timeout(
        &SocketAddrV4::new(Ipv4Addr::LOCALHOST, port).into(),
        Duration::from_millis(300),
    )
    .is_ok()
}

fn wait_for(port: u16) -> bool {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if listening(port) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

struct Fixture {
    ui: Child,
    dir: std::path::PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.ui.kill();
        let _ = self.ui.wait();
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

fn start_ui() -> Option<Fixture> {
    if listening(PORT) {
        return None; // something else owns the port; don't fight it
    }
    let dir = std::env::temp_dir().join(format!("diskwise-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("payload.bin"), vec![0u8; 3_000_000]).unwrap();

    let ui = Command::new(BIN)
        .args([
            "ui",
            dir.to_str().unwrap(),
            "--port",
            &PORT.to_string(),
            "--no-open",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the binary should run");
    assert!(
        wait_for(PORT),
        "the UI must bind its port promptly, before scanning finishes"
    );
    Some(Fixture { ui, dir })
}

/// One request/response round trip against the real MCP server on stdio.
fn mcp_call(name: &str, args: serde_json::Value) -> serde_json::Value {
    let mut child = Command::new(BIN)
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    {
        let stdin = child.stdin.as_mut().unwrap();
        writeln!(
            stdin,
            r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{}}}}"#
        )
        .unwrap();
        let call = serde_json::json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": { "name": name, "arguments": args }
        });
        writeln!(stdin, "{call}").unwrap();
    }
    let out = BufReader::new(child.stdout.take().unwrap());
    let mut result = serde_json::Value::Null;
    for line in out.lines().map_while(Result::ok) {
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        if v["id"] == 2 {
            result = v["result"].clone();
            break;
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    result
}

#[test]
fn ui_serves_before_the_scan_finishes_and_open_ui_reuses_it() {
    let Some(_fx) = start_ui() else { return };

    let r = mcp_call(
        "open_ui",
        serde_json::json!({ "port": PORT, "open_browser": false, "view": "processes", "days": 3 }),
    );
    let body: serde_json::Value =
        serde_json::from_str(r["content"][0]["text"].as_str().unwrap()).unwrap();

    assert_eq!(r["isError"], false);
    assert_eq!(
        body["already_running"], true,
        "must reuse the running server, not start a second"
    );
    assert_eq!(
        body["url"],
        format!("http://127.0.0.1:{PORT}/?days=3#processes")
    );
}

#[test]
fn screenshot_comes_back_as_a_real_png_image_block() {
    let Some(_fx) = start_ui() else { return };
    if !std::path::Path::new("/Applications/Google Chrome.app").exists() {
        return; // no headless browser available here
    }

    let r = mcp_call(
        "screenshot",
        serde_json::json!({ "port": PORT, "width": 900, "height": 600 }),
    );
    assert_eq!(r["isError"], false, "{r}");
    assert_eq!(r["content"][0]["type"], "image");
    assert_eq!(r["content"][0]["mimeType"], "image/png");

    let data = r["content"][0]["data"].as_str().unwrap();
    let png = base64_decode(data);
    assert_eq!(
        &png[..4],
        b"\x89PNG",
        "must be an actual PNG, not a description of one"
    );
    assert!(
        png.len() > 5_000,
        "a blank page would be tiny; got {} bytes",
        png.len()
    );
}

/// The test crate has no base64 dependency of its own, and this is four lines.
fn base64_decode(s: &str) -> Vec<u8> {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes().filter(|c| *c != b'=' && !c.is_ascii_whitespace()) {
        let v = A
            .iter()
            .position(|a| *a == c)
            .expect("valid base64 alphabet") as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}
