//! Borrowing the agent you already pay for.
//!
//! diskwise has no API key and wants none. When you ask it a question it hands
//! the scan to whichever agent CLI is already logged in on this machine and
//! lets that subscription do the thinking. The call costs your quota, so it
//! only ever happens because you asked for it, and the token count comes back
//! with the answer.

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Agent {
    Codex,
    Claude,
}

impl Agent {
    pub fn as_str(&self) -> &'static str {
        match self {
            Agent::Codex => "codex",
            Agent::Claude => "claude",
        }
    }

    pub fn parse(s: &str) -> Option<Agent> {
        match s {
            "codex" => Some(Agent::Codex),
            "claude" => Some(Agent::Claude),
            _ => None,
        }
    }

    fn binary(&self) -> Option<std::path::PathBuf> {
        which(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Answer {
    pub agent: &'static str,
    pub text: String,
    /// Tokens this question cost, when the CLI reports them.
    pub tokens: Option<u64>,
}

/// Agent CLIs present on this machine, preferred order first.
pub fn available() -> Vec<Agent> {
    [Agent::Codex, Agent::Claude]
        .into_iter()
        .filter(|a| a.binary().is_some())
        .collect()
}

fn which(bin: &str) -> Option<std::path::PathBuf> {
    let out = Command::new("which").arg(bin).output().ok()?;
    out.status
        .success()
        .then(|| std::path::PathBuf::from(String::from_utf8_lossy(&out.stdout).trim().to_string()))
}

pub fn ask(agent: Agent, question: &str, context: &str) -> Result<Answer> {
    let bin = agent
        .binary()
        .ok_or_else(|| anyhow!("{} is not installed on this machine", agent.as_str()))?;
    let prompt = build_prompt(question, context);
    match agent {
        Agent::Codex => ask_codex(&bin, &prompt),
        Agent::Claude => ask_claude(&bin, &prompt),
    }
}

fn build_prompt(question: &str, context: &str) -> String {
    format!(
        "You are helping someone understand their Mac's disk usage. Below is a scan from \
         `diskwise`, which classifies directories by rule. Answer the question directly and \
         concretely, in the language the question is written in. Prefer specific paths and \
         numbers over general advice. Say plainly when something is not safe to delete.\n\n\
         Do not run any commands or read any files — everything you need is here.\n\n\
         === SCAN ===\n{context}\n=== END SCAN ===\n\nQuestion: {question}"
    )
}

/// `codex exec --json` emits JSONL; the answer is the last `agent_message`.
fn ask_codex(bin: &std::path::Path, prompt: &str) -> Result<Answer> {
    let out = run(Command::new(bin)
        .args([
            "exec",
            "--skip-git-repo-check",
            "--json",
            "--ephemeral",
            "--ignore-user-config",
            prompt,
        ])
        .current_dir(std::env::temp_dir()))?;

    let mut text = String::new();
    let mut tokens = None;
    for line in out.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v["item"]["type"] == "agent_message" {
            if let Some(t) = v["item"]["text"].as_str() {
                text = t.to_string();
            }
        }
        if v["type"] == "turn.completed" {
            let u = &v["usage"];
            tokens = Some(
                u["input_tokens"].as_u64().unwrap_or(0) + u["output_tokens"].as_u64().unwrap_or(0),
            );
        }
    }
    if text.is_empty() {
        bail!("codex returned no answer");
    }
    Ok(Answer {
        agent: "codex",
        text,
        tokens,
    })
}

fn ask_claude(bin: &std::path::Path, prompt: &str) -> Result<Answer> {
    let out = run(Command::new(bin).args(["-p", prompt, "--output-format", "json"]))?;
    let v: serde_json::Value =
        serde_json::from_str(out.trim()).map_err(|_| anyhow!("claude returned: {}", out.trim()))?;
    let text = v["result"]
        .as_str()
        .ok_or_else(|| anyhow!("claude returned no result field"))?
        .to_string();
    let tokens = v["usage"]["input_tokens"]
        .as_u64()
        .zip(v["usage"]["output_tokens"].as_u64())
        .map(|(i, o)| i + o);
    Ok(Answer {
        agent: "claude",
        text,
        tokens,
    })
}

/// These CLIs think for a while and read stdin if you leave it open, so it gets
/// closed immediately and the whole thing is bounded.
const TIMEOUT: Duration = Duration::from_secs(240);

fn run(cmd: &mut Command) -> Result<String> {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    drop(child.stdin.take().map(|mut s| s.write_all(b"")));

    let start = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            let out = child.wait_with_output()?;
            let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
            if !status.success() && stdout.trim().is_empty() {
                let err = String::from_utf8_lossy(&out.stderr);
                bail!("{}", first_lines(err.trim(), 4));
            }
            return Ok(stdout);
        }
        if start.elapsed() > TIMEOUT {
            let _ = child.kill();
            bail!(
                "the agent did not answer within {} seconds",
                TIMEOUT.as_secs()
            );
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn first_lines(s: &str, n: usize) -> String {
    s.lines().take(n).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_carries_the_data_and_forbids_tool_use() {
        let p = build_prompt(
            "为什么我的磁盘满了？",
            "29.61 GB  agent-session  archive  ~/.codex/sessions",
        );
        assert!(p.contains("为什么我的磁盘满了？"));
        assert!(p.contains("~/.codex/sessions"));
        // The agent must answer from the scan, not go wandering the filesystem.
        assert!(p.contains("Do not run any commands"));
        // and must reply in the user's language
        assert!(p.contains("in the language the question is written in"));
    }

    #[test]
    fn agent_names_round_trip() {
        assert_eq!(Agent::parse("codex"), Some(Agent::Codex));
        assert_eq!(Agent::parse("claude"), Some(Agent::Claude));
        assert_eq!(Agent::parse("gpt"), None);
        assert_eq!(Agent::Codex.as_str(), "codex");
    }

    /// Whatever is installed here must be discoverable; if nothing is, the list
    /// is empty rather than wrong.
    #[test]
    fn availability_reflects_what_is_installed() {
        for a in available() {
            assert!(a.binary().is_some_and(|p| p.exists()));
        }
    }
}
