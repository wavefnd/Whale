#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Compare actual Wave emit/load/run with an explicitly built Rust IR reference."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[1]
CASES = {
    "interpreted-calls-v4.wir": [(0, ["41"]), (3, []), (5, ["4"]), (6, ["4"])],
    "interpreter-loop-v3.wir": [(7, ["0"]), (7, ["1"]), (7, ["11"])],
    "tracked-memory-v4.wir": [(0, []), (1, [])],
    "initialization-loop-v4.wir": [(0, [])],
    "integer-operations-v3.wir": [(index, []) for index in range(5)],
    "float-storage-v7.wir": [(0, ["0x7e55"]), (1, ["0x7fc12345"]),
                             (2, ["0x7ff8123456789abc"]), (3, [])],
    "float-comparisons-v7.wir": [(0, ["0x7ff8000000000001", "0x3ff0000000000000"]),
                                 (1, ["0x7ff8000000000001", "0x3ff0000000000000"]),
                                 (2, ["0x0000", "0x8000"])],
    "runtime-storage-v6.wir": [(index, []) for index in (0, 1, 3, 4, 5, 6, 7)],
    "runtime-diagnostics-v7.wir": [(0, [])],
}


def compare(binary, reference, artifacts):
    binary = binary.resolve(strict=True)
    reference = reference.resolve(strict=True)
    artifacts.mkdir(parents=True, exist_ok=True)
    transcript = []
    with tempfile.TemporaryDirectory(prefix="whale-bytecode-reference-") as temporary:
        directory = Path(temporary)
        environment = dict(os.environ, PATH="")

        def invoke(executable, arguments):
            result = subprocess.run([str(executable), *map(str, arguments)], cwd=directory,
                                    env=environment, capture_output=True, timeout=20)
            transcript.append({"binary": str(executable), "args": list(map(str, arguments)),
                               "status": result.returncode, "stdout": result.stdout.decode(),
                               "stderr": result.stderr.decode()})
            return result

        try:
            for name, cases in CASES.items():
                source = (ROOT / "ir/tests/fixtures" / name).read_text()
                source = source.replace('"x86_64-whale-linux"', '"whale-bytecode-v1"')
                text = directory / name
                image = text.with_suffix(".wbc")
                text.write_text(source)
                emitted = invoke(binary, ["ir", "emit", text, "-o", image])
                if emitted.returncode:
                    raise AssertionError(f"{name}: bytecode compilation failed")
                content = image.read_bytes()
                second = invoke(binary, ["ir", "emit", text, "-o", image])
                if second.returncode or image.read_bytes() != content:
                    raise AssertionError(f"{name}: nondeterministic bytecode output")
                for function, arguments in cases:
                    flags = ["--function", f"@f{function}"]
                    for literal in arguments:
                        flags += ["--arg", literal]
                    for limits in ([], ["--max-steps", "0"], ["--max-memory", "0"]):
                        expected = invoke(reference, ["ir", "run", text, *flags, *limits])
                        actual = invoke(binary, ["bytecode", "run", image, *flags, *limits])
                        # Paths identify the different input containers, not semantic sites.
                        expected_error = expected.stderr.replace((str(text) + ": ").encode(), b"")
                        actual_error = actual.stderr.replace((str(image) + ": ").encode(), b"")
                        if (actual.returncode, actual.stdout, actual_error) != (
                                expected.returncode, expected.stdout, expected_error):
                            raise AssertionError(f"{name}: @f{function} {arguments} {limits} differs")
                transcript.append({"image": name, "bytes": len(content),
                                   "sha256": hashlib.sha256(content).hexdigest()})
        finally:
            (artifacts / "bytecode-reference.json").write_text(
                json.dumps(transcript, indent=2) + "\n")
    count = sum(map(len, CASES.values())) * 3
    print(f"Wave bytecode: {count} value/trap/site/limit comparisons with Rust reference passed")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--reference", type=Path, required=True)
    parser.add_argument("--artifacts", type=Path, required=True)
    arguments = parser.parse_args()
    compare(arguments.binary, arguments.reference, arguments.artifacts)


if __name__ == "__main__":
    main()
