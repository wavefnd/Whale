# SPDX-License-Identifier: MPL-2.0
"""Meson/Cargo bridge checks independent of installed Rust/Wave compilers."""

import argparse
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from meson_cargo import (BuildError, build, cargo_command, cargo_dependencies,
                         cargo_environment, make_escape, native_target,
                         smoke, source_dependencies, validate_features, validate_host)


class CommandTests(unittest.TestCase):
    def test_product_features_do_not_choose_the_executable_host(self):
        command = cargo_command("cargo", Path("source"), Path("private"), "aarch64-unknown-linux-gnu",
                                "dev", ["asm-amd64"], True, 2)
        self.assertIn("--locked", command)
        self.assertIn("--no-default-features", command)
        self.assertIn("--offline", command)
        self.assertEqual(command[command.index("--target") + 1], "aarch64-unknown-linux-gnu")
        self.assertEqual(command[command.index("--features") + 1], "asm-amd64")
        self.assertNotIn("--workspace", command)

    def test_no_isa_product_has_no_implicit_default_features(self):
        command = cargo_command("cargo", Path("source"), Path("private"), "native-host", "dev", [], False, 1)
        self.assertNotIn("--features", command)
        self.assertIn("--no-default-features", command)
        self.assertNotIn("--offline", command)

    def test_ambient_wave_and_cargo_target_cannot_change_selection(self):
        with patch.dict(os.environ, {"WHALE_ASMOBJ_DIR": "old", "CARGO_BUILD_TARGET": "old"}):
            environment = cargo_environment("private", None, "dev", "2", "2")
        self.assertNotIn("WHALE_ASMOBJ_DIR", environment)
        self.assertNotIn("CARGO_BUILD_TARGET", environment)
        self.assertEqual(environment["CARGO_TARGET_DIR"], "private")
        self.assertEqual(environment["CARGO_PROFILE_DEV_OPT_LEVEL"], "2")
        self.assertEqual(environment["CARGO_PROFILE_DEV_DEBUG"], "2")

    def test_wave_is_explicit_and_unknown_features_fail(self):
        self.assertEqual(validate_features(["asm-amd64", "asm-amd64"]), ["asm-amd64"])
        for features in [["unknown"], ["wave-reverse"]]:
            with self.subTest(features=features), self.assertRaises(BuildError):
                validate_features(features)

    def test_native_host_is_observed_only_when_requested_at_build(self):
        result = subprocess.CompletedProcess([], 0, stdout="cargo 1.96\nhost: x86_64-unknown-linux-gnu\n")
        with patch("meson_cargo.subprocess.run", return_value=result) as execute:
            self.assertEqual(native_target("cargo", ".", {}), "x86_64-unknown-linux-gnu")
        self.assertEqual(execute.call_args.args[0], ["cargo", "-vV"])
        with patch("meson_cargo.subprocess.run", return_value=subprocess.CompletedProcess([], 0, stdout="bad")):
            with self.assertRaises(BuildError):
                native_target("cargo", ".", {})

    def test_rust_target_must_match_the_configured_meson_host(self):
        cfg = 'target_arch="aarch64"\ntarget_os="linux"\ntarget_env="gnu"\n'
        with patch("meson_cargo.subprocess.run", return_value=subprocess.CompletedProcess([], 0, stdout=cfg)):
            validate_host("rustc", "aarch64-unknown-linux-gnu", "linux", "aarch64", ".", {})
            with self.assertRaisesRegex(BuildError, "disagrees with Meson host"):
                validate_host("rustc", "aarch64-unknown-linux-gnu", "linux", "x86_64", ".", {})

    def test_explicit_runner_arguments_remain_exact_without_a_shell(self):
        with tempfile.TemporaryDirectory() as temporary:
            binary = Path(temporary).resolve() / "whale"
            binary.write_bytes(b"binary")
            arguments = argparse.Namespace(binary=binary, feature=[],
                                           runner_arg=["wrapper with space", "-L", "literal $sysroot"])

            def answer(command, **kwargs):
                self.assertEqual(command[:4], ["wrapper with space", "-L", "literal $sysroot", str(binary)])
                self.assertNotIn("shell", kwargs)
                if command[-1] == "--version":
                    return subprocess.CompletedProcess(command, 0, b"whale 0.1.0\n", b"")
                if "asm" in command:
                    return subprocess.CompletedProcess(command, 1, b"", b"Error: unregistered assembler; available: \n")
                return subprocess.CompletedProcess(command, 0, b"i32 42\n", b"")

            with patch("meson_cargo.subprocess.run", side_effect=answer) as execute:
                smoke(arguments)
            self.assertEqual(execute.call_count, 3)


class DependencyTests(unittest.TestCase):
    def test_depfile_roundtrip_handles_spaces_unicode_and_make_metacharacters(self):
        dependencies = ["/source with space/α.rs", "/source/hash#dollar$.rs", r"C:\source folder\a.rs"]
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary).resolve() / "cargo.d"
            path.write_text(make_escape("/build with space/whale") + ": " +
                            " ".join(map(make_escape, dependencies)) + "\n", encoding="utf-8")
            self.assertEqual(cargo_dependencies(path), dependencies)

    def test_depfile_continuations_and_windows_drive_colons(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary).resolve() / "cargo.d"
            path.write_text("C:\\build\\whale.exe: C:\\src\\one.rs \\\n /src/two.rs\n\nother: ignored\n")
            self.assertEqual(cargo_dependencies(path), [r"C:\src\one.rs", "/src/two.rs"])

    def test_malformed_depfile_fails_instead_of_publishing_untracked_binary(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary).resolve() / "cargo.d"
            path.write_text("no dependency rule\n")
            with self.assertRaises(BuildError):
                cargo_dependencies(path)

    def test_source_dirs_and_manifests_track_additions_without_disabled_isas(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary).resolve()
            for name in ["src", "assembler/amd64/src", "assembler/aarch64/src"]:
                (source / name).mkdir(parents=True)
                (source / name / "lib.rs").write_text("// source")
            (source / "Cargo.toml").write_text("manifest")
            dependencies = source_dependencies(source, ["asm-aarch64"])
            self.assertIn(source / "assembler/aarch64/src", dependencies)
            self.assertIn(source / "assembler/aarch64/src/lib.rs", dependencies)
            self.assertNotIn(source / "assembler/amd64/src/lib.rs", dependencies)
            self.assertIn(source / "Cargo.toml", dependencies)


class PublicationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.source = self.root / "source tree"
        self.source.mkdir()
        self.target = self.root / "private target"
        self.output = self.root / "whale"
        self.output.write_bytes(b"previous")
        self.depfile = self.root / "whale.d"
        self.depfile.write_bytes(b"previous dependencies")
        self.arguments = argparse.Namespace(source_root=self.source, target_dir=self.target,
            output=self.output, depfile=self.depfile, asmobj_dir=None, feature=[],
            profile="dev", opt_level="0", debug_info="2", host_target="x86_64-unknown-linux-gnu",
            cargo="explicit cargo", rustc="explicit rustc", offline=True, jobs=2,
            host_system="linux", host_cpu_family="x86_64")

    def fake_build(self, command, **kwargs):
        if "--print" in command:
            return subprocess.CompletedProcess(command, 0, stdout='target_arch="x86_64"\ntarget_os="linux"\n')
        binary_dir = self.target / self.arguments.host_target / "debug"
        binary_dir.mkdir(parents=True)
        (binary_dir / "whale").write_bytes(b"new executable")
        (binary_dir / "whale").chmod(0o755)
        os.utime(binary_dir / "whale", (1, 1))
        (binary_dir / "whale.d").write_text(make_escape(binary_dir / "whale") + ": " +
                                           make_escape(self.source / "src/lib.rs") + "\n")
        return subprocess.CompletedProcess(command, 0)

    def test_failed_cargo_preserves_last_good_binary_and_dependencies(self):
        with patch("meson_cargo.subprocess.run", side_effect=subprocess.CalledProcessError(1, ["cargo"])):
            with self.assertRaises(subprocess.CalledProcessError):
                build(self.arguments)
        self.assertEqual(self.output.read_bytes(), b"previous")
        self.assertEqual(self.depfile.read_bytes(), b"previous dependencies")

    def test_success_publishes_binary_and_actual_dependency_file(self):
        with patch("meson_cargo.subprocess.run", side_effect=self.fake_build) as execute:
            build(self.arguments)
        self.assertEqual(self.output.read_bytes(), b"new executable")
        built = self.target / self.arguments.host_target / "debug/whale"
        self.assertEqual(self.output.stat().st_mode & 0o777, built.stat().st_mode & 0o777)
        self.assertGreater(self.output.stat().st_mtime, 1)
        self.assertIn(str(self.source / "src/lib.rs"), cargo_dependencies(self.depfile))
        self.assertEqual(execute.call_count, 2)  # rustc cfg validation, then Cargo.
        self.assertEqual(list(self.root.glob(".whale-*")), [])

    def test_missing_depfile_keeps_previous_binary(self):
        def no_depfile(command, **kwargs):
            result = self.fake_build(command, **kwargs)
            if "--print" not in command:
                (self.target / self.arguments.host_target / "debug/whale.d").unlink()
            return result
        with patch("meson_cargo.subprocess.run", side_effect=no_depfile):
            with self.assertRaises(OSError):
                build(self.arguments)
        self.assertEqual(self.output.read_bytes(), b"previous")

    def test_failed_binary_replace_preserves_old_executable(self):
        actual_replace = os.replace

        def replace(source, destination):
            if destination == self.output:
                raise OSError("injected publication failure")
            return actual_replace(source, destination)

        with patch("meson_cargo.subprocess.run", side_effect=self.fake_build), \
                patch("meson_cargo.os.replace", side_effect=replace):
            with self.assertRaises(OSError):
                build(self.arguments)
        self.assertEqual(self.output.read_bytes(), b"previous")
        self.assertEqual(list(self.root.glob(".whale-*")), [])

    def test_root_cargo_target_dir_is_rejected_before_running_cargo(self):
        self.arguments.target_dir = self.source / "target"
        with patch("meson_cargo.subprocess.run") as execute:
            with self.assertRaisesRegex(BuildError, "private Cargo"):
                build(self.arguments)
            execute.assert_not_called()

    def test_publication_replaces_output_symlink_without_touching_its_target(self):
        source_file = self.source / "must-stay.txt"
        source_file.write_bytes(b"source bytes")
        self.output.unlink()
        try:
            self.output.symlink_to(source_file)
        except (OSError, NotImplementedError) as error:
            self.skipTest(str(error))
        with patch("meson_cargo.subprocess.run", side_effect=self.fake_build):
            build(self.arguments)
        self.assertFalse(self.output.is_symlink())
        self.assertEqual(self.output.read_bytes(), b"new executable")
        self.assertEqual(source_file.read_bytes(), b"source bytes")


if __name__ == "__main__":
    unittest.main()
