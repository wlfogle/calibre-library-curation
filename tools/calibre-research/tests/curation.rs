#[test]
fn universe_completeness_does_not_verify_an_independent_subseries_number() {
    let (mut book, mut corpus) = fixture();
    book.hierarchy[1] = SeriesValue {
        name: Some("Small Arc".into()),
        index: Some(37.0),
    };
    corpus.pages.remove("https://scope.example/small-arc");
    let incomplete = curation::calculate(&book, &corpus).unwrap();
    assert_eq!(incomplete.status, "partial");
    assert_eq!(incomplete.field_mask, [true, false, false, false]);
    assert_eq!(incomplete.hierarchy[1], book.hierarchy[1]);
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
    assert_eq!(result.status, "partial");
    assert_eq!(result.field_mask, [true, false, false, false]);
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
    assert_eq!(result.status, "partial");
    assert_eq!(result.field_mask, [true, false, false, false]);
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
    assert_eq!(result.status, "partial");
    assert_eq!(result.field_mask, [true, false, false, false]);
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

fn unfinished_scope_fixture() -> (Book, curation::Corpus, Vec<facts::Chunk>) {
    let (book, mut corpus) = fixture();
    let url = "https://scope.example/small-arc";
    let mut works = corpus.pages[url]
        .chunks
        .iter()
        .flat_map(|c| c.evidence.facts.works.iter().cloned())
        .collect::<Vec<_>>();
    for index in 2..5 {
        let mut work = works[1].clone();
        work.title = format!("Unowned Work {index}");
        work.settings[0].literal = format!("{} AD", (index + 1) * 100);
        work.memberships[0].published_number = Some((index + 1) as f64);
        works.push(work);
    }
    let mut document = bibliography::series_document("Small Arc", &works, url);
    let retained = document.chunks.clone();
    assert_eq!(retained.len(), 3);
    document.chunks.truncate(1);
    document.processed = 1;
    corpus.pages.insert(url.into(), document);
    (book, corpus, retained)
}

#[test]
fn known_identity_resumes_cached_membership_chunks_within_each_call_budget() {
    let (book, mut corpus, retained) = unfinished_scope_fixture();
    assert_eq!(
        curation::calculate(&book, &corpus).unwrap().missing_scopes,
        vec!["Small Arc"]
    );
    for expected_remaining in [1, 0] {
        let progress = curation::resume_cached_sources(&book, &mut corpus, 1, 1, |page, input| {
            let hash = curator_core::sources::hash(&serde_json::to_vec(input).unwrap());
            let chunk = retained.iter().find(|c| c.segments_hash == hash).unwrap();
            assert_eq!(chunk.evidence.source.sha256, page.source.sha256);
            Ok(chunk.clone())
        })
        .unwrap();
        assert_eq!(progress.pages, 1);
        assert_eq!(progress.calls, 1);
        assert!(progress.errors.is_empty());
        assert_eq!(
            curation::pending_chunk_indices(
                &corpus.pages["https://scope.example/small-arc"],
                &book
            )
            .unwrap()
            .len(),
            expected_remaining
        );
        let result = curation::calculate(&book, &corpus).unwrap();
        if expected_remaining > 0 {
            assert_eq!(result.status, "partial");
            assert_eq!(result.field_mask, [true, false, false, false]);
        } else {
            assert_eq!(result.status, "ready");
            assert_eq!(result.hierarchy[1].index, Some(1.0));
        }
    }
    let progress = curation::resume_cached_sources(&book, &mut corpus, 1, 1, |_, _| {
        panic!("Completed source chunks must never be extracted again")
    })
    .unwrap();
    assert_eq!(progress.calls, 0);
}

#[test]
fn cached_scope_failures_keep_the_unprocessed_chunk_retryable() {
    let (book, mut corpus, _) = unfinished_scope_fixture();
    let page = curator_core::chronology::parse_page(
        b"<h1>Complete bibliography of New Universe</h1><p>Another Work by New Writer in New Universe.</p>",
        "https://z.example/other-cached-page",
    ).unwrap();
    corpus.pages.insert(
        page.source.location.clone(),
        curation::Document {
            page,
            processed: 0,
            chunks: vec![],
        },
    );
    let before = serde_json::to_value(&corpus).unwrap();
    let progress = curation::resume_cached_sources(&book, &mut corpus, 2, 2, |_, _| {
        anyhow::bail!("Retained extraction failure")
    })
    .unwrap();
    assert_eq!(progress.calls, 1);
    assert_eq!(progress.errors, vec!["Retained extraction failure"]);
    assert_eq!(serde_json::to_value(&corpus).unwrap(), before);
    assert_eq!(
        curation::cached_document_urls(&book, &corpus)
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn zero_cache_budgets_never_start_an_extraction() {
    for (pages, calls) in [(0, 2), (2, 0)] {
        let (book, mut corpus, _) = unfinished_scope_fixture();
        let progress = curation::resume_cached_sources(&book, &mut corpus, pages, calls, |_, _| {
            panic!("A zero budget must prevent extraction")
        })
        .unwrap();
        assert_eq!(progress.pages, 0);
        assert_eq!(progress.calls, 0);
    }
}

#[test]
fn cached_scope_priority_excludes_stale_unrelated_and_completed_pages() {
    let (book, mut corpus, _) = unfinished_scope_fixture();
    for (url, html) in [
        ("https://a.example/universe", "<h1>Complete bibliography of New Universe</h1><p>Another Work by New Writer in New Universe.</p>"),
        ("https://a.example/unrelated", "<h1>Complete bibliography of Other Universe</h1><p>Other Work by Other Writer in Other Universe.</p>"),
    ] {
        let page = curator_core::chronology::parse_page(html.as_bytes(), url).unwrap();
        corpus.pages.insert(url.into(), curation::Document { page, processed: 0, chunks: vec![] });
    }
    assert_eq!(
        curation::cached_document_urls(&book, &corpus).unwrap(),
        vec![
            "https://scope.example/small-arc",
            "https://a.example/universe"
        ]
    );
    corpus
        .pages
        .get_mut("https://scope.example/small-arc")
        .unwrap()
        .page
        .fetched_at = 0;
    assert_eq!(
        curation::cached_document_urls(&book, &corpus).unwrap(),
        vec!["https://a.example/universe"]
    );
    let (book, complete) = fixture();
    assert!(curation::cached_document_urls(&book, &complete)
        .unwrap()
        .is_empty());
}

#[test]
fn scoped_investigations_are_namespaced_by_the_verified_universe() {
    let (book, _) = fixture();
    let first = curation::investigation_key(&book, Some("New Universe"), Some("Small Arc"));
    let other = curation::investigation_key(&book, Some("Other Universe"), Some("Small Arc"));
    assert_eq!(first, "scope:newuniverse:smallarc");
    assert_ne!(first, other);
    assert_ne!(
        first,
        curation::investigation_key(&book, Some("New Universe"), None)
    );
    assert_eq!(
        curation::investigation_key(&book, Some("New Universe"), Some("New Universe")),
        "universe:newuniverse"
    );
    assert_eq!(
        curation::investigation_key(&book, None, None),
        "work:newwork:newwriter"
    );
}

#[test]
fn linked_bibliography_frontier_survives_a_saved_corpus_checkpoint() {
    use curator_core::store::Store;
    let (book, mut corpus) = fixture();
    let guide = "https://scope.example/small-arc";
    let root_url = "https://example.org/chronology";
    let mut scope = corpus
        .pages
        .remove("https://scope.example/small-arc")
        .unwrap();
    corpus.pages.get_mut(root_url).unwrap().page.links = vec![guide.into()];
    let key = curation::investigation_key(&book, Some("New Universe"), Some("Small Arc"));
    let mut investigation = curation::Investigation::default();
    investigation.pending_urls =
        curation::pending_source_urls(&book, &corpus, &investigation, &[], false).unwrap();
    assert_eq!(investigation.pending_urls, vec![guide]);
    corpus.investigations.insert(key.clone(), investigation);
    let temp = tempfile::tempdir().unwrap();
    {
        let store = Store::open(temp.path()).unwrap();
        store.cache_value(curation::CORPUS_KEY, &corpus).unwrap();
    }
    let store = Store::open(temp.path()).unwrap();
    let mut resumed: curation::Corpus = store.cached_value(curation::CORPUS_KEY).unwrap().unwrap();
    // Even if the parent link is no longer in memory, the queued target survives.
    resumed.pages.get_mut(root_url).unwrap().page.links.clear();
    let queued =
        curation::pending_source_urls(&book, &resumed, &resumed.investigations[&key], &[], false)
            .unwrap();
    assert_eq!(queued, vec![guide]);
    assert_eq!(
        curation::calculate(&book, &resumed).unwrap().status,
        "partial"
    );
    // Replay the already-screened synthetic bibliography at its original URL.
    let scope_url = scope.page.source.location.clone();
    scope.page.links.clear();
    resumed.pages.insert(scope_url, scope);
    assert!(curation::pending_source_urls(
        &book,
        &resumed,
        &resumed.investigations[&key],
        &[],
        false,
    )
    .unwrap()
    .is_empty());
    assert_eq!(
        curation::calculate(&book, &resumed).unwrap().status,
        "ready"
    );
}

#[test]
fn completed_cached_sources_forward_links_once_without_cycle_or_page_slots() {
    let (book, mut corpus) = fixture();
    let root = "https://example.org/chronology";
    let arc = "https://scope.example/small-arc";
    let next = "https://guide.example/series";
    corpus.pages.get_mut(root).unwrap().page.links = vec![arc.into(), next.into()];
    corpus.pages.get_mut(arc).unwrap().page.links = vec![root.into(), format!("{next}#books")];
    let pending =
        curation::source_frontier(&book, &corpus, &[root.into(), arc.into()], false).unwrap();
    assert_eq!(pending, vec![next]);
}

#[test]
fn stale_refreshed_or_unfinished_sources_are_not_skipped_as_completed() {
    let (book, mut corpus) = fixture();
    let root = "https://example.org/chronology";
    let urls = vec![root.into()];
    assert!(curation::source_frontier(&book, &corpus, &urls, false)
        .unwrap()
        .is_empty());
    assert_eq!(
        curation::source_frontier(&book, &corpus, &urls, true).unwrap(),
        urls
    );
    corpus.pages.get_mut(root).unwrap().page.fetched_at = 0;
    assert_eq!(
        curation::source_frontier(&book, &corpus, &urls, false).unwrap(),
        urls
    );
    corpus.pages.get_mut(root).unwrap().page.fetched_at = metadata::now();
    corpus.pages.get_mut(root).unwrap().chunks.clear();
    assert_eq!(
        curation::source_frontier(&book, &corpus, &urls, false).unwrap(),
        urls
    );
}

#[test]
fn retained_frontier_revalidates_links_and_deduplicates_fragments() {
    let (book, corpus) = fixture();
    let urls = [
        "http://guide.example/series",
        "https://127.0.0.1/series",
        "https://localhost/series",
        "https://user@guide.example/series",
        "file:///tmp/bibliography",
        "https://audible.com/series",
        "https://guide.example/audiobook/series",
        "https://guide.example/series#one",
        "https://guide.example/series#two",
    ]
    .into_iter()
    .map(String::from)
    .collect::<Vec<_>>();
    assert_eq!(
        curation::source_frontier(&book, &corpus, &urls, false).unwrap(),
        vec!["https://guide.example/series"]
    );
}

#[test]
fn legacy_investigations_default_to_an_empty_frontier_and_recover_cached_links() {
    let (book, mut corpus) = fixture();
    let root = "https://example.org/chronology";
    let guide = "https://guide.example/bibliography";
    let investigation: curation::Investigation = serde_json::from_value(serde_json::json!({
        "successful_queries": ["old successful query"],
        "completed_urls": [root]
    }))
    .unwrap();
    assert!(investigation.pending_urls.is_empty());
    corpus.pages.get_mut(root).unwrap().page.links = vec![guide.into()];
    assert_eq!(
        curation::pending_source_urls(&book, &corpus, &investigation, &[], false).unwrap(),
        vec![guide]
    );
}

#[test]
fn a_completed_url_marker_never_hides_a_missing_snapshot() {
    let (book, corpus) = fixture();
    let missing = "https://guide.example/missing";
    let mut investigation = curation::Investigation::default();
    investigation.completed_urls.insert(missing.into());
    investigation.pending_urls.push(missing.into());
    assert_eq!(
        curation::pending_source_urls(&book, &corpus, &investigation, &[], false).unwrap(),
        vec![missing]
    );
}

#[test]
fn deferred_sources_preserve_unvisited_priority_across_serialized_retries() {
    let (book, corpus) = fixture();
    let mut investigation = curation::Investigation {
        pending_urls: vec![
            "https://guide.example/unreachable".into(),
            "https://guide.example/unvisited".into(),
        ],
        ..Default::default()
    };
    investigation.defer_source("https://guide.example/unreachable");
    let restored: curation::Investigation =
        serde_json::from_value(serde_json::to_value(&investigation).unwrap()).unwrap();
    assert_eq!(
        curation::pending_source_urls(&book, &corpus, &restored, &[], false).unwrap(),
        vec![
            "https://guide.example/unvisited",
            "https://guide.example/unreachable"
        ]
    );
}

#[test]
fn failed_fetch_discovery_flag_survives_without_discarding_pending_sources() {
    let mut investigation = curation::Investigation {
        pending_urls: vec!["https://guide.example/unreachable".into()],
        ..Default::default()
    };
    assert!(!investigation.discovery_needed(&investigation.pending_urls, false));
    assert!(investigation.discovery_needed(&[], false));
    assert!(investigation.discovery_needed(&investigation.pending_urls, true));
    investigation.discovery_due = true;
    let restored: curation::Investigation =
        serde_json::from_value(serde_json::to_value(&investigation).unwrap()).unwrap();
    assert!(restored.discovery_needed(&restored.pending_urls, false));
    assert_eq!(restored.pending_urls, investigation.pending_urls);
}

#[test]
fn unrelated_common_title_pages_never_reach_cached_extraction() {
    let (mut book, _) = fixture();
    book.title = "Sentinel".into();
    book.authors = vec!["Natalie Grey".into()];
    book.hierarchy[0] = SeriesValue {
        name: Some("Transformers".into()),
        index: Some(126.0),
    };
    let page = curator_core::chronology::parse_page(
        b"<h1>Sentinel Prime</h1><p>Sentinel Prime is a character in Transformers.</p>",
        "https://example.org/sentinel-prime",
    )
    .unwrap();
    assert!(!curation::relevant_page(&book, &page, &Default::default()));
    let url = page.source.location.clone();
    let mut corpus = curation::Corpus::default();
    corpus.pages.insert(
        url.clone(),
        curation::Document {
            page,
            processed: 0,
            chunks: vec![],
        },
    );
    assert!(curation::source_frontier(&book, &corpus, &[url], false)
        .unwrap()
        .is_empty());
    let progress = curation::resume_cached_sources(&book, &mut corpus, 2, 4, |_, _| {
        panic!("An unrelated source must not consume model calls")
    })
    .unwrap();
    assert_eq!(progress.calls, 0);
    assert_eq!(
        curation::calculate(&book, &corpus).unwrap().field_mask,
        [false; 4]
    );
}

#[test]
fn target_identity_or_verified_universe_makes_a_page_a_research_candidate() {
    let (book, _) = fixture();
    for (html, accepted) in [
        ("<p>New Work by Other Writer.</p>", false),
        ("<p>New Work by New Writer.</p>", true),
    ] {
        let page =
            curator_core::chronology::parse_page(html.as_bytes(), "https://example.org/book")
                .unwrap();
        assert_eq!(
            curation::relevant_page(&book, &page, &Default::default()),
            accepted
        );
    }
    let page = curator_core::chronology::parse_page(
        b"<h1>Complete bibliography of New Universe</h1><p>Unowned works follow.</p>",
        "https://example.org/universe",
    )
    .unwrap();
    assert!(!curation::relevant_page(&book, &page, &Default::default()));
    assert!(curation::relevant_page(
        &book,
        &page,
        &["New Universe".into()].into_iter().collect()
    ));
}

#[test]
fn stored_series_can_disambiguate_a_query_but_cannot_verify_a_mapping() {
    let (mut book, _) = fixture();
    book.hierarchy[0] = SeriesValue {
        name: Some("Old Universe".into()),
        index: Some(126.0),
    };
    book.hierarchy[1] = SeriesValue {
        name: Some("Candidate Arc".into()),
        index: Some(27.0),
    };
    assert_eq!(
        curation::identity_query_hint(&book),
        "\"New Work\" New Writer Candidate Arc"
    );
    let result = curation::calculate(&book, &curation::Corpus::default()).unwrap();
    assert_eq!(result.status, "research_pending");
    assert_eq!(result.field_mask, [false; 4]);
    assert_eq!(result.hierarchy, book.hierarchy);
}

#[test]
fn complete_local_scope_preserves_an_unverified_main_index() {
    let (mut book, mut corpus) = fixture();
    book.hierarchy[0] = SeriesValue {
        name: Some("New Universe".into()),
        index: Some(999.0),
    };
    corpus.pages.remove("https://example.org/chronology");
    let result = curation::calculate(&book, &corpus).unwrap();
    assert_eq!(result.status, "partial");
    assert_eq!(result.field_mask, [false, true, false, false]);
    assert_eq!(result.hierarchy[0], book.hierarchy[0]);
    assert_eq!(result.hierarchy[1].name.as_deref(), Some("Small Arc"));
    assert_eq!(result.hierarchy[1].index, Some(1.0));
    assert_eq!(result.missing_scopes, vec!["New Universe"]);
    let nested = result.nested.unwrap();
    assert_eq!(nested.index, None);
    assert_eq!(nested.subseries[0].index, Some(1.0));
}

#[test]
fn local_index_is_not_applied_under_a_missing_or_different_parent_context() {
    for old_name in [None, Some("Unrelated Universe")] {
        let (mut book, mut corpus) = fixture();
        book.hierarchy[0] = SeriesValue {
            name: old_name.map(String::from),
            index: Some(77.0),
        };
        corpus.pages.remove("https://example.org/chronology");
        let result = curation::calculate(&book, &corpus).unwrap();
        assert_eq!(result.status, "research_pending");
        assert_eq!(result.field_mask, [false; 4]);
        assert_eq!(result.hierarchy, book.hierarchy);
        assert_eq!(result.nested.unwrap().subseries[0].index, Some(1.0));
    }
}

#[test]
fn ambiguous_global_order_does_not_discard_a_verified_local_rank() {
    let (mut book, mut corpus) = fixture();
    book.hierarchy[0] = SeriesValue {
        name: Some("New Universe".into()),
        index: Some(77.0),
    };
    let url = "https://example.org/chronology";
    let mut works = corpus.pages["https://scope.example/small-arc"]
        .chunks
        .iter()
        .flat_map(|c| c.evidence.facts.works.iter().cloned())
        .collect::<Vec<_>>();
    let mut parallel = works[0].clone();
    parallel.title = "Unrelated Parallel Work".into();
    parallel.memberships.clear();
    works.push(parallel);
    corpus.pages.insert(
        url.into(),
        bibliography::series_document("New Universe", &works, url),
    );
    let result = curation::calculate(&book, &corpus).unwrap();
    assert_eq!(result.status, "partial");
    assert_eq!(result.field_mask, [false, true, false, false]);
    assert_eq!(result.hierarchy[0], book.hierarchy[0]);
    assert_eq!(result.hierarchy[1].index, Some(1.0));
    assert!(result.missing_scopes.is_empty());
    assert!(result
        .notes
        .iter()
        .any(|n| n.contains("No unique in-universe order")));
}

#[test]
fn partial_hierarchy_revalidation_rejects_an_invented_global_index() {
    let (mut book, mut corpus) = fixture();
    book.hierarchy[0] = SeriesValue {
        name: Some("New Universe".into()),
        index: Some(77.0),
    };
    corpus.pages.remove("https://example.org/chronology");
    let root = tempfile::tempdir().unwrap();
    let store = curator_core::store::Store::open(root.path()).unwrap();
    store.cache_value(curation::CORPUS_KEY, &corpus).unwrap();
    let mut run = curation::calculate(&book, &corpus).unwrap();
    assert_eq!(run.field_mask, [false, true, false, false]);
    curation::validate_run(&run, &book, &store).unwrap();
    run.nested.as_mut().unwrap().index = Some(77.0);
    assert!(curation::validate_run(&run, &book, &store).is_err());
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
