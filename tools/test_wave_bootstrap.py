# SPDX-License-Identifier: MPL-2.0
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import wave_bootstrap as bootstrap


class BootstrapTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.source = self.root / "Wave"
        self.source.mkdir()
        self.marker = self.source / "unrelated.wave"
        self.marker.write_bytes(b"uncommitted changes stay here")
        self.output = self.root / "cache"
        self.calls = []
        self.rustc = "rustc 1.89.0 (test)"
        self.version = "wavec 0.0.1-alpha-dev (test host)\n  backend: LLVM 21.1.8"
        self.failure = None
        archive = io.BytesIO()
        with tarfile.open(fileobj=archive, mode="w") as tree:
            for name, data in {"Cargo.toml": b'[package]\nname="wavec"\nversion="0.0.1-alpha-dev"\n',
                               "Cargo.lock": b"pinned lockfile", "std/base.wave": b"pub const X: u32 = 1;"}.items():
                member = tarfile.TarInfo(name)
                member.size = len(data)
                tree.addfile(member, io.BytesIO(data))
        self.archive = archive.getvalue()
        self.check = patch.object(bootstrap.subprocess, "check_output", side_effect=self.check_output).start()
        self.run = patch.object(bootstrap, "run", side_effect=self.run_build).start()
        self.addCleanup(patch.stopall)

    def check_output(self, command, **kwargs):
        arguments = list(map(str, command))
        self.calls.append(arguments)
        if arguments[0] == "git":
            self.assertEqual(arguments[1:3], ["-C", str(self.source)])
            self.assertNotIn("checkout", arguments)
            self.assertNotIn("reset", arguments)
            if "archive" in arguments:
                return self.archive
            return "a" * 40 if arguments[-1].endswith(":std") else bootstrap.WAVE_REVISION
        if arguments[0] == "rustup":
            self.assertEqual(arguments[1:], ["which", "--toolchain", "1.89", "rustc"])
            if self.failure == "toolchain":
                raise subprocess.CalledProcessError(1, arguments)
            return "/installed/rustc"
        if arguments[0] == "/installed/rustc":
            return self.rustc
        self.assertEqual(arguments[1:], ["--version"])
        if self.failure == "version" and "candidate" in arguments[0]:
            return "wavec 99.0.0 (wrong)"
        return self.version

    def run_build(self, command, **kwargs):
        arguments = list(map(str, command))
        self.assertEqual(arguments[:5], ["cargo", "+1.89", "build", "--locked", "--jobs"])
        self.assertNotEqual(kwargs["cwd"], self.source)
        self.assertEqual(kwargs["env"]["WAVE_STD_REVISION"], bootstrap.WAVE_REVISION)
        self.assertEqual(kwargs["env"]["CARGO_INCREMENTAL"], "0")
        self.assertEqual(kwargs["env"]["RUSTUP_AUTO_INSTALL"], "0")
        self.assertIn("--bin", arguments)
        if self.failure == "build":
            raise subprocess.CalledProcessError(1, arguments)
        compiler = self.output / "build/debug" / ("wavec.exe" if os.name == "nt" else "wavec")
        compiler.parent.mkdir(parents=True, exist_ok=True)
        compiler.write_bytes(b"pinned compiler")
        compiler.chmod(0o755)

    def ensure(self, **kwargs):
        result = bootstrap.ensure_bootstrap(self.source, self.output, rust_toolchain="1.89", **kwargs)
        self.assertEqual(self.marker.read_bytes(), b"uncommitted changes stay here")
        return result

    def test_build_publishes_compiler_std_and_provenance_as_one_generation(self):
        result = self.ensure()
        self.assertFalse(result.reused)
        self.assertEqual(result.compiler.parent, result.std_root.parent)
        self.assertEqual(result.compiler.parent, result.provenance.parent)
        self.assertEqual(result.metadata["std_revision"], bootstrap.WAVE_REVISION)
        self.assertEqual(result.metadata["std_tree"], "a" * 40)
        self.assertEqual(result.metadata["compiler_sha256"], bootstrap.digest(result.compiler))
        self.assertEqual(result.metadata["std_sha256"], bootstrap._std_digest(result.std_root))
        self.assertEqual(result.metadata, json.loads(result.provenance.read_text()))
        self.assertIn("--offline", self.run.call_args.args[0])
        self.assertEqual(self.run.call_args.kwargs["env"]["CARGO_NET_OFFLINE"], "true")
        self.assertFalse(any(path.name.startswith(".wave-bootstrap-") for path in self.output.iterdir()))

    def test_second_build_validates_and_reuses_cache_without_cargo_or_archive(self):
        first = self.ensure()
        second = self.ensure()
        self.assertTrue(second.reused)
        self.assertEqual(first.compiler, second.compiler)
        self.assertEqual(self.run.call_count, 1)
        self.assertEqual(sum("archive" in call for call in self.calls), 1)

    def test_allow_network_is_explicit_and_never_installs_rust(self):
        self.ensure(allow_network=True)
        self.assertNotIn("--offline", self.run.call_args.args[0])
        self.assertFalse(any("install" in call for call in self.calls))

    def test_stage_zero_does_not_inherit_final_host_or_whale_archives(self):
        ambient = {"CARGO_BUILD_TARGET": "aarch64-unknown-linux-gnu",
                   "WHALE_WAVE_ELF_DIR": "/previous/elf",
                   "WHALE_WAVE_REVERSE_DIR": "/previous/reverse"}
        with patch.dict(os.environ, ambient):
            self.ensure()
        environment = self.run.call_args.kwargs["env"]
        for name in ambient:
            self.assertNotIn(name, environment)

    def test_compiler_identity_probe_disables_ambient_terminal_colors(self):
        def probe(command, **kwargs):
            self.assertEqual(command, ["wavec", "--version"])
            self.assertEqual(kwargs["env"]["NO_COLOR"], "1")
            self.assertEqual(kwargs["env"]["CLICOLOR"], "0")
            self.assertEqual(kwargs["env"]["CLICOLOR_FORCE"], "0")
            return self.version

        with patch.dict(os.environ, {"CLICOLOR": "1", "CLICOLOR_FORCE": "1"}):
            os.environ.pop("NO_COLOR", None)
            with patch.object(bootstrap.subprocess, "check_output", side_effect=probe):
                self.assertEqual(bootstrap._compiler_version(Path("wavec")), self.version)
            self.assertNotIn("NO_COLOR", os.environ)
            self.assertEqual(os.environ["CLICOLOR_FORCE"], "1")

    def test_missing_or_wrong_rust_does_not_build_or_modify_source(self):
        self.failure = "toolchain"
        with self.assertRaises(subprocess.CalledProcessError):
            self.ensure()
        self.failure = None
        self.rustc = "rustc 1.96.0 (wrong)"
        with self.assertRaisesRegex(bootstrap.BootstrapError, "1.89.0"):
            self.ensure()
        self.run.assert_not_called()
        self.assertFalse(self.output.exists())

    def test_corrupt_compiler_and_std_are_rejected_before_cached_execution(self):
        result = self.ensure()
        original = result.compiler.read_bytes()
        result.compiler.write_bytes(b"corrupt compiler")
        self.check.reset_mock()
        with self.assertRaisesRegex(bootstrap.BootstrapError, "compiler"):
            bootstrap.load_bootstrap(self.output)
        self.check.assert_not_called()
        result.compiler.write_bytes(original)
        (result.std_root / "base.wave").write_bytes(b"changed std")
        with self.assertRaisesRegex(bootstrap.BootstrapError, "std"):
            bootstrap.load_bootstrap(self.output)
        self.check.assert_not_called()

    def test_revision_versions_hashes_and_pointer_paths_are_checked(self):
        result = self.ensure()
        original = result.provenance.read_text()
        for field, value in (("std_revision", "b" * 40), ("wave_revision", "b" * 40),
                             ("std_tree", "not-a-tree"), ("wave_version", "99"),
                             ("schema", True), ("compiler_sha256", "bad")):
            with self.subTest(field=field):
                metadata = json.loads(original)
                metadata[field] = value
                result.provenance.write_text(json.dumps(metadata))
                with self.assertRaises(bootstrap.BootstrapError):
                    bootstrap.load_bootstrap(self.output)
        result.provenance.write_text(original)
        with self.assertRaisesRegex(bootstrap.BootstrapError, "std revision"):
            bootstrap.load_bootstrap(self.output, std_tree="b" * 40)
        (self.output / bootstrap.POINTER_NAME).write_text('{"schema":1,"generation":"../../Wave"}')
        with self.assertRaisesRegex(bootstrap.BootstrapError, "pointer"):
            bootstrap.load_bootstrap(self.output)

    def test_failed_rebuild_or_publication_preserves_last_good_generation(self):
        first = self.ensure()
        pointer = (self.output / bootstrap.POINTER_NAME).read_bytes()
        files = {path: path.read_bytes() for path in (first.compiler, first.provenance, first.std_root / "base.wave")}
        # A different Rust build identity requires a new generation while the
        # pinned Rust release itself remains valid.
        self.rustc = "rustc 1.89.0 (different build)"
        for failure in ("build", "version", "publication"):
            self.failure = failure
            replace = os.replace

            def replacement(source, destination):
                if failure == "publication" and Path(destination).name == bootstrap.POINTER_NAME:
                    raise OSError("interrupted publication")
                return replace(source, destination)

            with self.subTest(failure=failure), patch.object(bootstrap.os, "replace", side_effect=replacement):
                with self.assertRaises((bootstrap.BootstrapError, OSError, subprocess.CalledProcessError)):
                    self.ensure()
            self.assertEqual((self.output / bootstrap.POINTER_NAME).read_bytes(), pointer)
            self.assertEqual(list((self.output / "generations").iterdir()), [first.compiler.parent])
            for path, payload in files.items():
                self.assertEqual(path.read_bytes(), payload)
            self.assertFalse(any(path.name.startswith(".wave-bootstrap-") for path in self.output.iterdir()))

    def test_failed_first_build_never_publishes_pointer_or_generation(self):
        self.failure = "version"
        with self.assertRaises(bootstrap.BootstrapError):
            self.ensure()
        self.assertFalse((self.output / bootstrap.POINTER_NAME).exists())
        self.assertFalse((self.output / "generations").exists())

    def test_output_inside_read_only_checkout_is_rejected_before_any_commands(self):
        with self.assertRaisesRegex(bootstrap.BootstrapError, "outside"):
            bootstrap.ensure_bootstrap(self.source, self.source / "cache")
        self.check.assert_not_called()
        self.run.assert_not_called()
        self.assertEqual(list(self.source.iterdir()), [self.marker])

    def test_symlinked_std_or_build_directory_is_rejected(self):
        result = self.ensure()
        alias = result.std_root / "alias.wave"
        try:
            alias.symlink_to(self.marker)
        except (OSError, NotImplementedError):
            self.skipTest("symlinks unavailable")
        with self.assertRaisesRegex(bootstrap.BootstrapError, "special file"):
            bootstrap.load_bootstrap(self.output)
        shutil = __import__("shutil")
        shutil.rmtree(self.output / "build")
        (self.output / "build").symlink_to(self.source, target_is_directory=True)
        with self.assertRaisesRegex(bootstrap.BootstrapError, "symlinks"):
            self.ensure()


if __name__ == "__main__":
    unittest.main()
