# Calibre Library Curation
Source-backed research and in-universe chronology tools for a Calibre ebook library.

## Status
The production batch executable uses shared source facts and a chronology solver.
Main series plus all three sub-series fields have independently numbered positions.
Verified sub-series positions can now be calculated and exported while the wider
universe remains incomplete or unordered. Each exported level still needs its own
complete scope and supported chronology. Unknown indices stay untouched, and
local writes require verified or matching stored parent names. Partial mappings
remain in the research retry report rather than being called fully verified.
Independent-index validation passed 82 default Rust tests (3 live opt-ins
skipped) and all 74 isolated Calibre-Python tests. The native workflow confirms
that verified sub-series fields can change while an unsupported main index is
preserved. This does not resolve the failed real-source discovery acceptance.
The guarded temporary-library workflow has passed, but whole-library curation is
**not complete**. Source discovery, extraction and incomplete bibliography cases can
still require automatic retry. Old metadata and research are leads, not proof.
Automatic continuation now prioritizes unfinished cached bibliography scopes even
after work identity is known. Existing page/call budgets still apply; extraction
failures stop the pass and retain diagnostics instead of triggering more searches.
Sub-series retry state is separated by universe and scope.
Unvisited source links now survive research checkpoints. Completed cached pages
forward their links without consuming another page slot, and failed fetches move
behind unvisited sources rather than repeatedly starving the queue.
Fetch failures also allow discovery on the next run instead of trapping research
in a permanently failing saved queue.

## Deployment readiness
The subsequent authorized one-book live acceptance **failed**: full search was
blocked and returned leads did not establish the target work. Four extraction
calls had been spent on an unrelated page, with the last response truncated.
Source selection now checks work/author or an already verified universe before
inference; the bounded follow-up made zero further extraction calls. Stored
series names are discovery hints only. This prevents demonstrated waste but does
not resolve search availability or establish a verified chronology.
After this correction, 73 default Rust tests passed (3 live opt-ins skipped),
including the production temporary-library workflow. Formatting and whitespace
checks passed; Clippy completed with warnings, without errors.

The full authorized validation run on 2026-11-19 passed:
- All 53 Rust tests, including the three opt-in public-HTTP/local-model tests.
- All 74 Python tests under Calibre Python, including all six native API tests.
- System Python: 74 tests ran, with the six native-only tests skipped.
- Formatting, Python syntax and whitespace checks. Clippy completed with warnings.

This is a tested development snapshot, **not approval for unattended whole-library
deployment**. Independent per-series completeness gates now reject missing scoped
bibliographies, omitted memberships and same-named scopes from another universe.
General bibliography coverage and independent source corroboration remain
unfinished. The previously external core is now vendored with its source hashes
and license; the tested build does not require another checkout.
No existing ebook library was modified
by the validation suite; native writes were confined to temporary fixtures.

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
The exact required `curator-core` source is checked in under `vendor/curator-core`,
with its MIT license and upstream revision/file hashes in `UPSTREAM.json`.
No sibling Curator or media-stack checkout is required. Generated build output
defaults to this project's `target/`; `CARGO_TARGET_DIR` may select another cache.
Optional inference uses the
installed local Ollama model; tests requiring inference or public HTTP are ignored
unless explicitly enabled. No paid research/model API is configured.

## Offline verification
Run from this repository:

```fish
python3 -m unittest discover -s tests -p 'test_calibre*.py' -v
cargo test --locked --offline --manifest-path tools/calibre-research/Cargo.toml
```

The Rust workflow test creates and writes only a new temporary library. Its normal
Calibre writer guards still require the GUI and other writers to be closed.
The minimum cached-evidence path is verified by that test: the normal Python
research-and-apply command rebuilds two book results from source facts, exports
the mapping CSV, rehearses and applies it, and compares every selected name/index
pair with native Calibre readback. All four series levels are exercised. Protected
metadata and the source fixture stay unchanged; a second apply makes no changes.
The source facts are synthetic offline fixtures, not fresh online research or
proof of whole-library chronology.
Do not run the benchmark or opt-in online/model tests merely to verify a move.
Relocation verification: the Python suite ran 69 tests with 6 native-runtime skips;
Rust ran 39 passing tests with 3 opt-in skips. The guarded temporary-library
workflow passed from this repository. These checks do not certify the chronology
of the actual library.
The later author-boundary regression run passed 50 Rust tests (3 opt-in skips).
The wrapper's `apply_status` contract is documented in
`docs/CALIBRE-CHRONOLOGY.md`, with all outcomes covered by offline tests.
After vendoring the required core, the full authorized run passed all 53 Rust
tests (including the live/local-model gates) and all 74 Python tests under
isolated Calibre Python. Builds no longer require another working checkout.
Evidence completeness, independent per-series coverage and source corroboration
are still required before unattended whole-library curation can be considered complete.
Bibliography verification on 2026-11-20 passed 57 default Rust tests, with 3
HTTP/model opt-ins skipped, and all 74 Python tests under isolated Calibre Python,
including all six native API cases. The Rust run includes the independent-scope
regressions and guarded temporary-library workflow, including refusal when any of
the three sub-series scopes is missing. The separate system-Python run and the
format/lint/syntax-check batch were cancelled, so neither is a fresh pass in this
verification. The initial diff whitespace check passed. Existing libraries were
not modified; native writes used temporary fixtures only.
Latest continuation verification passed 70 default Rust tests (3 HTTP/model
opt-ins skipped) and all 74 isolated Calibre-Python tests. Thirteen new offline
regressions cover cached extraction and durable linked-source continuation.
Rust formatting, Python syntax (10 files) and whitespace checks passed; Clippy
completed with warnings, without errors. Existing libraries were not modified.

## Actual-library verification, 2026-11-20
The read-only duplicate-library check covered all 8,524 records. Both original and
duplicate metadata databases passed SQLite integrity checks. UUIDs were unique
and valid, series indices were finite, all three custom series columns were
present, and no original-baseline books were missing. There were 218 write-protected
records; protection does not establish verified chronology.

Export validation against a snapshot of the default retained research workspace
produced **zero verified mappings**, with one stale/invalid saved result. Thus all
8,524 records still lack an exportable verified mapping in that workspace:
**whole-library chronology verification has not passed**. No fresh online research
or metadata apply was performed. Database hashes remained unchanged. Verification
used a separate research workspace because the library filesystem was below the
unchanged 21 GiB reserve.

Final code validation passed 70 default Rust tests (3 live opt-ins skipped), all
74 isolated Calibre-Python tests, and 74 system-Python tests with 6 native-only
skips. These passing tests do not override the zero verified library coverage.

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
