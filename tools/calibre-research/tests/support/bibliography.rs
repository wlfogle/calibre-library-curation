//! Synthetic source pages for verification tests, never production research.
use crate::{curation, facts, timeline};
use curator_core::{ai::Citation, chronology};

pub fn series_document(scope: &str, works: &[timeline::Work], url: &str) -> curation::Document {
    assert!(!works.is_empty());
    let mut html = format!("<h1>Complete bibliography of {scope}</h1>");
    for work in works {
        let mut text = format!(
            "{} by {} in {}.",
            work.title,
            work.authors.join(" and "),
            work.universe.as_deref().expect("Fixture universe")
        );
        for setting in &work.settings {
            text.push_str(&format!(" {} is set in {}.", work.title, setting.literal));
        }
        for member in &work.memberships {
            text.push_str(&format!(" {} belongs to {}.", work.title, member.name));
            if let Some(number) = member.published_number {
                text.push_str(&format!(
                    " {} is {} book {number}.",
                    work.title, member.name
                ));
            }
            if let Some(parent) = &member.parent {
                text.push_str(&format!(" {} is part of {parent}.", member.name));
            }
        }
        html.push_str(&format!("<p>{text}</p>"));
    }
    let page = chronology::parse_page(html.as_bytes(), url).unwrap();
    let mut proposals = works.to_vec();
    for (work, row) in proposals
        .iter_mut()
        .zip(page.source.segments.iter().filter(|s| s.kind == "p"))
    {
        let citations = vec![Citation {
            segment: row.ordinal,
            quote: row.text.clone(),
        }];
        work.citations = citations.clone();
        for setting in &mut work.settings {
            setting.citations = citations.clone();
        }
        for member in &mut work.memberships {
            member.citations = citations.clone();
        }
    }
    let inputs = facts::chunks(&page);
    let chunks = inputs
        .iter()
        .map(|input| {
            facts::screen(
                facts::Extraction {
                    facts: timeline::Facts {
                        works: proposals
                            .iter()
                            .filter(|w| {
                                w.citations
                                    .iter()
                                    .all(|c| input.iter().any(|s| s.ordinal == c.segment))
                            })
                            .cloned()
                            .collect(),
                        relations: vec![],
                    },
                    coverage: vec![],
                },
                &page,
                input,
                "synthetic-bibliography",
            )
            .unwrap()
        })
        .collect();
    curation::Document {
        page,
        processed: inputs.len(),
        chunks,
    }
}
