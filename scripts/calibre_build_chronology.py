#!/usr/bin/env python3
"""Build private, source-cited universe mappings, not a copy of old series indices."""

import argparse
from collections import Counter, defaultdict
from contextlib import closing
import csv
import hashlib
import json
import os
from pathlib import Path
import re
import sqlite3
import sys
import unicodedata

from calibre_chronology import (
    FIELDS, INDEX, PROTECTED, SERIES, Refusal, build_report, connect_readonly,
    number, private_json, read_csv, snapshot, sync_directory,
)

DEFAULT_CATALOG = Path(__file__).resolve().parents[1] / "data/calibre-chronology/catalogs.json"


def normalize(value):
    value = unicodedata.normalize("NFKC", value).casefold()
    return re.sub(r"[^\w]+", " ", value, flags=re.UNICODE).strip()


def catalogs(path):
    document = json.loads(Path(path).read_text())
    result = []
    for entry in document["catalogs"]:
        if not entry["sources"] or any(not s.startswith("https://") for s in entry["sources"]):
            raise Refusal("Every catalog needs HTTPS chronology/membership citations")
        source_path = Path(path).parent / entry["file"]
        raw = source_path.read_bytes()
        works, names, counts = [], set(), Counter()
        reader = csv.DictReader(raw.decode("utf-8").splitlines(), delimiter="\t", strict=True)
        required = {"title", "subseries", "date", "status", "aliases", "note"}
        optional = {"authors", "position", "subseries3", "subseries4", "subposition",
                    "subposition3", "subposition4", "source"}
        if (not reader.fieldnames or not required.issubset(reader.fieldnames)
                or len(set(reader.fieldnames)) != len(reader.fieldnames)
                or set(reader.fieldnames) - required - optional):
            raise Refusal(f"Unexpected catalog columns: {source_path}")
        for position, work in enumerate(reader, 1):
            if None in work or any(v is None for v in work.values()):
                raise Refusal(f"Malformed catalog row {position}: {source_path}")
            if work["status"] not in ("ready", "held"):
                raise Refusal("Catalog status must be ready or held")
            aliases = {normalize(t) for t in [work["title"], *work["aliases"].split("|")] if t}
            identity_keys = {(key, normalize(work.get("authors", ""))) for key in aliases}
            if identity_keys & names:
                raise Refusal(f"Ambiguous catalog title: {work['title']}")
            names.update(identity_keys)
            hierarchy = [work["subseries"], work.get("subseries3", ""),
                         work.get("subseries4", "")]
            indices = []
            for depth, name in enumerate(hierarchy):
                key = tuple(hierarchy[:depth + 1])
                if name:
                    counts[key] += 1
                index_key = ("subposition", "subposition3", "subposition4")[depth]
                explicit = work.get(index_key)
                indices.append(number(explicit, index_key) if explicit else
                               counts[key] if name else None)
                if not name and explicit:
                    raise Refusal(f"Index without nested series: {work['title']}")
            explicit_position = work.get("position")
            work.update(
                position=number(explicit_position, "position") if explicit_position else position,
                subposition=indices[0], hierarchy=hierarchy, indices=indices, keys=aliases,
            )
            works.append(work)
        result.append({**entry, "works": works, "sha256": hashlib.sha256(raw).hexdigest()})
    return document, result


def context(row, catalog):
    labels = normalize(" ".join(row.get(s, "") for s in SERIES))
    author = normalize(row.get("authors", ""))
    return any(normalize(h) in labels for h in catalog["context_hints"]) or any(
        normalize(h) in author for h in catalog.get("author_hints", [])
    )


def construct(rows, entries):
    mappings, coverage, missing = [], [], []
    by_title = defaultdict(list)
    for book_id, row in rows.items():
        by_title[normalize(row["title"])].append((book_id, row))
    protected_uuids = {r["uuid"] for r in rows.values()
                       if any(r.get(s) in PROTECTED for s in SERIES)}
    decisions = defaultdict(list)
    for catalog in entries:
        for work in catalog["works"]:
            matches = [(i, r) for key in work["keys"] for i, r in by_title.get(key, [])
                       if context(r, catalog) and (
                           not work.get("authors") or any(normalize(author) in
                           normalize(r.get("authors", "")) for author in work["authors"].split("|"))
                       )]
            if not matches:
                missing.append({
                    "universe": catalog["universe"], "title": work["title"],
                    "position": work["position"],
                    "status": "not_matched_in_export_not_proven_absent",
                })
                continue
            for book_id, row in matches:
                decision = {"id": book_id, "uuid": row["uuid"], "title": row["title"],
                            "universe": catalog["universe"],
                            "source_title": work["title"], "date": work["date"],
                            "position": work["position"], "note": work["note"],
                            "sources": catalog["sources"], "catalog_sha256": catalog["sha256"]}
                if row["uuid"] in protected_uuids:
                    decision["status"] = "protected"
                elif len(matches) != 1:
                    decision["status"] = "held_duplicate_edition_or_work_identity"
                elif any(word in normalize(" ".join(row.get(s, "") for s in SERIES))
                         for word in ("comic", "graphic novel", "sourcebook", "roleplaying")):
                    decision["status"] = "held_edition_or_format"
                elif work["status"] == "held":
                    decision["status"] = "held_source_or_content_ambiguity"
                else:
                    decision["status"] = "source_mapped"
                    target = {s: "" for s in SERIES}
                    target.update({INDEX[s]: "" for s in SERIES})
                    target.update({
                        "series": catalog["universe"],
                        "series_index": str(work["position"]),
                    })
                    for field, name, rank in zip(SERIES[1:], work["hierarchy"], work["indices"]):
                        target[field] = name
                        target[INDEX[field]] = str(rank) if name else ""
                    decision["mapping"] = {
                        "id": str(book_id), "uuid": row["uuid"], "title": row["title"],
                        "authors": row.get("authors", ""), **target,
                        "order_verified": "yes", "order_basis": "in-universe",
                        "order_source": " | ".join(catalog["sources"]) +
                        (f" | per-work={work['source']}" if work.get("source") else "") +
                        f" | work={work['title']}; date={work['date']}; "
                        f"catalog-position={work['position']}; {work['note']}",
                    }
                decisions[book_id].append(decision)
    for book_id, row in rows.items():
        candidates = decisions.get(book_id, [])
        if len(candidates) > 1:
            for decision in candidates:
                decision["status"] = "held_cross_universe_title_collision"
                decision.pop("mapping", None)
        if candidates:
            for decision in candidates:
                mapping = decision.pop("mapping", None)
                if mapping:
                    mappings.append(mapping)
                coverage.append(decision)
        else:
            coverage.append({
                "id": book_id, "uuid": row["uuid"], "title": row["title"],
                "status": "protected" if row["uuid"] in protected_uuids else "needs_research",
                "existing_series": {s: row.get(s, "") for s in SERIES},
            })
    return mappings, coverage, missing


def private_csv(path, rows, headers):
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8-sig", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=headers)
        writer.writeheader()
        writer.writerows(rows)
        stream.flush()
        os.fsync(stream.fileno())
    sync_directory(Path(path).parent)


def curator_inventory(path):
    """Inspect existing evidence only; never approve an old application automatically."""
    if path is None:
        return None
    with closing(connect_readonly(path)) as conn:
        record = conn.execute("SELECT value FROM state WHERE key='app'").fetchone()
        state = json.loads(record[0])
    sources = {s["id"]: s for s in state.get("sources", [])}
    references = []
    for rule in state.get("references", []):
        source = sources.get(rule.get("source_id"), {})
        references.append({
            "title": rule["title"], "book_uuids": rule.get("book_uuids", []),
            "approved": rule.get("approved", False),
            "hierarchy": rule.get("hierarchy"), "coverage": rule.get("coverage"),
            "evidence": rule.get("evidence"), "source_name": source.get("name"),
            "source_kind": source.get("kind"), "source_location": source.get("location"),
            "status": "existing_evidence_requires_scope_review",
        })
    return {"references": references, "sources": len(sources),
            "meaning": "Existing approvals retained as research inputs, not newly certified"}


def build(csv_path, library, baseline, catalog_path, output, curator_state=None, documents=None):
    rows, original_hash = read_csv(csv_path)
    document, entries = catalogs(catalog_path)
    mappings, coverage, missing = construct(rows, entries)
    # Current membership also protects books whose original export was misclassified.
    current = snapshot(library)["books"]
    current_protected = {
        i for i, r in current.items() if any(r.get(s) in PROTECTED for s in SERIES)
    }
    mappings = [r for r in mappings if int(r["id"]) not in current_protected]
    for record in coverage:
        if record["id"] in current_protected:
            record["status"] = "protected"
    output = Path(output)
    lib = Path(library).resolve()
    if output.resolve() == lib or lib in output.resolve().parents:
        raise Refusal("Mapping outputs must be outside the library")
    output.mkdir(mode=0o700, parents=True, exist_ok=False)
    headers = ["id", "uuid", "title", "authors", *FIELDS,
               "order_verified", "order_basis", "order_source"]
    mapping_path = output / "source_mapped_chronology.csv"
    private_csv(mapping_path, mappings, headers)
    audit = build_report(library, mapping_path, baseline) if mappings else None
    if audit:
        private_json(output / "importer-dry-run.json", audit)
    # A mapping with a collision against an unresearched library record is not apply-ready.
    counts = dict(Counter(r["status"] for r in coverage))
    summary = {
        "version": 1, "input_sha256": original_hash, "input_rows": len(rows),
        "source_catalogs": [{"universe": c["universe"], "works": len(c["works"]),
                             "sha256": c["sha256"], "limitations": c["limitations"],
                             "sources": c["sources"]} for c in entries],
        "policy": document["policy"], "coverage_counts": counts,
        "mapped_rows": len(mappings), "full_library_complete": False,
        "importer_errors": audit["errors"] if audit else [],
        "apply_ready": bool(audit and not audit["errors"]),
        "mapping_csv": str(mapping_path.resolve()), "library_modified": False,
    }
    private_json(output / "coverage.json", coverage)
    private_json(output / "source_works_not_matched.json", missing)
    private_json(output / "curator-evidence-inventory.json", curator_inventory(curator_state))
    if documents:
        from calibre_chronology_sources import local_documents
        summary["local_sources"] = local_documents(documents, output, entries)
    private_json(output / "summary.json", summary)
    return summary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--csv", type=Path, required=True)
    parser.add_argument("--library", type=Path, required=True)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--catalog", type=Path, default=DEFAULT_CATALOG)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--curator-state", type=Path)
    parser.add_argument("--documents", type=Path, action="append",
                        help="Local chronology reference directory; repeat for multiple roots")
    args = parser.parse_args()
    try:
        result = build(args.csv, args.library, args.baseline, args.catalog,
                       args.output_dir, args.curator_state, args.documents)
        print(json.dumps(result, indent=2))
        return 0 if result["apply_ready"] else 2
    except (Refusal, OSError, ValueError, csv.Error, sqlite3.Error) as exc:
        print(f"Refused: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
