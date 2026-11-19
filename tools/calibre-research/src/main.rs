mod curation;
mod facts;
mod timeline;

use anyhow::{bail, Context, Result};
use curation::Run as ChronologyRun;
use curator_core::{
    ai::{AiSettings, Ollama},
    app::App,
    metadata,
    model::{Book, Settings},
    store::{atomic_json, Store},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    os::fd::AsRawFd,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

const VERSION: &str = "nexus-facts-batch-v2";
const PROTECTED: [&str; 4] = [
    "Greater Eternal Champion Saga",
    "Greater Foundation Universe",
    "The 39 Clues",
    "The Grand Tour",
];

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    library: PathBuf,
    workspace: PathBuf,
    job_id: String,
    mode: String,
    model: String,
    endpoint: String,
    enable_ai: bool,
    refresh: bool,
    book_ids: Vec<i64>,
    limit: Option<usize>,
    protected_uuids: Vec<String>,
    #[serde(default = "default_pages")]
    source_pages: usize,
    #[serde(default = "default_calls")]
    extraction_calls: usize,
}

fn default_pages() -> usize {
    6
}
fn default_calls() -> usize {
    8
}

#[derive(Clone, Serialize, Deserialize)]
struct Saved {
    version: String,
    created: u64,
    book: Book,
    fingerprint: String,
    status: String,
    error: Option<String>,
    run: Option<ChronologyRun>,
    #[serde(default)]
    checked_sources: Vec<String>,
    #[serde(default)]
    discovered_sources: Vec<String>,
}

fn protected(book: &Book, uuids: &BTreeSet<String>) -> bool {
    uuids.contains(&book.uuid.to_lowercase())
        || book.hierarchy.iter().any(|v| {
            v.name
                .as_ref()
                .is_some_and(|name| PROTECTED.contains(&name.as_str()))
        })
}

fn validate_saved(saved: &Saved, book: &Book, store: &Store) -> Result<()> {
    if saved.version != VERSION
        || !["ready", "partial"].contains(&saved.status.as_str())
        || saved.book.id != book.id
        || saved.book.uuid != book.uuid
        || saved.fingerprint != metadata::book_fingerprint(book)?
    {
        bail!("Saved research is stale, unsupported or belongs to another work");
    }
    let run = saved
        .run
        .as_ref()
        .context("Saved research has no evidence")?;
    if run.evidence.is_empty() {
        bail!("Ready research must contain source evidence");
    }
    curation::validate_run(run, book, store)
}

fn emit(event: Value) {
    println!("{event}");
}

fn storage_guard(workspace: &Path) -> Result<()> {
    let total = ["state.sqlite3", "state.sqlite3-wal"]
        .iter()
        .filter_map(|name| fs::metadata(workspace.join(name)).ok())
        .map(|s| s.len())
        .sum::<u64>();
    if total > 512 * 1024 * 1024 {
        bail!("Research database reached the 512 MiB safety budget; retain results and review storage");
    }
    Ok(())
}

fn process(request: &Request) -> Result<PathBuf> {
    if !["research", "export"].contains(&request.mode.as_str())
        || request.job_id.len() != 32
        || !request.job_id.bytes().all(|c| c.is_ascii_hexdigit())
        || request.limit == Some(0)
        || !(1..=12).contains(&request.source_pages)
        || !(1..=32).contains(&request.extraction_calls)
    {
        bail!("Invalid research mode, job identity or batch size");
    }
    let library = request.library.canonicalize()?;
    if !library.join("metadata.db").is_file() {
        bail!("Choose an existing Calibre library");
    }
    // Store::open separately rejects symlinks and workspaces inside libraries.
    let mut app = App::open(&request.workspace, PathBuf::new())?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(app.store.root.join("batch.lock"))?;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        bail!("Another research batch owns this workspace");
    }
    let binding = app.store.root.join("batch-library.json");
    let expected_binding = json!({"library":library});
    if binding.exists() {
        let actual: Value = serde_json::from_slice(&fs::read(&binding)?)?;
        if actual != expected_binding {
            bail!("Research workspace is already bound to a different library");
        }
    } else {
        atomic_json(&binding, &expected_binding)?;
    }
    app.settings(Settings {
        library: library.to_string_lossy().into(),
        ..app.data.settings.clone()
    })?;
    app.configure_ai(AiSettings {
        enabled: request.enable_ai,
        model: request.model.clone(),
        endpoint: request.endpoint.clone(),
        allow_lan: false,
        timeout_seconds: 90,
    })?;
    let checked_sources = Arc::new(Mutex::new(BTreeSet::<String>::new()));
    let acquisition_log = checked_sources.clone();
    app.progress = Some(Box::new(move |raw| {
        if let Ok(progress) = serde_json::from_str::<Value>(raw) {
            if let Some(url) = progress["phase"]
                .as_str()
                .and_then(|s| s.strip_prefix("Research: fetching "))
            {
                if let Ok(mut log) = acquisition_log.lock() {
                    log.insert(url.to_string());
                }
            }
        }
        emit(json!({"event":"engine_progress","progress":
            serde_json::from_str::<Value>(raw).unwrap_or_else(|_|json!({"phase":raw}))}));
    }));
    let results = app.store.root.join("book-results");
    fs::create_dir_all(&results)?;
    let exports = app.store.root.join("exports");
    fs::create_dir_all(&exports)?;
    let export = exports.join(format!("{}.json", request.job_id));
    if export.exists() {
        bail!("Job export already exists; use a fresh job ID");
    }
    let books = app
        .scan
        .as_ref()
        .context("Library scan missing")?
        .books
        .clone();
    let wanted: BTreeSet<i64> = request.book_ids.iter().copied().collect();
    if wanted.len() != request.book_ids.len()
        || wanted.iter().any(|id| !books.iter().any(|b| b.id == *id))
    {
        bail!("Selection contains duplicate or unknown book IDs");
    }
    let pinned: BTreeSet<String> = request
        .protected_uuids
        .iter()
        .map(|s| s.to_lowercase())
        .collect();
    let mut selected: Vec<Book> = books
        .iter()
        .filter(|book| wanted.is_empty() || wanted.contains(&book.id))
        .cloned()
        .collect();
    selected.sort_by_key(|book| {
        (
            results.join(format!("book-{}.json", book.id)).exists(),
            book.id,
        )
    });
    // Protected and catalogue rows do not consume the requested research limit.
    let mut attempted = 0;
    let mut statuses: BTreeMap<String, usize> = BTreeMap::new();
    let model_digest = if request.enable_ai && request.mode == "research" {
        Ollama::new(&app.data.ai_settings)
            .and_then(|client| client.models())
            .ok()
            .and_then(|models| {
                models
                    .into_iter()
                    .find(|m| m.name == request.model && m.usable)
                    .map(|m| m.digest)
            })
    } else {
        None
    };
    if request.mode == "research" {
        let mut blocked_streak = 0;
        for book in &selected {
            storage_guard(&app.store.root)?;
            let path = results.join(format!("book-{}.json", book.id));
            let fingerprint = metadata::book_fingerprint(book)?;
            let mut saved = Saved {
                version: VERSION.into(),
                created: metadata::now(),
                book: book.clone(),
                fingerprint,
                status: "unresolved".into(),
                error: None,
                run: None,
                checked_sources: Vec::new(),
                discovered_sources: Vec::new(),
            };
            let mut cached = false;
            if metadata::catalogue(book) {
                saved.status = "catalogue_not_narrative".into();
            } else {
                if !request.refresh && path.exists() {
                    if let Ok(previous) = serde_json::from_slice::<Saved>(&fs::read(&path)?) {
                        let fresh = metadata::now().saturating_sub(previous.created) < 86400;
                        let same_model = previous.run.as_ref().is_some_and(|run| {
                            if request.enable_ai {
                                run.model == request.model
                                    && model_digest.as_ref() == Some(&run.digest)
                            } else {
                                run.model.is_empty()
                            }
                        });
                        if fresh
                            && same_model
                            && validate_saved(&previous, book, &app.store).is_ok()
                        {
                            saved = previous;
                            cached = true;
                        }
                    }
                }
                if !cached {
                    if request.limit.is_some_and(|limit| attempted >= limit) {
                        continue;
                    }
                    attempted += 1;
                    checked_sources
                        .lock()
                        .map_err(|_| anyhow::anyhow!("Acquisition log lock failed"))?
                        .clear();
                    emit(json!({"event":"researching","book_id":book.id,
                        "attempted":attempted,"selected":selected.len()}));
                    match curation::research_book(
                        &mut app,
                        book,
                        request.refresh,
                        request.source_pages,
                        request.extraction_calls,
                    ) {
                        Ok(run) => {
                            saved.status = run.status.clone();
                            saved.run = Some(run);
                            if ["ready", "partial"].contains(&saved.status.as_str()) {
                                if let Err(error) = validate_saved(&saved, book, &app.store) {
                                    saved.status = "invalid_evidence".into();
                                    saved.error = Some(error.to_string());
                                }
                            }
                        }
                        Err(error) => {
                            saved.status = "research_error".into();
                            saved.error = Some(error.to_string());
                        }
                    }
                    saved.checked_sources = checked_sources
                        .lock()
                        .map_err(|_| anyhow::anyhow!("Acquisition log lock failed"))?
                        .iter()
                        .cloned()
                        .collect();
                    if let Some(run) = &saved.run {
                        let mut leads = BTreeSet::new();
                        for discovery in &app.data.research_runs {
                            if run.queries.contains(&discovery.query) {
                                leads.extend(discovery.leads.iter().map(|l| l.url.clone()));
                            }
                        }
                        saved.discovered_sources = leads.into_iter().collect();
                    }
                }
            }
            atomic_json(&path, &saved)?;
            *statuses.entry(saved.status.clone()).or_default() += 1;
            emit(
                json!({"event":"book","book_id":book.id,"status":saved.status,
                "cached":cached,"supported_fields":saved.run.as_ref().map(|r|r.field_mask)}),
            );
            if saved.status == "blocked" {
                blocked_streak += 1;
                if blocked_streak >= 3 {
                    emit(
                        json!({"event":"paused","reason":"Three consecutive blocked results; retaining progress instead of hammering unavailable sources"}),
                    );
                    break;
                }
            } else {
                blocked_streak = 0;
            }
        }
    }
    // Revalidate against today's scan and the source snapshots retained by the engine.
    let field_names = [
        ("series", "series_index"),
        ("#series2", "#series2_index"),
        ("#series3", "#series3_index"),
        ("#series4", "#series4_index"),
    ];
    let mut mappings = Vec::new();
    let mut exceptions = Vec::new();
    let mut retries = Vec::new();
    let mut protected_verification = Vec::new();
    for book in &books {
        let path = results.join(format!("book-{}.json", book.id));
        if !path.exists() {
            continue;
        }
        let mut saved: Saved = match serde_json::from_slice(&fs::read(&path)?) {
            Ok(saved) => saved,
            Err(error) => {
                retries.push(json!({"id":book.id,"status":"stale_or_invalid",
                    "error":error.to_string()}));
                continue;
            }
        };
        if saved.status == "catalogue_not_narrative" {
            continue;
        }
        // Refresh every saved mapping against the shared universe, including books
        // not selected for new research. Changed evidence cannot leave stale ranks.
        if let Some(old) = saved.run.as_ref() {
            let corpus = app
                .store
                .cached_value::<curation::Corpus>(curation::CORPUS_KEY)?
                .unwrap_or_default();
            match curation::calculate(book, &corpus) {
                Ok(mut run) => {
                    run.queries = old.queries.clone();
                    run.model = old.model.clone();
                    run.digest = old.digest.clone();
                    saved.status = run.status.clone();
                    saved.run = Some(run);
                    saved.book = book.clone();
                    saved.fingerprint = metadata::book_fingerprint(book)?;
                    atomic_json(&path, &saved)?;
                }
                Err(error) => {
                    retries.push(
                        json!({"id":book.id,"status":"invalid_evidence","error":error.to_string()}),
                    );
                    continue;
                }
            }
        }
        if protected(book, &pinned) {
            protected_verification.push(json!({
                "id":book.id,"status":saved.status,
                "source_verified_hierarchy":saved.run.as_ref().map(|r|&r.hierarchy),
                "verified_fields":saved.run.as_ref().map(|r|r.field_mask),
                "notes":saved.run.as_ref().map(|r|&r.notes),
                "write_protected":true
            }));
            continue;
        }
        if !["ready", "partial"].contains(&saved.status.as_str()) {
            let item = json!({"id":book.id,"status":saved.status,"error":saved.error,
                "notes":saved.run.as_ref().map(|r|&r.notes)});
            if saved.status == "unverified" {
                exceptions.push(item);
            } else {
                retries.push(item);
            }
            continue;
        }
        if let Err(error) = validate_saved(&saved, book, &app.store) {
            retries.push(json!({"id":book.id,"status":"stale_or_invalid",
                "error":error.to_string()}));
            continue;
        }
        let run = saved.run.as_ref().context("Validated run missing")?;
        if run.status == "partial" {
            retries.push(json!({"id":book.id,"status":"partial","notes":run.notes}));
        }
        let mut fields = BTreeMap::new();
        for (slot, (name, index)) in field_names.iter().enumerate() {
            if run.field_mask[slot] {
                fields.insert(*name, json!(run.hierarchy[slot].name));
                fields.insert(*index, json!(run.hierarchy[slot].index));
            }
        }
        mappings.push(json!({"id":book.id,"uuid":book.uuid,"title":book.title,
            "authors":book.authors,"fields":fields,"field_mask":run.field_mask,
            "research_id":run.id,"evidence":run.evidence,"notes":run.notes,
            "order_basis":run.order_basis,"hierarchy":run.nested}));
    }
    atomic_json(
        &export,
        &json!({
            "adapter_version":VERSION,"research_version":curation::VERSION,
            "library":library,"job_id":request.job_id,"created":metadata::now(),
            "library_books":books.len(),"attempted":attempted,"statuses":statuses,
            "mappings":mappings,"exceptions":exceptions,"retries":retries,
            "protected_verification":protected_verification,"library_write_performed":false,
            "full_library_verified":false
        }),
    )?;
    emit(
        json!({"event":"complete","export":export,"attempted":attempted,
        "validated_mappings":mappings.len(),"statuses":statuses,
        "library_write_performed":false}),
    );
    Ok(export)
}

fn main() -> Result<()> {
    // Private source text, prompts and per-book results stay owner-only.
    unsafe { libc::umask(0o077) };
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 2 {
        bail!("Usage: nexus-calibre-research REQUEST.json");
    }
    let raw = fs::read(&args[1])?;
    if raw.len() > 1024 * 1024 {
        bail!("Request exceeds 1 MiB");
    }
    process(&serde_json::from_slice(&raw)?)?;
    Ok(())
}
