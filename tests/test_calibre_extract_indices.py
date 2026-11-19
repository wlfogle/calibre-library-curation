"""Extraction tests never touch a real library."""

import importlib.util
from pathlib import Path
import sys
import unittest
import uuid
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1] / "scripts"
sys.path.insert(0, str(SCRIPTS))
SPEC = importlib.util.spec_from_file_location("extract_indices", SCRIPTS / "calibre_extract_indices.py")
m = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(m)


class ExtractionTests(unittest.TestCase):
    def setUp(self):
        self.ident = str(uuid.uuid4())
        self.rows = {1: {
            "id": "1", "uuid": self.ident, "title": "Keep this",
            "series": "Keep universe", "series_index": "27",
            "#series2": "Branch", "#series3": "", "#series4": "",
        }}
        self.values = {"#series2": {1: ("Branch", 2.5)}, "#series3": {}, "#series4": {}}
        self.db = ({1: self.ident}, self.values)

    def test_exact_fraction_and_original_values_preserved(self):
        rows, counts, issues = m.enrich(self.rows, self.db, self.db)
        self.assertFalse(issues)
        self.assertEqual(rows[0]["#series2_index"], "2.5")
        self.assertEqual(rows[0]["#series3_index"], "")
        self.assertEqual(rows[0]["index_match_series2"], "exact")
        for field, value in self.rows[1].items():
            self.assertEqual(rows[0][field], value)
        self.assertEqual(counts["#series2"]["extracted"], 1)
        self.assertNotIn("order_verified", rows[0])

    def test_whitespace_equivalence_does_not_rename_original(self):
        self.values["#series2"][1] = ("Branch\n Name", 0.3)
        self.rows[1]["#series2"] = "Branch  Name"
        rows, counts, issues = m.enrich(self.rows, self.db, self.db)
        self.assertFalse(issues)
        self.assertEqual(rows[0]["#series2"], "Branch  Name")
        self.assertEqual(rows[0]["#series2_index"], "0.3")
        self.assertEqual(counts["#series2"]["whitespace_equivalent"], 1)

    def test_real_name_change_is_unresolved(self):
        self.values["#series2"][1] = ("Other branch", 2)
        rows, _, issues = m.enrich(self.rows, self.db)
        self.assertEqual(rows[0]["#series2_index"], "")
        self.assertEqual(issues[0]["reason"], "series_name_conflict")

    def test_case_change_is_not_guessed(self):
        self.values["#series2"][1] = ("branch", 2)
        self.assertTrue(m.enrich(self.rows, self.db)[2])

    def test_backup_conflict_not_silently_preferred(self):
        other = ({1: self.ident}, {**self.values, "#series2": {1: ("Branch", 99)}})
        rows, _, issues = m.enrich(self.rows, self.db, other)
        self.assertEqual(rows[0]["#series2_index"], "")
        self.assertEqual(issues[0]["reason"], "database_backup_conflict")

    def test_uuid_missing_or_mismatch_refuses(self):
        for identity in ({}, {1: str(uuid.uuid4())}):
            with self.subTest(identity=identity):
                with self.assertRaises(m.Refusal):
                    m.enrich(self.rows, (identity, self.values))

    def test_missing_and_invalid_indices_not_defaulted(self):
        for value in (None, float("nan"), float("inf")):
            self.values["#series2"][1] = ("Branch", value)
            rows, _, issues = m.enrich(self.rows, self.db)
            self.assertTrue(issues)
            self.assertEqual(rows[0]["#series2_index"], "")
        self.values["#series2"].clear()
        self.assertTrue(m.enrich(self.rows, self.db)[2])

    def test_existing_index_conflict_held_and_approval_cleared(self):
        self.rows[1]["#series2_index"] = "99"
        self.rows[1]["order_verified"] = "yes"
        rows, _, issues = m.enrich(self.rows, self.db)
        self.assertEqual(rows[0]["order_verified"], "")
        self.assertEqual(issues[0]["reason"], "existing_csv_index_conflict")

    def test_no_writable_sqlite_mode(self):
        with patch.object(m, "connect_readonly") as connect:
            connect.side_effect = RuntimeError("read-only entrypoint")
            with self.assertRaisesRegex(RuntimeError, "read-only entrypoint"):
                m.database_values(Path("/nonexistent/test.db"))
            connect.assert_called_once()


if __name__ == "__main__":
    unittest.main()
