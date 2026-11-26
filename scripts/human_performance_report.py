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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("samples", type=Path, nargs="+")
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    rows = [json.loads(line) for path in args.samples
            for line in path.read_text(encoding="utf-8-sig").splitlines() if line.strip()]
    groups = summarize(rows)
    print(json.dumps(dict(minimum_samples=5, percentile_method="nearest-rank", core=groups), indent=2))
    if args.check and (not groups or not all(group["core_gates_passed"] for group in groups)):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
