//! Opt-in live research -> factual ordering -> validated native-library readback.
//! No test result is substituted when the public source is unavailable.

#[path = "../src/timeline.rs"]
mod timeline;

#[path = "support/discovery.rs"]
mod discovery;

use anyhow::{bail, Context, Result};
use curator_core::{
    ai::Citation, app::App, chronology, matching::normalize, model::Source, store::atomic_json,
};
use regex::Regex;
use serde_json::{json, Value};
use std::{fs, path::Path, process::Command};
use timeline::{Evidence, Facts, Membership, Setting, Work};

const CHRONOLOGY: &str = "https://larryniven.net/?q=lensmans-known-space-chronology";
const MEMBERSHIP: &str = "https://larryniven.net/?q=how-to-explore-known-space";
const TITLES: [&str; 3] = [
    "Ringworld",
    "The Ringworld Engineers",
    "The Ringworld Throne",
];
const REPO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

#[test]
#[ignore = "Live search, at most four source fetches and new private Calibre fixture writes"]
fn live_discovery_requires_verified_evidence_without_configured_url_fallback() -> Result<()> {
    let temporary = tempfile::Builder::new()
        .prefix("nexus-live-discovery-")
        .tempdir()?;
    // Retain acquisition failures too. Only verified evidence reaches a new fixture.
    let root = temporary.keep();
    let mut app = App::open(&root.join("search-cache"), Default::default())?;
    let query = format!("{} Larry Niven in-universe chronology", TITLES[1]);
    app.discover_sites(&query, false)?;
    let run = app
        .data
        .research_runs
        .first()
        .context("Missing discovery record")?
        .clone();
    let mut pending = discovery::candidates(&run, TITLES[1], "Larry Niven");
    let mut seen = std::collections::BTreeSet::new();
    let mut pages = Vec::new();
    let mut acquisition_errors = Vec::new();
    let mut verified = None;
    while !pending.is_empty() && seen.len() < 4 {
        let url = pending.remove(0);
        if !seen.insert(url.clone()) {
            continue;
        }
        match chronology::fetch_page(&url) {
            Ok(page) => {
                pending.splice(
                    0..0,
                    page.links.iter().filter(|s| !seen.contains(*s)).cloned(),
                );
                pages.push(page);
                for a in &pages {
                    for b in &pages {
                        if let Ok(evidence) = research_from_pages(a.clone(), b.clone()) {
                            let placements = timeline::solve(&evidence)?;
                            if placements.len() == TITLES.len()
                                && placements.iter().all(|p| p.global_rank.is_some())
                            {
                                verified = Some((evidence, placements));
                            }
                        }
                    }
                }
                if verified.is_some() {
                    break;
                }
            }
            Err(error) => acquisition_errors.push(format!("{url}: {error}")),
        }
    }
    // Reusing the identical query must hit the existing engine cache.
    app.discover_sites(&query, false)?;
    let replay = app
        .data
        .research_runs
        .first()
        .context("Missing cached search")?;
    assert!(replay.cached);
    assert_eq!(
        serde_json::to_value(&run.leads)?,
        serde_json::to_value(&replay.leads)?
    );
    let status = if verified.is_some() {
        "verified"
    } else {
        "automatic_retry"
    };
    atomic_json(
        &root.join("discovery.json"),
        &json!({
            "status":status, "query":query, "search":run, "fetched_urls":seen,
            "source_page_budget":4, "acquisition_errors":acquisition_errors, "pages":pages,
            "cached_search_replay_verified":true, "evidence_and_placements":verified,
            "rank_scope":"three-work acceptance fixture; NOT complete Known Space numbering",
            "configured_url_fallback_used":false, "library_write_performed":false,
            "manual_review_required":false
        }),
    )?;
    println!(
        "Discovery status {status}; evidence retained at {}",
        root.display()
    );
    let Some((evidence, _)) = verified else {
        bail!("Discovery acceptance blocked; retained for automatic retry, not manual curation");
    };
    verify_native_updates(evidence, true, seen.len())
}

fn cite(ordinal: usize, text: &str) -> Citation {
    Citation {
        segment: ordinal,
        quote: text.into(),
    }
}

fn identity(source: &Source) -> Result<Citation> {
    let segment = source
        .segments
        .iter()
        .find(|s| {
            s.kind == "title"
                && normalize(&s.text).contains("larryniven")
                && normalize(&s.text).contains("knownspace")
        })
        .context("Live source does not establish the author and universe")?;
    Ok(cite(segment.ordinal, &segment.text))
}

fn research_from_pages(
    chronological: chronology::Page,
    membership: chronology::Page,
) -> Result<Vec<Evidence>> {
    let id = identity(&chronological.source)?;
    let notes = Regex::new(r"\s*\[\s*notes\s*\]\s*$")?;
    let mut works = Vec::new();
    for title in TITLES {
        let mut candidates = Vec::new();
        for segment in &chronological.source.segments {
            if segment.kind != "tr" {
                continue;
            }
            let mut year = None;
            let mut story = None;
            for field in segment.text.split(" | ") {
                if let Some((label, value)) = field.split_once(':') {
                    match normalize(label).as_str() {
                        "year" => year = Some(value.trim()),
                        "storyorevent" => {
                            story = Some(notes.replace(value.trim(), "").into_owned())
                        }
                        _ => {}
                    }
                }
            }
            if story
                .as_ref()
                .is_some_and(|s| normalize(s) == normalize(title))
            {
                candidates.push((segment, year.context("Chronology row lacks its year")?));
            }
        }
        if candidates.len() != 1 {
            bail!(
                "Live source must uniquely identify {title}; found {}",
                candidates.len()
            );
        }
        let (row, literal) = candidates[0];
        let citations = vec![id.clone(), cite(row.ordinal, &row.text)];
        works.push(Work {
            title: title.into(),
            authors: vec!["Larry Niven".into()],
            universe: Some("Known Space".into()),
            standalone: false,
            memberships: vec![],
            settings: vec![Setting {
                calendar: "ad".into(),
                literal: literal.into(),
                role: "principal".into(),
                citations: citations.clone(),
            }],
            contains: vec![],
            citations,
        });
    }
    let member_id = identity(&membership.source)?;
    let paragraphs = membership
        .source
        .segments
        .iter()
        .filter(|segment| {
            segment.kind == "p"
                && segment.text.contains("Ringworld tales")
                && TITLES.iter().all(|title| segment.text.contains(title))
        })
        .collect::<Vec<_>>();
    if paragraphs.len() != 1 {
        bail!("Live series-membership passage is missing or ambiguous");
    }
    let paragraph = paragraphs[0];
    let citations = vec![member_id, cite(paragraph.ordinal, &paragraph.text)];
    let members = TITLES
        .iter()
        .map(|title| Work {
            title: (*title).into(),
            authors: vec!["Larry Niven".into()],
            universe: Some("Known Space".into()),
            standalone: false,
            memberships: vec![Membership {
                name: "Ringworld".into(),
                parent: None,
                published_number: None,
                citations: citations.clone(),
            }],
            settings: vec![],
            contains: vec![],
            citations: citations.clone(),
        })
        .collect();
    let bundles = vec![
        Evidence {
            source: chronological.source,
            facts: Facts {
                works,
                relations: vec![],
            },
            fetched_at: chronological.fetched_at,
        },
        Evidence {
            source: membership.source,
            facts: Facts {
                works: members,
                relations: vec![],
            },
            fetched_at: membership.fetched_at,
        },
    ];
    for bundle in &bundles {
        timeline::validate(bundle)?;
    }
    Ok(bundles)
}

#[test]
#[ignore = "Live public HTTP and native Calibre fixture writes; explicitly opt in"]
fn live_books_research_determines_fictional_placement_and_verifies_native_updates() -> Result<()> {
    let bundles = research_from_pages(
        chronology::fetch_page(CHRONOLOGY)?,
        chronology::fetch_page(MEMBERSHIP)?,
    )
    .context("Live research failed; no mocked/cached success is allowed")?;
    verify_native_updates(bundles, false, 2)
}

fn verify_native_updates(
    bundles: Vec<Evidence>,
    discovered: bool,
    source_fetches: usize,
) -> Result<()> {
    let placements = timeline::solve(&bundles)?;
    let rank = |title: &str| -> Result<f64> {
        placements
            .iter()
            .find(|w| w.title == title)
            .and_then(|w| w.global_rank)
            .context("Factual chronology was not established")
    };
    let target = placements
        .iter()
        .find(|w| w.title == TITLES[1])
        .context("Target research result missing")?;
    assert!(rank(TITLES[0])? < rank(TITLES[1])?);
    assert!(rank(TITLES[1])? < rank(TITLES[2])?);
    let date = &bundles[0]
        .facts
        .works
        .iter()
        .find(|w| w.title == TITLES[1])
        .unwrap()
        .settings[0];
    let parsed = timeline::interval(&date.calendar, &date.literal)?;
    // Independent regression assertions, not inputs to the research/order calculation.
    assert_eq!((parsed.first, parsed.last), (2878, 2881));
    let subrank = target
        .memberships
        .iter()
        .find(|(name, _)| name == "Ringworld")
        .and_then(|(_, rank)| *rank)
        .context("Independent sub-series ordering missing")?;
    assert_eq!(subrank, 2.0);

    let temporary = tempfile::Builder::new()
        .prefix("nexus-live-book-e2e-")
        .tempdir()?;
    let root = temporary.path().canonicalize()?;
    let target_library = root.join("fixture-library");
    let research_path = root.join("research.json");
    atomic_json(
        &research_path,
        &json!({
            "target":TITLES[1],"research_mode":if discovered {
                "live automatic search and followed source links"
            } else {"live configured primary references"},
            "automatic_search_discovery_tested":discovered,
            "rank_scope":"three-work acceptance fixture; NOT complete Known Space numbering",
            "sources":bundles,"placements":placements,
            "source_fetches":source_fetches,
            "mappings":placements.iter().map(|p| -> Result<Value> {
                let subrank = p.memberships.iter().find(|(name,_)|name=="Ringworld")
                    .and_then(|(_,rank)|*rank).context("Missing sub-series rank")?;
                Ok(json!({"title":p.title,"series":"Known Space Universe",
                    "series_index":p.global_rank.context("Missing fictional-time rank")?,
                    "#series2":"Known Space: Ringworld","#series2_index":subrank}))
            }).collect::<Result<Vec<_>>>()?
        }),
    )?;
    // Replay one shared universe evidence snapshot, not three per-book web requests.
    let cached: Value = serde_json::from_slice(&fs::read(&research_path)?)?;
    let cached_evidence: Vec<Evidence> = serde_json::from_value(cached["sources"].clone())?;
    assert_eq!(
        serde_json::to_value(timeline::solve(&cached_evidence)?)?,
        serde_json::to_value(&placements)?
    );
    let python = r#"
import csv,json,os,pathlib,sys,hashlib
from calibre.db.backend import DB
from calibre.db.cache import Cache
from calibre.ebooks.metadata.book.base import Metadata
root=pathlib.Path(os.environ['LIVE_E2E_ROOT']).resolve(strict=True)
assert root.name.startswith('nexus-live-book-e2e-')
lib=root/'fixture-library'
assert lib.parent==root and not lib.exists()
sys.path.insert(0,os.environ['CURATION_SCRIPTS'])
import calibre_chronology as m
real=pathlib.Path('/media/loufogle/Data/Calibre Library/metadata.db')
real_before=hashlib.sha256(real.read_bytes()).hexdigest() if real.exists() else None
lib.mkdir(mode=0o700)
cache=Cache(DB(str(lib),load_user_formatter_functions=False)); cache.init()
for label in ('series2','series3','series4'):
    cache.create_custom_column(label,label,'series',False)
cache.close()
cache=Cache(DB(str(lib),load_user_formatter_functions=False)); cache.init()
titles=['Ringworld','The Ringworld Engineers','The Ringworld Throne',
        'Protected Grand Tour Fixture','Unrelated Untouched Fixture']
ids,_=cache.add_books([(Metadata(t,['Larry Niven'] if i<3 else ['Fixture Writer']),{})
                      for i,t in enumerate(titles)],run_hooks=False)
target_id=ids[1]
changed=set(ids[:3])
cache.set_field('series',{**{i:'Incorrect Imported Universe' for i in changed},ids[3]:'The Grand Tour'})
cache.set_field('series_index',{**{i:999 for i in changed},ids[3]:8})
cache.set_field('#series2',{i:'Incorrect Imported Subseries' for i in changed})
cache.set_field('#series2_index',{i:99 for i in changed})
cache.set_field('#series3',{i:'Retained Independent Membership' for i in changed})
cache.set_field('#series3_index',{i:4.5 for i in changed})
cache.close()
before=m.snapshot(lib)['books']
guard=m.metadata_guard(lib,changed)
baseline=root/'baseline.csv'
with baseline.open('w',newline='') as stream:
    w=csv.DictWriter(stream,fieldnames=['id','uuid',*m.FIELDS]); w.writeheader()
    for book_id,book in before.items():
        w.writerow({'id':str(book_id),'uuid':book['uuid'],
                    **{f:'' if book[f] is None else str(book[f]) for f in m.FIELDS}})
research=json.loads((root/'research.json').read_text())
mapping=root/'mapping.csv'
rows=[]
for mapping_row in research['mappings']:
    book_id=ids[titles.index(mapping_row['title'])]
    rows.append({'id':str(book_id),'uuid':before[book_id]['uuid'],**mapping_row,
        'update_fields':'series;series_index;#series2;#series2_index',
        'order_verified':'yes','order_basis':'in-universe',
        'order_source':'Live source dates and independently validated series membership; '+research['rank_scope']})
with mapping.open('w',newline='') as stream:
    w=csv.DictWriter(stream,fieldnames=list(rows[0])); w.writeheader(); w.writerows(rows)
audit=m.build_report(lib,mapping,baseline)
assert not audit['errors'],audit['errors']
assert {c['id'] for c in audit['changes']}==changed
assert all(set(c['fields'])=={'series','series_index','#series2','#series2_index'} for c in audit['changes'])
# Only this newly created, private fixture is passed to the native write routine.
# Production offline/GUI locks are not weakened or overridden.
assert lib.resolve().parent==root and lib.name=='fixture-library'
expected=m.expected_books(before,audit['changes'])
m.native_write(lib,audit['changes'])
m.verify(lib,expected)
m.verify_guard(lib,changed,guard)
after=m.snapshot(lib)['books']
for position,book_id in enumerate(ids[:3],1):
    assert after[book_id]['series']=='Known Space Universe'
    assert after[book_id]['series_index']==position
    assert after[book_id]['#series2']=='Known Space: Ringworld'
    assert after[book_id]['#series2_index']==position
    assert after[book_id]['#series3']==before[book_id]['#series3']
    assert after[book_id]['#series3_index']==4.5
for book_id in ids:
    if book_id not in changed: assert after[book_id]==before[book_id]
repeat=m.build_report(lib,mapping,baseline)
assert not repeat['errors'] and not repeat['changes']
if real_before is not None: assert hashlib.sha256(real.read_bytes()).hexdigest()==real_before
with m.connect_readonly(lib/'metadata.db') as db:
    assert db.execute('PRAGMA integrity_check').fetchall()==[('ok',)]
m.private_json(root/'update-verified.json',{
    'status':'verified','target':titles[1],'target_id':target_id,
    'library':str(lib),'books':len(after),'updated_books':len(changed),
    'updated_titles':titles[:3],'source_fetches':research['source_fetches'],
    'cached_evidence_replay_verified':True,
    'researched_years':[2878,2881],'result':after[target_id],
    'protected_and_unrelated_books_unchanged':True,
    'unsupported_memberships_preserved':True,'idempotent':True,
    'original_library_unchanged':True,
    'rank_scope':research['rank_scope'],
    'automatic_search_discovery_tested':research['automatic_search_discovery_tested'],
    'production_global_writer_guard_tested':False})
print(json.dumps({'status':'verified','updated_books':len(changed),'target':titles[1],
                  'report':str(root/'update-verified.json')}))
"#;
    let result = Command::new("calibre-debug")
        .args(["-c", python])
        .env("LIVE_E2E_ROOT", &root)
        .env("CURATION_SCRIPTS", Path::new(REPO).join("scripts"))
        .env(
            "CALIBRE_CONFIG_DIRECTORY",
            root.join("isolated-calibre-config"),
        )
        .output()?;
    assert!(
        result.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&fs::read(root.join("update-verified.json"))?)?;
    assert_eq!(report["status"], "verified");
    assert_eq!(report["updated_books"], 3);
    assert_eq!(report["source_fetches"], source_fetches);
    assert_eq!(report["automatic_search_discovery_tested"], discovered);
    assert_eq!(report["cached_evidence_replay_verified"], true);
    assert_eq!(report["original_library_unchanged"], true);
    assert_eq!(report["unsupported_memberships_preserved"], true);
    assert_eq!(report["idempotent"], true);
    assert!(target_library.join("metadata.db").is_file());
    println!("{}", String::from_utf8_lossy(&result.stdout));
    let saved = temporary.keep();
    println!(
        "Live research and readback evidence retained at {}",
        saved.display()
    );
    Ok(())
}
