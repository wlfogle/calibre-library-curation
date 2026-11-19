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
