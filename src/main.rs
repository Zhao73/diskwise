mod rules;
mod scan;

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
        /// Only show entries this big or bigger, e.g. 500M, 2G.
        #[arg(long, default_value = "10M")]
        min: String,
        /// Emit JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Scan { path, top, files, shallow, category, min, json } => {
            let root = path.unwrap_or_else(rules::home_dir);
            let min = parse_size(&min)?;
            cmd_scan(root, top, files, shallow, category, min, json)
        }
    }
}

fn cmd_scan(
    root: PathBuf,
    top: usize,
    files: bool,
    shallow: bool,
    category: Option<String>,
    min: u64,
    json: bool,
) -> Result<()> {
    let rules = rules::Rules::load_default()?;
    let started = std::time::Instant::now();
    let s = scan::scan(&root);
    let elapsed = started.elapsed();

    #[derive(serde::Serialize)]
    struct Row {
        path: PathBuf,
        size: u64,
        human: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        verdict: Option<rules::Verdict>,
    }

    let mut rows: Vec<Row> = if files {
        s.big_files
            .iter()
            .map(|f| Row {
                verdict: rules.classify(&f.path),
                path: f.path.clone(),
                size: f.size,
                human: format_size(f.size, DECIMAL),
            })
            .collect()
    } else {
        let listed = if shallow { s.children(&s.root) } else { s.ranked() };
        let mut rows: Vec<Row> = listed
            .into_iter()
            .filter(|(p, _)| shallow || p != &s.root)
            .map(|(p, e)| Row {
                verdict: rules.classify(&p),
                path: p,
                size: e.total,
                human: format_size(e.total, DECIMAL),
            })
            .collect();
        if shallow {
            // A directory listing mixes folders and the loose files sitting
            // beside them — a 600MB sqlite file matters as much as a folder.
            rows.extend(s.big_files.iter().filter(|f| f.path.parent() == Some(&s.root)).map(|f| Row {
                verdict: rules.classify(&f.path),
                path: f.path.clone(),
                size: f.size,
                human: format_size(f.size, DECIMAL),
            }));
            rows.sort_by(|a, b| b.size.cmp(&a.size));
        }
        rows
    };

    rows.retain(|r| r.size >= min);
    if let Some(cat) = &category {
        rows.retain(|r| r.verdict.as_ref().is_some_and(|v| &v.category == cat));
    }
    if !files && !shallow {
        // Drop directories whose parent is already in the list — otherwise a
        // deep tree prints the same bytes a dozen times.
        let shown: std::collections::HashSet<PathBuf> = rows.iter().map(|r| r.path.clone()).collect();
        rows.retain(|r| !r.path.parent().is_some_and(|p| shown.contains(p)));
    }
    rows.truncate(top);

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
    let mut reclaimable = 0u64;
    for r in &rows {
        let (tag, note) = match &r.verdict {
            Some(v) => (format!("{:<14}", v.category), v.suggest.clone()),
            None => (format!("{:<14}", "-"), "-".into()),
        };
        if matches!(note.as_str(), "trash" | "archive") {
            reclaimable += r.size;
        }
        println!("{:>10}  {tag}  {:<8}  {}", r.human, note, r.path.display());
    }
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
