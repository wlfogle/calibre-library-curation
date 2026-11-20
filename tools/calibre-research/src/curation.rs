use crate::{
    facts::{self, Chunk},
    timeline::{self, SeriesHierarchy},
};
use anyhow::{bail, Context, Result};
use curator_core::{
    ai::{AiModel, Ollama},
    app::App,
    chronology::{self, Page},
    matching::normalize,
    model::{new_id, Book, Hierarchy, SeriesValue},
    research, sources,
    store::Store,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const VERSION: &str = "verified-universe-curation-v1";
pub const CORPUS_KEY: &str = "verified-universe-corpus-v2-fresh-web";

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Corpus {
    pub pages: BTreeMap<String, Document>,
    #[serde(default)]
    pub investigations: BTreeMap<String, Investigation>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Investigation {
    pub successful_queries: BTreeSet<String>,
    pub completed_urls: BTreeSet<String>,
}

fn bibliography_accounted_for(document: &Document, scope: &str, universe: &str) -> bool {
    let scope = normalize(scope);
    let universe = normalize(universe);
    let works = document
        .chunks
        .iter()
        .flat_map(|c| &c.evidence.facts.works)
        .collect::<Vec<_>>();
    let mut entries = 0;
    for segment in &document.page.source.segments {
        let candidate = segment.kind == "tr"
            || segment.kind.starts_with("ordered:")
            || (segment.kind == "p" && segment.text.contains(" by "));
        if !candidate {
            continue;
        }
        // Table header rows are not bibliography entries.
        if segment.kind == "tr" && !segment.text.contains(" | ") {
            continue;
        }
        if segment.kind == "tr"
            && ["title", "story or event", "year", "author"]
                .iter()
                .any(|s| segment.text.eq_ignore_ascii_case(s))
        {
            continue;
        }
        entries += 1;
        if !works.iter().any(|w| {
            w.universe
                .as_ref()
                .is_some_and(|u| normalize(u) == universe)
                && (universe == scope || w.memberships.iter().any(|m| normalize(&m.name) == scope))
                && normalize(&segment.text).contains(&normalize(&w.title))
                && w.citations.iter().any(|c| {
                    c.segment == segment.ordinal
                        && normalize(&c.quote).contains(&normalize(&w.title))
                })
        }) {
            return false;
        }
    }
    entries > 0
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Document {
    pub page: Page,
    pub processed: usize,
    pub chunks: Vec<Chunk>,
}

pub fn pending_chunk_indices(document: &Document, book: &Book) -> Result<Vec<usize>> {
    let inputs = facts::chunks(&document.page);
    let completed = document
        .chunks
        .iter()
        .map(|c| c.segments_hash.clone())
        .collect::<BTreeSet<_>>();
    let temporal = regex::Regex::new(r"(?i)\b\d+\s*(?:DR|AD|CE|AC|PC)\b|\b\d{1,3}\.M\d+\b")?;
    let title = book.title.to_lowercase();
    let author = book
        .authors
        .first()
        .map(|a| normalize(a))
        .unwrap_or_default();
    let mut pending = Vec::new();
    for (index, input) in inputs.iter().enumerate() {
        if completed.contains(&sources::hash(&serde_json::to_vec(input)?)) {
            continue;
        }
        let body = input
            .iter()
            .filter(|s| s.kind != "title" && !s.kind.starts_with('h'))
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let score = (
            !title.is_empty() && body.to_lowercase().contains(&title),
            temporal.is_match(&body),
            !author.is_empty() && normalize(&body).contains(&author),
        );
        pending.push((index, score));
    }
    pending.sort_by_key(|(index, score)| (std::cmp::Reverse(*score), *index));
    Ok(pending.into_iter().map(|(index, _)| index).collect())
}

pub fn align_chunk_plans(store: &Store, corpus: &mut Corpus) -> Result<()> {
    let mut reset = Vec::new();
    for (url, document) in &mut corpus.pages {
        for chunk in &document.chunks {
            if chunk.segments_hash
                != sources::hash(&serde_json::to_vec(&chunk.evidence.source.segments)?)
                || chunk.evidence.source.sha256 != document.page.source.sha256
                || chunk.evidence.source.location != document.page.source.location
                || chunk.evidence.source.segments.iter().any(|s| {
                    !document
                        .page
                        .source
                        .segments
                        .iter()
                        .any(|p| p.ordinal == s.ordinal && p.text == s.text)
                })
            {
                bail!("Changed source snapshot cannot be repaired as a chunk-plan migration");
            }
            if chunk.extraction_version == facts::VERSION {
                facts::validate(chunk)?;
            }
        }
        let inputs = facts::chunks(&document.page)
            .iter()
            .map(|input| serde_json::to_vec(input).map(|raw| sources::hash(&raw)))
            .collect::<std::result::Result<BTreeSet<_>, _>>()?;
        if document
            .chunks
            .iter()
            .any(|c| !inputs.contains(&c.segments_hash) || c.extraction_version != facts::VERSION)
        {
            let archived = serde_json::to_vec(document)?;
            store.cache_value(
                &format!("archived-source-plan:{}", sources::hash(&archived)),
                document,
            )?;
            document.chunks.clear();
            document.processed = 0;
            reset.push(url.clone());
        }
    }
    for investigation in corpus.investigations.values_mut() {
        for url in &reset {
            investigation.completed_urls.remove(url);
        }
    }
    Ok(())
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Proof {
    pub url: String,
    pub source_hash: String,
    pub chunk: Chunk,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub book_id: i64,
    pub book_uuid: String,
    pub status: String,
    pub notes: Vec<String>,
    pub queries: Vec<String>,
    pub evidence: Vec<Proof>,
    pub hierarchy: Hierarchy,
    pub nested: Option<SeriesHierarchy>,
    pub field_mask: [bool; 4],
    pub order_basis: String,
    pub model: String,
    pub digest: String,
    pub version: String,
    #[serde(default)]
    pub missing_scopes: Vec<String>,
}

pub fn preserve_research_context(previous: &Run, mut recalculated: Run) -> Run {
    recalculated.queries = previous.queries.clone();
    recalculated.model = previous.model.clone();
    recalculated.digest = previous.digest.clone();
    if recalculated.status == "research_pending" {
        for note in &previous.notes {
            if !recalculated.notes.contains(note) {
                recalculated.notes.push(note.clone());
            }
        }
        if previous.status == "blocked"
            || previous.status == "unverified" && recalculated.evidence.is_empty()
        {
            recalculated.status = previous.status.clone();
        }
    }
    recalculated
}

fn matches(book: &Book, work: &timeline::Work) -> bool {
    normalize(&book.title) == normalize(&work.title)
        && book
            .authors
            .iter()
            .any(|a| work.authors.iter().any(|b| normalize(a) == normalize(b)))
}

pub fn calculate(book: &Book, corpus: &Corpus) -> Result<Run> {
    let mut run = Run {
        id: new_id(),
        book_id: book.id,
        book_uuid: book.uuid.clone(),
        status: "research_pending".into(),
        notes: vec![],
        queries: vec![],
        evidence: vec![],
        hierarchy: book.hierarchy.clone(),
        nested: None,
        field_mask: [false; 4],
        order_basis: "in-universe".into(),
        model: String::new(),
        digest: String::new(),
        version: VERSION.into(),
        missing_scopes: vec![],
    };
    let mut chunks = Vec::new();
    let mut complete = BTreeSet::new();
    for document in corpus.pages.values() {
        if !facts::fresh(document.page.fetched_at) {
            continue;
        }
        if document.page.source.kind != "web-research"
            || !document.page.source.location.starts_with("https://")
        {
            bail!("Old/local research cannot be promoted as fresh online evidence");
        }
        let inputs = facts::chunks(&document.page);
        let total = inputs.len();
        let expected_hashes = inputs
            .iter()
            .map(|input| serde_json::to_vec(input).map(|raw| sources::hash(&raw)))
            .collect::<std::result::Result<BTreeSet<_>, _>>()?;
        let mut seen_hashes = BTreeSet::new();
        for chunk in &document.chunks {
            facts::validate(chunk)?;
            if chunk.evidence.source.sha256 != document.page.source.sha256
                || chunk.evidence.source.location != document.page.source.location
                || chunk.evidence.source.segments.iter().any(|s| {
                    !document
                        .page
                        .source
                        .segments
                        .iter()
                        .any(|original| original.ordinal == s.ordinal && original.text == s.text)
                })
            {
                bail!("Universe facts do not match their retained source snapshot");
            }
            if !expected_hashes.contains(&chunk.segments_hash)
                || !seen_hashes.insert(chunk.segments_hash.clone())
            {
                bail!("Extraction does not cover its recorded source chunk");
            }
            if document.processed == total && document.chunks.len() == total {
                for coverage in &chunk.coverage {
                    if coverage.complete {
                        let universes = document
                            .chunks
                            .iter()
                            .flat_map(|c| &c.evidence.facts.works)
                            .filter_map(|w| w.universe.as_deref())
                            .collect::<BTreeSet<_>>();
                        for universe in universes {
                            if bibliography_accounted_for(document, &coverage.universe, universe) {
                                complete
                                    .insert((normalize(universe), normalize(&coverage.universe)));
                            }
                        }
                    }
                }
            }
            chunks.push(chunk);
        }
    }
    let identities = chunks
        .iter()
        .flat_map(|c| &c.evidence.facts.works)
        .filter(|w| matches(book, w))
        .collect::<Vec<_>>();
    if identities.is_empty() {
        run.notes
            .push("Work identity has not yet been established by a cited source".into());
        return Ok(run);
    }
    let universes = identities
        .iter()
        .filter_map(|w| w.universe.clone())
        .collect::<BTreeSet<_>>();
    if identities.iter().any(|w| w.standalone)
        && universes.is_empty()
        && identities
            .iter()
            .all(|w| w.standalone && w.memberships.is_empty())
    {
        run.hierarchy = Default::default();
        run.field_mask = [true; 4];
        run.status = "ready".into();
        run.order_basis = "standalone".into();
        run.evidence = chunks
            .iter()
            .filter(|c| c.evidence.facts.works.iter().any(|w| matches(book, w)))
            .map(|c| proof(c))
            .collect();
        return Ok(run);
    }
    if universes.len() != 1 || identities.iter().any(|w| w.standalone) {
        run.notes
            .push("Conflicting or missing universe membership requires further research".into());
        return Ok(run);
    }
    let universe = universes.iter().next().unwrap();
    let selected = chunks
        .into_iter()
        .filter(|c| {
            c.evidence
                .facts
                .works
                .iter()
                .any(|w| w.universe.as_ref() == Some(universe))
                || c.evidence
                    .facts
                    .relations
                    .iter()
                    .any(|r| &r.universe == universe)
        })
        .collect::<Vec<_>>();
    run.evidence = selected.iter().map(|c| proof(c)).collect();
    if !complete.contains(&(normalize(universe), normalize(universe))) {
        run.missing_scopes.push(universe.clone());
        run.notes.push(format!("Complete {universe} bibliography coverage has not been verified; subset ranks cannot be exported"));
        return Ok(run);
    }
    let mut bundles = selected
        .iter()
        .map(|c| c.evidence.clone())
        .collect::<Vec<_>>();
    let works = bundles
        .iter()
        .flat_map(|b| b.facts.works.iter().cloned())
        .collect::<Vec<_>>();
    let source_keys = selected
        .iter()
        .map(|c| {
            (
                c.evidence.source.location.clone(),
                c.evidence.source.sha256.clone(),
            )
        })
        .collect::<BTreeSet<_>>();
    for document in corpus.pages.values().filter(|d| {
        source_keys.contains(&(d.page.source.location.clone(), d.page.source.sha256.clone()))
    }) {
        for relations in timeline::ordered_relationships(&document.page.source, &works).chunks(32) {
            let mut source = document.page.source.clone();
            source.segments.retain(|s| {
                relations
                    .iter()
                    .flat_map(|r| &r.citations)
                    .any(|c| c.segment == s.ordinal)
            });
            let evidence = timeline::Evidence {
                source,
                facts: timeline::Facts {
                    works: vec![],
                    relations: relations.to_vec(),
                },
                fetched_at: document.page.fetched_at,
            };
            timeline::validate(&evidence)?;
            let chunk = Chunk {
                segments_hash: sources::hash(&serde_json::to_vec(&evidence.source.segments)?),
                evidence: evidence.clone(),
                coverage: vec![],
                extraction_version: facts::VERSION.into(),
                model_digest: "deterministic-chronology-list-v1".into(),
                rejected_claims: vec![],
            };
            run.evidence.push(proof(&chunk));
            bundles.push(evidence);
        }
    }
    let placements = timeline::solve(&bundles)?;
    let keys = identities
        .iter()
        .map(|w| timeline::key(w))
        .collect::<BTreeSet<_>>();
    let target = placements
        .iter()
        .filter(|p| keys.contains(&p.work_key))
        .collect::<Vec<_>>();
    if target.len() != 1 {
        run.notes
            .push("Ambiguous work identity across editions or coauthor records".into());
        return Ok(run);
    }
    let target = target[0];
    if target.global_rank.is_none() {
        run.notes.extend(target.problems.clone());
        return Ok(run);
    }
    let mut levels = Vec::new();
    flatten(&target.hierarchy, &mut levels);
    // A closed universe does not prove which of its works belong to a sub-series.
    // Each sub-series needs its own complete source-scoped declaration.
    for level in levels.iter().skip(1) {
        if !complete.contains(&(normalize(universe), normalize(&level.name))) {
            run.missing_scopes.push(level.name.clone());
            run.notes.push(format!("Independent {} membership coverage is incomplete; its chronological number is not verified",level.name));
        }
    }
    if !run.missing_scopes.is_empty() {
        return Ok(run);
    }
    if levels.len() > 4 {
        run.status = "schema_capacity".into();
        run.notes.push("Verified memberships exceed four display fields; full hierarchy retained, no membership discarded".into());
        run.nested = Some(target.hierarchy.clone());
        return Ok(run);
    }
    for (slot, level) in levels.iter().enumerate() {
        if level.index.is_none() {
            run.notes.push(format!(
                "{} has no verified independent chronological index",
                level.name
            ));
            return Ok(run);
        }
        run.hierarchy[slot] = SeriesValue {
            name: Some(level.name.clone()),
            index: level.index,
        };
        run.field_mask[slot] = true;
    }
    run.nested = Some(target.hierarchy.clone());
    run.status = if book
        .hierarchy
        .iter()
        .enumerate()
        .any(|(i, old)| !run.field_mask[i] && old.name.is_some())
    {
        "partial"
    } else {
        "ready"
    }
    .into();
    if run.status == "partial" {
        run.notes.push("Additional stored memberships retained pending verification; not cleared or declared verified".into());
    }
    Ok(run)
}

fn flatten<'a>(node: &'a SeriesHierarchy, output: &mut Vec<&'a SeriesHierarchy>) {
    output.push(node);
    for child in &node.subseries {
        flatten(child, output);
    }
}

fn proof(chunk: &Chunk) -> Proof {
    Proof {
        url: chunk.evidence.source.location.clone(),
        source_hash: chunk.evidence.source.sha256.clone(),
        chunk: chunk.clone(),
    }
}

pub fn validate_run(run: &Run, book: &Book, store: &Store) -> Result<()> {
    if run.version != VERSION
        || run.book_id != book.id
        || run.book_uuid != book.uuid
        || !["ready", "partial"].contains(&run.status.as_str())
        || run.evidence.is_empty()
    {
        bail!("Research run is unsupported, incomplete or bound to another book");
    }
    let corpus = store
        .cached_value::<Corpus>(CORPUS_KEY)?
        .context("Universe corpus is missing")?;
    let expected = calculate(book, &corpus)?;
    if expected.hierarchy != run.hierarchy
        || expected.field_mask != run.field_mask
        || expected.order_basis != run.order_basis
        || expected.status != run.status
        || serde_json::to_value(&expected.evidence)? != serde_json::to_value(&run.evidence)?
    {
        bail!("Universe research changed; recalculate affected mappings before export");
    }
    Ok(())
}

pub fn research_book(
    app: &mut App,
    book: &Book,
    refresh: bool,
    page_budget: usize,
    extraction_budget: usize,
) -> Result<Run> {
    let mut corpus = app
        .store
        .cached_value::<Corpus>(CORPUS_KEY)?
        .unwrap_or_default();
    align_chunk_plans(&app.store, &mut corpus)?;
    app.store.cache_value(CORPUS_KEY, &corpus)?;
    if !refresh {
        let run = calculate(book, &corpus)?;
        if run.status == "ready" {
            return Ok(run);
        }
    }
    if !app.data.ai_settings.enabled {
        let mut run = calculate(book, &corpus)?;
        run.status = "blocked".into();
        run.notes
            .push("No verified cached result and local model extraction is disabled".into());
        return Ok(run);
    }
    let client = Ollama::new(&app.data.ai_settings)?;
    let model: AiModel = client
        .models()?
        .into_iter()
        .find(|m| m.name == app.data.ai_settings.model && m.usable)
        .context("Installed local extraction model is unavailable")?;
    let author = book.authors.first().context("Writing author is missing")?;
    let identity_known = corpus
        .pages
        .values()
        .flat_map(|d| &d.chunks)
        .flat_map(|c| &c.evidence.facts.works)
        .any(|w| matches(book, w) && w.universe.is_some());
    let mut cached_calls = 0;
    let mut cached_pages = 0;
    let mut cached_errors = Vec::new();
    if !refresh && !identity_known {
        for document in corpus.pages.values_mut().filter(|d| {
            facts::fresh(d.page.fetched_at)
                && d.page
                    .source
                    .segments
                    .iter()
                    .any(|s| s.text.to_lowercase().contains(&book.title.to_lowercase()))
        }) {
            if cached_pages >= page_budget || cached_calls >= extraction_budget {
                break;
            }
            let pending = pending_chunk_indices(document, book)?;
            if pending.is_empty() {
                continue;
            }
            cached_pages += 1;
            let inputs = facts::chunks(&document.page);
            for index in pending {
                if cached_calls >= extraction_budget {
                    break;
                }
                cached_calls += 1;
                match facts::extract(
                    &app.store,
                    Some((&client, &model)),
                    book,
                    &document.page,
                    &inputs[index],
                ) {
                    Ok(chunk) => {
                        document.chunks.push(chunk);
                        document.processed = document.chunks.len();
                    }
                    Err(error) => {
                        cached_errors.push(error.to_string());
                        break;
                    }
                }
            }
        }
        app.store.cache_value(CORPUS_KEY, &corpus)?;
        let mut run = calculate(book, &corpus)?;
        if run.status == "ready" || cached_calls >= extraction_budget || cached_pages >= page_budget
        {
            run.model = model.name;
            run.digest = model.digest;
            let failed = !cached_errors.is_empty();
            run.notes.extend(cached_errors);
            if failed && run.status == "research_pending" {
                run.status = "blocked".into();
            }
            if run.status == "research_pending" && !run.notes.is_empty() {
                run.notes.push(
                    "Cached-source extraction checkpoint retained; no new web search was performed"
                        .into(),
                );
            }
            return Ok(run);
        }
    }
    let page_budget = page_budget - cached_pages;
    let extraction_budget = extraction_budget - cached_calls;
    let known = corpus
        .pages
        .values()
        .flat_map(|d| &d.chunks)
        .flat_map(|c| &c.evidence.facts.works)
        .find(|w| matches(book, w))
        .and_then(|w| w.universe.clone());
    let missing = calculate(book, &corpus)?.missing_scopes;
    let target_scope = missing.first().cloned().or_else(|| known.clone());
    let investigation_key = target_scope
        .as_ref()
        .map(|u| format!("universe:{}", normalize(u)))
        .unwrap_or_else(|| format!("work:{}:{}", normalize(&book.title), normalize(author)));
    let mut investigation = corpus
        .investigations
        .get(&investigation_key)
        .cloned()
        .unwrap_or_default();
    let mut queries = match target_scope.as_ref() {
        Some(universe) => vec![
            format!("{universe} complete chronology bibliography"),
            format!("{universe} fictional timeline chronological order"),
        ],
        None => vec![
            format!(
                "\"{}\" {} in-universe chronology series",
                book.title, author
            ),
            format!(
                "\"{}\" {} universe series chronology bibliography",
                book.title, author
            ),
        ],
    };
    if investigation.successful_queries.len() >= 2 {
        queries = match target_scope.as_ref() {
            Some(universe) => vec![
                format!("{universe} author official in-universe timeline"),
                format!("{universe} specialist fictional dates chronological bibliography"),
            ],
            None => vec![
                format!("\"{}\" {} official publisher series", book.title, author),
                format!(
                    "\"{}\" {} story setting fictional chronology",
                    book.title, author
                ),
            ],
        };
    }
    let mut pending = Vec::new();
    let mut operational = !cached_errors.is_empty();
    let mut notes = cached_errors;
    for query in &queries {
        app.discover_sites(query, refresh)?;
        let search = app
            .data
            .research_runs
            .iter()
            .find(|r| &r.query == query)
            .context("Search result missing")?;
        if search.status != "complete" {
            operational = true;
            notes.extend(search.notes.clone());
        } else {
            investigation.successful_queries.insert(query.clone());
        }
        pending.extend(
            search
                .leads
                .iter()
                .filter(|l| research::written_work_source(&l.url))
                .filter_map(|l| research::safe_link(&l.url))
                .filter(|s| s.starts_with("https://")),
        );
    }
    pending.sort_by_key(|url| {
        let host = url
            .trim_start_matches("https://")
            .split('/')
            .next()
            .unwrap_or("");
        let author_site = normalize(host).contains(&normalize(author));
        let temporal = ["chronolog", "timeline"]
            .iter()
            .any(|s| url.to_lowercase().contains(s));
        std::cmp::Reverse((author_site, temporal))
    });
    // Reuse already acquired universe sources before spending new requests.
    pending.splice(
        0..0,
        corpus
            .pages
            .iter()
            .filter(|(_, d)| {
                d.chunks
                    .iter()
                    .flat_map(|c| &c.evidence.facts.works)
                    .any(|w| {
                        matches(book, w)
                            || known
                                .as_ref()
                                .is_some_and(|u| w.universe.as_ref() == Some(u))
                    })
            })
            .map(|(url, _)| url.clone())
            .collect::<Vec<_>>(),
    );
    pending.retain(|url| {
        refresh
            || !investigation.completed_urls.contains(url)
            || corpus
                .pages
                .get(url)
                .is_some_and(|d| !facts::fresh(d.page.fetched_at))
    });
    let mut seen = BTreeSet::new();
    let mut calls = 0;
    while !pending.is_empty() && seen.len() < page_budget && calls < extraction_budget {
        let url = pending.remove(0);
        if !seen.insert(url.clone()) {
            continue;
        }
        if let Some(progress) = &app.progress {
            progress(&serde_json::json!({"phase":format!("Research: fetching {url}")}).to_string());
        }
        let page = match corpus
            .pages
            .get(&url)
            .filter(|d| !refresh && facts::fresh(d.page.fetched_at))
        {
            Some(document) => document.page.clone(),
            None => match chronology::fetch_page(&url) {
                Ok(page) => page,
                Err(error) => {
                    operational = true;
                    notes.push(format!("{url}: {error}"));
                    continue;
                }
            },
        };
        pending.extend(page.links.iter().filter(|u| !seen.contains(*u)).cloned());
        let source_chunks = facts::chunks(&page);
        let document = corpus.pages.entry(url.clone()).or_insert_with(|| Document {
            page: page.clone(),
            processed: 0,
            chunks: vec![],
        });
        if document.page.source.sha256 != page.source.sha256 {
            *document = Document {
                page: page.clone(),
                processed: 0,
                chunks: vec![],
            };
        } else {
            document.page = page.clone();
        }
        for input_index in pending_chunk_indices(document, book)? {
            if calls >= extraction_budget {
                break;
            }
            calls += 1;
            match facts::extract(
                &app.store,
                Some((&client, &model)),
                book,
                &page,
                &source_chunks[input_index],
            ) {
                Ok(chunk) => {
                    document.chunks.push(chunk);
                    document.processed = document.chunks.len();
                }
                Err(error) => {
                    operational = true;
                    notes.push(error.to_string());
                    break;
                }
            }
        }
        if document.processed == source_chunks.len() {
            investigation.completed_urls.insert(url);
        }
        corpus
            .investigations
            .insert(investigation_key.clone(), investigation.clone());
        app.store.cache_value(CORPUS_KEY, &corpus)?;
        let run = calculate(book, &corpus)?;
        if run.status == "ready" {
            break;
        }
    }
    let mut run = calculate(book, &corpus)?;
    let rejected = corpus
        .pages
        .values()
        .flat_map(|d| &d.chunks)
        .flat_map(|c| &c.rejected_claims)
        .any(|r| normalize(&r.subject) == normalize(&book.title));
    if !operational
        && !rejected
        && pending.is_empty()
        && investigation.successful_queries.len() >= 4
        && run.status == "research_pending"
        && run
            .notes
            .iter()
            .any(|n| n.contains("Work identity") || n.contains("universe membership"))
    {
        run.status = "unverified".into();
        run.notes.push("Identity/universe could not be established after successful alternative online research; factual review required".into());
    }
    corpus
        .investigations
        .insert(investigation_key, investigation);
    app.store.cache_value(CORPUS_KEY, &corpus)?;
    run.queries = queries;
    run.model = model.name;
    run.digest = model.digest;
    run.notes.extend(notes);
    if operational && run.status == "research_pending" {
        run.status = "blocked".into();
    }
    if run.status == "research_pending" {
        run.notes.push("Research checkpoint retained for automatic continuation; not a manual-curation exception".into());
    }
    Ok(run)
}
