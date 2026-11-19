#!/usr/bin/env python3
"""Look up books using Curator's Rust research engine and curate the verified test clone."""

import argparse
from contextlib import contextmanager
import csv
from enum import Enum
import fcntl
import hashlib
import json
import math
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import unicodedata
from urllib.parse import urlsplit
import uuid

from calibre_chronology import (
    FIELDS, INDEX, SERIES, Refusal, build_report, private_json, protected_ids,
    read_csv, snapshot, sync_directory,
)

REPO = Path(__file__).resolve().parents[1]
CORE = REPO / "vendor/curator-core"
LIVE_LIBRARY = Path("/media/loufogle/Data/Calibre Library")
TEST_LIBRARY = Path("/media/loufogle/Data/Calibre Test Workspace-20261119/Calibre Library")
BASELINE = Path("/home/loufogle/Downloads/all books.csv")
MANIFEST = REPO / "tools/calibre-research/Cargo.toml"

class ApplyStatus(str, Enum):
    DRY_RUN_ONLY = "dry_run_only"
    APPLIED = "applied"
    BLOCKED_BY_ERRORS = "blocked_by_errors"
    NO_CHANGES = "no_changes"
    NO_VERIFIED_MAPPING = "no_verified_mapping"
    SKIPPED_NO_SELECTION = "skipped_no_selection"
    FAILED = "failed"


def validate_clone(library, receipt):
    library = Path(library).resolve(strict=True)
    live = LIVE_LIBRARY.resolve()
    if library == live or live in library.parents:
        raise Refusal("Live-library writes are blocked; select the verified test clone")
    if not (library / "metadata.db").is_file():
        raise Refusal("Choose an existing test library")
    data = json.loads(Path(receipt).read_text())
    if data.get("status") != "verified" or Path(data.get("test_library", "")).resolve() != library:
        raise Refusal("Test-library clone receipt is missing, unverified or bound elsewhere")
    source = Path(data.get("source", "")).resolve()
    if source == library or source in library.parents or library in source.parents:
        raise Refusal("Test library must be separate from its source")
    if source.joinpath("metadata.db").exists():
        a, b = (source / "metadata.db").stat(), (library / "metadata.db").stat()
        if (a.st_dev, a.st_ino) == (b.st_dev, b.st_ino):
            raise Refusal("Test database is hardlinked to its source")
    return library


def storage_guard(library):
    if shutil.disk_usage(library).free < 21 * 1024**3:
        raise Refusal("Less than 21 GiB free; research/apply paused to protect storage")

def storage_preflight(library, workspace, *, apply):
    if apply:
        storage_guard(library)
    destination = next(p for p in (workspace, *workspace.parents) if p.exists())
    storage_guard(destination)


@contextmanager
def workspace_lock(root):
    resolved = root.resolve()
    if any(p.joinpath("metadata.db").is_file() for p in (resolved, *resolved.parents)):
        raise Refusal("Research workspace must be outside any Calibre library")
    root.mkdir(mode=0o700, parents=True, exist_ok=True)
    fd = os.open(root / "workflow.lock",
                 os.O_WRONLY | os.O_CREAT | os.O_APPEND | os.O_NOFOLLOW | os.O_NONBLOCK, 0o600)
    with os.fdopen(fd, "a", encoding="utf-8") as stream:
        if not stat.S_ISREG(os.fstat(stream.fileno()).st_mode):
            raise Refusal("Workspace lock must be a regular file")
        os.fchmod(stream.fileno(), 0o600)
        try:
            fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as exc:
            raise Refusal("Another curation workflow owns this workspace") from exc
        yield


def clean_text(value, label, *, empty=False):
    if (not isinstance(value, str) or (not empty and not value.strip())
            or value != value.strip()
            or any(unicodedata.category(c).startswith("C") for c in value)):
        raise Refusal(f"Rust export has invalid {label}")
    return value


def validate_mapping(mapping):
    if not isinstance(mapping, dict):
        raise Refusal("Rust export mapping must be an object")
    try:
        if type(mapping["id"]) is not int or mapping["id"] < 1:
            raise Refusal("Rust export has invalid book identity")
        identity = clean_text(mapping["uuid"], "UUID")
        if str(uuid.UUID(identity)) != identity.lower():
            raise ValueError("Non-canonical UUID")
        clean_text(mapping["title"], "title")
        clean_text(mapping["research_id"], "research identity")
        authors = mapping["authors"]
        if not isinstance(authors, list) or not authors:
            raise Refusal("Rust export has no writing authors")
        for author in authors:
            clean_text(author, "author")
        mask = mapping["field_mask"]
        if (not isinstance(mask, list) or len(mask) != 4
                or any(type(v) is not bool for v in mask) or not any(mask)):
            raise Refusal("Rust export has invalid field support mask")
        fields = mapping["fields"]
        if not isinstance(fields, dict):
            raise Refusal("Rust export fields must be an object")
        selected = {f for i, name in enumerate(SERIES) if mask[i] for f in (name, INDEX[name])}
        if set(fields) != selected:
            raise Refusal("Rust export fields lack matching evidence/support")
        for name in SERIES:
            if name not in selected:
                continue
            value, index = fields[name], fields[INDEX[name]]
            if value is not None:
                clean_text(value, "series name", empty=True)
            if value:
                if type(index) not in (int, float):
                    raise Refusal("Rust export has a non-numeric series index")
                try:
                    finite = math.isfinite(index)
                except OverflowError:
                    finite = False
                if not finite:
                    raise Refusal("Rust export has a non-finite series index")
            elif index is not None:
                raise Refusal("Rust export supplies an index without a series name")
        basis = mapping.get("order_basis", "in-universe")
        if basis not in ("in-universe", "standalone"):
            raise Refusal("Automatic curation cannot use publication or recommended ordering")
        if basis == "standalone" and (not all(mask) or any(fields[name] for name in SERIES)):
            raise Refusal("Verified standalone must explicitly clear all four series levels")
        evidence = mapping["evidence"]
        if not isinstance(evidence, list) or not evidence:
            raise Refusal("Rust export is missing source provenance")
        for item in evidence:
            url = clean_text(item["url"], "source URL")
            parsed = urlsplit(url)
            if (parsed.scheme != "https" or not parsed.hostname
                    or parsed.username is not None or parsed.password is not None
                    or parsed.port is not None):
                raise Refusal("Rust export has invalid HTTPS source provenance")
            if not isinstance(item["source_hash"], str) or not re.fullmatch(
                    r"[0-9a-fA-F]{64}", item["source_hash"]):
                raise Refusal("Rust export is missing a hexadecimal source hash")
    except (KeyError, TypeError, ValueError, AttributeError) as exc:
        raise Refusal("Rust export mapping structure or provenance is invalid") from exc


def ensure_engine(override=None):
    if override:
        engine = Path(override).resolve(strict=True)
    else:
        if not (CORE / "Cargo.toml").is_file():
            raise Refusal(f"Vendored research core is missing: {CORE}")
        target = Path(os.environ.get("CARGO_TARGET_DIR") or REPO / "target").expanduser().resolve()
        if any((p/"metadata.db").is_file() for p in (target,*target.parents)):
            raise Refusal("Build cache must be outside every Calibre library")
        env = dict(os.environ, CARGO_TARGET_DIR=str(target))
        subprocess.run(
            ["cargo", "build", "--locked", "--offline", "--manifest-path", str(MANIFEST)],
            env=env, check=True,
        )
        engine = target / "debug/nexus-calibre-research"
    if not engine.is_file() or not os.access(engine, os.X_OK):
        raise Refusal("Rust research adapter is not executable")
    return engine


def convert_export(export, library, job_id):
    if (not isinstance(export, dict) or export.get("library_write_performed") is not False
            or export.get("library") != str(library)
            or export.get("job_id") != job_id
            or export.get("adapter_version") != "nexus-facts-batch-v2"):
        raise Refusal("Rust engine export binding/version is invalid")
    if not isinstance(export.get("mappings"), list):
        raise Refusal("Rust export mappings must be an array")
    rows, seen = [], set()
    for mapping in export["mappings"]:
        validate_mapping(mapping)
        book_id = mapping["id"]
        if type(book_id) is not int or book_id < 1 or book_id in seen:
            raise Refusal("Rust export has invalid or duplicate book identities")
        seen.add(book_id)
        mask = mapping["field_mask"]
        selected = [f for i, s in enumerate(SERIES) if mask[i] for f in (s, INDEX[s])]
        row = {
            "id": str(book_id), "uuid": mapping["uuid"], "title": mapping["title"],
            "authors": " & ".join(mapping["authors"]),
            **{f: "" for f in FIELDS},
            "update_fields": ";".join(selected),
            "order_verified": "yes", "order_basis": mapping.get("order_basis", "in-universe"),
        }
        for f in selected:
            value = mapping["fields"][f]
            row[f] = "" if value is None else str(value)
        citations = []
        for item in mapping["evidence"]:
            citations.append(f"{item['url']} sha256={item['source_hash']}")
        row["order_source"] = (
            f"Validated Rust research {mapping['research_id']}; " + " | ".join(citations)
        )
        rows.append(row)
    return rows


def write_mapping(path, rows):
    headers = ["id", "uuid", "title", "authors", *FIELDS, "update_fields",
               "order_verified", "order_basis", "order_source"]
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8-sig", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=headers)
        writer.writeheader()
        writer.writerows(rows)
        stream.flush()
        os.fsync(stream.fileno())
    sync_directory(path.parent)


def read_json(path, *, raw=None):
    try:
        return json.loads(path.read_text() if raw is None else raw)
    except (ValueError, UnicodeError) as exc:
        raise Refusal(f"Invalid JSON in {path}") from exc


def job_metadata(job):
    path = job / "job.json"
    if not path.exists():
        return None  # Legacy jobs cannot override current state by file timestamps.
    data = read_json(path)
    if (not isinstance(data, dict) or data.get("job_id") != job.name
            or not re.fullmatch(r"[0-9a-f]{32}", job.name)
            or type(data.get("sequence")) is not int or data["sequence"] < 1):
        raise Refusal(f"Invalid job identity/sequence in {path}")
    return data


def create_job(jobs, job_id, library):
    """Caller owns workflow.lock; immutable manifests provide a monotonic order."""
    previous = [job_metadata(path) for path in jobs.iterdir() if path.is_dir()]
    sequence = 1 + max((item["sequence"] for item in previous if item), default=0)
    job = jobs / job_id
    job.mkdir(mode=0o700)
    private_json(job / "job.json", {
        "job_id": job_id, "sequence": sequence, "library": str(library),
    })
    return job, sequence


def queue_observations(items, workspace, job_id, sequence):
    if not isinstance(items, list):
        raise Refusal("Engine queue must be an array")
    result = []
    for item in items:
        if not isinstance(item, dict) or type(item.get("id")) is not int or item["id"] < 1:
            raise Refusal("Engine queue contains an invalid book identity")
        path = workspace / "engine/book-results" / f"book-{item['id']}.json"
        result.append({
            **item, "job_id": job_id, "job_sequence": sequence,
            "result_sha256": hashlib.sha256(path.read_bytes()).hexdigest() if path.is_file() else None,
        })
    return result


def inspect_summary(workspace, library, job_id=None):
    jobs = Path(workspace) / "jobs"
    if job_id is None:
        candidates = []
        for job in jobs.glob("*"):
            if not job.is_dir() or not (job / "summary.json").is_file():
                continue
            metadata = job_metadata(job)
            if metadata and metadata.get("library") == str(library):
                candidates.append((metadata["sequence"], job.name))
        if not candidates:
            raise Refusal("No sequenced completed summary found; inspect a legacy job with --summary JOB_ID")
        job_id = max(candidates)[1]
    if not isinstance(job_id, str) or not re.fullmatch(r"[0-9a-f]{32}", job_id):
        raise Refusal("Summary job ID must be 32 lowercase hexadecimal characters")
    job = jobs / job_id
    summary = read_json(job / "summary.json")
    metadata = job_metadata(job)
    if (not isinstance(summary, dict) or summary.get("job_id") != job_id
            or summary.get("library") != str(library)
            or metadata and (metadata.get("library") != str(library)
                             or summary.get("job_sequence") != metadata["sequence"])):
        raise Refusal("Summary identity, library or sequence is invalid")
    if "apply_status" in summary:
        try:
            ApplyStatus(summary["apply_status"])
        except (ValueError, TypeError) as exc:
            raise Refusal("Summary has an invalid apply_status") from exc
    return summary


def list_exceptions(workspace, book_ids=None, query=None, statuses=None, *, retry=False):
    """Inspect saved exceptions without making a model call or library write."""
    workspace = Path(workspace)
    wanted, filters = set(book_ids or []), set(statuses or [])
    result = {}
    for path in sorted((workspace / "engine/book-results").glob("book-*.json")):
        raw = path.read_bytes()
        saved = read_json(path, raw=raw)
        if (not isinstance(saved, dict) or not isinstance(saved.get("book"), dict)
                or type(saved["book"].get("id")) is not int
                or saved["book"]["id"] < 1
                or path.name != f"book-{saved['book']['id']}.json"
                or not all(k in saved["book"] for k in ("uuid", "title"))
                or not isinstance(saved.get("status"), str)
                or saved.get("run") is not None and not isinstance(saved["run"], dict)):
            raise Refusal(f"Invalid saved book record in {path}")
        book, research = saved["book"], saved.get("run") or {}
        evidence, checked = research.get("evidence", []), saved.get("checked_sources", [])
        if (not isinstance(evidence, list)
                or any(not isinstance(e, dict) or not isinstance(e.get("url"), str) for e in evidence)
                or not isinstance(checked, list) or any(not isinstance(s, str) for s in checked)):
            raise Refusal(f"Invalid saved source evidence in {path}")
        result[book["id"]] = {
            "id": book["id"], "uuid": book["uuid"], "title": book["title"],
            "authors": book.get("authors", []), "status": saved["status"],
            "error": saved.get("error"), "notes": research.get("notes", []),
            "sources": sorted(set(checked) | {e["url"] for e in evidence}),
            "discovered_sources": saved.get("discovered_sources", []),
            "queries": research.get("queries", []),
            "proposed_hierarchy": research.get("hierarchy"),
            "supported_fields": research.get("field_mask"),
            "result_file": str(path), "_sha256": hashlib.sha256(raw).hexdigest(),
            "_queue_order": (0, ""),
        }
    for job in sorted((workspace / "jobs").glob("*")):
        if not job.is_dir():
            continue
        metadata = job_metadata(job)
        if not metadata:
            continue
        for name in ("exceptions.json", "retries.json"):
            path = job / name
            if not path.exists():
                continue
            items = read_json(path)
            if not isinstance(items, list):
                raise Refusal(f"Invalid queue in {path}")
            for item in items:
                if not isinstance(item, dict) or type(item.get("id")) is not int:
                    raise Refusal(f"Invalid queue item in {path}")
                if item.get("job_id") != job.name or item.get("job_sequence") != metadata["sequence"]:
                    raise Refusal(f"Queue identity/sequence mismatch in {path}")
                existing = result.get(item["id"])
                if not existing or item.get("result_sha256") != existing["_sha256"]:
                    continue
                order = (metadata["sequence"], job.name)
                if order <= existing["_queue_order"]:
                    continue
                for key in ("status", "error", "notes"):
                    if key in item:
                        existing[key] = item[key]
                existing["_queue_order"] = order
    entries = []
    for book_id, item in sorted(result.items()):
        if item["status"] in ("ready", "protected", "catalogue_not_narrative"):
            continue
        if (item["status"] == "unverified") == retry:
            continue
        if (wanted and book_id not in wanted) or (filters and item["status"] not in filters):
            continue
        if query and query.casefold() not in json.dumps(item, ensure_ascii=False).casefold():
            continue
        item.pop("_sha256", None)
        item.pop("_queue_order", None)
        entries.append(item)
    return entries


def run(args):
    library = Path(args.library).resolve(strict=True)
    receipt = args.clone_receipt or library.parent / "clone-verified.json"
    workspace = Path(args.workspace or library.parent / "automated-research").resolve()
    if workspace == library or library in workspace.parents:
        raise Refusal("Research workspace cannot be inside the library")
    with workspace_lock(workspace):
        library = validate_clone(library, receipt)
        storage_preflight(library, workspace, apply=args.apply)
        baseline, _ = read_csv(args.baseline)
        current = snapshot(library)["books"]
        protected = protected_ids(baseline, current, {})
        selected = args.book or []
        if args.retry_exceptions:
            exceptions = list_exceptions(workspace, selected, args.query, args.exception_status)
            selected = [item["id"] for item in exceptions]
            if not selected:
                summary = {
                    "validation_errors": [], "exceptions": 0, "applied": False,
                    "apply_status": ApplyStatus.SKIPPED_NO_SELECTION,
                    "apply_note": "No matching factual exceptions to retry; no engine or apply started",
                }
                print(json.dumps(summary, indent=2))
                return summary
            args.refresh = True
        if len(set(selected)) != len(selected) or any(i not in current for i in selected):
            raise Refusal("Choose unique existing book IDs")
        engine = ensure_engine(args.engine)
        job_id = uuid.uuid4().hex
        jobs = workspace / "jobs"
        jobs.mkdir(mode=0o700, exist_ok=True)
        job, sequence = create_job(jobs, job_id, library)
        request = {
            "library": str(library), "workspace": str(workspace / "engine"),
            "job_id": job_id, "mode": "export" if args.export_only else "research",
            "model": args.model, "endpoint": "http://127.0.0.1:11434",
            "enable_ai": not args.no_ai and not args.export_only,
            "refresh": args.refresh and not args.export_only,
            "book_ids": [] if args.export_only else selected,
            "limit": None if args.export_only else args.limit,
            "protected_uuids": [current[i]["uuid"] for i in sorted(protected)],
            "source_pages": args.source_pages, "extraction_calls": args.extraction_calls,
        }
        private_json(job / "request.json", request)
        # One Rust process scans/researches the batch; no per-book Calibre boots.
        subprocess.run([str(engine), str(job / "request.json")], check=True)
        storage_preflight(library, workspace, apply=args.apply)
        exported = workspace / "engine/exports" / f"{job_id}.json"
        export = json.loads(exported.read_text())
        rows = convert_export(export, library, job_id)
        mapping_path = job / "researched-mapping.csv"
        write_mapping(mapping_path, rows)
        private_json(job / "exceptions.json", queue_observations(
            export["exceptions"], workspace, job_id, sequence))
        private_json(job / "retries.json", queue_observations(
            export.get("retries", []), workspace, job_id, sequence))
        summary = {
            "job_id": job_id, "library": str(library), "job_directory": str(job),
            "job_sequence": sequence,
            "library_books": len(current), "validated_mapping_rows": len(rows),
            "research_statuses": export["statuses"], "protected_books": len(protected),
            "exceptions": len(export["exceptions"]), "applied": False,
            "apply_status": ApplyStatus.DRY_RUN_ONLY,
            "automatic_retries": len(export.get("retries", [])),
            "full_library_verified": False,
            "mapping_csv_path": str(mapping_path),
            "dry_run_report_path": None,
        }
        if rows:
            audit = build_report(library, mapping_path, args.baseline)
            private_json(job / "dry-run.json", audit)
            summary["dry_run_report_path"] = str(job / "dry-run.json")
            summary["validation_errors"] = audit["errors"]
            summary["proposed_changes"] = len(audit["changes"])
            if args.apply and not audit["errors"] and audit["changes"]:
                summary["apply_attempted"] = False
                try:
                    validate_clone(library, receipt)
                    storage_guard(library)
                    config = workspace / "calibre-runtime-config"
                    config.mkdir(mode=0o700, exist_ok=True)
                    summary["apply_attempted"] = True
                    subprocess.run([
                        "calibre-debug", "-e", str(REPO / "scripts/calibre_chronology.py"), "--",
                        "--library", str(library), "--csv", str(mapping_path),
                        "--baseline", str(args.baseline), "--report", str(job / "apply.json"),
                        "--apply", "--approve", audit["approval"],
                        "--backup-dir", str(workspace / "metadata-backups"),
                    ], env=dict(os.environ, CALIBRE_CONFIG_DIRECTORY=str(config)), check=True)
                except (Refusal, OSError, ValueError, subprocess.SubprocessError) as exc:
                    summary["apply_status"] = ApplyStatus.FAILED
                    summary["apply_error"] = str(exc)
                    summary["apply_blocked"] = (
                        "Apply did not complete; inspect the apply report and recovery journal before retrying"
                    )
                    private_json(job / "summary.json", summary)
                    raise
                summary["applied"] = True
                summary["apply_status"] = ApplyStatus.APPLIED
            elif args.apply and audit["errors"]:
                summary["apply_status"] = ApplyStatus.BLOCKED_BY_ERRORS
                summary["apply_blocked"] = "Resolve reported conflicts; no metadata written"
            elif args.apply and not audit["changes"]:
                summary["apply_status"] = ApplyStatus.NO_CHANGES
                summary["apply_note"] = "No metadata changes proposed; already matches verified assignments"
        else:
            summary["validation_errors"] = []
            summary["proposed_changes"] = 0
            summary["apply_blocked"] = "No source-validated mapping available"
            if args.apply:
                summary["apply_status"] = ApplyStatus.NO_VERIFIED_MAPPING
        private_json(job / "summary.json", summary)
        print(json.dumps(summary, indent=2))
        return summary


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", type=Path, default=TEST_LIBRARY)
    parser.add_argument("--clone-receipt", type=Path)
    parser.add_argument("--workspace", type=Path)
    parser.add_argument("--baseline", type=Path, default=BASELINE)
    parser.add_argument("--book", type=int, action="append", help="Repeat to select books; default all")
    parser.add_argument("--limit", type=int, help="Maximum fresh book lookups in this invocation")
    parser.add_argument("--source-pages",type=int,default=6,
                        help="Maximum source pages per research checkpoint (1-12)")
    parser.add_argument("--extraction-calls",type=int,default=8,
                        help="Maximum extraction calls per checkpoint (1-32); progress is retained")
    parser.add_argument("--model", default="qwen3.5:4b")
    parser.add_argument("--engine", type=Path, help="Use an already built Rust adapter")
    parser.add_argument("--no-ai", action="store_true", help="Deterministic source parsing only")
    parser.add_argument("--refresh", action="store_true")
    parser.add_argument("--export-only", action="store_true", help="Revalidate saved results without web calls")
    parser.add_argument("--apply", action="store_true", help="Apply supported changes to verified test clone only")
    inspection = parser.add_mutually_exclusive_group()
    inspection.add_argument("--last-summary", action="store_true",
                            help="Print the latest completed job summary; no engine, model or lock")
    inspection.add_argument("--summary", metavar="JOB_ID",
                            help="Print one saved job summary; no engine, model or lock")
    inspection.add_argument("--exceptions", action="store_true",
                        help="List genuinely unverified factual questions only; no online calls")
    inspection.add_argument("--retries", action="store_true",
                        help="List pending research and operational failures separately; no online calls")
    parser.add_argument("--retry-exceptions", action="store_true",
                        help="Research matching exceptions again; combine with --book or --query")
    parser.add_argument("--query", help="Search exceptions by title, author, ID or reason")
    parser.add_argument("--exception-status", action="append", help="Filter exception status")
    args = parser.parse_args(argv)
    if args.limit is not None and args.limit < 1:
        parser.error("--limit must be positive")
    if not 1 <= args.source_pages <= 12 or not 1 <= args.extraction_calls <= 32:
        parser.error("--source-pages must be 1-12; --extraction-calls must be 1-32")
    if args.export_only and (args.refresh or args.limit is not None or args.book or args.retry_exceptions):
        parser.error("--export-only cannot be combined with research selection, --refresh or --limit")
    os.umask(0o077)
    try:
        if args.last_summary or args.summary:
            library = Path(args.library).resolve()
            workspace = args.workspace or library.parent / "automated-research"
            print(json.dumps(inspect_summary(workspace, library, args.summary), indent=2,
                             ensure_ascii=False))
            return 0
        if args.exceptions or args.retries:
            library = Path(args.library).resolve(strict=True)
            validate_clone(library, args.clone_receipt or library.parent / "clone-verified.json")
            workspace = args.workspace or library.parent / "automated-research"
            entries = list_exceptions(workspace, args.book, args.query, args.exception_status,
                                      retry=args.retries)
            print(json.dumps({"automatic_retries" if args.retries else "exceptions": len(entries),
                              "books": entries}, indent=2,
                             ensure_ascii=False))
            return 0
        summary = run(args)
        return 0 if not summary["validation_errors"] else 2
    except (Refusal, OSError, ValueError, subprocess.SubprocessError) as exc:
        print(f"Stopped safely: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
