#[path = "../src/facts.rs"]
mod facts;
#[path = "../src/timeline.rs"]
mod timeline;

use curator_core::{chronology, model::Book, sources, store::Store};
use serde_json::json;
use std::collections::BTreeMap;

fn book() -> Book {
    Book {
        id: 1,
        uuid: "fixture".into(),
        title: "New Work".into(),
        authors: vec!["New Writer".into()],
        identifiers: BTreeMap::new(),
        hierarchy: Default::default(),
        publisher: None,
        has_comments: false,
        formats: vec![],
    }
}

#[test]
fn retained_proposal_is_replayed_without_repeating_model_inference() {
    use curator_core::ai::{AiModel, AiSettings, Ollama};
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let original = chunk();
    let page = chronology::Page {
        source: original.evidence.source,
        links: vec![],
        fetched_at: 1,
    };
    let extraction = facts::Extraction {
        facts: original.evidence.facts,
        coverage: original.coverage,
    };
    let model = AiModel {
        name: "offline-fixture-not-an-installed-model".into(),
        digest: "f".repeat(64),
        size: 1_000_000,
        usable: true,
        reason: String::new(),
    };
    let client = Ollama::new(&AiSettings::default()).unwrap();
    let segment_hash = sources::hash(&serde_json::to_vec(&page.source.segments).unwrap());
    let key = format!(
        "facts:{}:diagnostic",
        sources::hash(
            &serde_json::to_vec(&json!({
                "version":"cited-temporal-facts-v4","source":page.source.sha256,
                "segments":segment_hash,"model":model.digest
            }))
            .unwrap()
        )
    );
    store
        .cache_value(
            &key,
            &json!({
                "book":book(),"source":page.source,"segments":page.source.segments,
                "response":{"done":true,"done_reason":"stop",
                    "message":{"content":serde_json::to_string(&extraction).unwrap()}}
            }),
        )
        .unwrap();
    // The named model does not exist: success requires using the retained response.
    let result = facts::extract(
        &store,
        Some((&client, &model)),
        &book(),
        &page,
        &page.source.segments,
    )
    .unwrap();
    assert_eq!(result.evidence.facts.works[0].title, "New Work");
    assert_eq!(result.extraction_version, facts::VERSION);
    let verified_key=format!("facts:{}:diagnostic",sources::hash(&serde_json::to_vec(&json!({
        "version":facts::VERSION,"source":page.source.sha256,"segments":segment_hash,
        "model":model.digest,
        "identity_hint":{"title":book().title,"authors":book().authors,"identifiers":book().identifiers}
    })).unwrap()));
    assert_eq!(
        store
            .cached_value::<serde_json::Value>(&verified_key)
            .unwrap()
            .unwrap()["proposal_replayed"],
        true
    );
}

fn chunk() -> facts::Chunk {
    let page = chronology::parse_page(
        b"<h1>Complete chronology of New Universe</h1><p>New Work by New Writer in New Universe is set in 100 AD.</p>",
        "https://example.org/chronology",
    ).unwrap();
    let extracted: facts::Extraction = serde_json::from_value(json!({
        "facts":{"works":[{
            "title":"New Work","authors":["New Writer"],"universe":"New Universe",
            "standalone":false,"memberships":[],"contains":[],
            "settings":[{"calendar":"ad","literal":"100 AD","role":"principal",
                "citations":[{"segment":2,"quote":page.source.segments[1].text}]}],
            "citations":[{"segment":2,"quote":page.source.segments[1].text}]
        }],"relations":[]},
        "coverage":[{"universe":"New Universe","complete":true,
            "citations":[{"segment":1,"quote":page.source.segments[0].text}]}]
    }))
    .unwrap();
    facts::Chunk {
        segments_hash: sources::hash(&serde_json::to_vec(&page.source.segments).unwrap()),
        evidence: timeline::Evidence {
            source: page.source,
            facts: extracted.facts,
            fetched_at: page.fetched_at,
        },
        coverage: extracted.coverage,
        extraction_version: facts::VERSION.into(),
        model_digest: "fixture".into(),
        rejected_claims: vec![],
    }
}

#[test]
fn general_facts_validate_without_a_known_author_or_universe_catalog() {
    let chunk = chunk();
    facts::validate(&chunk).unwrap();
    assert_eq!(
        timeline::solve(&[chunk.evidence]).unwrap()[0].global_rank,
        Some(1.0)
    );
    assert_eq!(facts::schema()["additionalProperties"], false);
}

#[test]
fn edited_snapshots_or_fabricated_quotes_are_rejected() {
    let mut changed = chunk();
    changed.evidence.source.segments[1].text.push_str(" edited");
    assert!(facts::validate(&changed).is_err());
    let mut changed = chunk();
    changed.evidence.facts.works[0].settings[0].citations[0].quote = "made up".into();
    assert!(facts::validate(&changed).is_err());
}

#[test]
fn partial_source_cannot_claim_complete_universe_coverage() {
    let mut changed = chunk();
    changed.evidence.source.segments[0].text = "Some New Universe books".into();
    changed.coverage[0].citations[0].quote = "Some New Universe books".into();
    changed.segments_hash =
        sources::hash(&serde_json::to_vec(&changed.evidence.source.segments).unwrap());
    assert!(facts::validate(&changed).is_err());
}

#[test]
fn chunking_preserves_every_source_segment_and_original_ordinals() {
    let html = format!(
        "<h1>New Writer bibliography</h1>{}",
        (0..37)
            .map(|n| format!("<p>Work {n} belongs to New Universe.</p>"))
            .collect::<String>()
    );
    let page = chronology::parse_page(html.as_bytes(), "https://example.org/books").unwrap();
    let chunks = facts::chunks(&page);
    assert!(chunks.len() > 1);
    for segment in &page.source.segments {
        assert!(chunks
            .iter()
            .flatten()
            .any(|s| s.ordinal == segment.ordinal && s.text == segment.text));
    }
    assert!(chunks
        .iter()
        .all(|c| c.iter().map(|s| s.text.len()).sum::<usize>() <= 12000));
}

#[test]
fn missing_model_is_operational_failure_not_success_or_manual_review() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path()).unwrap();
    let page = chronology::parse_page(b"<p>New Work by New Writer</p>", "https://example.org/book")
        .unwrap();
    let error = facts::extract(&store, None, &book(), &page, &page.source.segments).unwrap_err();
    assert!(error.to_string().contains("unavailable"));
}

#[test]
#[ignore = "Uses the installed local Ollama model; no internet or library access"]
fn installed_model_extracts_unfamiliar_temporal_facts_not_final_ranks() {
    use curator_core::ai::{AiSettings, Ollama};
    let root = tempfile::Builder::new()
        .prefix("nexus-extractor-proof-")
        .tempdir()
        .unwrap()
        .keep();
    println!("Private extraction diagnostics: {}", root.display());
    let store = Store::open(&root).unwrap();
    let page = chronology::parse_page(
        b"<h1>Complete chronology of New Universe</h1>\
        <p>New Work by New Writer in New Universe is set in 100 AD. New Work is Small Arc book 1.</p>\
        <p>Later Work by New Writer in New Universe is set in 200 AD. Later Work is Small Arc book 2.</p>",
        "https://example.org/chronology",
    ).unwrap();
    let settings = AiSettings {
        enabled: true,
        ..AiSettings::default()
    };
    let client = Ollama::new(&settings).unwrap();
    let model = client
        .models()
        .unwrap()
        .into_iter()
        .find(|m| m.name == settings.model && m.usable)
        .expect("Installed local model required");
    let extracted = facts::extract(
        &store,
        Some((&client, &model)),
        &book(),
        &page,
        &page.source.segments,
    )
    .unwrap();
    assert_eq!(extracted.evidence.facts.works.len(), 2);
    assert_eq!(extracted.coverage.len(), 1);
    assert!(extracted.coverage[0].complete);
    let placements = timeline::solve(&[extracted.evidence]).unwrap();
    assert_eq!(
        placements
            .iter()
            .find(|p| p.title == "New Work")
            .unwrap()
            .global_rank,
        Some(1.0)
    );
    assert_eq!(
        placements
            .iter()
            .find(|p| p.title == "Later Work")
            .unwrap()
            .global_rank,
        Some(2.0)
    );
    assert!(facts::fresh(page.fetched_at));
}

#[test]
fn inferred_relations_are_rejected_while_literal_dates_survive() {
    let original = chunk();
    let page = chronology::Page {
        source: original.evidence.source.clone(),
        links: vec![],
        fetched_at: original.evidence.fetched_at,
    };
    let mut extraction = facts::Extraction {
        facts: original.evidence.facts,
        coverage: original.coverage,
    };
    extraction.facts.relations.push(timeline::Relation {
        universe: "New Universe".into(),
        before: "New Work".into(),
        after: "Other Work".into(),
        basis: "story_time".into(),
        citations: extraction.facts.works[0].citations.clone(),
    });
    let screened = facts::screen(extraction, &page, &page.source.segments, "fixture").unwrap();
    assert_eq!(screened.evidence.facts.works.len(), 1);
    assert_eq!(screened.evidence.facts.works[0].settings.len(), 1);
    assert!(screened.evidence.facts.relations.is_empty());
    assert_eq!(screened.rejected_claims[0].field, "relation");
}

#[test]
fn unsupported_parent_is_not_silently_rewritten_into_verified_membership() {
    let original = chunk();
    let page = chronology::Page {
        source: original.evidence.source.clone(),
        links: vec![],
        fetched_at: original.evidence.fetched_at,
    };
    let mut extraction = facts::Extraction {
        facts: original.evidence.facts,
        coverage: original.coverage,
    };
    let citations = extraction.facts.works[0].citations.clone();
    extraction.facts.works[0]
        .memberships
        .push(timeline::Membership {
            name: "Invented Arc".into(),
            parent: Some("Invented Parent".into()),
            published_number: Some(1.0),
            citations,
        });
    let screened = facts::screen(extraction, &page, &page.source.segments, "fixture").unwrap();
    assert!(screened.evidence.facts.works[0].memberships.is_empty());
    assert_eq!(screened.rejected_claims[0].field, "membership");
}

#[test]
fn retained_installed_model_response_replays_without_inference() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/extractor-cross-work-response.json")).unwrap();
    let source = serde_json::from_value(fixture["source"].clone()).unwrap();
    let page = chronology::Page {
        source,
        links: vec![],
        fetched_at: 1,
    };
    let extraction = serde_json::from_str(fixture["content"].as_str().unwrap()).unwrap();
    let result = facts::screen(
        extraction,
        &page,
        &page.source.segments,
        "retained-local-model",
    )
    .unwrap();
    let placements = timeline::solve(&[result.evidence]).unwrap();
    assert_eq!(
        placements
            .iter()
            .find(|p| p.title == "New Work")
            .unwrap()
            .global_rank,
        Some(1.0)
    );
    assert_eq!(
        placements
            .iter()
            .find(|p| p.title == "Later Work")
            .unwrap()
            .global_rank,
        Some(2.0)
    );
    assert!(result
        .rejected_claims
        .iter()
        .any(|r| r.field == "setting" && r.subject == "New Work"));
    assert!(result.rejected_claims.iter().any(|r| r.field == "relation"));
}

#[test]
fn explicit_author_chronology_scope_binds_real_table_facts_but_forum_branding_does_not() {
    // Short factual excerpt from the retained source; no network/model call.
    for (heading, accepted) in [
        (
            "Comprehensive Chronology of R.A. Salvatore Forgotten Realms Works",
            true,
        ),
        ("RASalvatore Forums - Forgotten Realms", false),
        (
            "Chronology of R.A. Salvatore at Forgotten Realms conventions",
            false,
        ),
        (
            "Comprehensive Chronology of R.A. Salvatorem Forgotten Realms Works",
            false,
        ),
    ] {
        let html = format!(
            "<title>{heading}</title><table><tr><td>&lt; The Crystal Shard &gt;</td>\
            <td>1350 DR, SUM – 1356 DR, SPR</td></tr></table>"
        );
        let page =
            chronology::parse_page(html.as_bytes(), "https://example.org/chronology").unwrap();
        let row = page
            .source
            .segments
            .iter()
            .find(|s| s.kind == "tr")
            .unwrap();
        let citation = vec![curator_core::ai::Citation {
            segment: row.ordinal,
            quote: row.text.clone(),
        }];
        let work = timeline::Work {
            title: "The Crystal Shard".into(),
            authors: vec!["R. A. Salvatore".into()],
            universe: Some("Forgotten Realms".into()),
            standalone: true,
            memberships: vec![],
            settings: vec![timeline::Setting {
                calendar: "dr".into(),
                literal: "1350 DR, SUM – 1356 DR, SPR".into(),
                role: "principal".into(),
                citations: citation.clone(),
            }],
            contains: vec![],
            citations: citation,
        };
        let result = facts::screen(
            facts::Extraction {
                facts: timeline::Facts {
                    works: vec![work],
                    relations: vec![],
                },
                coverage: vec![],
            },
            &page,
            &page.source.segments,
            "retained-proposal-shape",
        )
        .unwrap();
        if accepted {
            assert_eq!(result.evidence.facts.works.len(), 1);
            let work = &result.evidence.facts.works[0];
            assert!(!work.standalone);
            assert_eq!(work.universe.as_deref(), Some("Forgotten Realms"));
            assert_eq!(work.settings.len(), 1);
            assert_eq!(work.settings[0].literal, "1350 DR, SUM – 1356 DR, SPR");
            assert!(result
                .rejected_claims
                .iter()
                .any(|r| r.field == "standalone"));
            assert!(result.coverage.iter().all(|c| !c.complete));
        } else {
            assert!(result.evidence.facts.works.is_empty());
            assert!(result.rejected_claims.iter().any(|r| r.field == "identity"));
        }
    }
}
