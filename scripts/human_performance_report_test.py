import unittest
from human_performance_report import summarize, summarize_worker


class PerformanceReportTest(unittest.TestCase):
    def worker_rows(self):
        return [dict(rows=1000000, id_kind="uuid", payload_bytes=200,
                     initial_worker_ms=9000, forced_worker_ms=6000,
                     unchanged_active_ms=[1, 2, 3, 4, 5], unchanged_queue_ms=[1200]*5,
                     summary_ms=20, writer_baseline_samples=200, writer_concurrent_samples=210,
                     writer_baseline_p95_ms=10, writer_concurrent_p95_ms=15,
                     writer_budget_pass=True, followup_caught_writes=True,
                     writer_passive_checkpoint_after_read=[0, 123, 123],
                     fixture_writer_truncated_wal_bytes=0) for _ in range(5)]

    def test_worker_counts_active_work_separately_from_debounce_queue(self):
        result=summarize_worker(self.worker_rows())
        self.assertTrue(result["worker_gates_passed"])
        self.assertEqual(result["metrics"]["unchanged_active_ms"]["samples"],25)
        self.assertEqual(result["metrics"]["unchanged_active_ms"]["p95_ms"],5)

    def test_worker_does_not_hide_a_slow_or_missing_round(self):
        rows=self.worker_rows()
        rows[1]["initial_worker_ms"]=11532
        self.assertFalse(summarize_worker(rows)["worker_gates_passed"])
        self.assertFalse(summarize_worker(self.worker_rows()[:4])["worker_gates_passed"])

    def test_worker_requires_writer_budget_caught_updates_and_complete_wal_release(self):
        for field,value in [("writer_concurrent_p95_ms",21),
                            ("followup_caught_writes",False),
                            ("writer_passive_checkpoint_after_read",[0,123,122])]:
            rows=self.worker_rows()
            rows[0][field]=value
            self.assertFalse(summarize_worker(rows)["worker_gates_passed"])
    def rows(self, initial=100):
        return [dict(rows=1000000, id_kind="uuid", cadence_ms=60000, round=i+1,
                     initial_sync_ms=initial, unchanged_check_ms=1,
                     one_added_sync_ms=100, one_updated_sync_ms=100,
                     ai_only_sync_ms=100, day={"ms":1}, week={"ms":2},
                     month_summary={"ms":3}) for i in range(5)]

    def test_median_and_p95_use_all_repeats_and_do_not_hide_a_slow_round(self):
        rows=self.rows()
        for row,value in zip(rows,[1,5,3,2,11000]):
            row["initial_sync_ms"]=value
        group=summarize(rows)[0]
        metric=group["metrics"]["initial_sync_ms"]
        self.assertEqual(metric["median_ms"],3)
        self.assertEqual(metric["p95_ms"],11000)
        self.assertEqual(metric["status"],"failed")
        self.assertFalse(group["core_gates_passed"])

    def test_missing_or_too_few_samples_are_not_success(self):
        rows=self.rows()
        del rows[-1]["week"]
        self.assertEqual(summarize(rows)[0]["metrics"]["week.ms"]["status"],"insufficient_samples")
        self.assertFalse(summarize(rows[:1])[0]["core_gates_passed"])

    def test_sync_failure_is_kept_and_unknown_error_text_is_not_propagated(self):
        rows=self.rows()
        rows[0]={"rows":1000000,"id_kind":"uuid","cadence_ms":60000,"round":1,"initial_error":"PRIVATE_INPUT"}
        group=summarize(rows)[0]
        self.assertEqual(group["failed_rounds"],1)
        self.assertFalse(group["core_gates_passed"])
        self.assertNotIn("PRIVATE_INPUT",str(group))

    def test_distributions_and_cadences_are_kept_separate(self):
        other=self.rows()
        for row in other:
            row["id_kind"]="sequential"
        groups=summarize(self.rows()+other)
        self.assertEqual(len(groups),2)
        self.assertTrue(all(group["core_gates_passed"] for group in groups))

    def test_nonfinite_or_boolean_timings_cannot_pass(self):
        for value in [float("nan"),float("inf"),True,-1]:
            rows=self.rows(value)
            with self.assertRaises(ValueError):
                summarize(rows)


if __name__ == "__main__":
    unittest.main()
