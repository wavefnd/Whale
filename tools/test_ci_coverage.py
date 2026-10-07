"""Exercise LCOV aggregation for Cargo workspace package reports."""

from pathlib import Path
import tempfile
import textwrap
import unittest

from ci_coverage import summarize


class CoverageAggregationTests(unittest.TestCase):
    def workspace(self):
        return tempfile.TemporaryDirectory()

    def write_workspace(self, root):
        (root / "Cargo.toml").write_text(
            textwrap.dedent(
                """\
                [package]
                name = "root"

                [workspace]
                members = ["crates/member", "crates/member/nested"]
                """
            ),
            encoding="utf-8",
        )
        for path, name in [
            ("crates/member", "member"),
            ("crates/member/nested", "nested"),
        ]:
            crate = root / path
            crate.mkdir(parents=True)
            (crate / "Cargo.toml").write_text(
                textwrap.dedent(
                    f"""\
                    [package]
                    name = "{name}"
                    """
                ),
                encoding="utf-8",
            )

    def test_nested_package_uses_most_specific_crate(self):
        with self.workspace() as directory:
            root = Path(directory)
            self.write_workspace(root)
            lcov = textwrap.dedent(
                f"""\
                SF:{root}/src/main.rs
                DA:1,1
                SF:{root}/crates/member/src/lib.rs
                DA:1,1
                SF:{root}/crates/member/nested/src/lib.rs
                DA:1,0
                """
            )

            report = summarize(lcov, root)

            self.assertIn("| root | 1 | 1 | 100.00% |", report)
            self.assertIn("| member | 1 | 1 | 100.00% |", report)
            self.assertIn("| nested | 0 | 1 | 0.00% |", report)

    def test_repeated_line_records_merge_by_highest_hit_count(self):
        with self.workspace() as directory:
            root = Path(directory)
            self.write_workspace(root)
            lcov = textwrap.dedent(
                f"""\
                SF:{root}/src/main.rs
                DA:1,0
                DA:1,3
                DA:2,0
                SF:{root}/crates/member/src/lib.rs
                DA:1,1
                SF:{root}/crates/member/nested/src/lib.rs
                DA:1,1
                """
            )

            report = summarize(lcov, root)

            self.assertIn("| root | 1 | 2 | 50.00% |", report)

    def test_missing_crate_coverage_reports_crate_name(self):
        with self.workspace() as directory:
            root = Path(directory)
            self.write_workspace(root)
            lcov = textwrap.dedent(
                f"""\
                SF:{root}/src/main.rs
                DA:1,1
                SF:{root}/crates/member/src/lib.rs
                DA:1,1
                """
            )

            with self.assertRaisesRegex(ValueError, "nested"):
                summarize(lcov, root)

    def test_root_relative_source_paths_resolve_from_workspace_root(self):
        with self.workspace() as directory:
            root = Path(directory)
            self.write_workspace(root)
            lcov = textwrap.dedent(
                """\
                SF:src/main.rs
                DA:1,1
                SF:crates/member/src/lib.rs
                DA:1,0
                SF:crates/member/nested/src/lib.rs
                DA:1,1
                """
            )

            report = summarize(lcov, root)

            self.assertIn("| root | 1 | 1 | 100.00% |", report)
            self.assertIn("| member | 0 | 1 | 0.00% |", report)
            self.assertIn("| nested | 1 | 1 | 100.00% |", report)


if __name__ == "__main__":
    unittest.main()
