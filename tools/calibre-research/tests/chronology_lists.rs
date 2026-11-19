#[path = "../src/curation.rs"]
mod curation;
#[path = "../src/facts.rs"]
mod facts;
#[path = "../src/timeline.rs"]
mod timeline;

use curator_core::{ai::Citation, chronology, model::Book};

fn fixture(heading: &str, split: bool) -> (Book, curation::Corpus) {
    let separator = if split { "</ol><ol>" } else { "" };
    let html = format!(
        "<h1>{heading}</h1><ol>\
        <li>Zeta Beginning by Test Writer</li>{separator}\
        <li>Alpha Later by Test Writer</li></ol>"
    );
    let page = chronology::parse_page(html.as_bytes(), "https://example.org/timeline").unwrap();
    let mut works = Vec::new();
    for title in ["Zeta Beginning", "Alpha Later"] {
        let row = page
            .source
            .segments
            .iter()
            .find(|s| s.kind.starts_with("ordered:") && s.text.contains(title))
            .unwrap();
        let citations = vec![
            Citation {
                segment: 1,
                quote: page.source.segments[0].text.clone(),
            },
            Citation {
                segment: row.ordinal,
                quote: row.text.clone(),
            },
        ];
        works.push(timeline::Work {
            title: title.into(),
            authors: vec!["Test Writer".into()],
            universe: Some("Test World".into()),
            standalone: false,
            memberships: vec![],
            settings: vec![],
            contains: vec![],
            citations,
        });
    }
    let inputs = facts::chunks(&page);
    assert_eq!(inputs.len(), 1);
    let chunk = facts::screen(
        facts::Extraction {
            facts: timeline::Facts {
                works,
                relations: vec![],
            },
            coverage: vec![facts::Coverage {
                universe: "Test World".into(),
                complete: true,
                citations: vec![Citation {
                    segment: 1,
                    quote: page.source.segments[0].text.clone(),
                }],
            }],
        },
        &page,
        &inputs[0],
        "offline-structured-fixture",
    )
    .unwrap();
    let mut corpus = curation::Corpus::default();
    corpus.pages.insert(
        page.source.location.clone(),
        curation::Document {
            page,
            processed: 1,
            chunks: vec![chunk],
        },
    );
    (
        Book {
            id: 1,
            uuid: "structured-fixture".into(),
            title: "Zeta Beginning".into(),
            authors: vec!["Test Writer".into()],
            identifiers: Default::default(),
            hierarchy: Default::default(),
            publisher: None,
            has_comments: false,
            formats: vec![],
        },
        corpus,
    )
}

#[test]
fn actual_production_calculation_uses_chronological_list_not_alphabetical_order() {
    let (book, corpus) = fixture("Complete chronology of Test World", false);
    let run = curation::calculate(&book, &corpus).unwrap();
    assert_eq!(run.status, "ready");
    assert_eq!(run.hierarchy[0].index, Some(1.0));
    let mut later = book.clone();
    later.title = "Alpha Later".into();
    assert_eq!(
        curation::calculate(&later, &corpus).unwrap().hierarchy[0].index,
        Some(2.0)
    );
    assert!(run.evidence.iter().any(|p| p
        .chunk
        .evidence
        .facts
        .relations
        .iter()
        .any(|r| r.basis == "story_time_list")));
}

#[test]
fn publication_recommended_and_uncertain_lists_never_become_story_order() {
    for heading in [
        "Complete publication chronology of Test World",
        "Complete recommended chronological order of Test World",
        "Complete possible chronology of Test World",
    ] {
        let (book, corpus) = fixture(heading, false);
        let run = curation::calculate(&book, &corpus).unwrap();
        assert_eq!(run.status, "research_pending", "{heading}");
        assert_eq!(run.field_mask, [false; 4], "{heading}");
    }
}

#[test]
fn separate_numbered_lists_do_not_gain_an_invented_cross_list_relationship() {
    let (book, corpus) = fixture("Complete chronology of Test World", true);
    let run = curation::calculate(&book, &corpus).unwrap();
    assert_eq!(run.status, "research_pending");
    assert_eq!(run.field_mask, [false; 4]);
}

#[test]
fn reversed_or_model_asserted_list_proofs_are_rejected() {
    let (_, corpus) = fixture("Complete chronology of Test World", false);
    let document = corpus.pages.values().next().unwrap();
    let works = &document.chunks[0].evidence.facts.works;
    let relations = timeline::ordered_relationships(&document.page.source, works);
    assert_eq!(relations.len(), 1);
    let mut reversed = relations[0].clone();
    std::mem::swap(&mut reversed.before, &mut reversed.after);
    assert!(timeline::validate(&timeline::Evidence {
        source: document.page.source.clone(),
        facts: timeline::Facts {
            works: vec![],
            relations: vec![reversed]
        },
        fetched_at: document.page.fetched_at,
    })
    .is_err());
    let chunk = facts::screen(
        facts::Extraction {
            facts: timeline::Facts {
                works: works.clone(),
                relations,
            },
            coverage: vec![],
        },
        &document.page,
        &facts::chunks(&document.page)[0],
        "model-proposal",
    )
    .unwrap();
    assert!(chunk.evidence.facts.relations.is_empty());
    assert!(chunk
        .rejected_claims
        .iter()
        .any(|r| r.reason.contains("derived by code")));
}
