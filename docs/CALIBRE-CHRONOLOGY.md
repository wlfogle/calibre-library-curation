# Safe Calibre chronology imports
**Current evidence policy:** The user subsequently clarified that old research may be wrong. Stored indices, old documents, prior catalogs and previous approvals are leads, not placement proof. The 39 Clues, Greater Eternal Champion Saga, Greater Foundation Universe and The Grand Tour remain write-protected, but the new engine does not skip their research or presume their correctness. Historical reports/benchmarks below do not override this policy.

`scripts/calibre_chronology.py` imports **reviewed assignments**, not invented chronology. It never derives story order from publication dates, alphabetical sorting, or an old series index. A human must research and review the book-to-timeline mapping first. A source reference and verification flag record that review; they do not independently prove a source is correct.

## Verified full-library test copy
The user requested a duplicate of the actual library for development/testing. The verified test library is:
`/media/loufogle/Data/Calibre Test Workspace-20261119/Calibre Library`

Use this path for subsequent development writes, not `/media/loufogle/Data/Calibre Library`. The copy contains all 8,524 database records and 31,034 files, representing 53.87 GiB of logical content, including ebooks, covers, OPFs, custom metadata and `.calnotes/notes.db`. The extra record titled `all books` is retained; it was not deleted or treated as a newly discovered novel.

Because no available volume could accommodate a full physical copy while retaining its free-space reserve, this is a Btrfs reflink/copy-on-write clone. Every copied file has a distinct inode; no hardlinks are used. Changes to the test files are independent, but unchanged extents share the same physical storage. **This is not an independent physical backup.** Further changes allocate additional space: avoid bulk ebook rewriting/conversion and recheck space before significant tests. Approximately 22.28 GiB remained immediately after verification.

The private workspace contains `clone-started.json`, the retained initial verification refusal/continuation record, and `clone-verified.json` with per-file SHA-256 values and validation details. All non-database file bytes matched the source. Fresh standalone SQLite backups of `metadata.db` and `.calnotes/notes.db` passed integrity checks and full schema/physical-table comparisons, including FTS backing tables. Calibre's notes database uses its custom `calibre` tokenizer; system SQLite cannot query those virtual search views directly. Their definitions and stored backing tables were verified instead, without dropping/rebuilding indexes or altering the source.

Final verification checked source inventories and database hashes remained unchanged. The original CSV, live metadata database and existing backup also still matched their earlier extraction hashes. Do not recopy over this test library or present a started-but-unverified copy as certified.

## Build the researched chronology mapping
The primary goal is **curation**, not merely importing existing numbers. `scripts/calibre_build_chronology.py` creates source-cited universe assignments from the research catalogs under `data/calibre-chronology/`.

The initial catalogs cover 184 Dragonlance works and 45 Known Space works, including held/ambiguous entries. Dragonlance chronology comes from the specialist Nexus chronological product list, with sub-series membership checked against its product catalog and Dragonlance Library. Known Space uses the Niven-hosted Lensman chronology and coauthor Edward M. Lerner's dated Fleet/Ringworld discussion. Source URLs, date ranges, approximation/conflict notes and catalog content hashes accompany proposed assignments.

**This is an initial, partial research catalog, not all universes in the library.** It deliberately distinguishes:
- Source-established exact work/title aliases with compatible existing universe context or author evidence.
- Protected universes, never remapped.
- Same-title editions/collections, comics and cross-universe collisions, held for work-identity review.
- Time-travel and multi-era anthology placements requiring an explicit consistent policy.
- Unresearched records, listed rather than assigned fake consecutive positions.

Primary positions retain unowned works present in each source catalog. Sub-series numbers follow that catalog's chronology independently; they are not copied from old Calibre numbers or publication volume numbers. Neither source list is asserted to cover every work ever published. Same-year whole-book order follows the selected specialist list, with the limitations reported. Source references count as factual placement review; the importer's separate exact-diff approval is still required before any write.

```fish
python3 scripts/calibre_build_chronology.py \
  --csv "/media/loufogle/Data/New Ebooks/calibre-index-extraction-20261119/all_books_with_subseries_indices.csv" \
  --library "/media/loufogle/Data/Calibre Library" \
  --baseline "/home/loufogle/Downloads/all books.csv" \
  --documents "/media/loufogle/Data/Documents on External Drive" \
  --curator-state "/home/loufogle/.local/share/org.nexus.calibre-curator/state.sqlite3" \
  --output-dir "/media/loufogle/Data/New Ebooks/chronology-curation-new-run"
```

Outputs include `source_mapped_chronology.csv`, per-book `coverage.json`, `summary.json`, unmatched source works, a read-only Curator reference inventory and the importer dry-run. A nonzero result can mean that a genuine researched placement collides with an old number still assigned to an unreviewed book. Such collisions remain blocking errors; the builder does not forge duplicate exceptions or renumber unrelated books to pass validation. Full-library completion remains explicitly false.

### Local source documents, including Markdown
The correct local reference directory is `/media/loufogle/Data/Documents on External Drive`, not the historical `/media/loufogle/Data/Documents` path saved in some older Curator references. The mounted external drive also has chronology originals under `/media/usb0/Documents`; repeat `--documents` to include both and deduplicate across roots. The optional document pass reads likely chronology files directly in those directories: `.md`, `.markdown`, `.odt`, `.pdf`, `.txt`, and `.csv`. It avoids unrelated account/personal/game files and does not execute instructions embedded in documents. PDFs use installed `pdftotext`; ODT extraction uses standard-library ZIP/XML parsing and preserves explicit whitespace. Automatic list counters not present in ODT paragraph text are not invented.

`local-sources.json` records source paths, raw-file hashes, normalized-text hashes, extracted-source duplicates, URLs and mentions of catalog works. Extracted text lives privately under `local-source-text/`. Duplicate copies do not count as independent corroboration. Missing/damaged ZIP members, undecodable text and invalid PDFs are listed as extraction failures, never repaired or silently promoted into evidence.

These documents are starting points. In particular, the local expanded Dragonlance Markdown dates The Irda around 2000 PC while the specialist chronology places it at 6320 PC; a local Known Space ODT dates Fleet of Worlds at 2850 while its coauthor gives 2650–2652 for the main narrative. The source files are retained unchanged and are **not auto-approved**. Their bibliography/publication lists, mixed universes, collection boundaries and contradictory dates must be evaluated during continued curation. Existing Curator approvals are inventoried for scope review rather than blindly re-approved as universal chronology.

## Input contract
CSV must be UTF-8 (BOM accepted), with unique headers and one row per unique positive Calibre `id`. `uuid` must also match the existing book. Use the original pre-import export as `--baseline`, not a later generated CSV.

The editable name/index pairs are:
- `series`, `series_index`
- `#series2`, `#series2_index`
- `#series3`, `#series3_index`
- `#series4`, `#series4_index`

Omitted columns preserve their existing values. An explicitly empty name clears that series and resets its index to the neutral `1.0`; its supplied index must be empty or omitted. Every supplied populated series needs an explicit numeric index. An index alone can change the index of an existing series without changing its name. Fractions, zero and negative indices are supported; NaN, infinity, overflow and precision loss are rejected. Series columns must actually be Calibre series-type custom columns.

Changed rows also require:
- `order_verified`: exactly `yes`, **only after actual review**.
- `order_basis`: `in-universe`; `publication` is allowed only when the resulting hierarchy includes Hard Case Crime; `standalone` requires all series levels to be empty.
- `order_source`: a nonempty reference to the researched order, with enough detail to locate the book's placement (URL, edition/page, or a reviewed local source).

Publication order for Hard Case Crime must also be supplied as reviewed explicit indices. This importer does not trust edition `pubdate` metadata to invent those ranks or break tied/unknown dates.

New/resulting duplicate indices involving changed books require explicit exceptions from **every** affected book, including unchanged or otherwise unlisted books: add `allow_duplicate_indices` containing the applicable field names, separated by `;`, together with `order_verified=yes` and an explanatory `order_source`. The duplicate check uses the hierarchy (parent universe/sub-series names), includes books outside the CSV, and does not renumber legitimate duplicate editions or omnibuses automatically.

Do not rename headers to imitate UI labels. A grid column labelled “Series” is not evidence that it is Calibre's built-in `series` field. Existing generated CSVs without sub-series indices/evidence are expected to fail validation.

## Protected books and missing rows
The following existing custom chronologies are never modified:
- Greater Eternal Champion Saga
- Greater Foundation Universe
- The 39 Clues
- The Grand Tour

Protection is the union of original-export UUID membership and current/incoming protected-series membership across all four levels. Re-labelling a protected book in the incoming CSV cannot evade protection. Reused IDs with different UUIDs are refused. Differences in available baseline names/main index are reported but not silently restored; absent original custom indices cannot be reconstructed from an export that never recorded them.

Books omitted from the incoming CSV remain untouched. An export need not have the same number of rows as the current library.

## Dry-run
### Extract existing sub-series numbering first
`scripts/calibre_extract_indices.py` reads the current database and, optionally, a backup through read-only SQLite transactions. It enriches the **original** export with `#series2_index`, `#series3_index` and `#series4_index`, preserving every original name and main-series index.

```fish
python3 scripts/calibre_extract_indices.py \
  --csv "/home/loufogle/Downloads/all books.csv" \
  --database "/media/loufogle/Data/Calibre Library/metadata.db" \
  --compare-database "/media/loufogle/Data/Calibre Library/metadata.db.bak" \
  --output-dir "/media/loufogle/Data/New Ebooks/calibre-index-extraction"
```

The output directory must be new and outside the library. Its CSV/report are private. All IDs and UUIDs must match. Name matching is exact or explicitly recorded whitespace-equivalent (newlines versus spaces); no prefix removal, case/punctuation guessing or renaming occurs. A stored index must be finite and agree with the comparison database when supplied. Conflicting names, missing/invalid indices and differing backup values stay blank and are listed for review, never defaulted to `1`.

Existing numbering is **not verified chronology**. This extractor does not add approval flags and clears any pre-existing `order_verified` flag. It does not attach an old sub-series index to a differently named sub-series in a generated chronology file. Source hashes/identities are checked before/after, output rows are reparsed and verified, and a final `extraction-report.json` records provenance, matches and unresolved cases.

### Audit reviewed assignments
Plain Python performs a read-only SQLite audit; it does not open Calibre's writable API. The following command is fish-compatible. Use a **new** report filename for every run; reports are private and never overwritten.

```fish
python3 scripts/calibre_chronology.py \
  --library "/media/loufogle/Data/Calibre Library" \
  --csv "/media/loufogle/Data/New Ebooks/reviewed_chronology.csv" \
  --baseline "/home/loufogle/Downloads/all books.csv" \
  --report "/media/loufogle/Data/New Ebooks/chronology-review.json"
```

Exit `0` means validation passed; exit `2` means refusal/validation errors. Inspect `errors`, `changes`, `protected_discrepancies`, and `unlisted_books` in the report. Each change includes exact before/after values plus its source and basis. The approval digest binds the full input bytes, baseline, library location, database contents and reviewed diff. Any subsequent library/input change invalidates approval.

No automatic “fix everything”/renumbering mode is provided.

## Explicit apply
Close the GUI, Content Server, editor and other Calibre tools. Do not start another writer until apply finishes. Offline use is mandatory: Calibre's shared `db` writer lock, normal GUI single-instance lock, importer library lock, and Linux process checks are safeguards, **not a universal lock against arbitrary SQLite writers or a newly started server**. Nothing is killed or stopped automatically. Calibre's own proven descendant helper workers are permitted; unrelated helpers block.

Apply runs through one installed Calibre Python process, using the supported native cache API. It rehearses the complete update on a fresh temporary copy, then makes a new consistent SQLite backup and durable before-values journal outside the library, applies only differences, and verifies all four name/index pairs, identities, titles and book membership across the whole library. It disables shared-name case changes; a requested case-only rename that the API cannot represent exactly is refused by rehearsal rather than changing other books.

```fish
set approval (python3 -c 'import json; print(json.load(open("/media/loufogle/Data/New Ebooks/chronology-review.json"))["approval"])')
calibre-debug -e scripts/calibre_chronology.py -- \
  --library "/media/loufogle/Data/Calibre Library" \
  --csv "/media/loufogle/Data/New Ebooks/reviewed_chronology.csv" \
  --baseline "/home/loufogle/Downloads/all books.csv" \
  --report "/media/loufogle/Data/New Ebooks/chronology-apply.json" \
  --apply --approve "$approval" \
  --backup-dir "/media/loufogle/Data/Calibre Chronology Backups"
```

The GUI/content server must stay closed even after the final concurrency check. Reports, backups and journals contain private metadata: do not commit them. This tool only updates the library database, not embedded metadata in EPUB/PDF files. Calibre's native dirty-book tracking handles later metadata backup; do not manually replace book-side OPF files.

## Failure and recovery
Calibre's field-level API calls commit independently. The importer **does not claim atomic multi-field updates**. Rehearsal failures leave the target database unchanged. A failure during the target write prints the exact recovery directory and retains:
- `metadata.db`: the fresh, integrity-checked pre-apply database backup.
- `before.json`: the full before-values snapshot and reviewed diff.
- `apply-started.json`: durable evidence that a write may have started.
- `verified.json`: created only after successful final readback.

If a started run lacks `verified.json`, stop and review it before any further apply. The library's `.chronology-incomplete.json` records its recovery directory and blocks further writes, even if a different backup directory is requested. After reviewed recovery and validation, this marker must be explicitly cleared; there is no bypass flag. A crash after final verification but before marker removal still requires that review. Keep Calibre closed. Do not blindly retry, replace the database with an old `metadata.db.bak`, or overwrite a database while WAL/SHM files or writers are active. The new backup can be inspected as a **separate disposable library** first. Recovery must be explicitly chosen: either a reviewed native-API correction using journal before-values, or an offline database restoration after confirming no subsequent book/file/library changes and safely handling any SQLite sidecars. There is intentionally no automatic destructive restore command.

The tool verifies series assignments, identities and counts, not book-file integrity or historical chronology. Its numeric/source validation cannot replace research.

## Tests and repeatable benchmark
### Automatic wrapper: reviewed safety and audit behavior
`scripts/calibre_auto_curate.py` uses the `nexus-facts-batch-v2` production export. The executable now calls the shared facts/chronology path; this does not mean the whole library is verified. The actual binary-to-wrapper-to-native-Calibre fixture test has passed with independently checked main plus all three sub-series fields, source-tampering refusal and unchanged protected/source records.

The workspace lock is acquired before clone receipt and storage preflights. Library directories and symlink lock files are refused before changing them. The lock serializes cooperating wrappers, not external receipt writers or arbitrary database tools; the existing importer/writer checks still apply.

`--export-only` revalidates saved evidence with model assistance disabled. It rejects `--refresh`, `--limit`, `--book` and `--retry-exceptions`; `--apply` remains optional and separately guarded. Without `--apply`, research writes only its workspace and reports. No extra Python Ollama probe is required: the engine checks the installed local model when uncached extraction is necessary, while offline export and reusable evidence do not require a running inference server.

Export conversion validates canonical UUIDs, authors/titles, four boolean support flags, exact name/index pairs, finite numeric indices, control-free names, credential-free HTTPS URLs and hexadecimal SHA-256 values. Publication/recommended ordering is rejected. Standalone clearing must explicitly cover all four fields. The downstream importer still verifies library identities, collisions and actual metadata semantics; these structural checks are not independent historical verification.

New jobs contain immutable `job.json` manifests with monotonically increasing sequence numbers allocated under the workspace lock. Exception/retry observations carry that sequence, job identity and a hash of the exact saved book result. Both queues are merged by job sequence only while that hash still matches; file modification times do not establish precedence. Legacy unsequenced queues cannot override current book state. Malformed records fail closed with the affected filename. Interrupted unique jobs/mapping files remain retained for diagnosis and must not be reused.

Summaries include `job_sequence`, `mapping_csv_path`, `dry_run_report_path`, and an explicit no-change note when `--apply` has nothing to write. Inspection requires no engine/model call or workspace lock:
```fish
python3 scripts/calibre_auto_curate.py --last-summary
python3 scripts/calibre_auto_curate.py --summary JOB_ID
```
`--last-summary` selects the latest completed sequenced job. Inspect legacy jobs by their explicit ID. Inspection may read private reports but never applies metadata.

The reported `Less than 21 GiB free` refusal is a storage preflight, not a research result. A read-only research run may use a separate filesystem with sufficient workspace reserve through `--workspace`; every apply still checks the unchanged 21 GiB reserve on the library filesystem as well. No files are deleted and no reserve is lowered automatically.

### Nested series data structure
`timeline::SeriesHierarchy` stores a series name, the book's independent optional index, and recursive `subseries`. Calculated placements include this tree alongside the compatibility membership list. Parent links preserve Forgotten Realms → The Legend of Drizzt → Icewind Dale; parallel branches and deeper nesting are supported. Unknown positions stay null. Unknown parents, cycles and conflicting parents/indices are rejected. Offline tests cover that three-level example, JSON round-trip, multiple branches, deeper nesting and the solver's nested output. This data structure does not establish new bibliographic facts or complete production research integration.
The user's verified **Greater Eternal Champion Saga is the structural and in-universe chronological reference**, not a publication-order template. Read-only inspection confirmed 94 protected books. A regression retains the actual independent assignments for Dreamthief's Daughter (saga 97 / Elric 10 / The Moonbeam Roads 1), The Skrayling Tree (98 / 11 / 2), and The White Wolf's Son (saga 99 / The Moonbeam Roads 3 / Oona Von Bek 3). The hierarchy does not copy a parent index into a child or renumber these protected books.

### Live discovery and three-book research to native updates
`tools/calibre-research/tests/live_book.rs` has two opt-in acceptance tests covering **Ringworld**, **The Ringworld Engineers** and **The Ringworld Throne**:
- Configured-source research fetches the live Niven-hosted chronology and separate membership guide.
- Automatic discovery starts with a book/author query, uses the existing guarded search/cache, prioritizes author-site chronology leads before truncating results, and follows relevant source links. It permits at most four source-page fetches and never falls back to the configured URLs to claim discovery success.
Both paths extract cited dates and memberships at runtime, solve the three-work chronology, generate three mappings and verify native Calibre updates. Missing/ambiguous/unavailable evidence fails the acceptance test rather than substituting recorded success. Discovery failures retain a private `automatic_retry` receipt, not a manual-review request; this receipt is not yet integrated into the production scheduler.

```fish
env CARGO_TARGET_DIR=/media/loufogle/Data/Repos/calibre-curator/target \
  cargo test --offline --manifest-path tools/calibre-research/Cargo.toml \
  --test live_book -- --ignored --nocapture
```

The tests establish The Ringworld Engineers' fictional setting as 2878–2881, after Ringworld and before The Ringworld Throne, and derive its Ringworld sub-series position as 2 from those temporal facts. Each creates a new private five-record Calibre library, validates and writes three mappings through the importer's native API routine, and verifies complete readback, integrity, unrelated/protected records, preservation of an unsupported third membership on all three changed books, and zero changes on repetition. Shared serialized evidence is revalidated and replayed without more source fetches; the discovery path also verifies search-cache reuse. The original metadata database hash must remain unchanged.

**Scope:** these particular live tests use the source-specific Known Space parser. Their numeric ranks apply only to the selected three-work context, not the complete universe. Native writes occur only in newly created path-pinned private fixtures; production GUI/writer locks are not overridden. The batch executable has since been switched to the general source-facts path, with separate operational retries, but complete live-library research, universe coverage and whole-library application are not established by these tests.

Observed expanded run on 2026-11-19: **both live tests passed**, each updating and independently reading back three books using two source-page fetches. Discovery-to-native evidence: `/tmp/nexus-live-discovery-6tCItZ/discovery.json` and `/tmp/nexus-live-book-e2e-evCSKH/{research.json,mapping.csv,update-verified.json}`. Configured-source evidence: `/tmp/nexus-live-book-e2e-BTMJF7/`. All 15 chronology-model regressions, six discovery-selection regressions and 11 Curator chronology regressions passed (three unrelated opt-in Curator tests skipped).
An earlier discovery attempt found relevant sources but consumed its page budget on lower-priority results. The new regression checks that a late author chronology lead survives selection; the shared Curator HTML parser also retains linked exploration guides while continuing to reject storefront navigation/audio links. No paid model/API calls or model downloads were used.
Formatting and whitespace checks passed. Strict targeted Clippy remains blocked by existing chronology-module/fixture lints: a collapsible conditional, regex construction inside a loop, a fixture helper's argument count and a cloned slice argument. These do not represent failing acceptance tests; strict lint success is not claimed.

### Offline regressions and synthetic performance
```fish
python3 -m unittest discover -s tests -p test_calibre_chronology.py -v
calibre-debug -e tests/test_calibre_chronology.py
calibre-debug -e tests/benchmark_calibre_chronology.py
```

The ordinary Python suite skips native tests. The Calibre suite executes them using new disposable libraries, including independent sub-series indices, protected/unlisted preservation, idempotence, rehearsal failure and retained recovery records after an injected failure.

The current benchmark creates an entirely synthetic 8,524-book library under `/tmp/nexus-calibre-benchmark-*`. It supplies 8,523 rows, deliberately attempts to relabel 218 protected books, changes 8,305 eligible books at all four series levels, and leaves one unlisted book untouched. It independently checks every expected assignment and reruns the audit to confirm zero remaining changes. Printed timings separate dry-run from apply including rehearsal, backup and readback; fixture creation, research and Calibre process startup are not part of those timings. Synthetic ranks are test data, not real chronology. Temporary benchmark receipts remain in the printed directory. Historical timings below were measured before the trusted-set correction, with 234 protected and 8,289 changed books; they are not a new performance run under the corrected policy.

Fixture generation disables SQLite synchronous commits **only on that new synthetic database connection** to avoid thousands of durable fake-book insertion commits. The connection is closed before timing; rehearsal and apply open normal native connections with their standard durability settings. Production importer durability is never relaxed.

Measured on the development workstation on 2026-11-18: **0.494 seconds dry-run; 58.249 seconds guarded apply**, including rehearsal, consistent backup and readback. Entire benchmark including fixture creation and independent extra checks: 68.447 seconds. All 8,289 expected updates matched, all 234 protected books plus the unlisted book remained unchanged, and the next audit proposed zero changes. A real library can be slower because database size, disk load and plugins differ; these are synthetic benchmark results, not a promise of live-library runtime.

## Reference implementations
Reviewed [Calibre Curator](https://github.com/wlfogle/calibre-curator), particularly `src-tauri/resources/bridge.py`, `tests/fixture.py`, `tests/test_backups.py` and its chronology research module. Its independent series/index writes, shared Calibre writer lock, source/identity separation and unrelated-metadata/category guards informed this importer.

Curator's current full-library backup and OPF verification are **broader** than this script's metadata-database backup. This importer does not copy ebooks or certify an independent full-library backup; maintain one separately. It does not invoke or alter Curator's workspace, rules, pins or history.

[Calibre Library Fixer for Pop!_OS](https://github.com/wlfogle/calibre-library-fixer-popos) was also reviewed. Its documented purpose is legacy filename/path renaming, and its README explicitly does not certify that direct-SQL engine for managed libraries. None of that writer is used here.
