# SPDX-License-Identifier: MPL-2.0
"""Assembler archive publication, selection and source/ABI guard regressions."""

import argparse
import hashlib
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import build_wave_assembler as helper
from wave_bootstrap import Bootstrap


ROOT = Path(__file__).resolve().parents[1]
NATIVE = "x86_64-unknown-linux-gnu"


class PublicationTests(unittest.TestCase):
    def test_all_owned_wave_roles_are_in_the_explicit_build_lists(self):
        owned = {path.relative_to(ROOT).as_posix()
                 for directory in (ROOT / "assembler", *(ROOT / "assembler" / name
                                                     for name in helper.MODULES if name != "common"))
                 for path in directory.glob("*.wave")}
        configured = {file for config in helper.MODULES.values() for file in config["sources"]}
        self.assertEqual(owned, configured)

    def test_compilation_archiving_and_partial_publication_preserve_last_good_set(self):
        for module, config in helper.MODULES.items():
            name = config["name"]
            names = (f"libwhale_wave_{name}.a", "provenance.json", f"wave-{name}.meta")
            failures = ("compile", "archive", *names)
            for failure in failures:
                with self.subTest(module=module, failure=failure), tempfile.TemporaryDirectory() as tmp:
                    root = Path(tmp).resolve()
                    wave = root / "Wave"
                    wave.mkdir()
                    marker = wave / "unrelated.wave"
                    marker.write_bytes(b"owned elsewhere")
                    output = root / "output"
                    output.mkdir()
                    for file in names:
                        (output / file).write_bytes(b"previous " + file.encode())
                    compiler = root / "compiler"
                    compiler.write_bytes(b"pinned compiler")
                    bootstrap = Bootstrap(compiler, root / "std", root / "bootstrap.json",
                                          {"std_sha256": "1" * 64}, True)
                    args = argparse.Namespace(module=module, wave_source=wave, out_dir=output,
                        bootstrap_dir=root / "cache", rust_toolchain="1.89", target=NATIVE,
                        opt_level="2", allow_network=False)
                    original_replace = os.replace
                    injected = False

                    def replace(source, destination):
                        nonlocal injected
                        if not injected and Path(destination) == output / failure:
                            injected = True
                            raise OSError("injected publication failure")
                        return original_replace(source, destination)

                    def run(command, **kwargs):
                        command = list(map(str, command))
                        if "-o" in command:
                            kernel = Path(command[command.index("build") + 1])
                            self.assertEqual(kernel.read_bytes(), b"kernel")
                            Path(command[command.index("-o") + 1]).write_bytes(b"object")
                            if failure == "compile":
                                raise subprocess.CalledProcessError(1, command)
                        else:
                            Path(command[-2]).write_bytes(b"new archive")
                            if failure == "archive":
                                raise subprocess.CalledProcessError(1, command)

                    with patch.object(helper, "ensure_bootstrap", return_value=bootstrap), \
                         patch.object(helper, "source_bytes", return_value=b"kernel"), \
                         patch.object(helper, "abi_bytes", return_value=b"ABI"), \
                         patch.object(helper, "run", side_effect=run), \
                         patch.object(helper.os, "replace", side_effect=replace):
                        with self.assertRaises((OSError, subprocess.CalledProcessError)):
                            helper.build(args)
                    for file in names:
                        self.assertEqual((output / file).read_bytes(), b"previous " + file.encode())
                    self.assertEqual(marker.read_bytes(), b"owned elsewhere")
                    self.assertEqual({p.name for p in output.iterdir()}, set(names))


class GuardTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory()
        cls.directory = Path(cls.temporary.name).resolve()
        cls.guards = {}
        for module in helper.MODULES:
            source = ROOT / "assembler" / ("build.rs" if module == "common" else module + "/build.rs")
            binary = cls.directory / module
            subprocess.run(["rustc", "--edition=2021", "-Dwarnings", str(source), "-o", str(binary)],
                           check=True, capture_output=True)
            cls.guards[module] = binary

    @classmethod
    def tearDownClass(cls):
        cls.temporary.cleanup()

    def test_plain_cargo_is_available_and_all_stale_or_corrupt_inputs_are_rejected(self):
        for module, config in helper.MODULES.items():
            with self.subTest(module=module), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp).resolve()
                for file in (*config["sources"], *config["abi"]):
                    path = root / file
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_bytes(file.encode())
                source = root / "assembler"
                if module != "common":
                    source /= module
                output = root / "archive"
                output.mkdir()
                name = config["name"]
                archive = output / f"libwhale_wave_{name}.a"
                provenance = output / "provenance.json"
                metadata = output / f"wave-{name}.meta"
                archive.write_bytes(b"archive")
                provenance.write_bytes(b"pinned provenance")
                original = helper.metadata(helper.source_bytes(root, module),
                    helper.abi_bytes(root, module), archive.read_bytes(), provenance.read_bytes(), NATIVE, "2")
                metadata.write_text(original)
                env = os.environ.copy()
                for variable in ("WHALE_WAVE_ASSEMBLER_DIR", "WHALE_WAVE_AMD64_DIR"):
                    env.pop(variable, None)
                env.update(CARGO_MANIFEST_DIR=str(source), TARGET=NATIVE)
                plain = subprocess.run([str(self.guards[module])], env=env, capture_output=True)
                self.assertEqual(plain.returncode, 0, plain.stderr)
                self.assertNotIn(b"rustc-link-lib", plain.stdout)
                env["WHALE_WAVE_ASSEMBLER_DIR"] = str(output)
                env[f"WHALE_WAVE_{name.upper()}_DIR"] = str(output)

                def check(path, replacement, accepted=False):
                    previous = path.read_bytes()
                    path.write_bytes(replacement)
                    try:
                        result = subprocess.run([str(self.guards[module])], env=env, capture_output=True)
                        self.assertEqual(result.returncode == 0, accepted, result.stderr)
                    finally:
                        path.write_bytes(previous)

                check(metadata, original.encode(), True)
                for file in (*config["sources"], *config["abi"]):
                    check(root / file, b"changed source or boundary")
                check(archive, b"corrupt archive")
                check(provenance, b"corrupt provenance")
                check(metadata, original.replace("schema=1", "schema=2").encode())
                check(metadata, original.replace("opt_level=2", "opt_level=1").encode())
                check(metadata, (original + "schema=1\n").encode())
                check(metadata, original.replace(NATIVE, "aarch64-unknown-linux-gnu").encode())
                check(metadata, b"x" * 4097)
                check(provenance, b"x" * (16 * 1024 + 1))
                self.assertEqual(hashlib.sha256(archive.read_bytes()).hexdigest(),
                                 hashlib.sha256(b"archive").hexdigest())


if __name__ == "__main__":
    unittest.main()
