//! poecraft local web server (spec §3): JSON API over the Rust core + the static web UI.
//! Binds to 127.0.0.1 only. Run: `cargo run --release -p poecraft-server` then open
//! http://127.0.0.1:8790
//!
//! API (all JSON):
//!   GET  /api/meta                       data source, classes, influences
//!   GET  /api/bases?q=&class=            base search
//!   POST /api/pool        {item}         pool view, hints, legality problems, elevated options
//!   POST /api/item/text   {item, roll?, quality?, name?}   PoB item text
//!   POST /api/item/parse  {text}         PoB / in-game text -> item
//!   GET  /api/recipes                    recipe files in recipes/ (name, path, source)
//!   POST /api/jobs/run     {source, path?, runs?, mode?}            -> {job}
//!   POST /api/jobs/compare {recipes: [{source, path?}], runs?, mode?} -> {job}
//!   GET  /api/jobs/{id}                  progress (phase, done, total, label) and result
//!   GET  /api/rules                      rules registry
//!   GET  /api/prices                     price table and its metadata
//!   POST /api/prices/fetch               run tools/fetch_prices.py (poe.ninja) and reload

use anyhow::{anyhow, Result};
use axum::extract::{Path as AxPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use poecraft_core::analysis::{analyse, Mode, Options, Progress, Report};
use poecraft_core::builder::{hints, pool_view, validate};
use poecraft_core::data::{default_data, title, GameData, Gen, Influence, Influences};
use poecraft_core::item::{Item, Rarity, Roll};
use poecraft_core::itemtext::{parse_text, to_pob_text, ExportOpts};
use poecraft_core::lang::parse;
use poecraft_core::prices::Prices;
use poecraft_core::rules::RULES;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

// ------------------------------------------------------------------ state
#[derive(Clone)]
struct App {
    gd: &'static GameData,
    prices: Arc<std::sync::RwLock<Arc<Prices>>>,
    root: PathBuf,
    jobs: Arc<Mutex<HashMap<u64, Job>>>,
    next: Arc<AtomicU64>,
}

struct Job {
    progress: Arc<Progress>,
    result: Option<Result<Value, String>>,
    started: std::time::Instant,
}

struct ApiError(StatusCode, String);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}
impl<E: std::fmt::Display> From<E> for ApiError {
    fn from(e: E) -> Self {
        ApiError(StatusCode::BAD_REQUEST, e.to_string())
    }
}
type ApiResult<T> = Result<Json<T>, ApiError>;

impl App {
    fn prices(&self) -> Arc<Prices> {
        self.prices.read().unwrap().clone()
    }
}

// ------------------------------------------------------------------ item DTOs
#[derive(Deserialize, Serialize, Clone, Debug)]
struct ModDto {
    id: String,
    #[serde(default)]
    crafted: bool,
    #[serde(default)]
    fractured: bool,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
struct ItemDto {
    base: String,
    #[serde(default = "default_ilvl")]
    ilvl: u32,
    #[serde(default = "default_rarity")]
    rarity: String,
    #[serde(default)]
    influences: Vec<String>,
    #[serde(default)]
    mods: Vec<ModDto>,
}
fn default_ilvl() -> u32 {
    86
}
fn default_rarity() -> String {
    "rare".into()
}

fn to_item(gd: &GameData, d: &ItemDto) -> Result<Item> {
    let base = gd.resolve_base(&d.base)?;
    let mut infl = Influences::default();
    for i in &d.influences {
        infl = infl.with(Influence::parse(i).ok_or_else(|| anyhow!("unknown influence {i}"))?);
    }
    let rarity = Rarity::parse(&d.rarity).ok_or_else(|| anyhow!("rarity must be normal, magic or rare"))?;
    let mut mods = vec![];
    for m in &d.mods {
        let mi = gd.mod_idx(&m.id).ok_or_else(|| anyhow!("unknown mod id {}", m.id))?;
        mods.push(Roll { m: mi, crafted: m.crafted || gd.m(mi).crafted, fractured: m.fractured });
    }
    Ok(Item::new(base, d.ilvl, rarity, infl).with_mods(mods))
}

fn item_view(gd: &GameData, it: &Item) -> Value {
    let b = gd.base(it.base);
    let mods: Vec<Value> = [Gen::Prefix, Gen::Suffix]
        .iter()
        .flat_map(|g| it.side(gd, *g).collect::<Vec<_>>())
        .map(|r| {
            let m = gd.m(r.m);
            json!({
                "id": m.id, "side": m.gen.name(), "tier": gd.tier(r.m, it.base),
                "elevated": m.elevated, "influence": m.influence.map(|i| i.name()),
                "crafted": r.crafted, "fractured": r.fractured,
                "text": poecraft_core::itemtext::clean(&m.text).replace('\n', " / "),
            })
        })
        .collect();
    json!({
        "base": b.name, "base_id": b.id, "item_class": b.item_class, "ilvl": it.ilvl, "rarity": it.rarity.name(),
        "influences": it.influences.iter().map(|i| i.name()).collect::<Vec<_>>(),
        "mods": mods, "open_prefix": it.open_slots(gd, Gen::Prefix), "open_suffix": it.open_slots(gd, Gen::Suffix),
        "implicits": b.implicits.iter().filter_map(|id| gd.mod_idx(id)).map(|m| gd.m(m).short()).collect::<Vec<_>>(),
    })
}

// ------------------------------------------------------------------ handlers
async fn meta(State(app): State<App>) -> Json<Value> {
    let gd = app.gd;
    let mut classes: Vec<&String> = gd.class_influence_tags.keys().collect();
    classes.sort();
    Json(json!({
        "source": gd.source, "classes": classes,
        "influences": Influence::ALL.iter().map(|i| i.name()).collect::<Vec<_>>(),
        "prices": app.prices().meta, "divine": app.prices().divine(),
    }))
}

#[derive(Deserialize)]
struct BaseQ {
    q: Option<String>,
    class: Option<String>,
}
async fn bases(State(app): State<App>, Query(q): Query<BaseQ>) -> Json<Value> {
    let low = q.q.unwrap_or_default().to_lowercase();
    let mut seen = std::collections::BTreeSet::new();
    let mut out = vec![];
    let mut list: Vec<_> = app.gd.bases.iter().collect();
    list.sort_by(|a, b| a.item_class.cmp(&b.item_class).then(b.drop_level.cmp(&a.drop_level)));
    for b in list {
        if !b.name.to_lowercase().contains(&low) || q.class.as_ref().is_some_and(|c| c != &b.item_class) || !seen.insert(b.name.clone()) {
            continue;
        }
        out.push(json!({"name": b.name, "class": b.item_class, "drop_level": b.drop_level}));
        if out.len() >= 60 {
            break;
        }
    }
    Json(json!(out))
}

#[derive(Deserialize)]
struct ItemReq {
    item: ItemDto,
}
async fn pool(State(app): State<App>, Json(r): Json<ItemReq>) -> ApiResult<Value> {
    let gd = app.gd;
    let it = to_item(gd, &r.item)?;
    let view = pool_view(gd, &it);
    // elevated options: influenced families of this base with an elevated version
    let mut elevated = vec![];
    let mut seen = std::collections::BTreeSet::new();
    for e in &view {
        if e.influence.is_some() && seen.insert(e.family) {
            if let Some(el) = gd.families[e.family as usize].elevated {
                let m = gd.m(el);
                elevated.push(json!({"id": m.id, "side": m.gen.name(), "influence": m.influence.map(|i| i.name()), "text": poecraft_core::itemtext::clean(&m.text).replace('\n', " / ")}));
            }
        }
    }
    // bench options for the class
    let cls = gd.item_class(it.base).to_string();
    let bench: Vec<Value> = gd
        .bench
        .iter()
        .filter(|b| b.classes.iter().any(|c| c == &cls))
        .map(|b| {
            let m = gd.m(b.module);
            json!({"id": m.id, "side": m.gen.name(), "level": m.level, "text": poecraft_core::itemtext::clean(&m.text).replace('\n', " / "), "cost": b.cost})
        })
        .collect();
    Ok(Json(json!({
        "item": item_view(gd, &it), "pool": view, "hints": hints(gd, &it), "problems": validate(gd, &it),
        "elevated": elevated, "bench": bench,
    })))
}

#[derive(Deserialize)]
struct TextReq {
    item: ItemDto,
    roll: Option<f64>,
    quality: Option<u32>,
    name: Option<String>,
}
async fn item_text(State(app): State<App>, Json(r): Json<TextReq>) -> ApiResult<Value> {
    let it = to_item(app.gd, &r.item)?;
    let o = ExportOpts { roll: r.roll.unwrap_or(1.0), quality: r.quality.unwrap_or(20), name: r.name.unwrap_or_else(|| "Crafted Item".into()), ..Default::default() };
    Ok(Json(json!({ "text": to_pob_text(app.gd, &it, &o) })))
}

#[derive(Deserialize)]
struct ParseReq {
    text: String,
}
async fn item_parse(State(app): State<App>, Json(r): Json<ParseReq>) -> ApiResult<Value> {
    let gd = app.gd;
    let p = parse_text(gd, &r.text)?;
    let dto = ItemDto {
        base: gd.base(p.item.base).name.clone(),
        ilvl: p.item.ilvl,
        rarity: p.item.rarity.name().into(),
        influences: p.item.influences.iter().map(|i| i.name().to_string()).collect(),
        mods: p.item.mods.iter().map(|r| ModDto { id: gd.m(r.m).id.clone(), crafted: r.crafted, fractured: r.fractured }).collect(),
    };
    Ok(Json(json!({ "item": dto, "view": item_view(gd, &p.item), "unmatched": p.unmatched, "ambiguous": p.ambiguous, "problems": validate(gd, &p.item), "quality": p.quality, "name": p.name })))
}

async fn recipes(State(app): State<App>) -> Json<Value> {
    fn walk(d: &Path, out: &mut Vec<PathBuf>) {
        if let Ok(rd) = std::fs::read_dir(d) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|x| x == "craft") {
                    out.push(p);
                }
            }
        }
    }
    let base = app.root.join("recipes");
    let mut files = vec![];
    walk(&base, &mut files);
    files.sort();
    let list: Vec<Value> = files
        .iter()
        .filter(|p| !p.components().any(|c| c.as_os_str() == "lib" || c.as_os_str() == "tests"))
        .map(|p| {
            let rel = p.strip_prefix(&app.root).unwrap_or(p).to_string_lossy().replace('\\', "/");
            json!({"name": p.file_stem().map(|s| s.to_string_lossy().to_string()), "path": rel, "source": std::fs::read_to_string(p).unwrap_or_default()})
        })
        .collect();
    Json(json!(list))
}

#[derive(Deserialize)]
struct RecipeIn {
    source: String,
    path: Option<String>,
    name: Option<String>,
}
#[derive(Deserialize)]
struct RunReq {
    #[serde(flatten)]
    recipe: RecipeIn,
    runs: Option<usize>,
    mode: Option<String>,
}
#[derive(Deserialize)]
struct CompareReq {
    recipes: Vec<RecipeIn>,
    runs: Option<usize>,
    mode: Option<String>,
}

fn parse_recipe(app: &App, r: &RecipeIn) -> Result<poecraft_core::lang::Program, String> {
    // includes resolve relative to the recipe's file; editor text behaves as if in recipes/
    let p = r.path.as_ref().map(|p| app.root.join(p)).unwrap_or_else(|| app.root.join("recipes").join("editor.craft"));
    let mut prog = parse(&r.source, Some(&p)).map_err(|e| e.to_string())?;
    if let Some(n) = &r.name {
        prog.name = n.clone();
    }
    Ok(prog)
}

fn start_job(app: &App, label: String, work: impl FnOnce(Arc<Progress>) -> Result<Value, String> + Send + 'static) -> u64 {
    let id = app.next.fetch_add(1, Ordering::Relaxed);
    let progress = Arc::new(Progress::default());
    *progress.label.lock().unwrap() = label;
    app.jobs.lock().unwrap().insert(id, Job { progress: progress.clone(), result: None, started: std::time::Instant::now() });
    let jobs = app.jobs.clone();
    tokio::task::spawn_blocking(move || {
        let res = work(progress);
        if let Some(j) = jobs.lock().unwrap().get_mut(&id) {
            j.result = Some(res);
        }
    });
    id
}

fn options(runs: Option<usize>, mode: &Option<String>, p: Arc<Progress>) -> Result<Options, String> {
    Ok(Options {
        runs: runs.unwrap_or(5000).clamp(1, 2_000_000),
        mode: match mode.as_deref() {
            None => Mode::Auto,
            Some(m) => Mode::parse(m).ok_or_else(|| format!("bad mode {m}"))?,
        },
        progress: Some(p),
        ..Default::default()
    })
}

fn report_json(r: &Report, prices: &Prices) -> Value {
    let mut v = serde_json::to_value(r).unwrap_or(Value::Null);
    v["text"] = json!(r.text(prices));
    v
}

async fn job_run(State(app): State<App>, Json(r): Json<RunReq>) -> ApiResult<Value> {
    let prog = parse_recipe(&app, &r.recipe).map_err(|e| ApiError(StatusCode::BAD_REQUEST, e))?;
    let (gd, prices, runs, mode) = (app.gd, app.prices(), r.runs, r.mode.clone());
    let id = start_job(&app, prog.name.clone(), move |p| {
        let o = options(runs, &mode, p)?;
        let rep = analyse(gd, &prices, &prog, &o).map_err(|e| e.to_string())?;
        Ok(report_json(&rep, &prices))
    });
    Ok(Json(json!({ "job": id })))
}

async fn job_compare(State(app): State<App>, Json(r): Json<CompareReq>) -> ApiResult<Value> {
    let mut progs = vec![];
    for x in &r.recipes {
        progs.push(parse_recipe(&app, x).map_err(|e| ApiError(StatusCode::BAD_REQUEST, e))?);
    }
    let (gd, prices, runs, mode) = (app.gd, app.prices(), r.runs, r.mode.clone());
    let n = progs.len();
    let id = start_job(&app, format!("compare {n} recipes"), move |p| {
        let mut reps = vec![];
        for (k, prog) in progs.iter().enumerate() {
            *p.label.lock().unwrap() = format!("[{}/{n}] {}", k + 1, prog.name);
            let o = options(runs, &mode, p.clone())?;
            reps.push(analyse(gd, &prices, prog, &o).map_err(|e| format!("{}: {e}", prog.name))?);
        }
        reps.sort_by(|a, b| a.cost_per_success.partial_cmp(&b.cost_per_success).unwrap());
        Ok(json!({ "reports": reps.iter().map(|r| report_json(r, &prices)).collect::<Vec<_>>(), "table": poecraft_core::analysis::compare_table(&reps) }))
    });
    Ok(Json(json!({ "job": id })))
}

async fn job_get(State(app): State<App>, AxPath(id): AxPath<u64>) -> ApiResult<Value> {
    let jobs = app.jobs.lock().unwrap();
    let j = jobs.get(&id).ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("no job {id}")))?;
    let p = &j.progress;
    let mut v = json!({
        "id": id,
        "phase": *p.phase.lock().unwrap(),
        "label": *p.label.lock().unwrap(),
        "done": p.done.load(Ordering::Relaxed),
        "total": p.total.load(Ordering::Relaxed),
        "elapsed": j.started.elapsed().as_secs_f64(),
        "status": match &j.result { None => "running", Some(Ok(_)) => "done", Some(Err(_)) => "error" },
    });
    match &j.result {
        Some(Ok(r)) => v["result"] = r.clone(),
        Some(Err(e)) => v["error"] = json!(e),
        None => {}
    }
    Ok(Json(v))
}

async fn rules() -> Json<Value> {
    Json(json!(RULES))
}

async fn prices_handler(State(app): State<App>) -> Json<Value> {
    let p = app.prices();
    Json(json!({ "meta": p.meta, "prices": p.table }))
}

/// Run the poe.ninja fetcher (Python, stdlib only) and reload the price table.
async fn prices_fetch(State(app): State<App>) -> ApiResult<Value> {
    let root = app.root.clone();
    let out = tokio::task::spawn_blocking(move || {
        let py = ["python", "py", "python3"].into_iter().find_map(|exe| {
            std::process::Command::new(exe).arg("--version").output().ok().filter(|o| o.status.success()).map(|_| exe)
        });
        let exe = py.ok_or("Python is needed to fetch prices (tools/fetch_prices.py)")?;
        std::process::Command::new(exe).arg(root.join("tools").join("fetch_prices.py")).current_dir(&root).output().map_err(|e| e.to_string().leak() as &str)
    })
    .await
    .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let log = String::from_utf8_lossy(&out.stderr).to_string();
    if !out.status.success() {
        return Err(ApiError(StatusCode::BAD_GATEWAY, format!("price fetch failed:\n{log}")));
    }
    let fresh = Prices::load(&app.gd.data_dir, &[], true).map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    *app.prices.write().unwrap() = Arc::new(fresh);
    Ok(Json(json!({ "log": log, "meta": app.prices().meta })))
}

// ------------------------------------------------------------------ main
#[tokio::main]
async fn main() -> Result<()> {
    let gd = default_data();
    let root = gd.data_dir.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let prices = Prices::load(&gd.data_dir, &[], true).map_err(|e| anyhow!(e))?;
    let app = App { gd, prices: Arc::new(std::sync::RwLock::new(Arc::new(prices))), root: root.clone(), jobs: Arc::new(Mutex::new(HashMap::new())), next: Arc::new(AtomicU64::new(1)) };
    let api = Router::new()
        .route("/api/meta", get(meta))
        .route("/api/bases", get(bases))
        .route("/api/pool", post(pool))
        .route("/api/item/text", post(item_text))
        .route("/api/item/parse", post(item_parse))
        .route("/api/recipes", get(recipes))
        .route("/api/jobs/run", post(job_run))
        .route("/api/jobs/compare", post(job_compare))
        .route("/api/jobs/{id}", get(job_get))
        .route("/api/rules", get(rules))
        .route("/api/prices", get(prices_handler))
        .route("/api/prices/fetch", post(prices_fetch))
        .with_state(app);
    let web = tower_http::services::ServeDir::new(root.join("web"));
    let router = api.fallback_service(web);
    let port: u16 = std::env::var("POECRAFT_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8790);
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("poecraft server: http://{addr}  (data {}, {})", gd.source["game_version"], title("local only"));
    axum::serve(listener, router).await?;
    Ok(())
}
