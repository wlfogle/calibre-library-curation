use curator_core::{
    matching::normalize,
    research::{self, ResearchRun},
};
use std::collections::BTreeSet;

/// These are acquisition candidates, never verified chronology or permission to write.
pub fn candidates(run: &ResearchRun, title: &str, author: &str) -> Vec<String> {
    let title = normalize(title);
    let author = normalize(author);
    let mut seen = BTreeSet::new();
    let mut leads = run
        .leads
        .iter()
        .filter(|lead| {
            let text = normalize(&lead.title);
            let chronology = [
                "chronolog",
                "timeline",
                "bibliograph",
                "readingorder",
                "series",
            ]
            .iter()
            .any(|term| text.contains(term));
            (!title.is_empty() && text.contains(&title))
                || (!author.is_empty() && text.contains(&author) && chronology)
        })
        .filter_map(|lead| research::safe_link(&lead.url))
        .filter(|url| url.starts_with("https://") && research::written_work_source(url))
        .filter(|url| seen.insert(url.clone()))
        .collect::<Vec<_>>();
    // Host resemblance is an acquisition hint, never proof of source authority.
    leads.sort_by_key(|url| {
        let host = url
            .trim_start_matches("https://")
            .split('/')
            .next()
            .unwrap_or("");
        let author_host = !author.is_empty() && normalize(host).contains(&author);
        let temporal = ["chronolog", "timeline"]
            .iter()
            .any(|term| url.contains(term));
        std::cmp::Reverse((author_host, temporal))
    });
    leads.truncate(4);
    leads
}

#[cfg(test)]
mod tests {
    use super::*;
    use curator_core::research::Lead;

    fn run(rows: &[(&str, &str)]) -> ResearchRun {
        ResearchRun {
            query: "example".into(),
            fetched_at: 0,
            cached: false,
            status: "complete".into(),
            notes: vec![],
            leads: rows
                .iter()
                .map(|(url, title)| Lead {
                    url: (*url).into(),
                    title: (*title).into(),
                    engine: "fixture".into(),
                })
                .collect(),
        }
    }

    #[test]
    fn irrelevant_results_are_not_discovery_success() {
        let result = run(&[("https://example.org/pawn", "Larry's Pistol and Pawn")]);
        assert!(candidates(&result, "The Ringworld Engineers", "Larry Niven").is_empty());
    }

    #[test]
    fn title_and_author_chronology_are_leads_not_required_known_urls() {
        let result = run(&[
            ("https://example.org/new-work", "The Ringworld Engineers"),
            ("https://example.org/new-timeline", "Larry Niven chronology"),
            ("https://example.org/about", "Larry Niven home page"),
        ]);
        assert_eq!(
            candidates(&result, "The Ringworld Engineers", "Larry Niven").len(),
            2
        );
    }

    #[test]
    fn unsafe_and_audio_leads_are_rejected() {
        let result = run(&[
            ("https://localhost/timeline", "Larry Niven chronology"),
            ("https://127.0.0.1/timeline", "Larry Niven chronology"),
            (
                "https://example.org:8443/timeline",
                "Larry Niven chronology",
            ),
            ("https://audible.com/book", "The Ringworld Engineers"),
            (
                "https://example.org/audiobook/engineers",
                "The Ringworld Engineers",
            ),
            ("http://example.org/book", "The Ringworld Engineers"),
        ]);
        assert!(candidates(&result, "The Ringworld Engineers", "Larry Niven").is_empty());
    }

    #[test]
    fn duplicate_leads_do_not_consume_the_four_page_budget() {
        let mut result = run(&[("https://example.org/book", "The Ringworld Engineers")]);
        result.leads = vec![result.leads[0].clone(); 30];
        assert_eq!(
            candidates(&result, "The Ringworld Engineers", "Larry Niven").len(),
            1
        );
        for n in 0..10 {
            result.leads.push(Lead {
                url: format!("https://example.org/{n}"),
                title: "The Ringworld Engineers".into(),
                engine: "fixture".into(),
            });
        }
        assert_eq!(
            candidates(&result, "The Ringworld Engineers", "Larry Niven").len(),
            4
        );
    }

    #[test]
    fn late_author_chronology_is_not_lost_to_the_page_budget() {
        let mut result = run(&[]);
        for n in 0..8 {
            result.leads.push(Lead {
                url: format!("https://example.org/{n}"),
                title: "The Ringworld Engineers".into(),
                engine: "fixture".into(),
            });
        }
        result.leads.push(Lead {
            url: "https://larryniven.example/chronology".into(),
            title: "Larry Niven chronology".into(),
            engine: "fixture".into(),
        });
        let selected = candidates(&result, "The Ringworld Engineers", "Larry Niven");
        assert_eq!(selected.len(), 4);
        assert_eq!(selected[0], "https://larryniven.example/chronology");
    }

    #[test]
    fn blocked_search_has_no_fabricated_fallback() {
        let mut result = run(&[]);
        result.status = "blocked".into();
        assert!(candidates(&result, "The Ringworld Engineers", "Larry Niven").is_empty());
    }
}
