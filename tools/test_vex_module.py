# SPDX-License-Identifier: MPL-2.0
"""Verify Vex result admission and complete-set publication failures."""

from argparse import Namespace
import copy
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import vex_module as helper


class PublicationTests(unittest.TestCase):
    def test_unchanged_outputs_keep_their_mtimes(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)
            files = {
                "archive.a": b"archive",
                "module.meta": b"metadata",
                "provenance.json": b"provenance",
            }
            helper.publish(output, files)
            before = {name: (output / name).stat().st_mtime_ns for name in files}
            helper.publish(output, files)
            self.assertEqual(
                before, {name: (output / name).stat().st_mtime_ns for name in files}
            )

    def test_every_failed_replace_preserves_the_previous_complete_set(self):
        replace = helper.os.replace
        for previous in (
            {},
            {
                "archive.a": b"old",
                "module.meta": b"old-meta",
                "provenance.json": b"old-proof",
            },
        ):
            for boundary in range(3):
                with (
                    self.subTest(previous=bool(previous), boundary=boundary),
                    tempfile.TemporaryDirectory() as temporary,
                ):
                    output = Path(temporary)
                    for name, data in previous.items():
                        (output / name).write_bytes(data)
                    calls = 0

                    def fail_once(source, destination):
                        nonlocal calls
                        calls += 1
                        if calls == boundary + 1:
                            raise OSError("injected publication failure")
                        return replace(source, destination)

                    with patch.object(helper.os, "replace", side_effect=fail_once):
                        with self.assertRaises(OSError):
                            helper.publish(
                                output,
                                {
                                    "archive.a": b"new",
                                    "module.meta": b"new-meta",
                                    "provenance.json": b"new-proof",
                                },
                            )
                    self.assertEqual(
                        {p.name: p.read_bytes() for p in output.iterdir()}, previous
                    )

    def test_symlinks_and_nonlocal_names_do_not_modify_other_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            other = root / "other"
            other.write_bytes(b"keep")
            output = root / "module"
            output.mkdir()
            for name in ("../other", "nested/file", ".", ".."):
                with self.assertRaises(ValueError):
                    helper.publish(output, {name: b"bad"})
            try:
                (output / "archive").symlink_to(other)
            except OSError:
                self.skipTest("symlinks unavailable")
            with self.assertRaises(ValueError):
                helper.publish(output, {"archive": b"bad"})
            self.assertEqual(other.read_bytes(), b"keep")


class ResultTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name).resolve()
        self.source = root / "role.wave"
        self.source.write_bytes(b"source")
        self.abi = root / "bridge.rs"
        self.abi.write_bytes(b"abi")
        self.args = Namespace(
            module="asmobj",
            vex=root / "vex",
            build_dir=root / "vex-build",
            state_dir=root / "state",
            output_dir=root / "output",
            toolchain=root / "toolchain.json",
            target="x86_64-unknown-linux-gnu",
            profile="dev",
            depfile=None,
        )
        self.args.vex.write_bytes(b"vex")
        self.toolchain = dict(
            wavec="/compiler",
            std="/std",
            source_revision="a" * 40,
            compiler_sha256="b" * 64,
            std_sha256="c" * 64,
            archive_sha256="d" * 64,
        )
        self.args.toolchain.write_text(json.dumps(self.toolchain))
        self.generation = self.args.build_dir / "target/.vex-run/123-456"
        self.generation.mkdir(parents=True)
        lock = self.args.build_dir / ".vex/run-locks/123-456.lock"
        lock.parent.mkdir(parents=True)
        lock.touch()
        self.archive = self.generation / "archive.a"
        self.archive.write_bytes(b"!<arch>\n")
        self.result = dict(
            schema="vex.artifact-generation",
            schema_version=1,
            generation=str(self.generation),
            provenance={"portable": "proof"},
            metadata=dict(
                artifact_kind="static",
                target=self.args.target,
                profile={"name": "dev", "opt_level": 0},
                package_target={"name": "whale_asmobj"},
                compiler={
                    **{k: self.toolchain[k] for k in ("compiler_sha256", "std_sha256")},
                    "compiler_version": "test",
                },
                inputs={"files": [], "directories": []},
            ),
            artifacts=[
                dict(
                    path=str(self.archive),
                    role="primary",
                    sha256=helper.digest(self.archive),
                )
            ],
        )
        self.action = None
        self.addCleanup(patch.stopall)
        patch.object(helper, "inputs", return_value=([self.source], [self.abi])).start()
        patch.object(
            helper,
            "input_hashes",
            side_effect=lambda paths: (
                "a" * 16,
                hashlib.sha256(b"".join(p.read_bytes() for p in paths)).hexdigest(),
            ),
        ).start()
        patch.object(
            helper.subprocess, "check_output", return_value="vex 0.0.3-beta\n"
        ).start()
        patch.object(helper.subprocess, "run", side_effect=self.invoke).start()

    def invoke(self, command, **kwargs):
        if self.action:
            self.action()
        report = dict(schema="vex.artifacts", schema_version=1, results=[self.result])
        (self.args.build_dir / "result.json").write_text(json.dumps(report))

    def test_admits_verified_generation_and_records_one_compiler_identity(self):
        helper.build(self.args)
        proof = json.loads((self.args.output_dir / "provenance.json").read_bytes())
        self.assertEqual(proof["wave_revision"], self.toolchain["source_revision"])
        self.assertEqual(proof["std_sha256"], self.toolchain["std_sha256"])
        self.assertEqual(proof["vex_sha256"], helper.digest(self.args.vex))
        self.assertEqual(
            (self.args.output_dir / "libwhale_asmobj.a").read_bytes(),
            self.archive.read_bytes(),
        )

    def test_bad_target_profile_package_compiler_and_digest_never_publish(self):
        initial = copy.deepcopy(self.result)
        for field, value in (
            ("target", "other-host"),
            ("profile", {"name": "release", "opt_level": 2}),
            ("package_target", {"name": "other"}),
            ("artifact_kind", "executable"),
            ("compiler", {"compiler_sha256": "e" * 64, "std_sha256": "c" * 64}),
        ):
            with self.subTest(field=field):
                self.result = copy.deepcopy(initial)
                self.result["metadata"][field] = value
                with self.assertRaises(ValueError):
                    helper.build(self.args)
                self.assertFalse(self.args.output_dir.exists())
        self.result = initial
        self.archive.write_bytes(b"corrupt")
        with self.assertRaisesRegex(ValueError, "SHA256"):
            helper.build(self.args)
        self.assertFalse(self.args.output_dir.exists())

    def test_failed_vex_and_mutated_inputs_preserve_previous_files(self):
        self.args.output_dir.mkdir()
        previous = self.args.output_dir / "libwhale_asmobj.a"
        previous.write_bytes(b"last-good")
        actions = [
            lambda: self.source.write_bytes(b"changed"),
            lambda: self.abi.write_bytes(b"changed"),
            lambda: self.args.vex.write_bytes(b"changed"),
        ]

        def fail():
            raise subprocess.CalledProcessError(1, "vex")

        actions.append(fail)
        for action in actions:
            with self.subTest(action=action):
                self.action = action
                with self.assertRaises((ValueError, subprocess.CalledProcessError)):
                    helper.build(self.args)
                self.assertEqual(previous.read_bytes(), b"last-good")
                self.assertEqual(list(self.args.output_dir.iterdir()), [previous])
                self.source.write_bytes(b"source")
                self.abi.write_bytes(b"abi")
                self.args.vex.write_bytes(b"vex")

    def test_old_vex_is_rejected_before_compilation(self):
        with patch.object(
            helper.subprocess, "check_output", return_value="vex 0.0.2-beta"
        ):
            with self.assertRaisesRegex(ValueError, "v0.0.3"):
                helper.build(self.args)
        helper.subprocess.run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
