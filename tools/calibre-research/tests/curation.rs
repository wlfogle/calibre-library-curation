#[test]
fn universe_completeness_does_not_verify_an_independent_subseries_number() {
    let (book, mut corpus) = fixture();
    corpus.pages.remove("https://scope.example/small-arc");
    let incomplete = curation::calculate(&book, &corpus).unwrap();
    assert_eq!(incomplete.status, "research_pending");
    assert_eq!(incomplete.field_mask, [false; 4]);
    assert_eq!(incomplete.missing_scopes, vec!["Small Arc".to_string()]);
    assert!(incomplete
        .notes
        .iter()
        .any(|n| n.contains("Independent Small Arc membership coverage")));
}

#[test]
fn extracted_title_without_scoped_membership_cannot_complete_a_subseries() {
    let (book, mut corpus) = fixture();
    let document = corpus
        .pages
        .get_mut("https://scope.example/small-arc")
        .unwrap();
    for chunk in &mut document.chunks {
        for work in &mut chunk.evidence.facts.works {
            if work.title == "Later Work" {
                work.memberships.clear();
            }
        }
    }
    let result = curation::calculate(&book, &corpus).unwrap();
    assert_eq!(result.status, "research_pending");
    assert_eq!(result.field_mask, [false; 4]);
    assert_eq!(result.missing_scopes, vec!["Small Arc"]);
}

#[test]
fn same_named_series_in_another_universe_cannot_supply_completeness() {
    let (book, mut corpus) = fixture();
    let url = "https://scope.example/small-arc";
    let mut foreign_works = corpus.pages[url]
        .chunks
        .iter()
        .flat_map(|c| c.evidence.facts.works.iter().cloned())
        .collect::<Vec<_>>();
    for work in &mut foreign_works {
        work.title = format!("Unrelated {}", work.title);
        work.authors = vec!["Other Writer".into()];
        work.universe = Some("Unrelated Universe".into());
    }
    corpus.pages.insert(
        url.into(),
        bibliography::series_document("Small Arc", &foreign_works, url),
    );
    let result = curation::calculate(&book, &corpus).unwrap();
    assert_eq!(result.status, "research_pending");
    assert_eq!(result.field_mask, [false; 4]);
    assert_eq!(result.missing_scopes, vec!["Small Arc"]);
}

#[test]
fn omitted_work_in_subseries_source_cannot_borrow_complete_universe_membership() {
    let (book, mut corpus) = fixture();
    let document = corpus
        .pages
        .get_mut("https://scope.example/small-arc")
        .unwrap();
    for chunk in &mut document.chunks {
        chunk
            .evidence
            .facts
            .works
            .retain(|w| w.title != "Later Work");
    }
    let result = curation::calculate(&book, &corpus).unwrap();
    assert_eq!(result.status, "research_pending");
    assert_eq!(result.field_mask, [false; 4]);
    assert_eq!(result.missing_scopes, vec!["Small Arc"]);
}

#[test]
fn complete_subseries_scope_requires_every_source_entry_and_enables_independent_index() {
    let (book, corpus) = fixture();
    assert!(corpus.pages["https://scope.example/small-arc"]
        .chunks
        .iter()
        .flat_map(|c| &c.coverage)
        .any(|c| c.universe == "Small Arc" && c.complete));
    let ready = curation::calculate(&book, &corpus).unwrap();
    assert_eq!(ready.status, "ready");
    assert!(ready.missing_scopes.is_empty());
    assert_eq!(ready.hierarchy[1].name.as_deref(), Some("Small Arc"));
    assert_eq!(ready.hierarchy[1].index, Some(1.0));
    let mut partial = corpus.clone();
    partial.pages.values_mut().next().unwrap().chunks[0]
        .evidence
        .facts
        .works
        .retain(|w| w.title == "New Work");
    assert_eq!(
        curation::calculate(&book, &partial).unwrap().field_mask,
        [false; 4]
    );
}

#[path = "support/bibliography.rs"]
mod bibliography;
#[path = "../src/curation.rs"]
mod curation;
#[path = "../src/facts.rs"]
mod facts;
#[path = "../src/timeline.rs"]
mod timeline;

use curator_core::{
    chronology::Page,
    metadata,
    model::{Book, SeriesValue},
};

fn fixture() -> (Book, curation::Corpus) {
    let raw: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/extractor-cross-work-response.json")).unwrap();
    let page = Page {
        source: serde_json::from_value(raw["source"].clone()).unwrap(),
        fetched_at: metadata::now(),
        links: vec![],
    };
    let inputs = facts::chunks(&page);
    assert_eq!(inputs.len(), 1);
    let chunk = facts::screen(
        serde_json::from_str(raw["content"].as_str().unwrap()).unwrap(),
        &page,
        &inputs[0],
        "retained-local-output",
    )
    .unwrap();
    let book = Book {
        id: 1,
        uuid: "new-work-fixture".into(),
        title: "New Work".into(),
        authors: vec!["New Writer".into()],
        identifiers: Default::default(),
        hierarchy: Default::default(),
        publisher: None,
        has_comments: false,
        formats: vec!["EPUB".into()],
    };
    let mut corpus = curation::Corpus::default();
    corpus.pages.insert(
        page.source.location.clone(),
        curation::Document {
            page,
            processed: 1,
            chunks: vec![chunk],
        },
    );
    let works = corpus
        .pages
        .values()
        .flat_map(|d| &d.chunks)
        .flat_map(|c| c.evidence.facts.works.iter().cloned())
        .collect::<Vec<_>>();
    corpus.pages.insert(
        "https://scope.example/small-arc".into(),
        bibliography::series_document("Small Arc", &works, "https://scope.example/small-arc"),
    );
    (book, corpus)
}

#[test]
fn actual_local_output_produces_cited_chronology_from_shared_profile() {
    let (book, corpus) = fixture();
    let result = curation::calculate(&book, &corpus).unwrap();
    assert_eq!(result.status, "ready");
    assert_eq!(result.hierarchy[0].name.as_deref(), Some("New Universe"));
    assert_eq!(result.hierarchy[0].index, Some(1.0));
    assert_eq!(result.hierarchy[1].name.as_deref(), Some("Small Arc"));
    assert_eq!(result.hierarchy[1].index, Some(1.0));
    let mut next = book.clone();
    next.id = 2;
    next.uuid = "later-work".into();
    next.title = "Later Work".into();
    let later = curation::calculate(&next, &corpus).unwrap();
    assert_eq!(later.hierarchy[0].index, Some(2.0));
    assert_eq!(later.hierarchy[1].index, Some(2.0));
}

#[test]
fn complete_heading_cannot_promote_an_omitted_source_entry() {
    let (book, mut corpus) = fixture();
    corpus.pages.values_mut().next().unwrap().chunks[0]
        .evidence
        .facts
        .works
        .retain(|w| w.title == "New Work");
    let result = curation::calculate(&book, &corpus).unwrap();
    assert_eq!(result.status, "research_pending");
    assert_eq!(result.field_mask, [false; 4]);
}

#[test]
fn unfinished_source_and_stale_evidence_cannot_authorize_writes() {
    let (book, mut corpus) = fixture();
    corpus.pages.values_mut().next().unwrap().processed = 0;
    assert_eq!(
        curation::calculate(&book, &corpus).unwrap().field_mask,
        [false; 4]
    );
    let document = corpus.pages.values_mut().next().unwrap();
    document.processed = 1;
    document.page.fetched_at = 0;
    assert_eq!(
        curation::calculate(&book, &corpus).unwrap().field_mask,
        [false; 4]
    );
}

#[test]
fn old_local_research_and_changed_source_snapshots_are_rejected() {
    let (book, mut corpus) = fixture();
    corpus.pages.values_mut().next().unwrap().page.source.kind = "local-document".into();
    assert!(curation::calculate(&book, &corpus).is_err());
    let (_, mut corpus) = fixture();
    corpus
        .pages
        .values_mut()
        .next()
        .unwrap()
        .page
        .source
        .segments[1]
        .text
        .push_str(" edited");
    assert!(curation::calculate(&book, &corpus).is_err());
}

#[test]
fn unknown_existing_memberships_are_retained_and_not_marked_verified() {
    let (mut book, corpus) = fixture();
    book.hierarchy[3] = SeriesValue {
        name: Some("Unverified old arc".into()),
        index: Some(8.5),
    };
    let result = curation::calculate(&book, &corpus).unwrap();
    assert_eq!(result.status, "partial");
    assert_eq!(result.hierarchy[3], book.hierarchy[3]);
    assert!(!result.field_mask[3]);
}

#[test]
fn export_revalidation_retains_blocked_source_diagnostics_until_evidence_resolves_them() {
    let (book, complete) = fixture();
    let empty = curation::Corpus::default();
    let mut previous = curation::calculate(&book, &empty).unwrap();
    previous.status = "blocked".into();
    previous
        .notes
        .push("Source HTTP 403: https://example.org/chronology".into());
    previous.queries.push("targeted source query".into());
    let pending =
        curation::preserve_research_context(&previous, curation::calculate(&book, &empty).unwrap());
    assert_eq!(pending.status, "blocked");
    assert!(pending.notes.iter().any(|n| n.contains("HTTP 403")));
    assert_eq!(pending.queries, previous.queries);
    let resolved = curation::preserve_research_context(
        &previous,
        curation::calculate(&book, &complete).unwrap(),
    );
    assert_eq!(resolved.status, "ready");
    assert!(!resolved.notes.iter().any(|n| n.contains("HTTP 403")));
}

#[test]
fn relevant_later_passages_are_prioritized_without_discarding_other_chunks() {
    let (mut book, _) = fixture();
    book.title = "Target Book".into();
    let html = format!(
        "<h1>Target Book chronology</h1>{}<p>Target Book is set in 1350 DR.</p>",
        (0..20)
            .map(|i| format!("<p>Navigation entry {i}</p>"))
            .collect::<String>()
    );
    let page =
        curator_core::chronology::parse_page(html.as_bytes(), "https://example.org/timeline")
            .unwrap();
    let inputs = facts::chunks(&page);
    let mut document = curation::Document {
        page,
        processed: 0,
        chunks: vec![],
    };
    let order = curation::pending_chunk_indices(&document, &book).unwrap();
    assert_eq!(order.len(), inputs.len());
    assert!(order[0] > 0);
    assert!(inputs[order[0]]
        .iter()
        .any(|s| s.kind == "p" && s.text.contains("1350 DR")));
    let chunk = facts::screen(
        facts::Extraction {
            facts: timeline::Facts::default(),
            coverage: vec![],
        },
        &document.page,
        &inputs[order[0]],
        "offline",
    )
    .unwrap();
    document.chunks.push(chunk);
    document.processed = 1;
    let resumed = curation::pending_chunk_indices(&document, &book).unwrap();
    assert_eq!(resumed.len(), inputs.len() - 1);
    assert!(!resumed.contains(&order[0]));
}

#[test]
fn old_chunk_plans_are_archived_but_edited_snapshots_are_not_migrated() {
    let (book, _) = fixture();
    let page = curator_core::chronology::parse_page(
        b"<h1>New Universe</h1><p>First</p><p>Second</p><p>Third</p>",
        "https://example.org/timeline",
    )
    .unwrap();
    let old = facts::screen(
        facts::Extraction {
            facts: timeline::Facts::default(),
            coverage: vec![],
        },
        &page,
        &page.source.segments,
        "old-plan",
    )
    .unwrap();
    let url = page.source.location.clone();
    let mut corpus = curation::Corpus::default();
    corpus.pages.insert(
        url.clone(),
        curation::Document {
            page,
            processed: 1,
            chunks: vec![old],
        },
    );
    let original = corpus.clone();
    corpus
        .investigations
        .entry("fixture".into())
        .or_default()
        .completed_urls
        .insert(url.clone());
    let root = tempfile::tempdir().unwrap();
    let store = curator_core::store::Store::open(root.path()).unwrap();
    let bytes = serde_json::to_vec(&corpus.pages[&url]).unwrap();
    let key = format!(
        "archived-source-plan:{}",
        curator_core::sources::hash(&bytes)
    );
    curation::align_chunk_plans(&store, &mut corpus).unwrap();
    assert!(corpus.pages[&url].chunks.is_empty());
    assert!(!corpus.investigations["fixture"]
        .completed_urls
        .contains(&url));
    assert!(store
        .cached_value::<curation::Document>(&key)
        .unwrap()
        .is_some());
    let mut edited = original;
    edited.pages.get_mut(&url).unwrap().page.source.segments[1]
        .text
        .push_str(" edited");
    assert!(curation::align_chunk_plans(&store, &mut edited).is_err());
    assert!(curation::calculate(&book, &edited).is_err());
}
