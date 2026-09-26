import unittest
from decimal import Decimal
from unittest import mock

from worker.billing import command_candidates, parse_report, period_of, run_report, summarize

SAMPLE = """
[
  {
    "object_id": "ap-h3",
    "description": "minimax-h3-latest-workflows",
    "environment": "main",
    "interval_start": "2026-09-24T00:00:00",
    "cost": "4.48481370"
  },
  {
    "object_id": "ap-h3",
    "description": "minimax-h3-latest-workflows",
    "environment": "main",
    "interval_start": "2026-09-25T00:00:00",
    "cost": "0.72047527"
  },
  {
    "object_id": "ap-yue2",
    "description": "yue2-music",
    "environment": "main",
    "interval_start": "2026-09-24T00:00:00",
    "cost": "0.15060521"
  },
  {
    "object_id": "ap-old",
    "description": "aura-japanese-asr",
    "environment": "main",
    "interval_start": "2026-08-31T00:00:00",
    "cost": "9.00000000"
  }
]
"""


class PeriodTest(unittest.TestCase):
    def test_month_is_taken_from_the_interval(self):
        self.assertEqual(period_of("2026-09-24T00:00:00"), "2026-09")

    def test_missing_interval_falls_back_to_a_month_tag(self):
        self.assertRegex(period_of(None), r"^\d{4}-\d{2}$")


class ParseReportTest(unittest.TestCase):
    def test_rows_are_normalized(self):
        rows = parse_report(SAMPLE)
        self.assertEqual(len(rows), 4)
        self.assertEqual(rows[0]["description"], "minimax-h3-latest-workflows")
        self.assertEqual(rows[0]["period"], "2026-09")

    def test_leading_noise_before_the_array_is_ignored(self):
        rows = parse_report("warning: newer client available\n" + SAMPLE)
        self.assertEqual(len(rows), 4)

    def test_empty_output_is_rejected(self):
        with self.assertRaises(ValueError):
            parse_report("   ")


class SummarizeTest(unittest.TestCase):
    def test_totals_and_object_breakdown_use_the_newest_month(self):
        summary = summarize(parse_report(SAMPLE))
        self.assertEqual(summary["period"], "2026-09")
        self.assertEqual(summary["periods"], ["2026-09"])
        self.assertEqual(Decimal(summary["total"]), Decimal("5.35589418"))
        self.assertEqual(summary["intervals"], 3)
        self.assertEqual(
            [(item["description"], Decimal(item["cost"])) for item in summary["apps"]],
            [
                ("minimax-h3-latest-workflows", Decimal("5.20528897")),
                ("yue2-music", Decimal("0.15060521")),
            ],
        )

    def test_explicit_period_keeps_only_that_month(self):
        summary = summarize(parse_report(SAMPLE), "2026-08")
        self.assertEqual(summary["period"], "2026-08")
        self.assertEqual(summary["periods"], ["2026-08"])
        self.assertEqual(Decimal(summary["total"]), Decimal("9.00000000"))

    def test_a_report_without_intervals_reports_no_period_to_write(self):
        summary = summarize([])
        self.assertEqual(summary["periods"], [])
        self.assertEqual(summary["intervals"], 0)
        self.assertEqual(Decimal(summary["total"]), Decimal("0"))
        self.assertRegex(summary["period"], r"^\d{4}-\d{2}$")

    def test_the_newest_period_wins_over_the_local_month(self):
        # A KST month start can fall in the previous UTC month; the report itself
        # decides which period is written, so older history is not replaced.
        rows = parse_report(SAMPLE)
        summary = summarize(rows)
        self.assertEqual(summary["period"], max(row["period"] for row in rows))

    def test_unparsable_cost_does_not_break_the_report(self):
        rows = [{"object_id": "a", "description": "b", "interval_start": "2026-09-01T00:00:00", "period": "2026-09", "cost": "n/a"}]
        summary = summarize(rows)
        self.assertEqual(Decimal(summary["total"]), Decimal("0"))


class ResolveCommandTest(unittest.TestCase):
    def test_a_command_is_always_available(self):
        candidates = command_candidates()
        self.assertTrue(candidates)
        self.assertTrue(all(isinstance(part, str) and part for part in candidates[0]))


class RunReportTest(unittest.TestCase):
    def _completed(self, returncode, stdout="", stderr=""):
        return mock.Mock(returncode=returncode, stdout=stdout, stderr=stderr)

    def test_the_first_working_runner_is_used(self):
        with mock.patch("worker.billing.command_candidates", return_value=[["modal"], ["python", "-m", "modal"]]):
            with mock.patch("worker.billing.subprocess.run", return_value=self._completed(0, "[]")) as run:
                self.assertEqual(run_report("today"), "[]")
        self.assertEqual(run.call_args[0][0][:3], ["modal", "billing", "report"])

    def test_a_missing_binary_falls_through_to_the_next_runner(self):
        runs = [FileNotFoundError("modal"), self._completed(0, "[1]")]
        with mock.patch("worker.billing.command_candidates", return_value=[["modal"], ["python", "-m", "modal"]]):
            with mock.patch("worker.billing.subprocess.run", side_effect=runs):
                self.assertEqual(run_report("today"), "[1]")

    def test_a_run_that_failed_loudly_is_reported_once(self):
        with mock.patch("worker.billing.command_candidates", return_value=[["modal"], ["python", "-m", "modal"]]):
            with mock.patch(
                "worker.billing.subprocess.run",
                return_value=self._completed(1, stderr="Token missing."),
            ) as run:
                with self.assertRaises(RuntimeError) as caught:
                    run_report("today")
        self.assertEqual(run.call_count, 1)
        self.assertIn("Token missing.", str(caught.exception))

    def test_a_hung_report_times_out(self):
        import subprocess

        with mock.patch("worker.billing.command_candidates", return_value=[["modal"]]):
            with mock.patch(
                "worker.billing.subprocess.run",
                side_effect=subprocess.TimeoutExpired(cmd="modal", timeout=5),
            ):
                with self.assertRaises(RuntimeError):
                    run_report("today")


if __name__ == "__main__":
    unittest.main()
