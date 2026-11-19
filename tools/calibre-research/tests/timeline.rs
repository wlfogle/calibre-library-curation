#[path = "../src/timeline.rs"]
mod timeline;

use curator_core::{
    ai::Citation,
    model::{Segment, Source},
    sources,
};
use timeline::{Evidence, Facts, Membership, Relation, Setting, Work};

#[test]
fn forgotten_realms_drizzt_icewind_dale_has_independent_nested_indices() {
    let member = |name: &str, parent: &str| Membership {
        name: name.into(),
        parent: Some(parent.into()),
        published_number: None,
        citations: vec![],
    };
    // Structural fixture, not a researched universe-wide book placement.
    let members = vec![
        member("Icewind Dale", "The Legend of Drizzt"),
        member("The Legend of Drizzt", "Forgotten Realms"),
    ];
    let ranks = vec![
        ("The Legend of Drizzt".into(), Some(4.0)),
        ("Icewind Dale".into(), Some(1.0)),
    ];
    let tree =
        timeline::SeriesHierarchy::from_memberships("Forgotten Realms", None, &members, &ranks)
            .unwrap();
    assert_eq!(tree.name, "Forgotten Realms");
    assert_eq!(tree.index, None);
    assert_eq!(tree.subseries.len(), 1);
    let drizzt = &tree.subseries[0];
    assert_eq!(
        (&*drizzt.name, drizzt.index),
        ("The Legend of Drizzt", Some(4.0))
    );
    assert_eq!(drizzt.subseries.len(), 1);
    let icewind = &drizzt.subseries[0];
    assert_eq!((&*icewind.name, icewind.index), ("Icewind Dale", Some(1.0)));
    assert!(icewind.subseries.is_empty());
    let encoded = serde_json::to_string(&tree).unwrap();
    let decoded: timeline::SeriesHierarchy = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, tree);
}

#[test]
fn hierarchy_supports_parallel_branches_and_deeper_nesting() {
    let mut members = Vec::new();
    let mut ranks = Vec::new();
    for (name, parent, index) in [
        ("Broad Saga", None, 7.0),
        ("Inner Arc", Some("Broad Saga"), 1.0),
        ("Smaller Arc", Some("Inner Arc"), 2.0),
        ("Parallel Saga", None, 3.0),
    ] {
        members.push(Membership {
            name: name.into(),
            parent: parent.map(String::from),
            published_number: None,
            citations: vec![],
        });
        ranks.push((name.into(), Some(index)));
    }
    let tree =
        timeline::SeriesHierarchy::from_memberships("Test World", Some(20.0), &members, &ranks)
            .unwrap();
    assert_eq!(tree.subseries.len(), 2);
    let broad = tree
        .subseries
        .iter()
        .find(|s| s.name == "Broad Saga")
        .unwrap();
    assert_eq!(broad.subseries[0].subseries[0].name, "Smaller Arc");
    assert_eq!(broad.subseries[0].subseries[0].index, Some(2.0));
}

#[test]
fn verified_eternal_champion_reference_keeps_chronology_and_each_subseries_number() {
    // Read-only reference: these are the user's verified Calibre assignments,
    // not publication positions or newly researched/overwritten metadata.
    let references = [
        (
            "The White Wolf's Son",
            99.0,
            "The Moonbeam Roads",
            3.0,
            "Oona Von Bek",
            3.0,
        ),
        (
            "The Skrayling Tree",
            98.0,
            "Elric",
            11.0,
            "The Moonbeam Roads",
            2.0,
        ),
        (
            "Dreamthief's Daughter",
            97.0,
            "Elric",
            10.0,
            "The Moonbeam Roads",
            1.0,
        ),
    ];
    let mut books = Vec::new();
    for (title, saga_index, branch, branch_index, arc, arc_index) in references {
        let members = vec![
            Membership {
                name: branch.into(),
                parent: None,
                published_number: None,
                citations: vec![],
            },
            Membership {
                name: arc.into(),
                parent: Some(branch.into()),
                published_number: None,
                citations: vec![],
            },
        ];
        let ranks = vec![
            (branch.into(), Some(branch_index)),
            (arc.into(), Some(arc_index)),
        ];
        let tree = timeline::SeriesHierarchy::from_memberships(
            "Greater Eternal Champion Saga",
            Some(saga_index),
            &members,
            &ranks,
        )
        .unwrap();
        assert_eq!(tree.index, Some(saga_index));
        assert_eq!(tree.subseries[0].index, Some(branch_index));
        assert_eq!(tree.subseries[0].subseries[0].index, Some(arc_index));
        books.push((title, tree));
    }
    books.sort_by(|a, b| a.1.index.unwrap().total_cmp(&b.1.index.unwrap()));
    assert_eq!(
        books.iter().map(|b| b.0).collect::<Vec<_>>(),
        vec![
            "Dreamthief's Daughter",
            "The Skrayling Tree",
            "The White Wolf's Son",
        ]
    );
}

#[test]
fn hierarchy_rejects_unknown_parents_cycles_and_conflicting_assignments() {
    let member = |name: &str, parent: &str| Membership {
        name: name.into(),
        parent: Some(parent.into()),
        published_number: None,
        citations: vec![],
    };
    for invalid in [
        vec![member("Inner", "Missing")],
        vec![member("A", "B"), member("B", "A")],
        vec![member("A", "Root"), member("A", "B"), member("B", "Root")],
    ] {
        assert!(timeline::SeriesHierarchy::from_memberships("Root", None, &invalid, &[]).is_err());
    }
    assert!(timeline::SeriesHierarchy::from_memberships(
        "Root",
        None,
        &[member("A", "Root")],
        &[("A".into(), Some(1.0)), ("A".into(), Some(2.0))],
    )
    .is_err());
}

fn evidence(lines: &[String], works: Vec<Work>, relations: Vec<Relation>) -> Evidence {
    Evidence {
        source: Source {
            id: "isolated-chronology-fixture".into(),
            name: "Explicit temporal evidence fixture; not a live website".into(),
            location: "https://fixture.example/chronology".into(),
            sha256: sources::hash(lines.join("\n").as_bytes()),
            kind: "test-fixture".into(),
            segments: lines
                .iter()
                .enumerate()
                .map(|(i, text)| Segment {
                    ordinal: i + 1,
                    kind: "p".into(),
                    text: text.clone(),
                })
                .collect(),
        },
        facts: Facts { works, relations },
        fetched_at: 1,
    }
}

#[test]
fn abbreviated_year_ranges_from_the_live_source_expand_without_reversing_time() {
    let first = timeline::interval("ad", "2850-51").unwrap();
    assert_eq!((first.first, first.last), (2850, 2851));
    let target = timeline::interval("ad", "2878-81").unwrap();
    assert_eq!((target.first, target.last), (2878, 2881));
    let next = timeline::interval("ad", "2882-93").unwrap();
    assert_eq!((next.first, next.last), (2882, 2893));
}

#[test]
fn correctly_bound_story_date_survives_a_publication_column() {
    let text = "Title: Some Book | Author: Test Writer | Universe: Test World | Publication date: 2000 AD | In-universe chronology: 1500 AD".to_string();
    let work = dated(
        "Some Book",
        "Test Writer",
        "Test World",
        "ad",
        "1500 AD",
        None,
        1,
        &text,
    );
    assert!(timeline::validate(&evidence(&[text], vec![work], vec![])).is_ok());
    assert_eq!(
        timeline::interval("imperial", "000.014.M31").unwrap().first,
        30014
    );
    assert_eq!(
        timeline::interval("imperial", "4.550.875.M41")
            .unwrap()
            .first,
        40875
    );
}

#[test]
fn publisher_word_ordinals_preserve_the_legacys_two_independent_labels() {
    // Synthetic fixture presentation of publisher-confirmed membership facts.
    let text = "Forgotten Realms: The Legacy by R. A. Salvatore. The Legacy is the first book in Legacy of the Drow series and the seventh book in The Legend of Drizzt series.".to_string();
    let mut work = dated(
        "The Legacy",
        "R. A. Salvatore",
        "Forgotten Realms",
        "dr",
        "",
        Some(("The Legend of Drizzt", 7.0)),
        1,
        &text,
    );
    work.memberships.push(Membership {
        name: "Legacy of the Drow".into(),
        parent: Some("The Legend of Drizzt".into()),
        published_number: Some(1.0),
        citations: citation(1, &text),
    });
    assert!(timeline::validate(&evidence(&[text], vec![work], vec![])).is_ok());
}

fn citation(line: usize, text: &str) -> Vec<Citation> {
    vec![Citation {
        segment: line,
        quote: text.into(),
    }]
}

fn dated(
    title: &str,
    author: &str,
    universe: &str,
    calendar: &str,
    date: &str,
    membership: Option<(&str, f64)>,
    line: usize,
    text: &str,
) -> Work {
    let proof = citation(line, text);
    Work {
        title: title.into(),
        authors: vec![author.into()],
        universe: Some(universe.into()),
        standalone: false,
        memberships: membership
            .map(|(name, number)| {
                vec![Membership {
                    name: name.into(),
                    parent: None,
                    published_number: Some(number),
                    citations: proof.clone(),
                }]
            })
            .unwrap_or_default(),
        settings: if date.is_empty() {
            vec![]
        } else {
            vec![Setting {
                calendar: calendar.into(),
                literal: date.into(),
                role: "principal".into(),
                citations: proof.clone(),
            }]
        },
        contains: vec![],
        citations: proof,
    }
}

#[test]
fn fictional_time_overrides_publication_and_recommended_order() {
    // Values checked against Lexicanum's cited in-universe chronology.
    // The prose below is an explicitly synthetic presentation of those values.
    let a = "Warhammer 40,000 in-universe chronology: Death or Glory by Sandy Mitchell is set in 920s.M41. Ciaphas Cain Book 4.".to_string();
    let b = "Warhammer 40,000 in-universe chronology: For the Emperor by Sandy Mitchell is set in 931.M41. Ciaphas Cain Book 1.".to_string();
    let c = "Warhammer 40,000 recommended reading order: For the Emperor before Death or Glory."
        .to_string();
    let works = vec![
        dated(
            "Death or Glory",
            "Sandy Mitchell",
            "Warhammer 40,000",
            "imperial",
            "920s.M41",
            Some(("Ciaphas Cain", 4.0)),
            1,
            &a,
        ),
        dated(
            "For the Emperor",
            "Sandy Mitchell",
            "Warhammer 40,000",
            "imperial",
            "931.M41",
            Some(("Ciaphas Cain", 1.0)),
            2,
            &b,
        ),
    ];
    let relation = Relation {
        universe: "Warhammer 40,000".into(),
        before: "For the Emperor".into(),
        after: "Death or Glory".into(),
        basis: "recommended".into(),
        citations: citation(3, &c),
    };
    let output = timeline::solve(&[evidence(&[a, b, c], works, vec![relation])]).unwrap();
    let early = output.iter().find(|w| w.title == "Death or Glory").unwrap();
    let late = output
        .iter()
        .find(|w| w.title == "For the Emperor")
        .unwrap();
    assert_eq!(early.global_rank, Some(1.0));
    assert_eq!(late.global_rank, Some(2.0));
    assert_eq!(early.memberships[0].1, Some(1.0));
    assert_eq!(late.memberships[0].1, Some(2.0));
}

#[test]
fn pre_cataclysm_years_do_not_sort_in_reverse() {
    assert_eq!(timeline::interval("krynn", "6320 PC").unwrap().first, -6320);
    assert_eq!(timeline::interval("krynn", "8500 PC").unwrap().first, -8500);
    assert_eq!(timeline::interval("dr", "-380 DR").unwrap().first, -380);
    assert!(timeline::interval("dr", "2000 AD").is_err());
}

#[test]
fn unfamiliar_books_are_ordered_without_a_known_universe_catalog() {
    let a = "Unregistered World: Zeta Beginning by Unknown Writer is set in 100 AD.".to_string();
    let b = "Unregistered World: Alpha Later by Unknown Writer is set in 200 AD.".to_string();
    let works = vec![
        dated(
            "Zeta Beginning",
            "Unknown Writer",
            "Unregistered World",
            "ad",
            "100 AD",
            None,
            1,
            &a,
        ),
        dated(
            "Alpha Later",
            "Unknown Writer",
            "Unregistered World",
            "ad",
            "200 AD",
            None,
            2,
            &b,
        ),
    ];
    let output = timeline::solve(&[evidence(&[a, b], works, vec![])]).unwrap();
    assert_eq!(
        output
            .iter()
            .find(|w| w.title == "Zeta Beginning")
            .unwrap()
            .global_rank,
        Some(1.0)
    );
}

#[test]
fn multiple_memberships_have_independent_positions() {
    // Generated chronology tests the 7/1 relationship without pretending that
    // these synthetic years are real Forgotten Realms dates.
    let mut lines = Vec::new();
    let mut works = Vec::new();
    for i in 1..=7 {
        let title = if i == 7 {
            "Dual Membership Work".into()
        } else {
            format!("Earlier Work {i}")
        };
        let text = format!(
            "Unregistered World: {title} by Test Writer is set in {i} AD. Broad Saga book {i}."
        );
        let mut work = dated(
            &title,
            "Test Writer",
            "Unregistered World",
            "ad",
            &format!("{i} AD"),
            Some(("Broad Saga", i as f64)),
            i,
            &text,
        );
        if i == 7 {
            let text = format!("{text} Inner Arc book 1 is part of Broad Saga.");
            work.citations = citation(i, &text);
            work.settings[0].citations = citation(i, &text);
            work.memberships[0].citations = citation(i, &text);
            work.memberships.push(Membership {
                name: "Inner Arc".into(),
                parent: Some("Broad Saga".into()),
                published_number: Some(1.0),
                citations: citation(i, &text),
            });
            lines.push(text);
        } else {
            lines.push(text);
        }
        works.push(work);
    }
    let output = timeline::solve(&[evidence(&lines, works, vec![])]).unwrap();
    let target = output
        .iter()
        .find(|w| w.title == "Dual Membership Work")
        .unwrap();
    assert_eq!(target.hierarchy.subseries[0].name, "Broad Saga");
    assert_eq!(target.hierarchy.subseries[0].index, Some(7.0));
    assert_eq!(target.hierarchy.subseries[0].subseries[0].name, "Inner Arc");
    assert_eq!(target.hierarchy.subseries[0].subseries[0].index, Some(1.0));
    assert_eq!(
        target
            .memberships
            .iter()
            .find(|(n, _)| n == "Broad Saga")
            .unwrap()
            .1,
        Some(7.0)
    );
    assert_eq!(
        target
            .memberships
            .iter()
            .find(|(n, _)| n == "Inner Arc")
            .unwrap()
            .1,
        Some(1.0)
    );
}

#[test]
fn overlapping_dates_do_not_gain_alphabetical_positions() {
    let a = "Test World: Alpha by Test Writer is set in 200 AD.".to_string();
    let b = "Test World: Zeta by Test Writer is set in 200 AD.".to_string();
    let works = vec![
        dated(
            "Alpha",
            "Test Writer",
            "Test World",
            "ad",
            "200 AD",
            None,
            1,
            &a,
        ),
        dated(
            "Zeta",
            "Test Writer",
            "Test World",
            "ad",
            "200 AD",
            None,
            2,
            &b,
        ),
    ];
    let output = timeline::solve(&[evidence(&[a, b], works, vec![])]).unwrap();
    assert!(output.iter().all(|w| w.global_rank.is_none()));
}

#[test]
fn a_reversed_relationship_is_not_verified_by_merely_mentioning_both_titles() {
    let text = "Test World: First is set after Second in the fictional timeline.".to_string();
    let invalid = evidence(
        &[text.clone()],
        vec![],
        vec![Relation {
            universe: "Test World".into(),
            before: "First".into(),
            after: "Second".into(),
            basis: "story_time".into(),
            citations: citation(1, &text),
        }],
    );
    assert!(timeline::validate(&invalid).is_err());
}

#[test]
fn membership_alone_does_not_establish_chronological_placement() {
    let text =
        "Test World: Only Known Book by Test Writer belongs to Some Series book 9.".to_string();
    let work = dated(
        "Only Known Book",
        "Test Writer",
        "Test World",
        "ad",
        "",
        Some(("Some Series", 9.0)),
        1,
        &text,
    );
    let output = timeline::solve(&[evidence(&[text], vec![work], vec![])]).unwrap();
    assert!(output[0].global_rank.is_none());
}

#[test]
fn edition_publication_dates_cannot_masquerade_as_fictional_dates() {
    let text = "Test World: Some Book by Test Writer was published in 2000 AD.".to_string();
    let work = dated(
        "Some Book",
        "Test Writer",
        "Test World",
        "ad",
        "2000 AD",
        None,
        1,
        &text,
    );
    assert!(timeline::validate(&evidence(&[text], vec![work], vec![])).is_err());
}

#[test]
fn publication_chronology_heading_does_not_make_a_release_year_a_story_date() {
    let text =
        "Test World publication chronology: Some Book by Test Writer was published in 2000 AD."
            .to_string();
    let work = dated(
        "Some Book",
        "Test Writer",
        "Test World",
        "ad",
        "2000 AD",
        None,
        1,
        &text,
    );
    assert!(timeline::validate(&evidence(&[text], vec![work], vec![])).is_err());
}

#[test]
fn date_must_belong_to_the_story_column_not_another_column_in_the_same_quote() {
    let text = "Title: Some Book | Author: Test Writer | Universe: Test World | Publication date: 2000 AD | In-universe chronology: 1500 AD".to_string();
    let work = dated(
        "Some Book",
        "Test Writer",
        "Test World",
        "ad",
        "2000 AD",
        None,
        1,
        &text,
    );
    assert!(timeline::validate(&evidence(&[text], vec![work], vec![])).is_err());
}

#[test]
fn unsupported_published_series_label_is_not_accepted_as_a_fact() {
    let text =
        "Test World: Some Book by Test Writer is set in 200 AD. Some Series book 2.".to_string();
    let work = dated(
        "Some Book",
        "Test Writer",
        "Test World",
        "ad",
        "200 AD",
        Some(("Some Series", 999.0)),
        1,
        &text,
    );
    assert!(timeline::validate(&evidence(&[text], vec![work], vec![])).is_err());
}

#[test]
fn invalid_imperial_dates_are_not_silently_truncated_to_three_digits() {
    for raw in ["1000.M41", "9999.M41", "931.M410"] {
        assert!(timeline::interval("imperial", raw).is_err(), "{raw}");
    }
}
