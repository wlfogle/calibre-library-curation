# Calibre Library Curation
Source-backed research and in-universe chronology tools for a Calibre ebook library.

## Status
The production batch executable uses shared source facts and a chronology solver.
Main series plus all three sub-series fields have independently numbered positions.
The guarded temporary-library workflow has passed, but whole-library curation is
**not complete**. Source discovery, extraction and incomplete bibliography cases can
still require automatic retry. Old metadata and research are leads, not proof.

## Layout
- `scripts/calibre_auto_curate.py`: batch workflow, evidence validation, retry/review inspection and guarded application.
- `scripts/calibre_chronology.py`: sparse metadata importer, dry-run, rehearsal, backup and readback.
- `scripts/calibre_build_chronology.py`, `calibre_chronology_sources.py`, `calibre_extract_indices.py`: catalog/reference and existing-index utilities.
- `tools/calibre-research/`: Rust source-facts extractor, shared research store and chronology solver.
- `tests/`: Python regressions and opt-in native fixtures.
- `data/calibre-chronology/`: historical source catalogs; not automatically trusted placements.
- `docs/CALIBRE-CHRONOLOGY.md`: contracts, safeguards, commands and known limits.

## Dependencies
Python 3, Rust/Cargo and installed Calibre (`calibre-debug`) are required.
The Rust adapter currently depends on `curator-core` from a sibling checkout:

    Repos/
      calibre-library-curation/
      calibre-curator/crates/curator-core/

That dependency remains explicit in `tools/calibre-research/Cargo.toml`; no
media-stack repository is needed. Do not assume an arbitrary upstream Curator
revision contains the local research API changes. Optional inference uses the
installed local Ollama model; tests requiring inference or public HTTP are ignored
unless explicitly enabled. No paid research/model API is configured.

## Offline verification
Run from this repository:

```fish
python3 -m unittest discover -s tests -p 'test_calibre*.py' -v
env CARGO_TARGET_DIR=../calibre-curator/target \
  cargo test --offline --manifest-path tools/calibre-research/Cargo.toml
```

The Rust workflow test creates and writes only a new temporary library. Its normal
Calibre writer guards still require the GUI and other writers to be closed.
Do not run the benchmark or opt-in online/model tests merely to verify a move.
Relocation verification: the Python suite ran 69 tests with 6 native-runtime skips;
Rust ran 39 passing tests with 3 opt-in skips. The guarded temporary-library
workflow passed from this repository. These checks do not certify the chronology
of the actual library.

## Use
The default target is the existing verified duplicate:
`/media/loufogle/Data/Calibre Test Workspace-20261119/Calibre Library`.
The original `/media/loufogle/Data/Calibre Library` is not an allowed apply target.
Normal runs do not apply metadata unless `--apply` is supplied.

```fish
python3 scripts/calibre_auto_curate.py --help
python3 scripts/calibre_auto_curate.py --retries
python3 scripts/calibre_auto_curate.py --last-summary
```

Research workspaces, audit reports, clone receipts, backups and ebook libraries
remain outside this repository. A separate `--workspace` may be used for read-only
research when its filesystem has sufficient reserve. Applying metadata still
requires at least 21 GiB free on the library filesystem and all normal safeguards.
