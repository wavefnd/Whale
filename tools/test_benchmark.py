"""Correctness gates for performance reports; no compiler or timer dependency."""
import copy
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import benchmark


def record():
    return {"phase": "asm-encode", "count": 64, "input_bytes": 640,
            "output_bytes": 198, "elapsed_ns": 100, "result_digest": "a" * 16, "correct": True}


class BenchmarkChecks(unittest.TestCase):
    def test_wrong_or_failed_probe_workload_is_rejected(self):
        for key, value in [("phase", "other"), ("count", 63), ("correct", False),
                           ("elapsed_ns", -1), ("output_bytes", "198")]:
            with self.subTest(key=key):
                changed = record()
                changed[key] = value
                with self.assertRaises(ValueError):
                    benchmark.validate_record(changed, "asm-encode", 64)

    def test_warmups_are_checked_but_only_samples_enter_the_median(self):
        records = [record() for _ in range(4)]
        for item, ns in zip(records, [999, 100, 200, 900]):
            item["elapsed_ns"] = ns
        with patch("benchmark.execute", side_effect=[(json.dumps(r).encode(), 0.1, 1024) for r in records]):
            result = benchmark.sample_phase("small/asm-encode", 64, ["probe"], 3, 1, 1, "timer",
                lambda data, _: benchmark.validate_record(json.loads(data), "asm-encode", 64))
        self.assertEqual(len(result["samples"]), 3)
        self.assertAlmostEqual(result["median"]["phase_seconds"], 200 / 1e9)

    def test_different_output_during_warmup_or_sample_fails(self):
        first, second = record(), record()
        second["result_digest"] = "b" * 16
        with patch("benchmark.execute", side_effect=[(json.dumps(r).encode(), 0.1, 1024) for r in [first, second]]):
            with self.assertRaisesRegex(ValueError, "non-deterministic"):
                benchmark.sample_phase("small/asm-encode", 64, ["probe"], 1, 1, 1, "timer",
                    lambda data, _: benchmark.validate_record(json.loads(data), "asm-encode", 64))

    def test_cli_checks_actual_bytes_and_does_not_accept_a_missing_output(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, expected, actual = root / "source", root / "expected", root / "actual"
            source.write_bytes(b"assembly")
            expected.write_bytes(b"valid object")
            check = benchmark.cli_validator("asm-cli-e2e", 64, source, actual, expected)
            with self.assertRaises(ValueError):
                check(b"", 0.1)
            actual.write_bytes(b"wrong object")
            with self.assertRaises(ValueError):
                check(b"", 0.1)
            actual.write_bytes(expected.read_bytes())
            self.assertEqual(check(b"", 0.1)["output_bytes"], 12)
            # The driver's before hook removes prior output before every run,
            # so an old successful artifact cannot hide a missing new write.
            with patch("benchmark.execute", return_value=(b"", 0.1, 1024)):
                with self.assertRaises(ValueError):
                    benchmark.sample_phase("small/asm-cli-e2e", 64, ["cli"], 1, 0, 1, "timer", check,
                        before=lambda: actual.unlink(missing_ok=True))

    def test_comparisons_reject_changed_environments_workloads_and_results(self):
        entry = {"name": "small/asm-encode", "fingerprint": benchmark.fingerprint(record()),
                 "median": {"phase_seconds": 2.0, "peak_rss_bytes": 1024}}
        base = {"format_version": 1, "generator": "v1", "profile": "release", "rustc": "rustc", "host": {"cpu": "CPU"},
                "build_environment": {"CARGO_INCREMENTAL": "0"},
                "warmups": 1, "runs": 3, "counts": [64, 32768], "measurements": [entry],
                "workloads": [{"size": "small", "count": 64, "sha256": {"input.asm": "a"}}]}
        after = copy.deepcopy(base)
        after["revision"] = "different revision is expected"
        after["measurements"][0]["median"]["phase_seconds"] = 1.0
        self.assertEqual(benchmark.compare_reports(base, after)["small/asm-encode"]["phase_time_ratio"], 0.5)
        for key in ["host", "rustc", "profile", "build_environment", "counts", "warmups", "runs"]:
            broken = copy.deepcopy(after)
            broken[key] = "different"
            with self.assertRaises(ValueError):
                benchmark.compare_reports(base, broken)
        broken = copy.deepcopy(after)
        broken["workloads"][0]["sha256"]["input.asm"] = "b"
        with self.assertRaises(ValueError):
            benchmark.compare_reports(base, broken)

    def test_comparison_rejects_duplicate_measurements_in_either_report(self):
        entry = {"name": "small/asm-encode", "fingerprint": benchmark.fingerprint(record()),
                 "median": {"phase_seconds": 2.0, "peak_rss_bytes": 1024}}
        base = {"format_version": 1, "generator": "v1", "profile": "release", "rustc": "rustc", "host": {"cpu": "CPU"},
                "build_environment": {"CARGO_INCREMENTAL": "0"}, "warmups": 1, "runs": 3,
                "counts": [64, 32768], "measurements": [entry], "workloads": []}
        current = copy.deepcopy(base)
        self.assertIn("small/asm-encode", benchmark.compare_reports(base, current))
        for side in ("baseline", "current"):
            for position in (0, 1):
                for same_fingerprint in (True, False):
                    with self.subTest(side=side, position=position, same=same_fingerprint):
                        before = copy.deepcopy(base)
                        after = copy.deepcopy(current)
                        target = before if side == "baseline" else after
                        duplicate = copy.deepcopy(entry)
                        if not same_fingerprint:
                            duplicate["fingerprint"]["result_digest"] = "wrong"
                        target["measurements"].insert(position, duplicate)
                        with self.assertRaisesRegex(ValueError, "duplicate measurement"):
                            benchmark.compare_reports(before, after)
        broken = copy.deepcopy(after)
        broken["measurements"][0]["fingerprint"]["output_bytes"] += 1
        with self.assertRaises(ValueError):
            benchmark.compare_reports(base, broken)


if __name__ == "__main__":
    unittest.main()
