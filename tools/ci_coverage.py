"""Aggregate LCOV line coverage for every Cargo workspace package."""

import argparse
import os
from pathlib import Path
import tomllib


def summarize(lcov, root):
    manifest = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
    packages = {}
    for member in [".", *manifest["workspace"]["members"]]:
        directory = (root / member).resolve()
        package = tomllib.loads((directory / "Cargo.toml").read_text(encoding="utf-8"))
        packages[directory] = package["package"]["name"]
    counts = {name: {} for name in packages.values()}
    selected = None
    source = None
    for line in lcov.splitlines():
        if line.startswith("SF:"):
            source_path = Path(line[3:])
            if not source_path.is_absolute():
                source_path = root / source_path
            source = source_path.resolve()
            selected = None
            # Prefer the most specific package over the root workspace package.
            for directory in sorted(packages, key=lambda path: len(path.parts), reverse=True):
                if source.is_relative_to(directory / "src"):
                    selected = counts[packages[directory]]
                    break
        elif line.startswith("DA:") and selected is not None:
            number, hits, *_ = line[3:].split(",")
            key = (source, int(number))
            selected[key] = max(selected.get(key, 0), int(hits))
    rows = ["| Crate | Covered lines | Instrumented lines | Line coverage |",
            "| --- | ---: | ---: | ---: |"]
    for name, lines in sorted(counts.items()):
        if not lines:
            raise ValueError(f"coverage report is missing source lines for {name}")
        covered = sum(hits > 0 for hits in lines.values())
        rows.append(f"| {name} | {covered} | {len(lines)} | {covered / len(lines):.2%} |")
    return "\n".join(rows) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("lcov", type=Path)
    args = parser.parse_args()
    report = summarize(args.lcov.read_text(encoding="utf-8"), Path.cwd())
    print(report)
    args.lcov.with_name("coverage-crates.md").write_text(report, encoding="utf-8")
    if summary := os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(summary, "a", encoding="utf-8") as output:
            output.write(report)


if __name__ == "__main__":
    main()
