"""Phase 0 SQLite projection probe. Inputs are generated inside a temporary directory.

This is not the production reader or a benchmark of Rust/IPC/UI performance.
Run: python scripts/human_activity_probe.py --sizes 10000 100000 1000000
"""
import argparse
import json
import sqlite3
import tempfile
import time
from pathlib import Path


def projection():
    # Nested CASE is intentional: SQLite must not parse huge or malformed JSON.
    # Normalize within SQLite: even an unknown original disposition cannot escape.
    return """
    SELECT id, captured_at,
      CASE WHEN status='ok' THEN json_extract(payload,'$.durationEstimate') END,
      CASE WHEN status='ok' THEN
        CASE json_extract(payload,'$.detection.diagnostics.entertainmentDisposition')
          WHEN 'ENTERTAINMENT' THEN 'entertainment'
          WHEN 'NON_ENTERTAINMENT' THEN 'non_entertainment'
          ELSE 'unknown' END
        ELSE 'unknown' END,
      status
    FROM (
      SELECT id,captured_at,payload,
        CASE WHEN typeof(payload)<>'text' THEN 'invalid_payload_type'
          WHEN length(CAST(payload AS BLOB))>1048576 THEN 'payload_too_large'
          WHEN NOT json_valid(payload) THEN 'invalid_json'
          WHEN json_type(payload,'$.durationEstimate') IS NOT 'integer'
            THEN 'invalid_duration_type'
          WHEN typeof(json_extract(payload,'$.durationEstimate'))<>'integer'
            THEN 'invalid_duration_type'
          ELSE 'ok' END AS status
      FROM activity_records ORDER BY id
    )
    """


def benchmark(size):
    with tempfile.TemporaryDirectory(prefix="codecal-synthetic-") as directory:
        path = Path(directory) / "source.db"
        writer = sqlite3.connect(path)
        try:
            writer.execute("PRAGMA journal_mode=WAL")
            writer.execute("CREATE TABLE activity_records(id TEXT PRIMARY KEY, captured_at INTEGER NOT NULL, payload TEXT NOT NULL)")
            writer.execute("CREATE INDEX activity_records_time ON activity_records(captured_at)")
            writer.execute("CREATE TABLE agent_events(sequence INTEGER)")
            payload = json.dumps({"durationEstimate": 60, "foreground": {"title": "SECRET_SYNTHETIC_ONLY"}, "detection": {"diagnostics": {"entertainmentDisposition": "NON_ENTERTAINMENT"}}})
            for first in range(0, size, 1000):
                writer.executemany("INSERT INTO activity_records VALUES(?,?,?)", ((f"{i:012}", 1700000000000 + i * 60000, payload) for i in range(first, min(first + 1000, size))))
                writer.commit()
            reader = sqlite3.connect(path.as_uri() + "?mode=ro", uri=True)
            try:
                reader.execute("PRAGMA query_only=ON")
                version = reader.execute("PRAGMA data_version").fetchone()[0]

                def scan():
                    start = time.perf_counter()
                    count = 0
                    reader.execute("BEGIN")
                    try:
                        cursor = reader.execute(projection())
                        while page := cursor.fetchmany(256):
                            count += len(page)
                            assert "SECRET_" not in repr(page)
                        reader.commit()
                    except BaseException:
                        reader.rollback()
                        raise
                    return {"ms": round((time.perf_counter() - start) * 1000, 2), "rows": count}

                result = {"rows": size, "initial_projection": scan()}
                start = time.perf_counter()
                assert reader.execute("PRAGMA data_version").fetchone()[0] == version
                result["unchanged_check_ms"] = round((time.perf_counter() - start) * 1000, 3)
                writer.execute("INSERT INTO activity_records VALUES(?,?,?)", (f"{size:012}", 1700000000000 + size * 60000, payload))
                writer.commit()
                result["one_added_projection"] = scan()
                writer.execute("UPDATE activity_records SET payload=? WHERE id='000000000000'", ('{"durationEstimate":1}',))
                writer.commit()
                result["one_updated_projection"] = scan()
                writer.execute("INSERT INTO agent_events VALUES(1)")
                writer.commit()
                result["ai_only_projection"] = scan()
                reader.execute("BEGIN")
                assert reader.execute("SELECT count(*) FROM activity_records").fetchone()[0] == size + 1
                wal_path = Path(str(path) + "-wal")
                before = wal_path.stat().st_size
                start = time.perf_counter()
                writer.execute("INSERT INTO activity_records VALUES(?,?,?)", (f"{size + 1:012}", 1700000000000 + (size + 1) * 60000, payload))
                writer.commit()
                result["writer_commit_with_reader_ms"] = round((time.perf_counter() - start) * 1000, 3)
                result["wal_growth_during_pinned_read_bytes"] = wal_path.stat().st_size - before
                assert reader.execute("SELECT count(*) FROM activity_records").fetchone()[0] == size + 1
                reader.commit()
                assert reader.execute("SELECT count(*) FROM activity_records").fetchone()[0] == size + 2
                result["sqlite_version"] = sqlite3.sqlite_version
                result["source_db_bytes"] = path.stat().st_size
                result["source_wal_bytes"] = Path(str(path) + "-wal").stat().st_size
                return result
            finally:
                reader.close()
        finally:
            writer.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sizes", type=int, nargs="+", default=[10000, 100000, 1000000])
    for count in parser.parse_args().sizes:
        if count < 1 or count > 1000000:
            parser.error("sizes must be within 1..1000000")
        print(json.dumps(benchmark(count)), flush=True)
