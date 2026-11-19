# Calibre curation mistakes and prevention

Read relevant entries before planning or editing. Newer evidence-policy corrections supersede historical trust claims. Keep reports factual, preserve both libraries, and diagnose saved failures offline before spending more calls.

### [2026-11-19] Rebuild location-dependent Rust artifacts after moving the project
**Mistake:** The first post-move Rust run reused a shared cached test binary containing the old compile-time repository path.
**Root cause:** File timestamps were preserved during relocation, while the test embeds `CARGO_MANIFEST_DIR`.
**Prevention:** Invalidate only this package's generated build artifacts after relocation; keep dependency builds and all user data intact.
**Verification:** A package-specific clean and offline rebuild from `calibre-library-curation` passed all 39 default Rust tests, including native temporary-library readback.

### [2026-11-19] Keep wrapper status precedence and validation independent of filesystem timestamps
**Mistake:** The wrapper used queue file modification times to override saved book statuses, accepted weakly typed provenance before CSV creation, and allowed ambiguous export-only/research flag combinations.
**Root cause:** Trusted filesystem ordering and deferred too much structural validation to downstream import checks.
**Prevention:** Lock before cooperating-wrapper preflight; reject library/symlink workspaces; use immutable job sequences and exact saved-result hashes for queue observations. Validate typed exports and force export-only to disable model assistance and reject research controls. Keep apply reserves and downstream identity/collision checks intact.
**Verification:** Offline regressions cover reversed queue mtimes, changed saved results, malformed records, symlink locks, invalid UUIDs/hashes/indices, flag conflicts, no-op applies and read-only summary inspection.

### [2026-11-19] Old research and write protection are not proof of chronology
**Mistake:** Earlier instructions and documentation described the protected four series as factually trusted after the user clarified that old research might be incorrect.
**Root cause:** Conflated protecting existing metadata from writes with accepting it as verified evidence.
**Prevention:** Reverify placement from fresh online sources. Keep the four series write-protected, but do not skip their research or promote old metadata/documents into factual proof. Earlier reference fixtures demonstrate structure and preservation only.
**Verification:** The new engine uses a separate fresh-web corpus, rejects old/local evidence and bypasses the old generic page cache; protected records receive verification reports rather than automatic approval.

### [2026-11-19] Stop substituting component work for the requested verified whole-library curator
**Mistake:** Repeatedly delivered importers, wrappers, Ringworld-specific demonstrations and a hierarchy structure while the actual batch executable continued using the old extractor. The user reports at least $60 in wasted credits. Repeated explanations and promises did not deliver the requested automation.
**Root cause:** Failed to keep the production call path and the user's acceptance criteria central. Treated isolated progress as a useful stopping point despite repeated corrections and requests to finish.
**Prevention:** Trace the actual executable before proposing changes. Use one research-backed, user-approved implementation plan and explicit end-to-end acceptance criteria. Replace the defective production path, reuse working safeguards, and avoid additional wrappers, franchise-specific demonstrations, agents or unrelated features. Do not call the curator complete until the real workflow researches books, derives source-supported chronology, retains every applicable sub-series and verifies updates on the duplicate library.
**Verification:** Identify the exact production extractor and test its real invocation. Report actual verified coverage and remaining blockers; passing component tests must never be presented as whole-library completion.

### [2026-11-19] Preserve main series plus ALL THREE sub-series fields and verify their placement
**Mistake:** Repeatedly emphasized a three-level example instead of consistently addressing all four available Calibre levels, and treated implementing a hierarchy as progress toward verified placement without establishing the placement itself.
**Root cause:** Confused data representation with bibliographic verification and did not consistently distinguish sub-series depth from the number of available fields.
**Prevention:** The schema is main `series` plus `#series2`, `#series3` and `#series4`, each with its own independent index. Preserve genuine nested and overlapping memberships. Greater Eternal Champion Saga is the user's verified IN-UNIVERSE chronological reference, not publication order. Do not infer that other stored metadata is verified or that a repeated universe label is a genuine sub-series.
**Verification:** Test all four name/index pairs, actual verified reference assignments, nested and parallel memberships, and preservation of protected books. Structural fixtures do not establish real-book chronology.

### [2026-11-19] Diagnose rejected model output instead of repeatedly guessing prompts
**Mistake:** Repeated local-model acceptance runs failed on an unsupported parent-series claim, an inferred temporal relationship, coverage and finally a missing calculated rank. Changed prompts and acceptance handling without first retaining and inspecting the complete rejected extraction. The latest real-model test still fails; production integration remains unfinished.
**Root cause:** Relied on a model to satisfy a complex extraction contract and used successive failures as diagnosis instead of examining one retained response and establishing a supported solution first.
**Prevention:** Before another inference, retain the exact source segments, raw response, parsed proposals, rejection reasons and solver problems privately. Diagnose the specific failure offline. Keep explicit facts separate from relationships derived by code. Never weaken literal or semantic checks, silently rewrite an unsupported claim, lower expected assertions or label a failed acceptance test successful. A complete-looking heading alone does not prove that every relevant work was extracted or that the resulting universe rank is established.
**Verification:** Replay the retained failing response without further model calls, add targeted negative and positive regressions, and require the installed-model acceptance test and production-path integration test to pass unchanged before promoting the extractor.

### [2026-11-19] Honor the user's credit limits through bounded work and concise reporting
**Mistake:** Continued lengthy analysis, repeated broad file reads, status explanations, unsuccessful patch attempts and repeated model tests after the user repeatedly raised the cost. Proposed a child agent despite the user's demand to reduce spending; its launch was cancelled.
**Root cause:** Failed to constrain investigation and implementation to the demonstrated defect and did not stop speculative retries when evidence was insufficient.
**Prevention:** Reuse known context, read only necessary code, batch independent reads and make small verified edits. Do not retry cancelled work or launch more agents without fresh authorization. Agree the bounded validation scope before expensive work; investigate saved failures offline rather than repeating network/model requests. Keep communication short and factual. When asked for a plan, create the complete plan and wait for approval rather than sending an incomplete draft or starting implementation.
**Verification:** No unnecessary new agents, broad rereads, benchmarks or repeated inference loops. State what actually changed, which tests passed or failed and whether any library was modified. Do not claim the ability to restore credits or issue refunds.

### [2026-11-19] Rank discovery leads before spending the source-page budget
**Mistake:** The expanded discovery acceptance test truncated results before prioritizing a relevant author-hosted chronology, exhausting its bounded fetches on lower-priority sources.
**Root cause:** Search-result position was used as the acquisition priority, and linked exploration guides were omitted.
**Prevention:** Prioritize relevant author/chronology leads before truncation and follow relevant guides within the existing budget. Host resemblance is only a discovery hint, never source verification; retain literal fact validation and do not substitute configured URLs for discovery success.
**Verification:** A late-author-lead regression passes, shared guide parsing is tested, and actual discovery through three native fixture updates passed with two source-page fetches.

### [2026-11-19] Trust only the chronologies explicitly verified by the user
**Mistake:** Treated Shannara and Federated Sentient Planets as verified/protected based on earlier assistant-generated assumptions, while omitting The 39 Clues and The Grand Tour.
**Root cause:** Reused prior conversation claims as authority instead of the user's current explicit verification.
**Prevention:** The only verified baseline series are The 39 Clues, Greater Eternal Champion Saga, Greater Foundation Universe and The Grand Tour. Research all other books/series; neither existing tags nor old generated mappings establish in-universe chronology.
**Verification:** Protection constants and regression tests match exactly those four; Shannara/FSP remain eligible for researched corrections. Preserve identities and do not alter the live library during development.

### [2026-11-19] Verify Calibre notes without assuming system SQLite has its tokenizer
**Mistake:** Initial test-library clone verification attempted to query every virtual table with system SQLite, which cannot instantiate the notes database's custom `calibre` FTS tokenizer.
**Root cause:** Assumed the notes and main metadata databases expose identical SQLite extension requirements.
**Prevention:** Inspect virtual-table definitions first. Use Calibre's registered tokenizer, or compare complete schema plus all physical tables including FTS backing tables without invoking derived virtual views. Never drop indexes, modify the source or repeat a completed copy to bypass a verification refusal.
**Verification:** The existing clone was retained, both standalone databases passed integrity and schema/physical-data equality checks, and all 31,034 files had independent inodes and byte/hash readback verification. Original library and backup hashes remained unchanged.

### [2026-11-19] Deliver researched chronology, not only an importer
**Mistake:** Implemented a safe metadata importer and extracted existing indices while leaving the user's actual goal—curated in-universe universe/sub-series mappings—unfinished.
**Root cause:** Focused on import failures, then wrapped an extractor that expected pre-existing explicit chronological ranks instead of researching temporal facts and constructing the requested ordering. Retail/audio discovery also diverted an ebook task into irrelevant narrator metadata.
**Prevention:** Research and prove representative ebook-to-placement cases before further implementation. Extract cited universe membership, fictional date ranges and narrative relationships, then construct universe-wide and independent sub-series ordering; do not require websites to supply the final Calibre rank. Cache research by universe, not repeated broad per-book searches. Keep narration/audiobook editions out of prose-work placement. Include the user's Markdown/ODT/PDF/TXT/CSV sources as leads and cross-check them. The importer and batch runner are downstream components, not the curation engine.
**Verification:** Mapping outputs must cite chronology and membership sources, show corrected cross-series ordering, preserve protected universes, and explicitly report remaining unresearched/ambiguous books. Existing stored numbering or a successful synthetic import is not proof of curated chronology.
