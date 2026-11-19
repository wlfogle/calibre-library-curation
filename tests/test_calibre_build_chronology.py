"""Source-backed chronology tests; no real library or reference workspace writes."""

import copy
import importlib.util
import json
from pathlib import Path
import re
import sys
import tempfile
import unittest
import uuid
import zipfile

SCRIPTS = Path(__file__).resolve().parents[1] / "scripts"
sys.path.insert(0, str(SCRIPTS))
SPEC = importlib.util.spec_from_file_location("build_chronology", SCRIPTS / "calibre_build_chronology.py")
m = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(m)
from calibre_chronology_sources import document_text, local_documents


class MappingTests(unittest.TestCase):
    def setUp(self):
        self.document, self.entries = m.catalogs(m.DEFAULT_CATALOG)
        self.dragon = next(c for c in self.entries if c["universe"] == "Dragonlance Universe")

    def row(self, title, label="Dragonlance Universe", author="Test Author"):
        return {"title": title, "uuid": str(uuid.uuid4()), "authors": author,
                "series": label, "#series2": "", "#series3": "", "#series4": ""}

    def test_real_source_order_is_not_publication_or_existing_index(self):
        rows = {1: self.row("The Irda"), 2: self.row("The Dragons"),
                3: self.row("The Kagonesti")}
        rows[1]["series_index"] = "9999"
        mappings, _, _ = m.construct(rows, self.entries)
        by_title = {r["title"]: r for r in mappings}
        self.assertLess(int(by_title["The Dragons"]["series_index"]),
                        int(by_title["The Irda"]["series_index"]))
        self.assertEqual(by_title["The Dragons"]["#series2_index"], "1")
        self.assertEqual(by_title["The Irda"]["#series2_index"], "2")
        self.assertEqual(by_title["The Kagonesti"]["#series2_index"], "3")

    def test_alias_and_missing_works_retain_positions(self):
        mappings, _, missing = m.construct({1: self.row("The Covenant of the Forge")}, self.entries)
        self.assertEqual(len(mappings), 1)
        self.assertGreater(int(mappings[0]["series_index"]), 1)
        self.assertTrue(missing)

    def test_known_space_exact_work_plus_author_not_whole_author(self):
        rows = {1: self.row("Ringworld", "", "Larry Niven"),
                2: self.row("The Magic Goes Away", "Known Space", "Larry Niven")}
        mappings, coverage, _ = m.construct(rows, self.entries)
        self.assertEqual([r["id"] for r in mappings], ["1"])
        self.assertEqual(mappings[0]["series"], "Known Space Universe")
        self.assertEqual(next(r for r in coverage if r["id"] == 2)["status"], "needs_research")

    def test_protected_universe_cannot_be_relabelled(self):
        rows = {1: self.row("Ringworld", "Greater Eternal Champion Saga", "Larry Niven")}
        mappings, coverage, _ = m.construct(rows, self.entries)
        self.assertFalse(mappings)
        self.assertEqual(coverage[0]["status"], "protected")

    def test_duplicate_work_and_cross_universe_collision_held(self):
        rows = {1: self.row("Firstborn"), 2: self.row("Firstborn")}
        self.assertFalse(m.construct(rows, self.entries)[0])
        duplicate = copy.deepcopy(self.dragon)
        duplicate["universe"] = "Other universe"
        self.assertFalse(m.construct({1: rows[1]}, [self.dragon, duplicate])[0])

    def test_wrong_universe_and_comic_are_not_forced(self):
        self.assertFalse(m.construct({1: self.row("Firstborn", "Other series")}, self.entries)[0])
        self.assertFalse(m.construct({1: self.row("Ringworld", "Known Space (comic)")}, self.entries)[0])

    def test_time_travel_and_ambiguous_collections_held(self):
        rows = {1: self.row("War of the Twins"), 2: self.row("Flatlander", "", "Larry Niven")}
        mappings, coverage, _ = m.construct(rows, self.entries)
        self.assertFalse(mappings)
        self.assertTrue(all(r["status"].startswith("held") for r in coverage))

    def test_all_works_have_sources_and_monotonic_dates(self):
        for catalog in self.entries:
            if catalog.get("relative_timeline"):
                positions = [w["position"] for w in catalog["works"]]
                self.assertEqual(positions, sorted(positions))
                continue
            previous = -float("inf")
            for work in catalog["works"]:
                if work["date"] == "Pre-History":
                    continue
                match = re.search(r"\d+", work["date"])
                self.assertIsNotNone(match)
                year = int(match[0]) * (-1 if "PC" in work["date"] else 1)
                self.assertGreaterEqual(year, previous, work["title"])
                previous = year
            self.assertTrue(catalog["sources"])

    def test_fleet_and_ringworld_interleave(self):
        known = next(c for c in self.entries if c["universe"] == "Known Space Universe")
        works = {w["title"]: w for w in known["works"]}
        self.assertLess(works["Betrayer of Worlds"]["position"], works["Ringworld"]["position"])
        self.assertGreater(works["Fate of Worlds"]["position"], works["Ringworld's Children"]["position"])
        self.assertEqual(works["Fate of Worlds"]["subposition"], 5)

    def test_exemplar_independent_main_series_and_nested_numbering(self):
        catalog = copy.deepcopy(self.dragon)
        work = copy.deepcopy(catalog["works"][0])
        work.update(title="Nested Work", keys={"nested work"}, status="ready",
                    position=97, hierarchy=["Elric", "The Moonbeam Roads", "Nested arc"],
                    indices=[10, 1, 2], subposition=10)
        catalog["works"] = [work]
        mapping = m.construct({1: self.row("Nested Work")}, [catalog])[0][0]
        self.assertEqual(mapping["series_index"], "97")
        self.assertEqual(mapping["#series2_index"], "10")
        self.assertEqual(mapping["#series3_index"], "1")
        self.assertEqual(mapping["#series4_index"], "2")
        self.assertEqual(mapping["#series3"], "The Moonbeam Roads")

    def test_source_author_is_required_when_explicit(self):
        catalog = copy.deepcopy(self.dragon)
        catalog["works"] = [copy.deepcopy(catalog["works"][0])]
        work = catalog["works"][0]
        work.update(title="Shared Title", keys={"shared title"}, status="ready",
                    authors="Correct Author")
        rows = {1: self.row("Shared Title", author="Wrong Author"),
                2: self.row("Shared Title", author="Correct Author")}
        mappings, _, _ = m.construct(rows, [catalog])
        self.assertEqual([r["id"] for r in mappings], ["2"])

    def test_markdown_and_odt_extraction(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            md = root / "Known Space chronology.md"
            md.write_text("# Known Space\n1. Ringworld\n")
            self.assertIn("Ringworld", document_text(md))
            odt = root / "source.odt"
            xml = ('<office xmlns:t="urn:oasis:names:tc:opendocument:xmlns:text:1.0">'
                   '<t:p>Title<t:s t:c="2"/>Two<t:line-break/>Lines</t:p></office>')
            with zipfile.ZipFile(odt, "w") as archive:
                archive.writestr("content.xml", xml)
            self.assertEqual(document_text(odt), "Title  Two\nLines")

    def test_local_markdown_duplicates_and_bad_copies_are_reported(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "documents"
            root.mkdir()
            (root / "Known Space chronology.md").write_text("# Known Space\nRingworld\n")
            (root / "Known Space chronology copy.md").write_text("# Known Space  \nRingworld\n")
            (root / "Dragonlance Series_1.odt").write_bytes(b"not an ODT ZIP archive")
            (root / "unrelated-personal.txt").write_text("Do not ingest")
            output = Path(tmp) / "output"
            output.mkdir()
            result = local_documents(root, output, self.entries)
            self.assertEqual(result["documents"], 3)
            self.assertEqual(result["unique_extracted_texts"], 1)
            self.assertEqual(result["failed"], 1)
            records = json.loads((output / "local-sources.json").read_text())
            self.assertTrue(all(r.get("file_sha256") for r in records))
            self.assertTrue(any(r.get("same_text_as") for r in records))


if __name__ == "__main__":
    unittest.main()
