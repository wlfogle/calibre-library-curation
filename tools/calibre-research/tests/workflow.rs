#[path = "../src/curation.rs"]
mod curation;
#[path = "../src/facts.rs"]
mod facts;
#[path = "../src/timeline.rs"]
mod timeline;

use curator_core::{app::App, chronology, model::Settings, store::atomic_json};
use serde_json::{json, Value};
use std::{fs, path::Path, process::Command};

const REPO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn command_ok(command: &mut Command) -> std::process::Output {
    let output = command.output().expect("Run fixture command");
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn research_resume_revalidate_exceptions_and_apply_without_live_library_writes() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let library = root.join("test-library");
    let workspace = root.join("workflow");
    let engine_workspace = workspace.join("engine");
    let script = r#"
import csv,os,sqlite3,json,pathlib
from calibre.db.backend import DB
from calibre.db.cache import Cache
from calibre.ebooks.metadata.book.base import Metadata
root=pathlib.Path(os.environ['FIXTURE_ROOT'])
lib=root/'test-library'; lib.mkdir()
cache=Cache(DB(str(lib),load_user_formatter_functions=False)); cache.init()
for label in ('series2','series3','series4'):
    cache.create_custom_column(label,label,'series',False)
cache.close()
cache=Cache(DB(str(lib),load_user_formatter_functions=False)); cache.init()
ids,_=cache.add_books([(Metadata(t,['Test Author']),{}) for t in
    ('Unplaced One','Unplaced Two','Protected Book')],run_hooks=False)
cache.set_field('#series2',{ids[0]:'Keep Existing Branch'})
cache.set_field('#series2_index',{ids[0]:7.5})
cache.set_field('series',{ids[2]:'Greater Foundation Universe'})
cache.set_field('series_index',{ids[2]:9})
fields=['series','series_index','#series2','#series2_index','#series3','#series3_index','#series4','#series4_index']
with (root/'baseline.csv').open('w',newline='') as stream:
    writer=csv.DictWriter(stream,fieldnames=['id','uuid',*fields]); writer.writeheader()
    for book_id in ids:
        row={'id':str(book_id),'uuid':cache.field_for('uuid',book_id)}
        for field in fields:
            val=cache.field_for(field,book_id)
            row[field]='' if val is None else str(val)
        writer.writerow(row)
cache.close()
source=root/'source-library'; source.mkdir()
a=sqlite3.connect((lib/'metadata.db').as_uri()+'?mode=ro',uri=True)
b=sqlite3.connect(source/'metadata.db'); a.backup(b); b.close(); a.close()
(root/'clone-verified.json').write_text(json.dumps({
    'status':'verified','source':str(source),'test_library':str(lib)}))
"#;
    command_ok(
        Command::new("calibre-debug")
            .args(["-c", script])
            .env("FIXTURE_ROOT", root)
            .env("CALIBRE_CONFIG_DIRECTORY", root.join("calibre-config")),
    );
    let initial_db = fs::read(library.join("metadata.db")).unwrap();
    let initial_source = fs::read(root.join("source-library/metadata.db")).unwrap();
    let mut app = App::open(&engine_workspace, Default::default()).unwrap();
    app.settings(Settings {
        library: library.to_string_lossy().into(),
        ..Settings::default()
    })
    .unwrap();
    let protected_before = app.scan.as_ref().unwrap().books[2].hierarchy.clone();
    let page = chronology::parse_page(
        b"<h1>Complete chronological order of Fixture Universe</h1>\
          <p>Earlier Unowned Work by Test Author in Fixture Universe is set in 100 AD. Earlier Unowned Work is Keep Existing Branch book 1.</p>\
          <p>Unplaced One by Test Author in Fixture Universe is set in 200 AD. Unplaced One is Keep Existing Branch book 2. Unplaced One is Inner Arc book 1. Inner Arc is part of Keep Existing Branch. Unplaced One is Smallest Arc book 1. Smallest Arc is part of Inner Arc.</p>\
          <p>Unplaced Two by Test Author in Fixture Universe is set in 300 AD. Unplaced Two is Keep Existing Branch book 3.</p>",
        "https://author.example/chronology",
    )
    .unwrap();
    let mut works = Vec::new();
    for (n, title) in ["Earlier Unowned Work", "Unplaced One", "Unplaced Two"]
        .iter()
        .enumerate()
    {
        let row = &page.source.segments[n + 1];
        let citations = vec![curator_core::ai::Citation {
            segment: row.ordinal,
            quote: row.text.clone(),
        }];
        let mut memberships = vec![timeline::Membership {
            name: "Keep Existing Branch".into(),
            parent: None,
            published_number: Some((n + 1) as f64),
            citations: citations.clone(),
        }];
        if n == 1 {
            for (name, parent) in [
                ("Inner Arc", "Keep Existing Branch"),
                ("Smallest Arc", "Inner Arc"),
            ] {
                memberships.push(timeline::Membership {
                    name: name.into(),
                    parent: Some(parent.into()),
                    published_number: Some(1.0),
                    citations: citations.clone(),
                });
            }
        }
        works.push(timeline::Work {
            title: (*title).into(),
            authors: vec!["Test Author".into()],
            universe: Some("Fixture Universe".into()),
            standalone: false,
            memberships,
            settings: vec![timeline::Setting {
                calendar: "ad".into(),
                literal: format!("{} AD", (n + 1) * 100),
                role: "principal".into(),
                citations: citations.clone(),
            }],
            contains: vec![],
            citations,
        });
    }
    let chunks = facts::chunks(&page);
    assert_eq!(chunks.len(), 1);
    let chunk = facts::screen(
        facts::Extraction {
            facts: timeline::Facts {
                works,
                relations: vec![],
            },
            coverage: vec![],
        },
        &page,
        &chunks[0],
        "offline-source-fixture",
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
    app.store
        .cache_value(curation::CORPUS_KEY, &corpus)
        .unwrap();
    drop(app);
    let binary = env!("CARGO_BIN_EXE_nexus-calibre-research");
    let request = |job: &str| {
        json!({
            "library":library,"workspace":engine_workspace,"job_id":job,
            "mode":"research","model":"qwen3.5:4b","endpoint":"http://127.0.0.1:11434",
            "enable_ai":false,"refresh":false,"book_ids":[],"limit":1,"protected_uuids":[]
        })
    };
    for job in [
        "11111111111111111111111111111111",
        "22222222222222222222222222222222",
    ] {
        let path = root.join(format!("{job}.json"));
        atomic_json(&path, &request(job)).unwrap();
        command_ok(Command::new(binary).arg(path));
    }
    let exported: Value = serde_json::from_slice(
        &fs::read(engine_workspace.join("exports/22222222222222222222222222222222.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(exported["attempted"], 1);
    assert_eq!(exported["mappings"].as_array().unwrap().len(), 2);
    assert_eq!(exported["mappings"][0]["fields"]["series_index"], 2.0);
    assert_eq!(fs::read(library.join("metadata.db")).unwrap(), initial_db);

    // Tampered source snapshots cannot be exported as a ready mapping.
    let mut app = App::open(&engine_workspace, Default::default()).unwrap();
    let original_corpus = corpus.clone();
    corpus
        .pages
        .values_mut()
        .next()
        .unwrap()
        .page
        .source
        .segments[1]
        .text
        .push_str(" tampered");
    app.store
        .cache_value(curation::CORPUS_KEY, &corpus)
        .unwrap();
    drop(app);
    let python = Path::new(REPO).join("scripts/calibre_auto_curate.py");
    let run_wrapper = |extra: &[&str]| {
        let mut cmd = Command::new("python3");
        cmd.arg(&python)
            .arg("--library")
            .arg(&library)
            .arg("--workspace")
            .arg(&workspace)
            .arg("--baseline")
            .arg(root.join("baseline.csv"))
            .arg("--clone-receipt")
            .arg(root.join("clone-verified.json"))
            .arg("--engine")
            .arg(binary)
            .arg("--no-ai")
            .args(extra);
        command_ok(&mut cmd)
    };
    run_wrapper(&["--export-only"]);
    let listed = run_wrapper(&["--retries", "--query", "Unplaced"]);
    let exceptions: Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(exceptions["automatic_retries"], 2);
    let app = App::open(&engine_workspace, Default::default()).unwrap();
    app.store
        .cache_value(curation::CORPUS_KEY, &original_corpus)
        .unwrap();
    drop(app);
    // Full handoff to native Calibre, with field masks preserving unsupported levels.
    run_wrapper(&["--export-only", "--apply"]);
    let mut app = App::open(&engine_workspace, Default::default()).unwrap();
    app.refresh().unwrap();
    let scan = app.scan.as_ref().unwrap();
    assert_eq!(
        scan.books[0].hierarchy[0].name.as_deref(),
        Some("Fixture Universe")
    );
    assert_eq!(scan.books[0].hierarchy[0].index, Some(2.0));
    assert_eq!(
        scan.books[0].hierarchy[1].name.as_deref(),
        Some("Keep Existing Branch")
    );
    assert_eq!(scan.books[0].hierarchy[1].index, Some(2.0));
    assert_eq!(
        scan.books[0].hierarchy[2].name.as_deref(),
        Some("Inner Arc")
    );
    assert_eq!(scan.books[0].hierarchy[2].index, Some(1.0));
    assert_eq!(
        scan.books[0].hierarchy[3].name.as_deref(),
        Some("Smallest Arc")
    );
    assert_eq!(scan.books[0].hierarchy[3].index, Some(1.0));
    assert_eq!(scan.books[1].hierarchy[0].index, Some(3.0));
    assert_eq!(scan.books[2].hierarchy, protected_before);
    assert_eq!(
        fs::read(root.join("source-library/metadata.db")).unwrap(),
        initial_source
    );
}
