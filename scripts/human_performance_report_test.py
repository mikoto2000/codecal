import unittest
from human_performance_report import summarize


class PerformanceReportTest(unittest.TestCase):
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
