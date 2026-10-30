#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Reproduce the scalar fixture using actual immutable Wave TypedProgram facts."""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile

from wave_bootstrap import DEFAULT_RUST_TOOLCHAIN, RUST_VERSION, WAVE_REVISION


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--wave-source", type=Path, required=True)
    parser.add_argument("--target-dir", type=Path, required=True)
    parser.add_argument("--rust-toolchain", default=DEFAULT_RUST_TOOLCHAIN)
    parser.add_argument("--update", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    repository = args.wave_source.resolve()
    output = args.target_dir.resolve()
    if output == repository or output.is_relative_to(repository):
        raise ValueError("HIR fixture output must be outside the read-only Wave checkout")
    output.mkdir(parents=True, exist_ok=True)
    revision = subprocess.check_output(
        ["git", "-C", str(repository), "rev-parse", WAVE_REVISION + "^{commit}"], text=True).strip()
    if revision != WAVE_REVISION:
        raise ValueError("pinned Wave commit is unavailable")
    rustc = subprocess.check_output(
        ["rustup", "which", "--toolchain", args.rust_toolchain, "rustc"], text=True).strip()
    version = subprocess.check_output([rustc, "--version"], text=True).strip()
    if not version.startswith("rustc " + RUST_VERSION + " "):
        raise ValueError("the pinned frontend requires Rust " + RUST_VERSION)
    archive = subprocess.check_output(
        ["git", "-C", str(repository), "archive", "--format=tar", WAVE_REVISION])
    environment = os.environ.copy()
    environment.update(CARGO_NET_OFFLINE="true", CARGO_INCREMENTAL="0", RUSTUP_AUTO_INSTALL="0")
    for name in ("CARGO_BUILD_TARGET", "WHALE_WAVE_IR_DIR", "WHALE_ASMOBJ_DIR"):
        environment.pop(name, None)
    with tempfile.TemporaryDirectory(prefix="wave-hir-fixture-", dir=output) as directory:
        directory = Path(directory)
        source = directory / "source"
        source.mkdir()
        with tarfile.open(fileobj=io.BytesIO(archive)) as tree:
            tree.extractall(source, filter="data")
        build = output / "build"
        messages = subprocess.check_output(
            ["cargo", "+" + args.rust_toolchain, "build", "--locked", "--offline",
             "--jobs", "2", "--message-format=json", "--manifest-path", str(source / "Cargo.toml"),
             "--target-dir", str(build), "-p", "hir", "-p", "parser"],
            text=True, env=environment)
        dependencies = build / "debug" / "deps"
        libraries = {}
        for line in messages.splitlines():
            message = json.loads(line)
            if message.get("reason") != "compiler-artifact":
                continue
            name = message["target"]["name"]
            if name in ("hir", "parser"):
                paths = [Path(path) for path in message["filenames"] if path.endswith(".rlib")]
                if len(paths) != 1:
                    raise ValueError("HIR compiler artifact requires one library: " + name)
                libraries[name] = paths[0]
        if set(libraries) != {"hir", "parser"}:
            raise ValueError("Cargo omitted pinned HIR/parser library artifacts")
        binary = directory / "wave-hir-fixture"
        exporter = root / "tools" / "wave_hir_fixture.rs"
        subprocess.run([rustc, "--edition=2021", str(exporter), "--extern",
                        "hir=" + str(libraries["hir"]), "--extern",
                        "parser=" + str(libraries["parser"]), "-L",
                        "dependency=" + str(dependencies), "-o", str(binary)], check=True, env=environment)
        actual = json.loads(subprocess.check_output([str(binary)], env=environment))
    if actual["wave_revision"] != WAVE_REVISION:
        raise ValueError("fixture exporter revision differs from the pinned source")
    data = (json.dumps(actual, indent=2) + "\n").encode()
    fixture = root / "ir" / "tests" / "fixtures" / "wave_typed_hir_scalar.json"
    if args.update:
        fixture.write_bytes(data)
    elif fixture.read_bytes() != data:
        raise ValueError("checked-in Wave TypedProgram fixture differs from the pinned frontend")
    (output / "provenance.json").write_text(json.dumps({
        "wave_revision": WAVE_REVISION,
        "source_archive_sha256": hashlib.sha256(archive).hexdigest(),
        "exporter_sha256": hashlib.sha256(exporter.read_bytes()).hexdigest(),
        "fixture_sha256": hashlib.sha256(data).hexdigest(),
        "rustc": version,
    }, indent=2) + "\n")
    print("Pinned Wave TypedProgram scalar fixture reproduced")


if __name__ == "__main__":
    main()
