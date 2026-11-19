"""Unit tests run with Python; native tests additionally run with calibre-debug."""

import copy
import csv
import importlib.util
from pathlib import Path
import sqlite3
import sys
import tempfile
import unittest
from unittest.mock import patch
import uuid

SPEC = importlib.util.spec_from_file_location(
    "calibre_chronology", Path(__file__).resolve().parents[1] / "scripts/calibre_chronology.py"
)
m = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(m)
try:
    import calibre  # noqa: F401
    NATIVE = True
except ImportError:
    NATIVE = False


def write_csv(path, rows, headers=None):
    headers = headers or list(rows[0])
    with path.open("w", encoding="utf-8-sig", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=headers)
        writer.writeheader()
        writer.writerows(rows)


def book(ident=None, series=None, rank=1):
    result = {"uuid": ident or str(uuid.uuid4()), "title": 'A book, "quoted"'}
    for s in m.SERIES:
        result[s] = None
        result[m.INDEX[s]] = 1.0
    result.update(series=series, series_index=float(rank))
    return result


def assignment(book_id, old, **fields):
    return {"id": str(book_id), "uuid": old["uuid"], **fields,
            "order_verified": "yes", "order_basis": "in-universe",
            "order_source": "Synthetic test fixture; not real chronology"}


class ImporterTests(unittest.TestCase):
    def test_only_user_verified_series_are_trusted(self):
        self.assertEqual(m.PROTECTED, {
            "The 39 Clues", "Greater Eternal Champion Saga",
            "Greater Foundation Universe", "The Grand Tour",
        })

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.csv = self.root / "input.csv"
        self.base = self.root / "original.csv"
        self.books = {1: book(series="Old", rank=2), 2: book()}
        self.state = {"books": self.books, "columns": {s: i for i, s in enumerate(m.SERIES[1:], 1)},
                      "db_digest": "unchanged"}
        write_csv(self.base, [{"id": str(i), **b} for i, b in self.books.items()])

    def report(self, rows):
        write_csv(self.csv, rows)
        with patch.object(m, "snapshot", return_value=copy.deepcopy(self.state)):
            return m.build_report(self.root, self.csv, self.base)

    def test_bom_quoted_unicode_and_omitted_fields(self):
        row = assignment(1, self.books[1], series="Éra, The", series_index="0.5")
        report = self.report([row])
        self.assertFalse(report["errors"])
        self.assertEqual(set(report["changes"][0]["fields"]), {"series", "series_index"})
        self.assertEqual(report["unlisted_books"], 1)

    def test_explicit_blank_clears_series_and_neutralizes_index(self):
        row = assignment(1, self.books[1], series="", series_index="")
        row["order_basis"] = "standalone"
        report = self.report([row])
        self.assertFalse(report["errors"])
        self.assertIsNone(report["changes"][0]["fields"]["series"]["after"])
        self.assertEqual(report["changes"][0]["fields"]["series_index"]["after"], 1)

    def test_all_custom_series_indices_required(self):
        for s in m.SERIES[1:]:
            with self.subTest(s=s):
                report = self.report([assignment(1, self.books[1], **{s: "Sub"})])
                self.assertIn("requires explicit", report["errors"][0])

    def test_every_index_is_independent(self):
        fields = {s: "Level " + str(i) for i, s in enumerate(m.SERIES)}
        fields.update({m.INDEX[s]: str(i + 3) for i, s in enumerate(m.SERIES)})
        report = self.report([assignment(1, self.books[1], **fields)])
        self.assertFalse(report["errors"])
        self.assertEqual(len(report["changes"][0]["fields"]), 8)

    def test_invalid_indices(self):
        for value in ("", "NaN", "inf", "-Infinity", "abc", "1e9999", "1e-9999",
                      "1.000000000000000001"):
            with self.subTest(value=value):
                report = self.report([assignment(1, self.books[1], series_index=value)])
                self.assertTrue(report["errors"])

    def test_zero_and_negative_fractional_indices(self):
        for value in ("0", "-0.5", "2.75"):
            self.assertFalse(self.report([assignment(
                1, self.books[1], series_index=value)])["errors"])

    def test_missing_evidence(self):
        row = assignment(1, self.books[1], series_index="3")
        del row["order_source"]
        self.assertIn("order_source", self.report([row])["errors"][0])

    def test_publication_only_hard_case_crime(self):
        row = assignment(1, self.books[1], series="New", series_index="3")
        row["order_basis"] = "publication"
        self.assertTrue(self.report([row])["errors"])
        row["series"] = "Hard Case Crime"
        self.assertFalse(self.report([row])["errors"])

    def test_original_identity_protects_relabelled_book(self):
        self.books[1]["series"] = next(iter(m.PROTECTED))
        write_csv(self.base, [{"id": "1", **self.books[1]}])
        self.books[1]["series"] = "Already damaged"
        report = self.report([assignment(1, self.books[1], series="More damage", series_index="9")])
        self.assertFalse(report["changes"])
        self.assertEqual(report["protected_books"], 1)
        self.assertTrue(report["protected_discrepancies"])

    def test_current_membership_protects_book(self):
        self.books[1]["series"] = next(iter(m.PROTECTED))
        report = self.report([assignment(1, self.books[1], series="Relabel", series_index="3")])
        self.assertFalse(report["changes"])

    def test_previously_assumed_verified_series_are_researchable(self):
        for name in ("Shannara Universe", "Federated Sentient Planets"):
            with self.subTest(name=name):
                self.books[1]["series"] = name
                write_csv(self.base, [{"id": str(i), **b} for i, b in self.books.items()])
                report = self.report([assignment(1, self.books[1], series_index="3")])
                self.assertEqual(report["protected_books"], 0)
                self.assertEqual(len(report["changes"]), 1)

    def test_uuid_mismatch_and_unknown_id(self):
        row = assignment(1, self.books[1], series_index="3")
        row["uuid"] = str(uuid.uuid4())
        self.assertIn("UUID", self.report([row])["errors"][0])
        row["id"] = "99"
        self.assertIn("unknown", self.report([row])["errors"][0])

    def test_baseline_id_reuse_refused(self):
        self.books[1]["uuid"] = str(uuid.uuid4())
        with self.assertRaisesRegex(m.Refusal, "Original-export identity"):
            self.report([assignment(1, self.books[1], series_index="3")])

    def test_duplicates_check_unlisted_books_and_explicit_exceptions(self):
        self.books[2]["series"] = "Old"
        row = assignment(1, self.books[1], series_index="1")
        self.assertIn("Duplicate", self.report([row])["errors"][0])
        row["allow_duplicate_indices"] = "series"
        row2 = assignment(2, self.books[2], series_index="1")
        row2["allow_duplicate_indices"] = "series"
        self.assertFalse(self.report([row, row2])["errors"])

    def test_duplicate_headers_ids_and_malformed_rows(self):
        ident = self.books[1]["uuid"]
        for text in (f"id,uuid,id\n1,{ident},1\n",
                     f"id,uuid\n1,{ident}\n1,{ident}\n",
                     f"id,uuid\n1,{ident},extra\n",
                     "id,uuid\n1\n"):
            self.csv.write_text(text)
            with self.assertRaises(m.Refusal):
                m.read_csv(self.csv)

    def test_wrong_column_type(self):
        self.state["columns"] = {}
        report = self.report([assignment(1, self.books[1], **{"#series2": "Sub", "#series2_index": "2"})])
        self.assertIn("does not exist", report["errors"][0])

    def test_idempotent_report_and_approval_bound_to_input(self):
        row = assignment(1, self.books[1], series_index="2")
        report = self.report([row])
        self.assertFalse(report["changes"])
        self.assertEqual(report, self.report([row]))
        row["order_source"] += " changed"
        self.assertNotEqual(report["approval"], self.report([row])["approval"])

    def test_misspelled_series_field_refused(self):
        with self.assertRaisesRegex(m.Refusal, "Unknown series field"):
            self.report([assignment(1, self.books[1], **{"#series2_indx": "2"})])

    def test_per_row_mask_preserves_unsupported_levels(self):
        row = assignment(1, self.books[1], **{
            "series": "", "series_index": "", "#series2": "Researched Subseries",
            "#series2_index": "4", "#series3": "", "#series3_index": "",
            "update_fields": "#series2;#series2_index",
        })
        report = self.report([row])
        self.assertFalse(report["errors"])
        self.assertEqual(set(report["changes"][0]["fields"]), {"#series2", "#series2_index"})

    def test_invalid_or_unpaired_masks_refused(self):
        for mask in ("", "series", "unknown", "series;series_index;series"):
            report = self.report([assignment(1, self.books[1], series="New",
                                            series_index="4", update_fields=mask)])
            self.assertTrue(report["errors"])

    def test_bad_approval_refuses_before_native_write(self):
        report = self.report([assignment(1, self.books[1], series_index="3")])
        with patch.object(m, "native_write") as write:
            with self.assertRaisesRegex(m.Refusal, "Approval"):
                m.apply_report(report, self.root, self.csv, self.base, "wrong", self.root)
            write.assert_not_called()

    def test_private_output_exclusive(self):
        path = self.root / "report.json"
        m.private_json(path, {"ok": True})
        self.assertEqual(path.stat().st_mode & 0o777, 0o600)
        with self.assertRaises(FileExistsError):
            m.private_json(path, {})

    def test_backup_is_consistent_and_never_replaced(self):
        src, dst = self.root / "source.db", self.root / "backup.db"
        with sqlite3.connect(src) as conn:
            conn.execute("CREATE TABLE example (value)")
            conn.execute("INSERT INTO example VALUES (42)")
        m.backup_database(src, dst)
        with sqlite3.connect(dst) as conn:
            self.assertEqual(conn.execute("SELECT value FROM example").fetchone(), (42,))
        with self.assertRaises(FileExistsError):
            m.backup_database(src, dst)


@unittest.skipUnless(NATIVE, "Run with calibre-debug for real native API integration")
class NativeTests(unittest.TestCase):
    def setUp(self):
        from calibre.db.backend import DB
        from calibre.db.cache import Cache
        from calibre.ebooks.metadata.book.base import Metadata
        self.tmp = tempfile.TemporaryDirectory(prefix="chronology-native-test-")
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.lib = self.root / "library"
        self.lib.mkdir()
        cache = Cache(DB(str(self.lib), load_user_formatter_functions=False))
        cache.init()
        for label in ("series2", "series3", "series4"):
            cache.create_custom_column(label, label.title(), "series", False)
        cache.close()
        cache = Cache(DB(str(self.lib), load_user_formatter_functions=False))
        cache.init()
        self.ids, _ = cache.add_books(
            [(Metadata(f"Synthetic book {i}", ["Test Author"]), {}) for i in range(6)],
            run_hooks=False,
        )
        cache.set_field("series", {self.ids[0]: "Greater Eternal Champion Saga",
                                   self.ids[1]: "Old"})
        cache.set_field("series_index", {self.ids[1]: 4.5})
        cache.set_field("#series2", {self.ids[1]: "Old sub"})
        cache.set_field("#series2_index", {self.ids[1]: 7.5})
        cache.close()
        self.before = m.snapshot(self.lib)["books"]
        self.baseline = self.root / "baseline.csv"
        self.csv = self.root / "input.csv"
        write_csv(self.baseline, [{"id": str(i), **b} for i, b in self.before.items()])

    def changes(self):
        i = self.ids[1]
        return [assignment(i, self.before[i], **{
            "series": "Synthetic universe", "series_index": "4.5",
            "#series2": "Synthetic sub", "#series2_index": "7.5",
            "#series3": "Synthetic arc", "#series3_index": "0.5",
            "#series4": "Synthetic collection", "#series4_index": "-1",
        })]

    def test_native_full_apply_readback_protection_and_idempotence(self):
        write_csv(self.csv, self.changes())
        report = m.build_report(self.lib, self.csv, self.baseline)
        self.assertFalse(report["errors"])
        result = m.apply_report(report, self.lib, self.csv, self.baseline,
                                report["approval"], self.root / "backups")
        self.assertEqual(result["status"], "verified")
        after = m.snapshot(self.lib)["books"]
        self.assertEqual(after[self.ids[1]]["#series2_index"], 7.5)
        self.assertEqual(after[self.ids[1]]["series_index"], 4.5)
        self.assertEqual(after[self.ids[1]]["#series3_index"], 0.5)
        self.assertEqual(after[self.ids[1]]["#series4_index"], -1)
        for i in self.ids:
            if i != self.ids[1]:
                self.assertEqual(after[i], self.before[i])
        self.assertFalse(m.build_report(self.lib, self.csv, self.baseline)["changes"])
        receipt = Path(result["backup"])
        self.assertTrue((receipt / "verified.json").exists())
        self.assertFalse((self.lib / ".chronology-incomplete.json").exists())
        with m.connect_readonly(receipt / "metadata.db") as conn:
            self.assertEqual(conn.execute("SELECT count(*) FROM books").fetchone()[0], 6)

    def test_rehearsal_failure_does_not_touch_target(self):
        write_csv(self.csv, self.changes())
        report = m.build_report(self.lib, self.csv, self.baseline)
        with patch.object(m, "native_write", side_effect=RuntimeError("injected")):
            with self.assertRaisesRegex(RuntimeError, "injected"):
                m.apply_report(report, self.lib, self.csv, self.baseline,
                               report["approval"], self.root / "backups")
        self.assertEqual(m.snapshot(self.lib)["books"], self.before)

    def test_failed_live_write_retains_recovery_records(self):
        write_csv(self.csv, self.changes())
        report = m.build_report(self.lib, self.csv, self.baseline)
        actual_write = m.native_write
        calls = []

        def fail_live(library, changes):
            calls.append(str(library))
            if len(calls) == 2:
                partial = copy.deepcopy(changes[:1])
                partial[0]["fields"] = {"series": partial[0]["fields"]["series"]}
                actual_write(library, partial)
                raise RuntimeError("simulated interruption")
            return actual_write(library, changes)

        with patch.object(m, "native_write", side_effect=fail_live):
            with self.assertRaisesRegex(RuntimeError, "simulated interruption"):
                m.apply_report(report, self.lib, self.csv, self.baseline,
                               report["approval"], self.root / "backups")
        runs = list((self.root / "backups").iterdir())
        self.assertEqual(len(runs), 1)
        self.assertTrue((runs[0] / "before.json").exists())
        self.assertTrue((runs[0] / "apply-started.json").exists())
        self.assertFalse((runs[0] / "verified.json").exists())
        self.assertTrue((self.lib / ".chronology-incomplete.json").exists())
        changed = m.build_report(self.lib, self.csv, self.baseline)
        with self.assertRaisesRegex(m.Refusal, "recovery review"):
            m.apply_report(changed, self.lib, self.csv, self.baseline,
                           changed["approval"], self.root / "other-backups")

    def test_shared_calibre_writer_lock_refuses(self):
        from calibre.utils.lock import SingleInstance
        write_csv(self.csv, self.changes())
        report = m.build_report(self.lib, self.csv, self.baseline)
        with SingleInstance("db") as acquired:
            self.assertTrue(acquired)
            with self.assertRaisesRegex(m.Refusal, "lock is already held"):
                m.apply_report(report, self.lib, self.csv, self.baseline,
                               report["approval"], self.root / "backups")
        self.assertEqual(m.snapshot(self.lib)["books"], self.before)

    def test_unrelated_metadata_mutation_detected(self):
        from calibre.db.backend import DB
        from calibre.db.cache import Cache
        guard = m.metadata_guard(self.lib, {self.ids[1]})
        cache = Cache(DB(str(self.lib), load_user_formatter_functions=False))
        cache.init()
        cache.set_field("comments", {self.ids[1]: "An unexpected metadata change"})
        cache.close()
        with self.assertRaisesRegex(m.Refusal, "Unrelated metadata"):
            m.verify_guard(self.lib, {self.ids[1]}, guard)

    def test_clear_all_series_levels(self):
        m.native_write(self.lib, [{
            "id": self.ids[1], "fields": {
                "#series3": {"before": None, "after": "Temporary"},
                "#series3_index": {"before": 1, "after": 8},
                "#series4": {"before": None, "after": "Temporary"},
                "#series4_index": {"before": 1, "after": 9},
            },
        }])
        row = assignment(self.ids[1], self.before[self.ids[1]], **{f: "" for f in m.FIELDS})
        row["order_basis"] = "standalone"
        write_csv(self.csv, [row])
        report = m.build_report(self.lib, self.csv, self.baseline)
        self.assertFalse(report["errors"])
        m.apply_report(report, self.lib, self.csv, self.baseline,
                       report["approval"], self.root / "backups")
        after = m.snapshot(self.lib)["books"][self.ids[1]]
        for name in m.SERIES:
            self.assertIsNone(after[name])
            self.assertEqual(after[m.INDEX[name]], 1.0)


if __name__ == "__main__":
    suite = unittest.TestSuite(
        unittest.defaultTestLoader.loadTestsFromTestCase(cls)
        for cls in (ImporterTests, NativeTests)
    )
    sys.exit(not unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful())
