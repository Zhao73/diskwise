mod actions;
mod launch;
mod mcp;
mod plan;
mod policy;
mod procs;
mod rules;
mod scan;
mod server;
mod view;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use humansize::{format_size, DECIMAL};

#[derive(Parser)]
#[command(
    name = "diskwise",
    version,
    about = "See where your disk went — and what an agent left behind"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Scan a path and rank what is taking up space.
    Scan {
        /// Directory to scan (default: your home directory).
        path: Option<PathBuf>,
        /// How many rows to print.
        #[arg(long, default_value_t = 30)]
        top: usize,
        /// List individual files instead of directories.
        #[arg(long)]
        files: bool,
        /// Only show the immediate children of the scanned path.
        #[arg(long)]
        shallow: bool,
        /// Only show entries a rule recognises (e.g. agent-session, build).
        #[arg(long)]
        category: Option<String>,
        /// Only show paths containing this text.
        #[arg(long)]
        contains: Option<String>,
        /// Only show entries this big or bigger, e.g. 500M, 2G.
        #[arg(long, default_value = "10M")]
        min: String,
        /// Emit JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
    /// What is running, what it costs, and how long it has been going.
    Ps {
        #[arg(long, default_value_t = 25)]
        top: usize,
        /// Only processes running longer than this many days.
        #[arg(long)]
        days: Option<f64>,
        /// Only your own processes.
        #[arg(long)]
        mine: bool,
        /// Sort by resident memory instead of CPU.
        #[arg(long)]
        by_mem: bool,
        #[arg(long)]
        json: bool,
    },
    /// Stop a process (SIGTERM; --force sends SIGKILL).
    Kill {
        pid: i32,
        #[arg(long)]
        force: bool,
    },
    /// Build a cleanup plan. Nothing is touched until you confirm it.
    Clean {
        /// Directory to consider (default: your home directory).
        path: Option<PathBuf>,
        /// Stop once this much would be freed, e.g. 50G.
        #[arg(long)]
        target: Option<String>,
        /// Only this category, e.g. build, toolchain-cache, agent-session.
        #[arg(long)]
        category: Option<String>,
        /// Ignore anything smaller than this.
        #[arg(long, default_value = "100M")]
        min: String,
        /// Also archive irreplaceable-but-compressible data (agent sessions).
        #[arg(long)]
        archives: bool,
    },
    /// Execute a plan produced by `clean`.
    Confirm { plan_id: String },
    /// Archive one directory to ~/.diskwise/archives and trash the original.
    Archive { path: PathBuf },
    /// Restore an archive. Without --to it goes back where it came from.
    Restore {
        archive: PathBuf,
        #[arg(long)]
        to: Option<PathBuf>,
    },
    /// List archives diskwise has made.
    Archives,
    /// Run as an MCP server on stdio, for Claude Code / Codex.
    Mcp,
    /// Open the visual browser in a local web page.
    Ui {
        /// Directory to scan (default: your home directory).
        path: Option<PathBuf>,
        #[arg(long, default_value_t = 7373)]
        port: u16,
        /// Don't open a browser window.
        #[arg(long)]
        no_open: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Scan {
            path,
            top,
            files,
            shallow,
            category,
            contains,
            min,
            json,
        } => {
            let root = path.unwrap_or_else(rules::home_dir);
            let min = parse_size(&min)?;
            cmd_scan(ScanArgs {
                root,
                top,
                files,
                shallow,
                category,
                contains,
                min,
                json,
            })
        }
        Cmd::Clean {
            path,
            target,
            category,
            min,
            archives,
        } => {
            let root = path.unwrap_or_else(rules::home_dir);
            let opts = plan::PlanOptions {
                target: target.map(|t| parse_size(&t)).transpose()?.unwrap_or(0),
                category,
                min: parse_size(&min)?,
                include_archives: archives,
            };
            cmd_clean(root, opts)
        }
        Cmd::Ps {
            top,
            days,
            mine,
            by_mem,
            json,
        } => cmd_ps(top, days, mine, by_mem, json),
        Cmd::Kill { pid, force } => {
            procs::kill(pid, force)?;
            println!(
                "Sent {} to {pid}.",
                if force { "SIGKILL" } else { "SIGTERM" }
            );
            Ok(())
        }
        Cmd::Confirm { plan_id } => cmd_confirm(&plan_id),
        Cmd::Archive { path } => {
            let guard = policy::Guard::load()?;
            guard.check(&path).map_err(|d| anyhow::anyhow!("{d}"))?;
            let out = actions::archive(&path)?;
            println!("Archived to {}", out.display());
            Ok(())
        }
        Cmd::Restore { archive, to } => {
            let dest = actions::restore(&archive, to.as_deref())?;
            println!("Restored into {}", dest.display());
            Ok(())
        }
        Cmd::Archives => {
            for (p, m) in actions::list_archives()? {
                match m {
                    Some(m) => println!(
                        "{:>10}  <- {}  ({} entries, {} original)",
                        format_size(m.compressed, DECIMAL),
                        m.source.display(),
                        m.entries,
                        format_size(m.uncompressed, DECIMAL)
                    ),
                    None => println!("{:>10}  {}", "?", p.display()),
                }
            }
            Ok(())
        }
        Cmd::Mcp => mcp::serve(),
        Cmd::Ui {
            path,
            port,
            no_open,
        } => {
            let root = path.unwrap_or_else(rules::home_dir);
            server::serve(root, port, !no_open)
        }
    }
}

struct ScanArgs {
    root: PathBuf,
    top: usize,
    files: bool,
    shallow: bool,
    category: Option<String>,
    contains: Option<String>,
    min: u64,
    json: bool,
}

fn cmd_scan(a: ScanArgs) -> Result<()> {
    let ScanArgs {
        root,
        top,
        files,
        shallow,
        category,
        contains,
        min,
        json,
    } = a;
    let rules = rules::Rules::load_default()?;
    let started = std::time::Instant::now();
    let s = scan::scan(&root);
    let elapsed = started.elapsed();

    let q = view::Query {
        dir: shallow.then(|| s.root.clone()),
        files_only: files,
        min,
        category,
        contains,
        limit: top,
        keep_nested: false,
    };
    let rows = view::rows(&s, &rules, &q);

    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }

    println!(
        "{} — {} across {} files, scanned in {:.1}s{}",
        s.root.display(),
        format_size(s.total(), DECIMAL),
        s.scanned_files,
        elapsed.as_secs_f32(),
        if s.denied > 0 {
            format!(
                "  ({} paths unreadable — grant Full Disk Access to see them)",
                s.denied
            )
        } else {
            String::new()
        }
    );
    println!();
    for r in &rows {
        let (cat, sug) = match &r.verdict {
            Some(v) => (v.category.as_str(), v.suggest.as_str()),
            None => ("-", "-"),
        };
        println!("{:>10}  {cat:<15} {sug:<8}  {}", r.human, r.path.display());
    }
    let reclaimable = view::reclaimable(&rows);
    if reclaimable > 0 {
        println!(
            "\nReclaimable in the rows above: {}",
            format_size(reclaimable, DECIMAL)
        );
    }
    Ok(())
}

fn cmd_ps(top: usize, days: Option<f64>, mine: bool, by_mem: bool, json: bool) -> Result<()> {
    let me = std::env::var("USER").unwrap_or_default();
    let mut list = procs::list()?;
    if let Some(d) = days {
        list.retain(|p| p.uptime as f64 >= d * 86_400.0);
    }
    if mine {
        list.retain(|p| p.user == me);
    }
    if by_mem {
        list.sort_by(|a, b| b.rss.cmp(&a.rss));
    }
    list.truncate(top);

    if json {
        println!("{}", serde_json::to_string_pretty(&list)?);
        return Ok(());
    }
    println!(
        "{:>7}  {:>6}  {:>9}  {:>9}  PROCESS",
        "PID", "CPU%", "MEM", "UPTIME"
    );
    for p in &list {
        println!(
            "{:>7}  {:>6.1}  {:>9}  {:>9}  {}{}",
            p.pid,
            p.cpu,
            format_size(p.rss, DECIMAL),
            p.uptime_human,
            p.name,
            if p.protected { "  (protected)" } else { "" }
        );
    }
    Ok(())
}

fn cmd_clean(root: PathBuf, opts: plan::PlanOptions) -> Result<()> {
    let rules = rules::Rules::load_default()?;
    let guard = policy::Guard::load()?;
    eprintln!("Scanning {} …", root.display());
    let s = scan::scan(&root);
    let plan = plan::build(&s, &rules, &guard, &opts);

    if plan.items.is_empty() {
        println!("Nothing eligible. Try --archives, a smaller --min, or a different --category.");
        return Ok(());
    }
    for item in &plan.items {
        println!(
            "{:>10}  {:<8}  {}",
            format_size(item.size, DECIMAL),
            format!("{:?}", item.action).to_lowercase(),
            item.path.display()
        );
        println!("            {}", item.reason);
    }
    println!(
        "\nWould free {} across {} paths.",
        format_size(plan.total(), DECIMAL),
        plan.items.len()
    );
    println!("Deletions go to the Trash; archives are verified before the original is released.");

    match guard.check_unattended(&plan.paths(), plan.total()) {
        Ok(()) => {
            println!("\nPolicy allows this unattended. Applying …");
            report(actions::apply(&plan, &guard));
            Ok(())
        }
        Err(d) => {
            plan.save()?;
            println!("\n{d}");
            println!("Review it, then run:  diskwise confirm {}", plan.id);
            Ok(())
        }
    }
}

fn cmd_confirm(plan_id: &str) -> Result<()> {
    let plan = actions::Plan::load(plan_id)?;
    let guard = policy::Guard::load()?;
    println!(
        "Applying plan {} ({} paths, {}) …",
        plan.id,
        plan.items.len(),
        format_size(plan.total(), DECIMAL)
    );
    report(actions::apply(&plan, &guard));
    Ok(())
}

fn report(outcomes: Vec<actions::Outcome>) {
    let mut freed = 0u64;
    for o in &outcomes {
        match &o.error {
            Some(e) => println!("  FAILED  {}  — {e}", o.path.display()),
            None => {
                freed += o.freed;
                let where_to = o
                    .archive
                    .as_ref()
                    .map(|a| format!(" -> {}", a.display()))
                    .unwrap_or_default();
                println!("  ok      {}{where_to}", o.path.display());
            }
        }
    }
    println!("\nFreed {}.", format_size(freed, DECIMAL));
}

/// "500M", "2G", "1024" -> bytes.
fn parse_size(s: &str) -> Result<u64> {
    let s = s.trim();
    let (num, mult) = match s.chars().last() {
        Some('K' | 'k') => (&s[..s.len() - 1], 1u64 << 10),
        Some('M' | 'm') => (&s[..s.len() - 1], 1u64 << 20),
        Some('G' | 'g') => (&s[..s.len() - 1], 1u64 << 30),
        Some('T' | 't') => (&s[..s.len() - 1], 1u64 << 40),
        _ => (s, 1),
    };
    Ok((num.trim().parse::<f64>()? * mult as f64) as u64)
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_human_sizes() {
        assert_eq!(super::parse_size("1024").unwrap(), 1024);
        assert_eq!(super::parse_size("10M").unwrap(), 10 << 20);
        assert_eq!(super::parse_size("1.5G").unwrap(), 1_610_612_736);
    }
}
