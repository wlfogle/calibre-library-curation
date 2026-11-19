"""Run with calibre-debug -e; creates only a new synthetic disposable library."""

import csv
import importlib.util
import json
from pathlib import Path
import tempfile
import time


def run():
    from calibre.db.backend import DB
    from calibre.db.cache import Cache
    from calibre.ebooks.metadata.book.base import Metadata

    spec = importlib.util.spec_from_file_location(
        "chronology_benchmark_importer",
        Path(__file__).resolve().parents[1] / "scripts/calibre_chronology.py",
    )
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    # Explicit /tmp avoids Calibre's automatically cleaned process temp directory.
    root = Path(tempfile.mkdtemp(prefix="nexus-calibre-benchmark-", dir="/tmp"))
    library = root / "library"
    library.mkdir()
    cache = Cache(DB(str(library), load_user_formatter_functions=False))
    cache.init()
    for name in ("series2", "series3", "series4"):
        cache.create_custom_column(name, name, "series", False)
    cache.close()
    cache = Cache(DB(str(library), load_user_formatter_functions=False))
    cache.init()
    # Fixture generation only: avoid thousands of durable commits for fake books.
    # A newly reopened native connection restores normal durability before timing.
    cache.backend.conn.execute("PRAGMA synchronous=OFF")
    ids = []
    for start in range(0, 8524, 500):
        created, duplicates = cache.add_books([
            (Metadata(f"Synthetic performance book {i:05}", ["Synthetic Author"]), {})
            for i in range(start, min(start + 500, 8524))
        ], run_hooks=False)
        assert not duplicates
        ids.extend(created)
        print(f"Synthetic fixture: {len(ids)}/8524 books", flush=True)
    assert len(ids) == 8524
    protected = {}
    offset = 0
    for name, count in (
        ("Greater Eternal Champion Saga", 94), ("Greater Foundation Universe", 56),
        ("The 39 Clues", 41), ("The Grand Tour", 27),
    ):
        for rank, book_id in enumerate(ids[offset:offset + count], 1):
            protected[book_id] = (name, rank)
        offset += count
    cache.set_field("series", {i: name for i, (name, rank) in protected.items()})
    cache.set_field("series_index", {i: rank for i, (name, rank) in protected.items()})
    cache.close()
    print("Fixture complete; timing audit and guarded apply", flush=True)
    before = m.snapshot(library)["books"]
    baseline = root / "original.csv"
    incoming = root / "reviewed-synthetic.csv"
    with baseline.open("w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=["id", *next(iter(before.values()))])
        w.writeheader()
        w.writerows({"id": i, **b} for i, b in before.items())
    rows = []
    independent_expected = {i: dict(b) for i, b in before.items()}
    for n, book_id in enumerate(ids[:-1], 1):
        row = {"id": book_id, "uuid": before[book_id]["uuid"],
               "order_verified": "yes", "order_basis": "in-universe",
               "order_source": "Synthetic performance fixture, NOT real book chronology"}
        values = {
            "series": "Synthetic Chronology", "series_index": float(n),
            "#series2": f"Synthetic Sub {(n - 1) // 50}", "#series2_index": float((n - 1) % 50 + 1),
            "#series3": f"Synthetic Arc {(n - 1) // 10}", "#series3_index": float((n - 1) % 10 + 1),
            "#series4": f"Synthetic Set {(n - 1) // 5}", "#series4_index": float((n - 1) % 5 + 1),
        }
        row.update(values)
        rows.append(row)
        if book_id not in protected:
            independent_expected[book_id].update(values)
    with incoming.open("w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=list(rows[0]))
        w.writeheader()
        w.writerows(rows)
    start = time.monotonic()
    report = m.build_report(library, incoming, baseline)
    audit_seconds = time.monotonic() - start
    assert not report["errors"], report["errors"][:3]
    assert report["protected_books"] == 218
    assert report["unlisted_books"] == 1
    assert len(report["changes"]) == 8305
    m.private_json(root / "dry-run.json", report)
    start = time.monotonic()
    result = m.apply_report(report, library, incoming, baseline,
                            report["approval"], root / "backups")
    apply_seconds = time.monotonic() - start
    after = m.snapshot(library)["books"]
    assert after == independent_expected, "Independent eight-field accuracy check failed"
    repeated = m.build_report(library, incoming, baseline)
    assert not repeated["errors"] and not repeated["changes"], "Import is not idempotent"
    for i in protected:
        assert after[i] == before[i], "Protected metadata changed"
    assert after[ids[-1]] == before[ids[-1]], "Unlisted book changed"
    summary = {
        "library_books": len(ids), "csv_rows": len(rows), "updated_books": 8305,
        "protected_books_unchanged": len(protected), "unlisted_books_unchanged": 1,
        "independent_index_levels_verified": 4,
        "dry_run_seconds": round(audit_seconds, 3),
        "apply_including_rehearsal_backup_readback_seconds": round(apply_seconds, 3),
        "second_run_changes": len(repeated["changes"]),
        "artifacts": str(root), "backup": result["backup"],
    }
    m.private_json(root / "benchmark.json", summary)
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    run()
