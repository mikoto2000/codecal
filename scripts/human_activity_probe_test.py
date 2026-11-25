"""Synthetic-only Phase 0 experiments; never open a user's Rei database."""
import json
import sqlite3
import tempfile
import unittest
from pathlib import Path

from human_activity_probe import projection


class ProjectionTest(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(":memory:")
        self.addCleanup(self.db.close)
        self.db.execute("CREATE TABLE activity_records(id TEXT PRIMARY KEY, captured_at INTEGER, payload TEXT)")

    def insert(self, payload):
        self.db.execute("INSERT INTO activity_records VALUES('synthetic-id', 1000, ?)", (payload,))

    def test_only_allowed_values_leave_sqlite(self):
        self.insert(json.dumps({"durationEstimate": 60, "foreground": {"title": "SECRET_PROBE_TITLE"}, "detection": {"diagnostics": {"entertainmentDisposition": "ENTERTAINMENT"}}}))
        row = self.db.execute(projection()).fetchone()
        self.assertEqual(row, ("synthetic-id", 1000, 60, "entertainment", "ok"))
        self.assertNotIn("SECRET_PROBE_TITLE", repr(row))

    def test_unknown_disposition_does_not_escape(self):
        self.insert(json.dumps({"durationEstimate": 0, "detection": {"diagnostics": {"entertainmentDisposition": "SECRET_UNKNOWN_VALUE"}}}))
        self.assertEqual(self.db.execute(projection()).fetchone()[2:], (0, "unknown", "ok"))

    def test_bad_json_is_a_countable_error_without_original_content(self):
        self.insert('{"SECRET_INVALID_JSON":')
        self.assertEqual(self.db.execute(projection()).fetchone()[2:], (None, "unknown", "invalid_json"))

    def test_oversized_blob_is_not_parsed(self):
        self.insert("SECRET_HUGE_" + "x" * (1024 * 1024))
        self.assertEqual(self.db.execute(projection()).fetchone()[2:], (None, "unknown", "payload_too_large"))

    def test_wrong_duration_type_does_not_escape(self):
        for duration in ["SECRET_DURATION", True, {}, [], 1.5, None]:
            with self.subTest(duration_type=type(duration).__name__):
                self.db.execute("DELETE FROM activity_records")
                self.insert(json.dumps({"durationEstimate": duration}))
                self.assertEqual(self.db.execute(projection()).fetchone()[2:], (None, "unknown", "invalid_duration_type"))

    def test_missing_disposition_is_unknown(self):
        self.insert('{"durationEstimate": 10}')
        self.assertEqual(self.db.execute(projection()).fetchone()[2:], (10, "unknown", "ok"))

    def test_json_integer_outside_sqlite_range_is_rejected(self):
        self.insert('{"durationEstimate":9223372036854775808}')
        self.assertEqual(self.db.execute(projection()).fetchone()[2:], (None, "unknown", "invalid_duration_type"))

    def test_all_saved_classifications_and_wrong_types(self):
        for value, expected in [("ENTERTAINMENT", "entertainment"), ("NON_ENTERTAINMENT", "non_entertainment"), ("UNCERTAIN", "unknown"), (None, "unknown"), ({"secret": "SECRET_OBJECT"}, "unknown"), (42, "unknown")]:
            with self.subTest(value_type=type(value).__name__):
                self.db.execute("DELETE FROM activity_records")
                self.insert(json.dumps({"durationEstimate": 1, "detection": {"diagnostics": {"entertainmentDisposition": value}}}))
                self.assertEqual(self.db.execute(projection()).fetchone()[3:], (expected, "ok"))

    def test_wal_and_data_version_on_same_readonly_connection(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "synthetic.db"
            writer = sqlite3.connect(path)
            self.addCleanup(writer.close)
            writer.execute("PRAGMA journal_mode=WAL")
            writer.execute("CREATE TABLE activity_records(id TEXT PRIMARY KEY, captured_at INTEGER, payload TEXT)")
            writer.commit()
            reader = sqlite3.connect(path.as_uri() + "?mode=ro", uri=True)
            try:
                reader.execute("PRAGMA query_only=ON")
                version = reader.execute("PRAGMA data_version").fetchone()[0]
                writer.execute("INSERT INTO activity_records VALUES('wal', 1000, '{\"durationEstimate\":1}')")
                writer.commit()
                self.assertNotEqual(reader.execute("PRAGMA data_version").fetchone()[0], version)
                self.assertEqual(reader.execute(projection()).fetchone()[2], 1)
                reader.execute("BEGIN")
                self.assertEqual(reader.execute("SELECT count(*) FROM activity_records").fetchone()[0], 1)
                writer.execute("INSERT INTO activity_records VALUES('concurrent', 2000, '{\"durationEstimate\":1}')")
                writer.commit()
                self.assertEqual(reader.execute("SELECT count(*) FROM activity_records").fetchone()[0], 1)
                reader.commit()
                self.assertEqual(reader.execute("SELECT count(*) FROM activity_records").fetchone()[0], 2)
                with self.assertRaises(sqlite3.OperationalError):
                    reader.execute("DELETE FROM activity_records")
            finally:
                reader.close()
                writer.close()


if __name__ == "__main__":
    unittest.main()
