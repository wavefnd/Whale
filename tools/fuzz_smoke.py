"""Run the dependency-free bounded mutation targets with fixed replay seeds.

Build: cargo build --locked --all-features --example fuzz_boundaries
Run: python tools/fuzz_smoke.py --binary target/debug/examples/fuzz_boundaries
The separate memory-model target is documented in fuzz/README.md.
"""
import argparse
import os
from pathlib import Path
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--cases", type=int, default=512)
    parser.add_argument("--seed", type=int, default=1464353100)
    parser.add_argument("--timeout", type=int, default=60)
    parser.add_argument("--target", choices=["assembler", "object", "ir", "ast", "bytecode"], action="append")
    parser.add_argument("--memory-mib", type=int, default=512)
    args = parser.parse_args()
    if not 1 <= args.cases <= 100000 or not 1 <= args.timeout <= 300 or not 0 <= args.seed < 2**64 or not 64 <= args.memory_mib <= 4096:
        parser.error("cases must be 1..100000, timeout 1..300 seconds and seed an unsigned u64")
    root = Path(__file__).resolve().parents[1]
    binary = args.binary.resolve()
    env = dict(os.environ)
    env.setdefault("WHALE_FUZZ_ARTIFACT_DIR", str(root / ".ci-artifacts/fuzz"))
    for target in args.target or ["assembler", "object", "ir", "ast", "bytecode"]:
        subprocess.run([sys.executable, str(root / "tools/fuzz_limits.py"), "--memory-mib", str(args.memory_mib), "--", str(binary), target, str(args.cases), str(args.seed)], cwd=root, env=env, check=True, timeout=args.timeout)


if __name__ == "__main__":
    main()
