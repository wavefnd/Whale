"""Verify that an explicitly selected Whale product contains only its ISA crates."""

import argparse
from pathlib import Path
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[1]
ISAS = frozenset({"amd64", "aarch64", "riscv64", "loongarch64"})


def package_names(tree):
    """Read package identities from Cargo's one-package-per-line tree output."""
    names = set()
    for line in tree.splitlines():
        name, separator, _ = line.partition(" v")
        if not separator or not name or any(char.isspace() for char in name):
            raise ValueError(f"unexpected cargo tree package: {line!r}")
        names.add(name)
    if "whale" not in names:
        raise ValueError("cargo tree omitted the Whale product")
    return names


def check_product(root, selected):
    unknown = set(selected) - ISAS
    if unknown:
        raise ValueError(f"unknown ISA selection: {sorted(unknown)}")
    command = [
        "cargo", "tree", "-p", "whale", "--no-default-features",
        "--locked", "--offline", "-e", "normal,build", "--prefix", "none",
        "--format", "{p}",
    ]
    if selected:
        command += ["--features", ",".join(f"asm-{isa}" for isa in sorted(selected))]
    result = subprocess.run(command, cwd=root, capture_output=True, text=True, check=False)
    if result.returncode:
        raise ValueError(f"cargo tree failed ({result.returncode}): {result.stderr.strip()}")
    actual = package_names(result.stdout) & ISAS
    if actual != set(selected):
        raise ValueError(f"selected ISA crates {sorted(selected)}, product contains {sorted(actual)}")
    return actual


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--isa", choices=sorted(ISAS), action="append", default=[])
    args = parser.parse_args()
    try:
        selected = set(args.isa)
        actual = check_product(args.root, selected)
        print(f"product ISA crates: {', '.join(sorted(actual)) or '(none)'}")
    except ValueError as error:
        parser.exit(1, f"product graph: {error}\n")


if __name__ == "__main__":
    main()
