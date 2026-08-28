//! Local web UI. Serves a single embedded page plus a small read-only JSON API.
//! Binds to loopback only — this thing can see your whole home directory.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use anyhow::Result;
use axum::extract::{Query as AxQuery, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::rules::Rules;
use crate::scan::{self, Scan};
use crate::view;

const INDEX_HTML: &str = include_str!("../web/index.html");

struct App {
    scan: RwLock<Scan>,
    rules: Rules,
    scanning: AtomicBool,
}

type Shared = Arc<App>;

pub fn cache_path() -> PathBuf {
    crate::rules::home_dir().join(".diskwise/index.json")
}

pub fn load_cache(root: &std::path::Path) -> Option<Scan> {
    let raw = std::fs::read(cache_path()).ok()?;
    let s: Scan = serde_json::from_slice(&raw).ok()?;
    (s.root == root).then_some(s)
}

pub fn save_cache(s: &Scan) {
    let p = cache_path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(bytes) = serde_json::to_vec(s) {
        let _ = std::fs::write(p, bytes);
    }
}

#[tokio::main]
pub async fn serve(root: PathBuf, port: u16, open_browser: bool) -> Result<()> {
    let root = root.canonicalize().unwrap_or(root);
    let cached = load_cache(&root);
    let have_cache = cached.is_some();
    let initial = match cached {
        Some(s) => {
            eprintln!("Loaded cached index for {} — rescanning in the background.", root.display());
            s
        }
        None => {
            eprintln!("Scanning {} …", root.display());
            let s = scan::scan(&root);
            save_cache(&s);
            s
        }
    };

    let app = Arc::new(App {
        scan: RwLock::new(initial),
        rules: Rules::load_default()?,
        scanning: AtomicBool::new(false),
    });

    if have_cache {
        spawn_rescan(Arc::clone(&app), root.clone());
    }

    let router = Router::new()
        .route("/", get(|| async { Html(INDEX_HTML) }))
        .route("/api/status", get(status))
        .route("/api/rows", get(rows))
        .route("/api/listdir", get(listdir))
        .route("/api/rescan", post(rescan))
        .with_state(app);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let url = format!("http://{addr}");
    eprintln!("diskwise UI on {url}  (ctrl-c to stop)");
    if open_browser {
        let _ = std::process::Command::new("open").arg(&url).spawn();
    }
    axum::serve(listener, router).await?;
    Ok(())
}

fn spawn_rescan(app: Shared, root: PathBuf) {
    if app.scanning.swap(true, Ordering::SeqCst) {
        return; // one at a time
    }
    std::thread::spawn(move || {
        let fresh = scan::scan(&root);
        save_cache(&fresh);
        *app.scan.write().unwrap() = fresh;
        app.scanning.store(false, Ordering::SeqCst);
    });
}

#[derive(Serialize)]
struct Status {
    root: PathBuf,
    total: u64,
    files: u64,
    denied: usize,
    scanning: bool,
    categories: Vec<String>,
}

async fn status(State(app): State<Shared>) -> Json<Status> {
    let s = app.scan.read().unwrap();
    let mut categories: Vec<String> = app.rules.all().iter().map(|r| r.category.clone()).collect();
    categories.sort();
    categories.dedup();
    Json(Status {
        root: s.root.clone(),
        total: s.total(),
        files: s.scanned_files,
        denied: s.denied,
        scanning: app.scanning.load(Ordering::SeqCst),
        categories,
    })
}

#[derive(Deserialize)]
struct RowsParams {
    dir: Option<String>,
    #[serde(default)]
    files: bool,
    #[serde(default)]
    min: u64,
    category: Option<String>,
    contains: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    200
}

#[derive(Serialize)]
struct RowsResponse {
    rows: Vec<view::Row>,
    reclaimable: u64,
    /// Total of the returned rows, so the treemap can size itself.
    shown: u64,
}

async fn rows(State(app): State<Shared>, AxQuery(p): AxQuery<RowsParams>) -> Json<RowsResponse> {
    let s = app.scan.read().unwrap();
    let q = view::Query {
        dir: p.dir.filter(|d| !d.is_empty()).map(PathBuf::from),
        files_only: p.files,
        min: p.min,
        category: p.category.filter(|c| !c.is_empty()),
        contains: p.contains.filter(|c| !c.is_empty()),
        limit: p.limit,
    };
    let rows = view::rows(&s, &app.rules, &q);
    Json(RowsResponse {
        reclaimable: view::reclaimable(&rows),
        shown: rows.iter().map(|r| r.size).sum(),
        rows,
    })
}

#[derive(Deserialize)]
struct PathParam {
    path: String,
}

/// Live listing of one directory, including the small files the index skips.
async fn listdir(
    State(app): State<Shared>,
    AxQuery(p): AxQuery<PathParam>,
) -> Result<Json<Vec<view::Row>>, (StatusCode, String)> {
    let dir = PathBuf::from(&p.path);
    let files = scan::list_files(&dir).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(view::file_rows(&app.rules, &files)))
}

async fn rescan(State(app): State<Shared>) -> impl IntoResponse {
    let root = app.scan.read().unwrap().root.clone();
    spawn_rescan(Arc::clone(&app), root);
    ([(header::CACHE_CONTROL, "no-store")], StatusCode::ACCEPTED)
}
