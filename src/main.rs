mod rules;
mod scan;
mod server;
mod view;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use humansize::{format_size, DECIMAL};

#[derive(Parser)]
#[command(name = "diskwise", version, about = "See where your disk went — and what an agent left behind")]
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
        Cmd::Scan { path, top, files, shallow, category, contains, min, json } => {
            let root = path.unwrap_or_else(rules::home_dir);
            let min = parse_size(&min)?;
            cmd_scan(root, top, files, shallow, category, contains, min, json)
        }
        Cmd::Ui { path, port, no_open } => {
            let root = path.unwrap_or_else(rules::home_dir);
            server::serve(root, port, !no_open)
        }
    }
}

fn cmd_scan(
    root: PathBuf,
    top: usize,
    files: bool,
    shallow: bool,
    category: Option<String>,
    contains: Option<String>,
    min: u64,
    json: bool,
) -> Result<()> {
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
            format!("  ({} paths unreadable — grant Full Disk Access to see them)", s.denied)
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
        println!("\nReclaimable in the rows above: {}", format_size(reclaimable, DECIMAL));
    }
    Ok(())
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
