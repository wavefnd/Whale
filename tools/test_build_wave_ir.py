# SPDX-License-Identifier: MPL-2.0
"""Pinned Wave IR publication and std-only Cargo guard regressions."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import build_wave_ir as helper
from wave_bootstrap import Bootstrap


ROOT = Path(__file__).resolve().parents[1]
NATIVE = "x86_64-unknown-linux-gnu"


class PublicationTests(unittest.TestCase):
    def test_wave_ir_sources_are_explicit_role_files(self):
        self.assertEqual({path.name for path in (ROOT / "ir").glob("*.wave")}, set(helper.SOURCES))

    def test_compile_archive_and_publication_failures_preserve_last_good_set(self):
        for failure in ("compile", "archive", "publish-archive", "publish-provenance", "publish-meta"):
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary).resolve()
                wave = root / "Wave"
                wave.mkdir()
                marker = wave / "user.wave"
                marker.write_bytes(b"unrelated user changes")
                output = root / "output"
                output.mkdir()
                compiler = root / "wavec"
                compiler.write_bytes(b"pinned compiler")
                std = root / "std"
                std.mkdir()
                bootstrap = Bootstrap(compiler, std, root / "bootstrap.json",
                                      {"std_sha256": "1" * 64}, True)
                names = ("libwhale_wave_ir.a", "provenance.json", "wave-ir.meta")
                for name in names:
                    (output / name).write_bytes(b"previous " + name.encode())
                args = argparse.Namespace(wave_source=wave, out_dir=output,
                    bootstrap_dir=root / "cache", rust_toolchain="1.89", target=NATIVE,
                    opt_level="0", allow_network=False)
                real_replace = os.replace
                failed = False

                def replace(source, destination):
                    nonlocal failed
                    selected = {"publish-archive": names[0], "publish-provenance": names[1],
                                "publish-meta": names[2]}.get(failure)
                    if not failed and selected and Path(destination) == output / selected:
                        failed = True
                        raise OSError("injected publication failure")
                    return real_replace(source, destination)

                def run(command, **kwargs):
                    command = list(map(str, command))
                    if "-o" in command:
                        kernel = Path(command[command.index("build") + 1])
                        self.assertEqual(kernel.read_bytes(), helper.source_bytes(ROOT))
                        self.assertEqual(command[0], str(compiler))
                        self.assertIn("--freestanding", command)
                        Path(command[command.index("-o") + 1]).write_bytes(b"object")
                        if failure == "compile":
                            raise subprocess.CalledProcessError(1, command)
                    else:
                        Path(command[-2]).write_bytes(b"new archive")
                        if failure == "archive":
                            raise subprocess.CalledProcessError(1, command)

                with patch.object(helper, "ensure_bootstrap", return_value=bootstrap), \
                     patch.object(helper, "run", side_effect=run), \
                     patch.object(helper.os, "replace", side_effect=replace):
                    with self.assertRaises(OSError if failure.startswith("publish") else subprocess.CalledProcessError):
                        helper.build(args)
                for name in names:
                    self.assertEqual((output / name).read_bytes(), b"previous " + name.encode())
                self.assertEqual(marker.read_bytes(), b"unrelated user changes")
                self.assertFalse(any(path.name.startswith(".wave-ir-") for path in output.iterdir()))

    def test_success_records_source_abi_and_compiler_identity(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            wave = root / "Wave"
            wave.mkdir()
            output = root / "output"
            compiler = root / "wavec"
            compiler.write_bytes(b"compiler")
            std = root / "std"
            std.mkdir()
            bootstrap = Bootstrap(compiler, std, root / "bootstrap.json",
                                  {"std_sha256": "1" * 64}, True)
            args = argparse.Namespace(wave_source=wave, out_dir=output, bootstrap_dir=root / "cache",
                                      rust_toolchain="1.89", target=NATIVE, opt_level="2",
                                      allow_network=False)

            def run(command, **kwargs):
                command = list(map(str, command))
                if "-o" in command:
                    Path(command[command.index("-o") + 1]).write_bytes(b"object")
                else:
                    Path(command[-2]).write_bytes(b"archive")

            with patch.object(helper, "ensure_bootstrap", return_value=bootstrap), \
                 patch.object(helper, "run", side_effect=run):
                self.assertEqual(helper.build(args), output)
            provenance = (output / "provenance.json").read_bytes()
            source = helper.source_bytes(ROOT)
            abi = helper.abi_bytes(ROOT)
            self.assertEqual((output / "wave-ir.meta").read_text(),
                             helper.metadata(source, abi, b"archive", provenance, NATIVE, "2"))
            record = json.loads(provenance)
            self.assertEqual(record["source_sha256"], hashlib.sha256(source).hexdigest())
            self.assertEqual(record["abi_sha256"], hashlib.sha256(abi).hexdigest())
            self.assertEqual(record["compiler_sha256"], hashlib.sha256(b"compiler").hexdigest())
            self.assertEqual(record["wave_revision"], helper.WAVE_REVISION)


class GuardTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory()
        cls.guard = Path(cls.temporary.name).resolve() / "guard"
        subprocess.run(["rustc", "--edition=2021", "-Dwarnings", str(ROOT / "ir/build.rs"),
                        "-o", str(cls.guard)], check=True, capture_output=True)

    @classmethod
    def tearDownClass(cls):
        cls.temporary.cleanup()

    def test_explicit_reference_and_corrupt_artifact_rejection(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            source = root / "ir"
            source.mkdir()
            for name in helper.SOURCES:
                (source / name).write_bytes((name + "\n").encode())
            abi_paths = {}
            for index, name in enumerate(helper.ABI_SOURCES):
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes((name + " ffi").encode())
                abi_paths[f"abi{index}"] = path
            output = root / "archive"
            output.mkdir()
            archive = output / "libwhale_wave_ir.a"
            archive.write_bytes(b"archive")
            provenance = output / "provenance.json"
            provenance.write_bytes(b"pinned provenance")
            metadata = output / "wave-ir.meta"
            original = helper.metadata(helper.source_bytes(root), helper.abi_bytes(root), b"archive",
                                       provenance.read_bytes(), NATIVE, "2")
            metadata.write_text(original)
            environment = os.environ.copy()
            environment.pop("WHALE_WAVE_IR_DIR", None)
            environment.update(CARGO_MANIFEST_DIR=str(source), TARGET=NATIVE)

            def check(changes, expected):
                paths = {"source": source / helper.SOURCES[0], **abi_paths,
                         "archive": archive,
                         "provenance": provenance, "meta": metadata}
                originals = {key: path.read_bytes() for key, path in paths.items()}
                for key, data in changes.items():
                    paths[key].write_bytes(data)
                result = subprocess.run([str(self.guard)], env={**environment,
                    "WHALE_WAVE_IR_DIR": str(output)}, capture_output=True, text=True)
                self.assertEqual(result.returncode == 0, expected is None, result.stderr)
                if expected:
                    self.assertIn(expected, result.stderr)
                    self.assertNotIn("cargo:rustc-link-lib", result.stdout)
                for key, data in originals.items():
                    paths[key].write_bytes(data)

            environment.pop("CARGO_FEATURE_RUST_REFERENCE", None)
            plain = subprocess.run([str(self.guard)], env=environment, capture_output=True)
            self.assertNotEqual(plain.returncode, 0)
            self.assertIn(b"requires its Wave module", plain.stderr)
            reference = subprocess.run([str(self.guard)], env={**environment,
                "CARGO_FEATURE_RUST_REFERENCE": "1"}, capture_output=True)
            self.assertEqual(reference.returncode, 0, reference.stderr)
            self.assertNotIn(b"cargo:rustc-link-lib", reference.stdout)
            check({}, None)
            check({"source": b"changed"}, "stale Wave IR archive")
            for name in abi_paths:
                check({name: b"changed"}, "stale Wave IR ABI")
            check({"archive": b"changed"}, "corrupt Wave IR archive")
            check({"provenance": b"changed"}, "corrupt Wave IR provenance")
            check({"meta": original.replace("schema=1", "schema=2").encode()},
                  "unsupported Wave IR metadata")


if __name__ == "__main__":
    unittest.main()
