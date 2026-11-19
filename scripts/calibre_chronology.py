#!/usr/bin/env python3
"""Audit reviewed chronology CSVs; apply using calibre-debug, never SQL updates."""

import argparse
from collections import defaultdict
from contextlib import closing, contextmanager
import csv
from decimal import Decimal, InvalidOperation
import fcntl
import hashlib
import io
import json
import math
import os
from pathlib import Path
import sqlite3
import sys
import tempfile
import time
import uuid


PROTECTED = frozenset({
    "Greater Eternal Champion Saga", "Greater Foundation Universe",
    "The 39 Clues", "The Grand Tour",
})
SERIES = ("series", "#series2", "#series3", "#series4")
INDEX = {s: "series_index" if s == "series" else s + "_index" for s in SERIES}
FIELDS = tuple(f for s in SERIES for f in (s, INDEX[s]))


class Refusal(ValueError):
    """Input or runtime state does not meet the safety contract."""


def digest(value):
    return hashlib.sha256(json.dumps(
        value, sort_keys=True, ensure_ascii=False, separators=(",", ":"),
        allow_nan=False,
    ).encode()).hexdigest()


def read_csv(path):
    raw = Path(path).read_bytes()
    reader = csv.DictReader(io.StringIO(raw.decode("utf-8-sig"), newline=""), strict=True)
    headers = reader.fieldnames
    if not headers or any(not h or h != h.strip() for h in headers):
        raise Refusal("CSV has empty or whitespace-padded headers")
    if len(set(headers)) != len(headers):
        raise Refusal("CSV has duplicate headers")
    if not {"id", "uuid"}.issubset(headers):
        raise Refusal("CSV must contain id and uuid identity columns")
    rows = {}
    for row in reader:
        if None in row or any(v is None for v in row.values()):
            raise Refusal(f"Malformed CSV record near line {reader.line_num}")
        if not row["id"].isascii() or not row["id"].isdigit() or int(row["id"]) < 1:
            raise Refusal(f"Invalid book id near line {reader.line_num}")
        book_id = int(row["id"])
        if book_id in rows:
            raise Refusal(f"Duplicate book id: {book_id}")
        try:
            if str(uuid.UUID(row["uuid"])) != row["uuid"].lower():
                raise ValueError()
        except ValueError as exc:
            raise Refusal(f"Invalid UUID for book {book_id}") from exc
        rows[book_id] = row
    if not rows:
        raise Refusal("CSV contains no books")
    return rows, hashlib.sha256(raw).hexdigest()


def number(text, context):
    try:
        n = Decimal(text)
        f = float(n)
    except (ValueError, InvalidOperation, OverflowError) as exc:
        raise Refusal(f"{context}: expected a finite numeric index") from exc
    if not n.is_finite() or not math.isfinite(f):
        raise Refusal(f"{context}: expected a finite numeric index")
    # Calibre stores IEEE doubles; refuse silent rounding and underflow.
    if Decimal(str(f)) != n:
        raise Refusal(f"{context}: index loses precision in Calibre")
    return f


def connect_readonly(path):
    return sqlite3.connect(Path(path).resolve().as_uri() + "?mode=ro", uri=True)


def snapshot(library):
    """Read a transactionally consistent normalized database without loading Calibre."""
    path = Path(library) / "metadata.db"
    if not path.is_file():
        raise Refusal(f"No existing metadata.db in {library}")
    with closing(connect_readonly(path)) as conn:
        conn.execute("BEGIN")
        if conn.execute("PRAGMA quick_check").fetchall() != [("ok",)]:
            raise Refusal("Database quick_check failed")
        books = {}
        for book_id, ident, title, rank in conn.execute(
            "SELECT id, uuid, title, series_index FROM books"
        ):
            books[book_id] = {"uuid": ident, "title": title, **{
                f: (None if f in SERIES else 1.0) for f in FIELDS
            }}
            books[book_id]["series_index"] = rank
        for book_id, name in conn.execute(
            "SELECT l.book, s.name FROM books_series_link l JOIN series s ON s.id=l.series"
        ):
            books[book_id]["series"] = name
        columns = {}
        definitions = conn.execute(
            "SELECT id,label,datatype,normalized,mark_for_delete FROM custom_columns"
        ).fetchall()
        for col_id, label, datatype, normalized, deleted in definitions:
            name = "#" + label
            if name not in SERIES:
                continue
            if datatype != "series" or not normalized or deleted:
                raise Refusal(f"{name} must be an active normalized series column")
            columns[name] = col_id
            # col_id comes only from SQLite's integer primary key, never CSV input.
            col_id = int(col_id)
            for book_id, value, extra in conn.execute(
                f"SELECT l.book,v.value,l.extra FROM books_custom_column_{col_id}_link l "
                f"JOIN custom_column_{col_id} v ON v.id=l.value"
            ):
                books[book_id][name] = value
                books[book_id][INDEX[name]] = extra
        # Bind approval to *all* logical DB contents, including metadata not edited here.
        logical = hashlib.sha256()
        tables = conn.execute(
            "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name"
        ).fetchall()
        for (table,) in tables:
            quoted = '"' + table.replace('"', '""') + '"'
            data = conn.execute(f"SELECT * FROM {quoted}").fetchall()
            encoded = sorted(json.dumps(r, default=lambda b: {"blob": bytes(b).hex()},
                                        ensure_ascii=False) for r in data)
            logical.update(json.dumps([table, encoded], ensure_ascii=False).encode())
        return {"books": books, "columns": columns, "db_digest": logical.hexdigest()}


def protected_ids(baseline, current, incoming):
    protected_uuids = {
        r["uuid"].lower() for r in baseline.values()
        if any(r.get(s) in PROTECTED for s in SERIES)
    }
    for book_id, row in baseline.items():
        if book_id in current and current[book_id]["uuid"].lower() != row["uuid"].lower():
            raise Refusal(f"Original-export identity mismatch for book {book_id}")
    return {
        book_id for book_id, row in current.items()
        if row["uuid"].lower() in protected_uuids
        or any(row.get(s) in PROTECTED for s in SERIES)
        or any(incoming.get(book_id, {}).get(s) in PROTECTED for s in SERIES)
    }


def build_report(library, csv_path, baseline_path):
    rows, csv_hash = read_csv(csv_path)
    baseline, baseline_hash = read_csv(baseline_path)
    state = snapshot(library)
    current = state["books"]
    for header in next(iter(rows.values())):
        if (header.startswith("#series") or header.startswith("series_index")) and header not in FIELDS:
            raise Refusal(f"Unknown series field: {header}")
    protected = protected_ids(baseline, current, rows)
    report = {
        "version": 1, "library": str(Path(library).resolve()),
        "csv_sha256": csv_hash, "baseline_sha256": baseline_hash,
        "db_digest": state["db_digest"], "rows": len(rows),
        "library_books": len(current), "protected_books": len(protected),
        "unlisted_books": len(set(current) - rows.keys()),
        "errors": [], "changes": [], "protected_discrepancies": [],
    }
    errors = report["errors"]
    final = {i: dict(r) for i, r in current.items()}
    changed_ids = set()
    for book_id, row in rows.items():
        if book_id not in current:
            errors.append(f"{book_id}: unknown book ID")
            continue
        old = current[book_id]
        if old["uuid"].lower() != row["uuid"].lower():
            errors.append(f"{book_id}: UUID does not match library")
            continue
        if book_id in protected:
            differences = {
                s: {"original": baseline.get(book_id, {}).get(s), "current": old.get(s)}
                for s in ("series", "#series2", "#series3", "#series4")
                if s in baseline.get(book_id, {}) and
                (baseline[book_id][s] or None) != old.get(s)
            }
            original_rank = baseline.get(book_id, {}).get("series_index")
            if original_rank and number(original_rank, f"{book_id} baseline") != old["series_index"]:
                differences["series_index"] = {
                    "original": original_rank, "current": old["series_index"],
                }
            if differences:
                report["protected_discrepancies"].append({"id": book_id, "fields": differences})
            continue
        proposed = dict(old)
        try:
            if "update_fields" in row:
                selected = row["update_fields"].split(";")
                if (not selected or any(not f or f not in FIELDS or f not in row for f in selected)
                        or len(set(selected)) != len(selected)):
                    raise Refusal(f"{book_id}: invalid update_fields mask")
                for name in SERIES:
                    pair = {name, INDEX[name]}
                    if pair.intersection(selected) and not pair.issubset(selected):
                        raise Refusal(f"{book_id}: update_fields must include each name/index pair")
                row = {k: v for k, v in row.items() if k not in FIELDS or k in selected}
            for name in SERIES:
                idx = INDEX[name]
                if name not in row and idx not in row:
                    continue
                if name != "series" and name not in state["columns"]:
                    raise Refusal(f"{book_id}: custom series column {name} does not exist")
                if name in row:
                    text = row[name]
                    if text != text.strip() or any(ord(c) < 32 for c in text):
                        raise Refusal(f"{book_id}: {name} has whitespace/control characters")
                    proposed[name] = text or None
                if proposed[name]:
                    if idx not in row or not row[idx].strip():
                        raise Refusal(f"{book_id}: populated {name} requires explicit {idx}")
                    proposed[idx] = number(row[idx], f"{book_id} {idx}")
                else:
                    if row.get(idx, "").strip():
                        raise Refusal(f"{book_id}: {idx} supplied without a series name")
                    # Clearing a series has a defined neutral index, not an empty float.
                    proposed[idx] = 1.0
            delta = {f: {"before": old[f], "after": proposed[f]}
                     for f in FIELDS if old[f] != proposed[f]}
            if delta:
                if row.get("order_verified") != "yes" or not row.get("order_source", "").strip():
                    raise Refusal(f"{book_id}: changes require order_verified=yes and order_source")
                basis = row.get("order_basis")
                if basis not in ("in-universe", "publication", "standalone"):
                    raise Refusal(f"{book_id}: invalid or missing order_basis")
                if basis == "publication" and "Hard Case Crime" not in (
                    proposed[s] for s in SERIES
                ):
                    raise Refusal(f"{book_id}: publication ordering allowed only for Hard Case Crime")
                if basis == "standalone" and any(proposed[s] for s in SERIES):
                    raise Refusal(f"{book_id}: standalone basis requires all series blank")
                report["changes"].append({
                    "id": book_id, "uuid": old["uuid"], "fields": delta,
                    "basis": basis, "source": row["order_source"],
                })
                final[book_id] = proposed
                changed_ids.add(book_id)
        except Refusal as exc:
            errors.append(str(exc))
    # Check resulting ranks against *all* library books, not just CSV rows.
    for depth, name in enumerate(SERIES):
        groups = defaultdict(list)
        for book_id, row in final.items():
            if row[name]:
                key = tuple(row[s] for s in SERIES[:depth + 1])
                groups[(key, row[INDEX[name]])].append(book_id)
        for (key, rank), ids in groups.items():
            if len(ids) < 2 or not changed_ids.intersection(ids):
                continue
            acknowledged = all(
                name in rows.get(i, {}).get("allow_duplicate_indices", "").split(";")
                and rows.get(i, {}).get("order_verified") == "yes"
                and rows.get(i, {}).get("order_source", "").strip()
                for i in ids
            )
            if not acknowledged:
                errors.append(f"Duplicate {name} index {rank} in {key}: IDs {ids}; "
                              "explicit per-book exception required")
    report["approval"] = digest(report)
    return report


def private_json(path, value):
    """Exclusive creation prevents replacing an old audit, backup or journal."""
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8") as f:
        json.dump(value, f, indent=2, ensure_ascii=False, allow_nan=False)
        f.write("\n")
        f.flush()
        os.fsync(f.fileno())
    sync_directory(Path(path).parent)


def sync_directory(path):
    fd = os.open(path, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def metadata_guard(library, changed_ids):
    """Fingerprint everything outside the explicitly editable series semantics.

    Design reference: calibre-curator's unrelated_hashes/category_payloads guards.
    """
    with closing(connect_readonly(Path(library) / "metadata.db")) as conn:
        conn.row_factory = sqlite3.Row
        conn.execute("BEGIN")
        series_tables = {"series", "books_series_link"}
        category_tables = {"series": "name"}
        for row in conn.execute("SELECT id,label,datatype FROM custom_columns"):
            if "#" + row["label"] in SERIES and row["datatype"] == "series":
                col = int(row["id"])
                series_tables.update({f"custom_column_{col}", f"books_custom_column_{col}_link"})
                category_tables[f"custom_column_{col}"] = "value"
        unrelated = {}
        for (table,) in conn.execute("SELECT name FROM sqlite_master WHERE type='table'"):
            if table in series_tables or table == "metadata_dirtied":
                continue
            quoted = '"' + table.replace('"', '""') + '"'
            records = []
            for raw in conn.execute(f"SELECT * FROM {quoted}"):
                row = dict(raw)
                if table == "sqlite_sequence" and row.get("name") in series_tables:
                    continue
                if table == "books" and row["id"] in changed_ids:
                    row.pop("series_index", None)
                    row.pop("last_modified", None)
                # Native Calibre housekeeping, not a user preference.
                if table == "preferences" and row.get("key") == "last_expired_trash_at":
                    continue
                records.append(json.dumps(row, sort_keys=True, default=repr))
            unrelated[table] = digest(sorted(records))
        categories = {}
        for table, name_field in category_tables.items():
            categories[table] = {}
            for raw in conn.execute(f"SELECT * FROM {table}"):
                row = dict(raw)
                name = row.pop(name_field)
                row.pop("id")
                categories[table][name] = row
        return {"unrelated": unrelated, "categories": categories}


def verify_guard(library, changed_ids, before):
    after = metadata_guard(library, changed_ids)
    if after["unrelated"] != before["unrelated"]:
        raise Refusal("Unrelated metadata or non-target book metadata changed")
    for table, names in before["categories"].items():
        for name, payload in names.items():
            actual = after["categories"][table].get(name)
            if actual is not None and actual != payload:
                raise Refusal("Existing series category metadata changed")
            if actual is None and any(value for key, value in payload.items() if key != "sort"):
                raise Refusal("A series category with nonempty metadata was removed")


def backup_database(source, destination):
    fd = os.open(destination, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    os.close(fd)
    with closing(connect_readonly(source)) as src, closing(sqlite3.connect(destination)) as dst:
        src.backup(dst)
        if dst.execute("PRAGMA quick_check").fetchall() != [("ok",)]:
            raise Refusal("Backup quick_check failed")
    os.chmod(destination, 0o600)
    with open(destination, "rb") as f:
        os.fsync(f.fileno())


def refuse_other_calibre():
    """Conservative Linux process guard; never stop processes on the user's behalf."""
    own = os.getpid()
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit() or int(entry.name) == own:
            continue
        try:
            comm = (entry / "comm").read_text().strip()
            status = dict(line.split(":", 1) for line in
                          (entry / "status").read_text().splitlines() if ":" in line)
            if status.get("State", "").strip().startswith(("Z", "X")):
                continue
            # Cache initialization can spawn calibre-parallel workers of our own.
            # Exempt only that executable and only a proven descendant of this PID.
            if comm == "calibre-paralle":
                parent = int(status["PPid"])
                visited = set()
                while parent > 1 and parent not in visited and parent != own:
                    visited.add(parent)
                    ancestor = Path("/proc") / str(parent) / "status"
                    parent_status = dict(line.split(":", 1) for line in
                                         ancestor.read_text().splitlines() if ":" in line)
                    parent = int(parent_status["PPid"])
                if parent == own:
                    continue
        except FileNotFoundError:
            continue
        except (PermissionError, KeyError, ValueError) as exc:
            raise Refusal("Cannot inspect processes; refusing offline update") from exc
        if comm in {"calibre", "calibre-paralle", "calibre-server", "calibredb",
                    "calibre-debug", "ebook-edit"}:
            raise Refusal(f"Close other Calibre processes first (PID {entry.name}, {comm})")


@contextmanager
def offline_guard(library):
    """Coordinate our invocations and block the normal GUI single-instance path."""
    from calibre.utils.lock import SingleInstance

    lock_path = Path(library) / ".chronology-import.lock"
    with open(lock_path, "a", encoding="utf-8") as lock:
        os.chmod(lock_path, 0o600)
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as exc:
            raise Refusal("Another chronology import owns this library") from exc
        # Calibre's own CLI DBCtx uses the shared 'db' single-instance lock.
        # Keep it for the whole operation, including backups and rehearsal.
        with SingleInstance("GUI") as gui, SingleInstance("db") as database:
            if not gui or not database:
                raise Refusal("Calibre GUI or database writer lock is already held")
            refuse_other_calibre()
            yield


def native_write(library, changes):
    # Imported only on explicit apply, inside calibre-debug.
    from calibre.db.backend import DB
    from calibre.db.cache import Cache

    backend = DB(str(library), load_user_formatter_functions=False)
    cache = Cache(backend)
    try:
        cache.init()
        retained_indices = {
            (c["id"], INDEX[s]): cache.field_for(INDEX[s], c["id"])
            for c in changes for s in SERIES
            if s in c["fields"] and INDEX[s] not in c["fields"]
        }
        # Names first; a series assignment may reset its index internally.
        for field in (*SERIES, *(INDEX[s] for s in SERIES)):
            values = {
                c["id"]: c["fields"][field]["after"]
                for c in changes if field in c["fields"]
            }
            # Reassert indices even when only the series name changes.
            if field in INDEX.values():
                name = next(s for s in SERIES if INDEX[s] == field)
                for change in changes:
                    if name in change["fields"] and change["id"] not in values:
                        values[change["id"]] = retained_indices[(change["id"], field)]
            if values:
                cache.set_field(field, values, allow_case_change=False)
    finally:
        cache.close()


def expected_books(before, changes):
    expected = {i: dict(r) for i, r in before.items()}
    for change in changes:
        for field, delta in change["fields"].items():
            expected[change["id"]][field] = delta["after"]
    return expected


def verify(library, expected):
    actual = snapshot(library)["books"]
    if actual != expected:
        ids = [i for i in set(actual) | set(expected) if actual.get(i) != expected.get(i)]
        raise Refusal(f"Readback differs from reviewed values (IDs {ids[:20]})")


def apply_report(report, library, csv_path, baseline_path, approval, backup_dir):
    if report["errors"]:
        raise Refusal("Validation errors must be resolved before apply")
    if approval != report["approval"]:
        raise Refusal("Approval does not match this exact input/library state")
    if not report["changes"]:
        return {"status": "no_changes", "seconds": 0}
    start = time.monotonic()
    with offline_guard(library):
        incomplete = Path(library) / ".chronology-incomplete.json"
        if incomplete.exists():
            raise Refusal(f"An earlier apply needs recovery review: {incomplete}")
        latest = build_report(library, csv_path, baseline_path)
        if latest != report:
            raise Refusal("Inputs or database changed after review")
        before = snapshot(library)["books"]
        expected = expected_books(before, report["changes"])
        changed_ids = {c["id"] for c in report["changes"]}
        guard = metadata_guard(library, changed_ids)
        # Validate API effects before any live mutation. Temporary directory is private.
        with tempfile.TemporaryDirectory(prefix="calibre-chronology-rehearsal-") as tmp:
            backup_database(Path(library) / "metadata.db", Path(tmp) / "metadata.db")
            native_write(tmp, report["changes"])
            verify(tmp, expected)
            verify_guard(tmp, changed_ids, guard)
        refuse_other_calibre()
        if build_report(library, csv_path, baseline_path) != report:
            raise Refusal("Inputs or database changed during rehearsal")
        parent = Path(backup_dir).resolve()
        lib = Path(library).resolve()
        if parent == lib or lib in parent.parents:
            raise Refusal("Backup directory must be outside the library")
        parent.mkdir(parents=True, exist_ok=True)
        run = parent / (time.strftime("%Y%m%dT%H%M%SZ", time.gmtime()) + "-" + uuid.uuid4().hex)
        run.mkdir(mode=0o700)
        backup_database(lib / "metadata.db", run / "metadata.db")
        if snapshot(run)["db_digest"] != report["db_digest"]:
            raise Refusal("Database changed while making the backup")
        private_json(run / "before.json", {
            "library": str(lib), "approval": approval, "books": before,
            "report": report,
        })
        # A durable latch remains if interrupted between independently committed API calls.
        private_json(run / "apply-started.json", {"approval": approval})
        if build_report(library, csv_path, baseline_path) != report:
            raise Refusal("Inputs or database changed while preparing recovery records")
        private_json(incomplete, {"approval": approval, "recovery": str(run)})
        try:
            native_write(lib, report["changes"])
            verify(lib, expected)
            verify_guard(lib, changed_ids, guard)
        except BaseException:
            print(f"Apply incomplete; preserve recovery records: {run}", file=sys.stderr)
            raise
        private_json(run / "verified.json", {
            "approval": approval, "changed_books": len(report["changes"]),
            "seconds": time.monotonic() - start,
        })
        incomplete.unlink()
        sync_directory(lib)
        return {"status": "verified", "backup": str(run),
                "changed_books": len(report["changes"]),
                "seconds": time.monotonic() - start}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", required=True, type=Path)
    parser.add_argument("--csv", required=True, type=Path)
    parser.add_argument("--baseline", required=True, type=Path,
                        help="Original pre-import Calibre export, including UUIDs")
    parser.add_argument("--report", required=True, type=Path,
                        help="New private JSON report; existing files are never overwritten")
    parser.add_argument("--apply", action="store_true")
    parser.add_argument("--approve", help="Exact approval digest from the reviewed dry-run")
    parser.add_argument("--backup-dir", type=Path)
    args = parser.parse_args(argv)
    start = time.monotonic()
    try:
        if args.apply and (not args.approve or not args.backup_dir):
            raise Refusal("--apply requires --approve and --backup-dir")
        if args.apply:
            try:
                import calibre  # noqa: F401
            except ImportError as exc:
                raise Refusal("Apply must be run with calibre-debug -e, not plain Python") from exc
        report = build_report(args.library, args.csv, args.baseline)
        private_json(args.report, report)
        print(json.dumps({
            "mode": "apply" if args.apply else "dry-run",
            "rows": report["rows"], "changes": len(report["changes"]),
            "protected": report["protected_books"], "errors": len(report["errors"]),
            "approval": report["approval"], "report": str(args.report),
            "audit_seconds": round(time.monotonic() - start, 3),
        }))
        if report["errors"]:
            print("Validation failed; no library writes. Review the report.", file=sys.stderr)
            return 2
        if args.apply:
            result = apply_report(report, args.library, args.csv, args.baseline,
                                  args.approve, args.backup_dir)
            print(json.dumps(result))
        return 0
    except (Refusal, OSError, UnicodeError, csv.Error, sqlite3.Error) as exc:
        print(f"Refused: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
