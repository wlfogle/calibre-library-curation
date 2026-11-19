"""Read local chronology documents as unverified research inputs, never writer commands."""

import hashlib
import os
from pathlib import Path
import re
import subprocess
import xml.etree.ElementTree as ET
import zipfile

from calibre_chronology import Refusal, private_json


def document_text(path):
    suffix = path.suffix.casefold()
    if suffix == ".odt":
        with zipfile.ZipFile(path) as archive:
            if archive.getinfo("content.xml").file_size > 32 * 1024 * 1024:
                raise Refusal("ODT content exceeds 32 MiB limit")
            root = ET.fromstring(archive.read("content.xml"))
        ns = "{urn:oasis:names:tc:opendocument:xmlns:text:1.0}"

        def flatten(element):
            if element.tag == ns + "s":
                count = int(element.get(ns + "c", "1"))
                if not 1 <= count <= 10000:
                    raise Refusal("Invalid ODT whitespace count")
                return " " * count
            if element.tag in (ns + "tab", ns + "line-break"):
                return "\n" if element.tag.endswith("line-break") else "\t"
            return (element.text or "") + "".join(
                flatten(child) + (child.tail or "") for child in element
            )

        return "\n".join(flatten(e) for e in root.iter() if e.tag in (ns + "p", ns + "h"))
    if suffix == ".pdf":
        result = subprocess.run(
            ["pdftotext", "-layout", str(path), "-"], capture_output=True,
            timeout=60, check=True,
        )
        return result.stdout.decode("utf-8")
    return path.read_text(encoding="utf-8-sig")


def local_documents(root, output, entries):
    from calibre_build_chronology import normalize

    roots = [Path(root)] if isinstance(root, (str, Path)) else [Path(p) for p in root]
    for directory in roots:
        if not directory.is_dir():
            raise Refusal(f"Chronology document directory not found: {directory}")
    destination = output / "local-source-text"
    destination.mkdir(mode=0o700)
    references, seen = [], {}
    hints = (
        "chronolog", "series", "universe", "known space", "star wars", "star trek",
        "warhammer", "forgotten realms", "conan", "riordan", "krewe",
        "brain & brawn", "planet pirates", "deep space nine", "voyager",
        "loose reh",
    )
    paths = sorted({p for directory in roots for p in directory.iterdir()})
    for path in paths:
        if (not path.is_file() or path.suffix.casefold() not in
                {".md", ".markdown", ".odt", ".pdf", ".txt", ".csv"} or
                not any(h in path.name.casefold() for h in hints)):
            continue
        raw = path.read_bytes()
        try:
            text = document_text(path)
        except (OSError, ValueError, ET.ParseError, zipfile.BadZipFile, KeyError,
                subprocess.SubprocessError, UnicodeError, Refusal) as exc:
            references.append({"path": str(path), "status": "extraction_failed",
                               "file_sha256": hashlib.sha256(raw).hexdigest(),
                               "error_type": type(exc).__name__})
            continue
        text_hash = hashlib.sha256(" ".join(text.split()).encode()).hexdigest()
        record = {
            "path": str(path), "file_sha256": hashlib.sha256(raw).hexdigest(),
            "text_sha256": text_hash, "characters": len(text),
            "status": "unverified_starting_point",
            "embedded_urls": sorted(set(re.findall(r"https?://[^\s<>\"')]+", text))),
        }
        if text_hash in seen:
            record["same_text_as"] = seen[text_hash]
        else:
            seen[text_hash] = str(path)
            fd = os.open(destination / (text_hash + ".txt"),
                         os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            with os.fdopen(fd, "w", encoding="utf-8") as stream:
                stream.write(text)
            normalized = normalize(text)
            record["catalog_work_mentions"] = {
                c["universe"]: [w["title"] for w in c["works"] if any(
                    re.search(r"(?<!\w)" + re.escape(key) + r"(?!\w)", normalized)
                    for key in w["keys"]
                )] for c in entries
            }
        if path.read_bytes() != raw:
            raise Refusal(f"Reference changed while reading: {path}")
        references.append(record)
    private_json(output / "local-sources.json", references)
    return {
        "documents": len(references), "unique_extracted_texts": len(seen),
        "failed": sum(r["status"] == "extraction_failed" for r in references),
        "meaning": "Source leads and disagreements, not automatically approved chronology",
    }
