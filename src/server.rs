//! Local web UI. Serves a single embedded page plus a small read-only JSON API.
//! Binds to loopback only — this thing can see your whole home directory.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use anyhow::Result;
use axum::extract::{Query as AxQuery, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::policy::Guard;
use crate::procs;
use crate::rules::Rules;
use crate::scan::{self, Scan};
use crate::view;
use crate::{actions, plan};

const INDEX_HTML: &str = include_str!("../web/index.html");
const APP_JS: &str = include_str!("../web/app.js");

struct App {
    scan: RwLock<Arc<Scan>>,
    rules: Arc<Rules>,
    scanning: AtomicBool,
    annotations: Arc<Mutex<crate::annotate::Store>>,
    annotating: Arc<crate::annotate::Progress>,
    /// Long-running questions, so the page never has to sit and wait.
    jobs: Mutex<HashMap<String, Job>>,
}

#[derive(Clone, Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum Job {
    Running,
    Done { answer: crate::ask::Answer },
    Failed { error: String },
}

type Shared = Arc<App>;

pub fn cache_path() -> PathBuf {
    crate::rules::home_dir().join(".diskwise/index.json")
}

pub fn load_cache(root: &std::path::Path) -> Option<Scan> {
    let file = std::fs::File::open(cache_path()).ok()?;
    let s: Scan = serde_json::from_reader(std::io::BufReader::new(file)).ok()?;
    (s.root == root).then_some(s)
}

pub fn save_cache(s: &Scan) {
    let p = cache_path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // Streamed, not buffered: this index is tens of megabytes.
    if let Ok(file) = std::fs::File::create(&p) {
        let _ = serde_json::to_writer(std::io::BufWriter::new(file), s);
    }
}

#[tokio::main]
pub async fn serve(root: PathBuf, port: u16, open_browser: bool) -> Result<()> {
    let root = root.canonicalize().unwrap_or(root);
    let initial = match load_cache(&root) {
        Some(s) => {
            eprintln!(
                "Loaded cached index for {} — rescanning in the background.",
                root.display()
            );
            s
        }
        None => {
            // Serve the page first in every case: a cold scan of a large home
            // directory takes the better part of a minute, and staring at a
            // refused connection is not a loading state.
            eprintln!("Scanning {} in the background …", root.display());
            Scan::empty(root.clone())
        }
    };

    let app = Arc::new(App {
        scan: RwLock::new(Arc::new(initial)),
        rules: Arc::new(Rules::load_default()?),
        scanning: AtomicBool::new(false),
        annotations: Arc::new(Mutex::new(crate::annotate::Store::load())),
        annotating: Arc::new(crate::annotate::Progress::default()),
        jobs: Mutex::new(HashMap::new()),
    });

    spawn_rescan(Arc::clone(&app), root.clone());
    // A cached index means nothing is scanning, so annotate what we already have.
    if !app.scanning.load(Ordering::SeqCst) {
        let s = Arc::clone(&app.scan.read().unwrap());
        start_annotating(&app, s);
    }

    let router = Router::new()
        .route("/", get(|| async { Html(INDEX_HTML) }))
        .route(
            "/app.js",
            get(|| async { ([(header::CONTENT_TYPE, "text/javascript")], APP_JS) }),
        )
        .route("/api/status", get(status))
        .route("/api/rows", get(rows))
        .route("/api/listdir", get(listdir))
        .route("/api/rescan", post(rescan))
        .route("/api/procs", get(processes))
        .route("/api/kill", post(kill_proc))
        .route("/api/policy", get(policy_info))
        .route("/api/plan", post(build_plan))
        .route("/api/confirm", post(confirm_plan))
        .route("/api/agents", get(agents))
        .route("/api/ask", post(ask_agent))
        .route("/api/inspect", get(inspect_path))
        .route("/api/annotations", get(annotations))
        .route("/api/job/{id}", get(job_status))
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
        let fresh = Arc::new(scan::scan(&root));
        save_cache(&fresh);
        *app.scan.write().unwrap() = Arc::clone(&fresh);
        app.scanning.store(false, Ordering::SeqCst);
        // The point of the whole exercise: by the time someone has finished
        // reading the first screen, the folders on it are already explained.
        start_annotating(&app, fresh);
    });
}

fn start_annotating(app: &Shared, scan: Arc<Scan>) {
    crate::annotate::spawn(
        scan,
        Arc::clone(&app.rules),
        Arc::clone(&app.annotations),
        Arc::clone(&app.annotating),
        crate::ask::available().first().copied(),
    );
}

#[derive(Serialize)]
struct Status {
    root: PathBuf,
    total: u64,
    files: u64,
    denied: usize,
    scanning: bool,
    categories: Vec<String>,
    /// Who the UI is running as, so it can filter to "only mine".
    user: String,
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
        user: procs::whoami(),
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
        keep_nested: false,
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

async fn processes() -> Result<Json<Vec<procs::Proc>>, (StatusCode, String)> {
    procs::list()
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

#[derive(Deserialize)]
struct KillParams {
    pid: i32,
    #[serde(default)]
    force: bool,
}

async fn kill_proc(AxQuery(p): AxQuery<KillParams>) -> Result<StatusCode, (StatusCode, String)> {
    procs::kill(p.pid, p.force)
        .map(|_| StatusCode::OK)
        .map_err(|e| (StatusCode::FORBIDDEN, e.to_string()))
}

#[derive(Serialize)]
struct PolicyInfo {
    config_file: PathBuf,
    mode: String,
    max_auto_delete_gb: f64,
    auto_allow: Vec<String>,
    never: Vec<String>,
    archives_dir: PathBuf,
}

async fn policy_info() -> Result<Json<PolicyInfo>, ApiError> {
    let g = Guard::load().map_err(ApiError::from)?;
    Ok(Json(PolicyInfo {
        config_file: crate::policy::policy_path(),
        mode: format!("{:?}", g.policy.default).to_lowercase(),
        max_auto_delete_gb: g.policy.max_auto_delete_gb,
        auto_allow: g.policy.auto_allow.clone(),
        never: g.policy.never.clone(),
        archives_dir: actions::archives_dir(),
    }))
}

#[derive(Deserialize)]
struct PlanRequest {
    /// Explicit paths chosen in the UI. Empty means "whatever the rules suggest".
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    include_archives: bool,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    min: u64,
}

/// Build a plan. Nothing is modified here — this is the preview the person
/// reviews before they press the second button.
async fn build_plan(
    State(app): State<Shared>,
    Json(req): Json<PlanRequest>,
) -> Result<Json<actions::Plan>, ApiError> {
    let guard = Guard::load().map_err(ApiError::from)?;
    let s = app.scan.read().unwrap();
    let p = if req.paths.is_empty() {
        plan::build(
            &s,
            &app.rules,
            &guard,
            &plan::PlanOptions {
                min: req.min,
                category: req.category,
                include_archives: req.include_archives,
                ..Default::default()
            },
        )
    } else {
        plan::for_paths(
            &s,
            &app.rules,
            &guard,
            &req.paths.iter().map(PathBuf::from).collect::<Vec<_>>(),
        )
    };
    p.save().map_err(ApiError::from)?;
    Ok(Json(p))
}

#[derive(Deserialize)]
struct ConfirmRequest {
    plan_id: String,
}

/// Apply a plan. Reaching this endpoint means a person clicked the confirm
/// button in their own browser — the same standing as `diskwise confirm` on the
/// command line, and deliberately not something the MCP server can reach.
async fn confirm_plan(
    State(app): State<Shared>,
    Json(req): Json<ConfirmRequest>,
) -> Result<Json<Vec<actions::Outcome>>, ApiError> {
    let guard = Guard::load().map_err(ApiError::from)?;
    let p = actions::Plan::load(&req.plan_id).map_err(ApiError::from)?;
    let outcomes = actions::apply(&p, &guard);
    let root = app.scan.read().unwrap().root.clone();
    spawn_rescan(Arc::clone(&app), root);
    Ok(Json(outcomes))
}

#[derive(Deserialize)]
struct InspectParams {
    path: String,
    kind: String,
}

/// Ask the tool that owns an opaque blob what is inside it.
async fn inspect_path(
    AxQuery(p): AxQuery<InspectParams>,
) -> Result<Json<crate::inspect::Inspection>, ApiError> {
    let path = PathBuf::from(p.path);
    tokio::task::spawn_blocking(move || crate::inspect::run(&p.kind, &path))
        .await
        .map_err(|e| ApiError(e.to_string()))?
        .map(Json)
        .map_err(ApiError::from)
}

async fn agents() -> Json<Vec<&'static str>> {
    Json(crate::ask::available().iter().map(|a| a.as_str()).collect())
}

#[derive(Deserialize)]
struct AskRequest {
    question: String,
    #[serde(default)]
    agent: Option<String>,
}

/// Hand the scan to an agent CLI the user is already signed into. This spends
/// their subscription quota, so it only happens on an explicit request.
/// Returns a job id straight away. An agent takes tens of seconds to think and
/// there is no reason for the page — or the person reading it — to sit still
/// for that.
async fn ask_agent(
    State(app): State<Shared>,
    Json(req): Json<AskRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let agent = match req.agent.as_deref() {
        Some(name) => crate::ask::Agent::parse(name)
            .ok_or_else(|| ApiError(format!("unknown agent: {name}")))?,
        None => *crate::ask::available()
            .first()
            .ok_or_else(|| ApiError("no agent CLI found; install codex or claude".into()))?,
    };
    let context = {
        let s = app.scan.read().unwrap();
        crate::view::digest(&s.clone(), &app.rules)
    };
    let id = format!(
        "{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    app.jobs.lock().unwrap().insert(id.clone(), Job::Running);

    let question = req.question;
    let jobs_app = Arc::clone(&app);
    let job_id = id.clone();
    std::thread::spawn(move || {
        let result = crate::ask::ask(agent, &question, &context);
        let entry = match result {
            Ok(answer) => Job::Done { answer },
            Err(e) => Job::Failed {
                error: e.to_string(),
            },
        };
        jobs_app.jobs.lock().unwrap().insert(job_id, entry);
    });
    Ok(Json(serde_json::json!({ "job_id": id })))
}

async fn job_status(
    State(app): State<Shared>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<Job>, ApiError> {
    app.jobs
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .map(Json)
        .ok_or_else(|| ApiError(format!("no such job: {id}")))
}

#[derive(Serialize)]
struct Annotations {
    #[serde(flatten)]
    progress: crate::annotate::Status,
    items: HashMap<PathBuf, crate::annotate::Annotation>,
}

async fn annotations(State(app): State<Shared>) -> Json<Annotations> {
    Json(Annotations {
        progress: app.annotating.status(),
        items: app.annotations.lock().unwrap().items.clone(),
    })
}

pub struct ApiError(String);

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError(e.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (StatusCode::BAD_REQUEST, self.0).into_response()
    }
}

async fn rescan(State(app): State<Shared>) -> impl IntoResponse {
    let root = app.scan.read().unwrap().root.clone();
    spawn_rescan(Arc::clone(&app), root);
    ([(header::CACHE_CONTROL, "no-store")], StatusCode::ACCEPTED)
}
