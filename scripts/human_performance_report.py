"""Summarize synthetic timing samples without treating missing data as success."""
import argparse
import json
import math
import statistics
from collections import defaultdict
from pathlib import Path

BUDGETS = {
    "initial_sync_ms": 10000,
    "unchanged_check_ms": 100,
    "one_added_sync_ms": 10000,
    "one_updated_sync_ms": 10000,
    "ai_only_sync_ms": 10000,
    "day.ms": 100,
    "week.ms": 200,
    "month_summary.ms": 500,
}


def summarize(rows):
    groups = defaultdict(list)
    for row in rows:
        size, kind, cadence = row.get("rows"), row.get("id_kind"), row.get("cadence_ms")
        if (type(size) is not int or not 1 <= size <= 1000000
                or kind not in ("sequential", "uuid")
                or type(cadence) is not int or not 1 <= cadence <= 60000):
            raise ValueError("invalid synthetic benchmark group")
        groups[(size, kind, cadence)].append(row)
    out = []
    for (size, kind, cadence), samples in sorted(groups.items()):
        metrics = {}
        for name, budget in BUDGETS.items():
            values = []
            for row in samples:
                value = row
                for field in name.split("."):
                    value = value.get(field) if isinstance(value, dict) else None
                if value is None:
                    continue
                if type(value) not in (float, int) or not math.isfinite(value) or value < 0:
                    raise ValueError("timings must be finite nonnegative numbers")
                values.append(value)
            values.sort()
            p95 = values[math.ceil(len(values) * .95) - 1] if values else None
            status = ("unmeasured" if not values else "failed" if p95 > budget
                      else "insufficient_samples" if len(values) < 5 else "passed")
            metrics[name] = dict(samples=len(values), median_ms=statistics.median(values) if values else None,
                                 p95_ms=p95, max_ms=max(values) if values else None,
                                 budget_ms=budget, status=status)
        failed = sum("initial_error" in row for row in samples)
        out.append(dict(rows=size, id_kind=kind, cadence_ms=cadence, rounds=len(samples),
                        failed_rounds=failed, metrics=metrics,
                        core_gates_passed=not failed and all(m["status"] == "passed" for m in metrics.values())))
    return out


def summarize_worker(rows):
    """Gate full worker trials; notification waiting is reported separately."""
    for row in rows:
        if row.get("rows") != 1000000 or row.get("id_kind") != "uuid" or row.get("payload_bytes") != 200:
            raise ValueError("worker fixture must use one million UUID rows and 200 byte payloads")
    metrics = {}
    for name, budget in [("initial_worker_ms", 10000), ("forced_worker_ms", 10000),
                         ("unchanged_active_ms", 100), ("summary_ms", 500)]:
        values = []
        for row in rows:
            value = row.get(name)
            if value is None:
                continue
            values.extend(value if name == "unchanged_active_ms" and isinstance(value, list) else [value])
        if any(type(v) not in (int, float) or not math.isfinite(v) or v < 0 for v in values):
            raise ValueError("worker timings must be finite nonnegative numbers")
        values.sort()
        p95 = values[math.ceil(len(values)*.95)-1] if values else None
        minimum = 25 if name == "unchanged_active_ms" else 5
        status = ("unmeasured" if not values else "failed" if p95 > budget
                  else "insufficient_samples" if len(values) < minimum else "passed")
        metrics[name] = dict(samples=len(values), median_ms=statistics.median(values) if values else None,
                             p95_ms=p95, max_ms=max(values) if values else None,
                             budget_ms=budget, status=status)
    writer = []
    wal = []
    for row in rows:
        baseline, concurrent = row.get("writer_baseline_p95_ms"), row.get("writer_concurrent_p95_ms")
        valid = all(type(v) in (int, float) and math.isfinite(v) and v >= 0 for v in (baseline, concurrent))
        writer.append(valid and type(row.get("writer_baseline_samples")) is int
                      and type(row.get("writer_concurrent_samples")) is int
                      and row["writer_baseline_samples"] >= 200 and row["writer_concurrent_samples"] >= 200
                      and concurrent <= baseline*2 and concurrent-baseline <= 100
                      and row.get("writer_budget_pass") is True)
        checkpoint = row.get("writer_passive_checkpoint_after_read")
        wal.append(isinstance(checkpoint, list) and len(checkpoint) == 3
                   and all(type(v) is int and v >= 0 for v in checkpoint)
                   and checkpoint[0] == 0 and checkpoint[1] == checkpoint[2]
                   and row.get("fixture_writer_truncated_wal_bytes") == 0)
    passed = len(rows) >= 5 and all(m["status"] == "passed" for m in metrics.values())
    passed = passed and all(writer) and all(wal) and all(r.get("followup_caught_writes") is True for r in rows)
    return dict(rounds=len(rows), metrics=metrics, writer_rounds_passed=sum(writer),
                wal_rounds_passed=sum(wal), worker_gates_passed=passed)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("samples", type=Path, nargs="+")
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--worker", action="store_true")
    args = parser.parse_args()
    rows = [json.loads(line) for path in args.samples
            for line in path.read_text(encoding="utf-8-sig").splitlines() if line.strip()]
    if args.worker:
        result = summarize_worker(rows)
        print(json.dumps(dict(minimum_samples=5, percentile_method="nearest-rank", worker=result), indent=2))
        if args.check and not result["worker_gates_passed"]:
            raise SystemExit(1)
        return
    groups = summarize(rows)
    print(json.dumps(dict(minimum_samples=5, percentile_method="nearest-rank", core=groups), indent=2))
    if args.check and (not groups or not all(group["core_gates_passed"] for group in groups)):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
