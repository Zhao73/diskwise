//! MCP server over stdio, so Claude Code and Codex can ask where the disk went
//! and — with a human in the loop — do something about it.
//!
//! The protocol surface an agent host actually needs is `initialize`,
//! `tools/list` and `tools/call`; that is a few hundred lines of JSON-RPC, so
//! there is no SDK dependency here.
//!
//! The safety model is the same one the CLI uses: `apply_cleanup` cannot
//! approve itself. It returns a plan id and stops, unless the user's own
//! policy.toml has explicitly opted that path into unattended mode.

use std::io::{BufRead, Write};
use std::path::PathBuf;

use anyhow::Result;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use serde_json::{json, Value};

use crate::rules::{home_dir, Rules};
use crate::scan::Scan;
use crate::{actions, launch, plan, policy, procs, scan, server, view};

const PROTOCOL_VERSION: &str = "2025-06-18";

pub fn serve() -> Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut session = Session::default();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let req: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                write(
                    &mut stdout,
                    error_response(Value::Null, -32700, &format!("parse error: {e}")),
                )?;
                continue;
            }
        };
        // Notifications have no id and expect no reply.
        let Some(id) = req.get("id").cloned() else {
            continue;
        };
        let method = req
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let params = req.get("params").cloned().unwrap_or(json!({}));

        let response = match session.dispatch(method, params) {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(e) => error_response(id, -32000, &e.to_string()),
        };
        write(&mut stdout, response)?;
    }
    Ok(())
}

fn write(out: &mut impl Write, v: Value) -> Result<()> {
    writeln!(out, "{v}")?;
    out.flush()?;
    Ok(())
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

#[derive(Default)]
struct Session {
    /// Reused between calls so an agent asking five questions scans once.
    cached: Option<Scan>,
}

impl Session {
    fn dispatch(&mut self, method: &str, params: Value) -> Result<Value> {
        match method {
            "initialize" => Ok(json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "diskwise", "version": env!("CARGO_PKG_VERSION") },
                "instructions": INSTRUCTIONS,
            })),
            "tools/list" => Ok(json!({ "tools": tool_specs() })),
            "tools/call" => {
                let name = params
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                match self.call(&name, args) {
                    // A refused action is a normal result an agent must read,
                    // not a transport error — so it comes back as isError text.
                    Ok(v) => Ok(json!({ "content": content_for(v), "isError": false })),
                    Err(e) => {
                        Ok(json!({ "content": [text(&json!(e.to_string()))], "isError": true }))
                    }
                }
            }
            "ping" => Ok(json!({})),
            other => anyhow::bail!("unknown method: {other}"),
        }
    }

    fn scan_for(&mut self, path: Option<&str>) -> &Scan {
        let root = path.map(PathBuf::from).unwrap_or_else(home_dir);
        let stale = self.cached.as_ref().is_none_or(|s| s.root != root);
        if stale {
            let fresh = server::load_cache(&root).unwrap_or_else(|| {
                let s = scan::scan(&root);
                server::save_cache(&s);
                s
            });
            self.cached = Some(fresh);
        }
        self.cached.as_ref().unwrap()
    }

    fn call(&mut self, name: &str, args: Value) -> Result<Value> {
        let rules = Rules::load_default()?;
        let guard = policy::Guard::load()?;
        let path = args.get("path").and_then(Value::as_str).map(str::to_string);
        let str_arg = |k: &str| args.get(k).and_then(Value::as_str).map(str::to_string);
        let num_arg = |k: &str| args.get(k).and_then(Value::as_f64);

        match name {
            "top_offenders" => {
                let limit = num_arg("limit").unwrap_or(25.0) as usize;
                let min = num_arg("min_bytes").unwrap_or(100e6) as u64;
                let category = str_arg("category");
                let s = self.scan_for(path.as_deref());
                let rows = view::rows(
                    s,
                    &rules,
                    &view::Query {
                        min,
                        category,
                        limit,
                        ..Default::default()
                    },
                );
                Ok(json!({
                    "root": s.root,
                    "total_bytes": s.total(),
                    "scanned_files": s.scanned_files,
                    "unreadable_paths": s.denied,
                    "reclaimable_bytes": view::reclaimable(&rows),
                    "rows": rows,
                }))
            }
            "explain_path" => {
                let target = PathBuf::from(
                    path.clone()
                        .ok_or_else(|| anyhow::anyhow!("path is required"))?,
                );
                let s = self.scan_for(target.parent().and_then(|p| p.to_str()));
                let entry = s.dirs.get(&target);
                Ok(json!({
                    "path": target,
                    "size_bytes": entry.map(|e| e.total),
                    "files": entry.map(|e| e.files),
                    "newest_mtime": entry.map(|e| e.newest),
                    "verdict": rules.classify(&target),
                    "protected": guard.is_protected(&target),
                }))
            }
            "plan_cleanup" => {
                let opts = plan::PlanOptions {
                    target: num_arg("target_bytes").unwrap_or(0.0) as u64,
                    category: str_arg("category"),
                    min: num_arg("min_bytes").unwrap_or(100e6) as u64,
                    include_archives: args
                        .get("include_archives")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                };
                let s = self.scan_for(path.as_deref());
                let p = plan::build(s, &rules, &guard, &opts);
                p.save()?;
                Ok(json!({
                    "plan_id": p.id,
                    "would_free_bytes": p.total(),
                    "items": p.items,
                    "next_step": format!(
                        "Show this to the user. To execute, they run `diskwise confirm {}`, \
                         or you call apply_cleanup with this plan_id (which will still refuse \
                         unless their policy.toml allows it unattended).",
                        p.id
                    ),
                }))
            }
            "apply_cleanup" => {
                let plan_id =
                    str_arg("plan_id").ok_or_else(|| anyhow::anyhow!("plan_id is required"))?;
                let p = actions::Plan::load(&plan_id)?;
                if let Err(denial) = guard.check_unattended(&p.paths(), p.total()) {
                    // This is the whole point of the design: the agent is told
                    // exactly why, and what the human has to do.
                    return Ok(json!({
                        "applied": false,
                        "reason": denial.to_string(),
                        "would_free_bytes": p.total(),
                        "items": p.items,
                        "user_must_run": format!("diskwise confirm {plan_id}"),
                    }));
                }
                let outcomes = actions::apply(&p, &guard);
                Ok(json!({
                    "applied": true,
                    "freed_bytes": outcomes.iter().map(|o| o.freed).sum::<u64>(),
                    "outcomes": outcomes,
                }))
            }
            "archive_path" => {
                let target =
                    PathBuf::from(path.ok_or_else(|| anyhow::anyhow!("path is required"))?);
                guard.check(&target).map_err(|d| anyhow::anyhow!("{d}"))?;
                let out = actions::archive(&target)?;
                Ok(json!({ "archive": out, "note": "verified before the original was released" }))
            }
            "list_archives" => Ok(json!(actions::list_archives()?
                .into_iter()
                .map(|(p, m)| json!({ "archive": p, "manifest": m }))
                .collect::<Vec<_>>())),
            "restore_archive" => {
                let archive = PathBuf::from(
                    str_arg("archive").ok_or_else(|| anyhow::anyhow!("archive is required"))?,
                );
                let to = str_arg("to").map(PathBuf::from);
                let dest = actions::restore(&archive, to.as_deref())?;
                Ok(json!({ "restored_into": dest }))
            }
            "list_processes" => {
                let mut list = procs::list()?;
                if let Some(d) = num_arg("min_days") {
                    list.retain(|p| p.uptime as f64 >= d * 86_400.0);
                }
                if args
                    .get("only_mine")
                    .and_then(Value::as_bool)
                    .unwrap_or(true)
                {
                    let me = procs::whoami();
                    list.retain(|p| p.user == me);
                }
                if args
                    .get("by_memory")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    list.sort_by(|a, b| b.rss.cmp(&a.rss));
                }
                list.truncate(num_arg("limit").unwrap_or(25.0) as usize);
                Ok(json!(list))
            }
            "kill_process" => {
                let pid = num_arg("pid").ok_or_else(|| anyhow::anyhow!("pid is required"))? as i32;
                let force = args.get("force").and_then(Value::as_bool).unwrap_or(false);
                procs::kill(pid, force)?;
                Ok(json!({ "signalled": pid, "signal": if force { "SIGKILL" } else { "SIGTERM" } }))
            }
            "open_ui" => {
                let port = num_arg("port").unwrap_or(launch::DEFAULT_PORT as f64) as u16;
                let root = path.as_deref().map(PathBuf::from);
                let server = launch::ensure_server(port, root.as_deref())?;
                let url = launch::view_url(&server.url, &view_arg(&args), &url_params(&args));
                if args
                    .get("open_browser")
                    .and_then(Value::as_bool)
                    .unwrap_or(true)
                {
                    launch::open_in_browser(&url)?;
                }
                Ok(json!({
                    "url": url,
                    "already_running": server.started.is_none(),
                    "note": "The page is live and refreshes on its own. Every filter is in the \
                             URL, so you can hand the user a link to exactly what you are \
                             describing. Use `screenshot` if you need to see it yourself.",
                }))
            }
            "screenshot" => {
                let port = num_arg("port").unwrap_or(launch::DEFAULT_PORT as f64) as u16;
                let root = path.as_deref().map(PathBuf::from);
                let server = launch::ensure_server(port, root.as_deref())?;
                let url = launch::view_url(&server.url, &view_arg(&args), &url_params(&args));
                let width = num_arg("width").unwrap_or(1440.0) as u32;
                let height = num_arg("height").unwrap_or(900.0) as u32;
                let png = launch::screenshot(&url, width, height)?;
                // Returned as an image so a vision-capable host can actually
                // look at the chart rather than being told about it.
                Ok(json!({ "__image_png": B64.encode(png), "url": url }))
            }
            "policy" => Ok(json!({
                "config_file": policy::policy_path(),
                "policy": guard.policy,
                "explanation": "mode `confirm` (the default) means destructive tools return a \
                    plan for the user to run; `auto` acts unattended only inside auto_allow and \
                    under max_auto_delete_gb; `readonly` blocks all changes.",
            })),
            other => anyhow::bail!("unknown tool: {other}"),
        }
    }
}

/// Tool results are text, except a screenshot, which is handed over as an
/// image block so the host can show it to a model that can see.
fn content_for(v: Value) -> Vec<Value> {
    if let Some(data) = v.get("__image_png").and_then(Value::as_str) {
        let url = v.get("url").and_then(Value::as_str).unwrap_or_default();
        return vec![
            json!({ "type": "image", "data": data, "mimeType": "image/png" }),
            text(&json!(format!("Screenshot of {url}"))),
        ];
    }
    vec![text(&v)]
}

fn view_arg(args: &Value) -> String {
    args.get("view")
        .and_then(Value::as_str)
        .unwrap_or("disk")
        .to_string()
}

/// Filter arguments become URL parameters, so the link and the picture always
/// show the same thing.
fn url_params(args: &Value) -> Vec<(&'static str, String)> {
    let get = |k: &str| {
        args.get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let num = |k: &str| {
        args.get(k)
            .and_then(Value::as_f64)
            .map(|n| n.to_string())
            .unwrap_or_default()
    };
    vec![
        ("dir", get("dir")),
        ("category", get("category")),
        ("find", get("find")),
        ("days", num("days")),
        ("min", num("min_bytes")),
    ]
}

fn text(v: &Value) -> Value {
    let body = match v.as_str() {
        Some(s) => s.to_string(),
        None => serde_json::to_string_pretty(v).unwrap_or_default(),
    };
    json!({ "type": "text", "text": body })
}

const INSTRUCTIONS: &str = "diskwise reports what is using disk space and CPU on this Mac, with a \
rule layer that knows which directories are AI agent sessions, caches or build output. Start with \
top_offenders. Destructive tools return a plan for the user to confirm rather than acting on their \
own, unless their policy.toml says otherwise.";

fn obj(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required })
}

fn tool_specs() -> Vec<Value> {
    let path_prop = json!({ "type": "string", "description": "Absolute path. Defaults to the home directory." });
    vec![
        json!({
            "name": "top_offenders",
            "description": "What is taking up space, ranked, each row classified by rule \
                (agent-session, build, toolchain-cache, …) with a suggested action.",
            "inputSchema": obj(json!({
                "path": path_prop,
                "limit": { "type": "number", "description": "Rows to return. Default 25." },
                "min_bytes": { "type": "number", "description": "Ignore anything smaller. Default 100MB." },
                "category": { "type": "string", "description": "Restrict to one rule category." }
            }), &[]),
        }),
        json!({
            "name": "explain_path",
            "description": "What one directory is, how big it is, when it was last written, \
                whether it is safe to reclaim, and what is lost if it goes.",
            "inputSchema": obj(json!({ "path": path_prop }), &["path"]),
        }),
        json!({
            "name": "plan_cleanup",
            "description": "Build a cleanup plan (regenerable caches first, irreplaceable data \
                only with include_archives). Returns a plan_id; nothing is modified.",
            "inputSchema": obj(json!({
                "path": path_prop,
                "target_bytes": { "type": "number", "description": "Stop once this much would be freed." },
                "min_bytes": { "type": "number", "description": "Ignore anything smaller. Default 100MB." },
                "category": { "type": "string" },
                "include_archives": { "type": "boolean", "description": "Also archive agent sessions and other irreplaceable-but-compressible data." }
            }), &[]),
        }),
        json!({
            "name": "apply_cleanup",
            "description": "Execute a plan. Refuses and returns the confirmation command unless \
                the user's policy explicitly allows this unattended. Deletions go to the Trash.",
            "inputSchema": obj(json!({ "plan_id": { "type": "string" } }), &["plan_id"]),
        }),
        json!({
            "name": "archive_path",
            "description": "tar.zst one directory, verify it reads back, then move the original \
                to the Trash. Refused for protected paths.",
            "inputSchema": obj(json!({ "path": path_prop }), &["path"]),
        }),
        json!({
            "name": "list_archives",
            "description": "Archives diskwise has made, with their original locations and sizes.",
            "inputSchema": obj(json!({}), &[]),
        }),
        json!({
            "name": "restore_archive",
            "description": "Unpack an archive, by default back where it came from.",
            "inputSchema": obj(json!({
                "archive": { "type": "string" },
                "to": { "type": "string", "description": "Optional destination directory." }
            }), &["archive"]),
        }),
        json!({
            "name": "list_processes",
            "description": "Running processes with CPU share, resident memory and uptime — for \
                finding things that have been running for days.",
            "inputSchema": obj(json!({
                "limit": { "type": "number" },
                "min_days": { "type": "number", "description": "Only processes running at least this long." },
                "only_mine": { "type": "boolean", "description": "Default true." },
                "by_memory": { "type": "boolean", "description": "Sort by memory instead of CPU." }
            }), &[]),
        }),
        json!({
            "name": "kill_process",
            "description": "Terminate a process (SIGTERM, or SIGKILL with force). System and \
                session-critical processes are always refused.",
            "inputSchema": obj(json!({
                "pid": { "type": "number" },
                "force": { "type": "boolean", "description": "SIGKILL. No chance to save work." }
            }), &["pid"]),
        }),
        json!({
            "name": "open_ui",
            "description": "Open the diskwise browser UI beside this conversation, starting the \
                local server if it is not already up. Returns a URL whose filters match the \
                arguments, so you can point the user at exactly what you are talking about.",
            "inputSchema": obj(json!({
                "path": path_prop,
                "view": { "type": "string", "enum": ["disk", "processes"], "description": "Which tab to open. Default disk." },
                "dir": { "type": "string", "description": "Open the disk view browsing this directory." },
                "category": { "type": "string" },
                "find": { "type": "string", "description": "Process filter text." },
                "days": { "type": "number", "description": "Processes running at least this long." },
                "min_bytes": { "type": "number" },
                "port": { "type": "number", "description": "Default 7373." },
                "open_browser": { "type": "boolean", "description": "Default true. False just returns the URL." }
            }), &[]),
        }),
        json!({
            "name": "screenshot",
            "description": "Render the UI to a PNG and return it as an image, so you can look \
                at the treemap yourself — useful for checking layout, or answering a question \
                about what the user is seeing on screen. Takes the same filters as open_ui.",
            "inputSchema": obj(json!({
                "path": path_prop,
                "view": { "type": "string", "enum": ["disk", "processes"] },
                "dir": { "type": "string" },
                "category": { "type": "string" },
                "find": { "type": "string" },
                "days": { "type": "number" },
                "min_bytes": { "type": "number" },
                "width": { "type": "number", "description": "Default 1440." },
                "height": { "type": "number", "description": "Default 900." },
                "port": { "type": "number" }
            }), &[]),
        }),
        json!({
            "name": "policy",
            "description": "The user's current safety policy and where to change it.",
            "inputSchema": obj(json!({}), &[]),
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(s: &mut Session, name: &str, args: Value) -> Value {
        let v = s
            .dispatch("tools/call", json!({ "name": name, "arguments": args }))
            .unwrap();
        let text = v["content"][0]["text"].as_str().unwrap().to_string();
        serde_json::from_str(&text).unwrap_or(json!(text))
    }

    #[test]
    fn handshake_advertises_every_tool_with_a_schema() {
        let mut s = Session::default();
        let init = s.dispatch("initialize", json!({})).unwrap();
        assert_eq!(init["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(init["serverInfo"]["name"], "diskwise");

        let tools = s.dispatch("tools/list", json!({})).unwrap();
        let tools = tools["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 12);
        for t in tools {
            assert!(t["name"].is_string());
            assert!(t["description"].as_str().unwrap().len() > 20);
            assert_eq!(t["inputSchema"]["type"], "object");
        }
    }

    #[test]
    fn unknown_tools_come_back_as_readable_errors_not_crashes() {
        let mut s = Session::default();
        let v = s
            .dispatch("tools/call", json!({ "name": "rm_rf", "arguments": {} }))
            .unwrap();
        assert_eq!(v["isError"], true);
        assert!(v["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("unknown tool"));
    }

    /// The central promise: an agent cannot talk itself into deleting anything.
    #[test]
    fn apply_cleanup_refuses_without_human_confirmation() {
        let tmp = std::env::temp_dir().join(format!("diskwise-mcp-{}", std::process::id()));
        let nm = tmp.join("proj/node_modules");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(nm.join("big.bin"), vec![0u8; 3_000_000]).unwrap();

        let mut s = Session::default();
        let planned = call(
            &mut s,
            "plan_cleanup",
            json!({ "path": tmp.to_str(), "min_bytes": 1_000_000 }),
        );
        let plan_id = planned["plan_id"].as_str().unwrap().to_string();
        assert!(planned["would_free_bytes"].as_u64().unwrap() >= 3_000_000);

        let applied = call(&mut s, "apply_cleanup", json!({ "plan_id": plan_id }));
        assert_eq!(applied["applied"], false);
        assert!(applied["user_must_run"]
            .as_str()
            .unwrap()
            .starts_with("diskwise confirm"));
        assert!(
            nm.join("big.bin").exists(),
            "nothing may be touched without confirmation"
        );

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn kill_process_refuses_protected_pids() {
        let mut s = Session::default();
        let v = s
            .dispatch(
                "tools/call",
                json!({ "name": "kill_process", "arguments": { "pid": 1 } }),
            )
            .unwrap();
        assert_eq!(v["isError"], true);
        assert!(v["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("protected"));
    }
}
