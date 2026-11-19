#!/usr/bin/env python3
"""Enrich an original CSV with stored sub-series indices, without approving chronology."""

import argparse
from collections import Counter
from contextlib import closing
import csv
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import sys

from calibre_chronology import (
    INDEX, SERIES, Refusal, connect_readonly, number, private_json, read_csv,
    sync_directory,
)


def file_record(path):
    path = Path(path)
    if not path.exists():
        return None
    before = path.stat()
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(chunk)
    after = path.stat()
    identity = lambda s: (s.st_dev, s.st_ino, s.st_size, s.st_mtime_ns)
    if identity(before) != identity(after):
        raise Refusal(f"Source changed while hashing: {path}")
    return {"sha256": value.hexdigest(), "identity": identity(after)}


def database_values(path):
    """Only data from a consistent read-only SQLite transaction is accepted."""
    with closing(connect_readonly(path)) as conn:
        conn.execute("BEGIN")
        if conn.execute("PRAGMA quick_check").fetchall() != [("ok",)]:
            raise Refusal(f"Database integrity check failed: {path}")
        identities = dict(conn.execute("SELECT id,uuid FROM books"))
        fields = {}
        for col, label, kind, normalized, deleted in conn.execute(
            "SELECT id,label,datatype,normalized,mark_for_delete FROM custom_columns"
        ):
            name = "#" + label
            if name not in SERIES[1:]:
                continue
            if kind != "series" or not normalized or deleted:
                raise Refusal(f"Incompatible custom series column {name}")
            col = int(col)
            fields[name] = {
                book_id: (text, rank)
                for book_id, text, rank in conn.execute(
                    f"SELECT l.book,v.value,l.extra FROM books_custom_column_{col}_link l "
                    f"JOIN custom_column_{col} v ON v.id=l.value"
                )
            }
        if set(fields) != set(SERIES[1:]):
            raise Refusal("Database must define #series2, #series3 and #series4")
        return identities, fields


def canonical_name(name):
    # Deliberately no prefix stripping, case folding, punctuation or title guessing.
    return " ".join(name.split())


def enrich(rows, primary, comparison=None):
    identities, values = primary
    output, issues = [], []
    counts = {s: Counter() for s in SERIES[1:]}
    for book_id, original in rows.items():
        for source in (primary, comparison):
            if source is not None and source[0].get(book_id, "").lower() != original["uuid"].lower():
                raise Refusal(f"Missing or mismatched database identity for book {book_id}")
        row = dict(original)
        if "order_verified" in row:
            row["order_verified"] = ""  # Extraction never carries forward approval.
        for name in SERIES[1:]:
            idx = INDEX[name]
            target = original.get(name, "")
            row[idx] = ""
            match = "not_applicable"
            if target:
                counts[name]["populated"] += 1
                pair = values[name].get(book_id)
                other = comparison[1][name].get(book_id) if comparison else None
                reason = None
                if not pair or pair[1] is None:
                    reason = "missing_stored_index"
                elif canonical_name(pair[0]) != canonical_name(target):
                    reason = "series_name_conflict"
                elif comparison and (
                    not other or canonical_name(other[0]) != canonical_name(pair[0])
                    or other[1] != pair[1]
                ):
                    reason = "database_backup_conflict"
                else:
                    try:
                        rank = number(str(pair[1]), f"{book_id} {idx}")
                        if original.get(idx, "").strip() and number(original[idx], idx) != rank:
                            reason = "existing_csv_index_conflict"
                    except Refusal:
                        reason = "invalid_stored_or_existing_index"
                if reason:
                    match = "unresolved"
                    counts[name]["unresolved"] += 1
                    issues.append({
                        "id": book_id, "uuid": original["uuid"], "field": name,
                        "reason": reason, "original_name": target,
                        "database_value": pair, "comparison_value": other,
                    })
                else:
                    row[idx] = str(pair[1])
                    match = "exact" if pair[0] == target else "whitespace_equivalent"
                    counts[name]["extracted"] += 1
                    counts[name][match] += 1
                    counts[name]["index_equals_one"] += rank == 1
            row["index_match_" + name[1:]] = match
        output.append(row)
    return output, {name: dict(count) for name, count in counts.items()}, issues


def extract(csv_path, database, compare_database, output_dir):
    csv_path, database, output_dir = map(Path, (csv_path, database, output_dir))
    databases = [database] + ([Path(compare_database)] if compare_database else [])
    for db in databases:
        if output_dir.resolve() == db.parent.resolve() or db.parent.resolve() in output_dir.resolve().parents:
            raise Refusal("Extraction output must be outside database/library directories")
    sources = [csv_path, *databases]
    for db in databases:
        sources.extend(Path(str(db) + suffix) for suffix in ("-wal", "-journal"))
    before = {str(p.resolve()): file_record(p) for p in sources}
    rows, csv_hash = read_csv(csv_path)
    for field in SERIES[1:]:
        if field not in next(iter(rows.values())):
            raise Refusal(f"Original CSV is missing {field}")
    primary = database_values(database)
    comparison = database_values(compare_database) if compare_database else None
    enriched, counts, issues = enrich(rows, primary, comparison)
    after = {str(p.resolve()): file_record(p) for p in sources}
    if before != after:
        raise Refusal("Source CSV/database changed during extraction; no output published")
    output_dir.mkdir(mode=0o700, parents=True, exist_ok=False)
    destination = output_dir / "all_books_with_subseries_indices.csv"
    headers = list(next(iter(rows.values())))
    for row in enriched:
        for field in row:
            if field not in headers:
                headers.append(field)
    fd = os.open(destination, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8-sig", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=headers)
        writer.writeheader()
        writer.writerows(enriched)
        stream.flush()
        os.fsync(stream.fileno())
    # Independently reparse and verify every retained original field and new index.
    reread, output_hash = read_csv(destination)
    for expected in enriched:
        if reread[int(expected["id"])] != expected:
            raise Refusal("CSV readback failed; output is not certified")
    report = {
        "status": "complete" if not issues else "needs_review",
        "csv": str(csv_path.resolve()), "input_csv_sha256": csv_hash,
        "database": str(database.resolve()),
        "comparison_database": str(Path(compare_database).resolve()) if compare_database else None,
        "rows": len(rows), "counts": counts, "issues": issues,
        "sources_unchanged": True, "source_records": before,
        "output": str(destination.resolve()), "output_sha256": output_hash,
        "chronology_verified": False,
        "meaning": "Existing database numbering, not newly researched chronology",
    }
    private_json(output_dir / "extraction-report.json", report)
    sync_directory(output_dir)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--csv", required=True, type=Path)
    parser.add_argument("--database", required=True, type=Path)
    parser.add_argument("--compare-database", type=Path)
    parser.add_argument("--output-dir", required=True, type=Path,
                        help="New private output directory, outside the library")
    args = parser.parse_args()
    try:
        report = extract(args.csv, args.database, args.compare_database, args.output_dir)
        print(json.dumps({k: report[k] for k in (
            "status", "rows", "counts", "sources_unchanged", "output", "chronology_verified",
        )}, indent=2))
        return 0 if not report["issues"] else 2
    except (Refusal, OSError, UnicodeError, csv.Error, sqlite3.Error) as exc:
        print(f"Refused: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
