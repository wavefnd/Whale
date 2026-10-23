# SPDX-License-Identifier: MPL-2.0
"""Regress complete asmobj publication and its real std-only Cargo build guard."""
import contextlib
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import build_asmobj as helper
from wave_bootstrap import Bootstrap

ROOT = Path(__file__).resolve().parents[1]
NATIVE = "x86_64-unknown-linux-gnu"


class PublicationTests(unittest.TestCase):
    def publication(self, *, failure=None, existing=True, target=NATIVE, allow_network=False):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            source = root / "Wave"
            source.mkdir()
            marker = source / "user.wave"
            marker.write_bytes(b"unrelated user changes")
            output = root / "output"
            output.mkdir()
            compiler = root / "wavec"
            compiler.write_bytes(b"pinned compiler")
            std = root / "std"
            std.mkdir()
            bootstrap = Bootstrap(compiler, std, root / "bootstrap-provenance.json", {"std_sha256": "1" * 64}, True)
            archive_name = "whale_asmobj.lib" if "msvc" in target else "libwhale_asmobj.a"
            names = (archive_name, "provenance.json", "asmobj.meta")
            original = {name: b"previous " + name.encode() for name in names}
            if existing:
                for name, data in original.items():
                    (output / name).write_bytes(data)
            calls = []

            def fake_bootstrap(*args, **kwargs):
                self.assertEqual(args, (source, output / "bootstrap-cache"))
                self.assertEqual(kwargs, {"rust_toolchain": "1.89", "allow_network": allow_network})
                if failure == "bootstrap":
                    raise subprocess.CalledProcessError(1, ["cargo"])
                return bootstrap

            def fake_run(command, **kwargs):
                command = list(map(str, command))
                calls.append(command)
                if "-o" in command:
                    self.assertEqual(command[0], str(compiler))
                    self.assertEqual(command[command.index("--std-root") + 1], str(std))
                    self.assertIn("-O0", command)
                    self.assertIn("relocation-model=pic", command)
                    self.assertIn("--freestanding", command)
                    self.assertIn("--emit=obj", command)
                    wave_target = target.replace("riscv64gc-", "riscv64-")
                    self.assertIn("--target=" + wave_target, command)
                    self.assertEqual(kwargs["env"]["WAVE_STD_REVISION"], helper.WAVE_REVISION)
                    kernel = Path(command[command.index("build") + 1])
                    self.assertEqual(kernel.read_bytes(), helper.source_bytes(ROOT))
                    self.assertEqual(Path(kwargs["cwd"]), kernel.parent)
                    Path(command[command.index("-o") + 1]).write_bytes(b"object")
                    if failure == "compile":
                        raise subprocess.CalledProcessError(1, command)
                else:
                    expected_archiver = ["asmobj-test-ar"]
                    if "msvc" in target:
                        expected_archiver.append("--format=coff")
                    elif "apple-darwin" in target:
                        expected_archiver.append("--format=darwin")
                    expected_archiver.append("crsD")
                    self.assertEqual(command[:-2], expected_archiver)
                    self.assertEqual(Path(command[-2]).name, archive_name)
                    self.assertEqual(Path(command[-1]).read_bytes(), b"object")
                    Path(command[-2]).write_bytes(b"new archive")
                    if failure == "archive":
                        raise subprocess.CalledProcessError(1, command)

            arguments = ["build_asmobj.py", "--wave-source", str(source), "--out-dir", str(output),
                         "--rust-toolchain", "1.89", "--target", target, "--opt-level", "0"]
            if allow_network:
                arguments.append("--allow-network")
            with patch.object(sys, "argv", arguments), \
                    patch.object(helper, "ensure_bootstrap", side_effect=fake_bootstrap), \
                    patch.object(helper, "run", side_effect=fake_run), \
                    patch.dict(os.environ, {"AR": "asmobj-test-ar"}), \
                    contextlib.redirect_stdout(io.StringIO()):
                if failure:
                    with self.assertRaises(subprocess.CalledProcessError):
                        helper.main()
                else:
                    helper.main()
            self.assertEqual(marker.read_bytes(), b"unrelated user changes")
            self.assertEqual(sorted(p.name for p in source.iterdir()), ["user.wave"])
            self.assertFalse(any(p.name.startswith(".asmobj-") for p in output.iterdir()))
            for name in names:
                path = output / name
                if failure and existing:
                    self.assertEqual(path.read_bytes(), original[name])
                elif failure:
                    self.assertFalse(path.exists(), name)
                else:
                    self.assertTrue(path.is_file(), name)
            if not failure:
                self.assertEqual(len(calls), 2)
                source_bytes = helper.source_bytes(ROOT)
                self.assertEqual((output / "asmobj.meta").read_text(),
                                 helper.metadata(source_bytes, b"new archive", target))
                provenance = json.loads((output / "provenance.json").read_text())
                self.assertEqual(provenance, {
                    "wave_revision": helper.WAVE_REVISION,
                    "target": target,
                    "opt_level": "0",
                    "source_sha256": hashlib.sha256(source_bytes).hexdigest(),
                    "archive_sha256": hashlib.sha256(b"new archive").hexdigest(),
                    "compiler_sha256": hashlib.sha256(b"pinned compiler").hexdigest(),
                    "std_sha256": "1" * 64,
                    "bootstrap_provenance": str(bootstrap.provenance),
                })

    def test_success_publishes_exact_source_archive_and_compiler_provenance(self):
        self.publication()

    def test_compile_archive_and_bootstrap_failures_preserve_last_good_set(self):
        for failure in ("bootstrap", "compile", "archive"):
            with self.subTest(failure=failure):
                self.publication(failure=failure)

    def test_failed_first_build_never_publishes_partial_artifacts(self):
        for failure in ("bootstrap", "compile", "archive"):
            with self.subTest(failure=failure):
                self.publication(failure=failure, existing=False)

    def test_cross_object_targets_keep_distinct_metadata_and_archive_names(self):
        for target in ("riscv64gc-unknown-linux-gnu", "aarch64-pc-windows-msvc", "aarch64-apple-darwin"):
            with self.subTest(target=target):
                self.publication(target=target)

    def test_network_permission_is_explicitly_forwarded(self):
        self.publication(allow_network=True)

    def test_output_inside_compiler_checkout_is_rejected_before_bootstrap(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary).resolve() / "Wave"
            source.mkdir()
            for output in (source, source / "output"):
                with self.subTest(output=output), \
                        patch.object(sys, "argv", ["build_asmobj.py", "--wave-source", str(source),
                                                  "--out-dir", str(output)]), \
                        patch.object(helper, "ensure_bootstrap") as bootstrap, \
                        contextlib.redirect_stderr(io.StringIO()):
                    with self.assertRaises(SystemExit) as error:
                        helper.main()
                    self.assertEqual(error.exception.code, 2)
                    bootstrap.assert_not_called()
            self.assertEqual(list(source.iterdir()), [])


class BuildGuardTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory()
        cls.guard = Path(cls.temporary.name).resolve() / ("guard.exe" if os.name == "nt" else "guard")
        subprocess.run([shutil.which("rustc") or "rustc", str(ROOT / "toolchain/build.rs"),
                        "--edition=2021", "-Dwarnings", "-o", str(cls.guard)],
                       check=True, capture_output=True)

    @classmethod
    def tearDownClass(cls):
        cls.temporary.cleanup()

    @staticmethod
    def make_project(root, target=NATIVE):
        manifest = root / "toolchain"
        manifest.mkdir()
        sources = root / "assembler/asmobj"
        sources.mkdir(parents=True)
        for index, name in enumerate(helper.SOURCES):
            (sources / name).write_bytes(f"source {index}: {name}\n".encode())
        archive_dir = root / "archive"
        archive_dir.mkdir()
        archive = archive_dir / ("whale_asmobj.lib" if "msvc" in target else "libwhale_asmobj.a")
        archive.write_bytes(b"archive")
        source_bytes = b"\n".join((sources / name).read_bytes() for name in helper.SOURCES)
        metadata = archive_dir / "asmobj.meta"
        metadata.write_text(helper.metadata(source_bytes, b"archive", target))
        return manifest, sources, archive_dir, archive, metadata

    def run_guard(self, corruption=None, *, enabled=True, target=NATIVE):
        with tempfile.TemporaryDirectory() as temporary:
            manifest, sources, archive_dir, archive, metadata = self.make_project(Path(temporary).resolve(), target)
            if corruption == "source":
                (sources / helper.SOURCES[2]).write_bytes(b"changed source")
            elif corruption == "archive":
                archive.write_bytes(b"changed archive")
            elif corruption == "missing-archive":
                archive.unlink()
            elif corruption == "missing-source":
                (sources / helper.SOURCES[2]).unlink()
            elif corruption == "missing-metadata":
                metadata.unlink()
            elif corruption == "oversized":
                metadata.write_text("x" * 4097)
            elif corruption == "duplicate":
                metadata.write_text(metadata.read_text() + "schema=1\n")
            elif corruption == "unknown":
                metadata.write_text(metadata.read_text() + "future=1\n")
            elif corruption == "incomplete":
                metadata.write_text("\n".join(metadata.read_text().splitlines()[:-1]) + "\n")
            elif corruption == "malformed":
                metadata.write_text(metadata.read_text() + "not an assignment\n")
            elif corruption == "invalid-utf8":
                metadata.write_bytes(b"\xff")
            elif corruption == "schema":
                metadata.write_text(metadata.read_text().replace("schema=1", "schema=2"))
            elif corruption == "target":
                metadata.write_text(metadata.read_text().replace("target=" + target, "target=aarch64-unknown-linux-gnu"))
            environment = os.environ.copy()
            environment.pop("WHALE_ASMOBJ_DIR", None)
            environment.update(CARGO_MANIFEST_DIR=str(manifest), HOST=NATIVE, TARGET=target)
            if enabled:
                environment["WHALE_ASMOBJ_DIR"] = str(archive_dir)
            return subprocess.run([str(self.guard)], env=environment, capture_output=True, text=True)

    def test_matching_guard_watches_all_five_sources_and_published_artifacts(self):
        result = self.run_guard()
        self.assertEqual(result.returncode, 0, result.stderr)
        for name in (*helper.SOURCES, "asmobj.meta", "libwhale_asmobj.a"):
            self.assertIn(name, result.stdout)
        self.assertIn("cargo:rustc-cfg=whale_asmobj", result.stdout)
        self.assertIn("cargo:rustc-link-lib=static=whale_asmobj", result.stdout)

    def test_mismatched_or_invalid_artifacts_fail_before_link_selection(self):
        expectations = {
            "source": "stale asmobj", "archive": "corrupt asmobj",
            "target": "target mismatch", "oversized": "oversized",
            "duplicate": "metadata fields", "unknown": "metadata fields",
            "incomplete": "incomplete", "malformed": "malformed",
            "invalid-utf8": "invalid asmobj metadata", "schema": "unsupported",
            "missing-source": "missing asmobj Wave source",
            "missing-archive": "missing asmobj archive",
            "missing-metadata": "missing asmobj.meta",
        }
        for corruption, reason in expectations.items():
            with self.subTest(corruption=corruption):
                result = self.run_guard(corruption)
                self.assertNotEqual(result.returncode, 0, result.stdout)
                self.assertIn(reason, result.stderr)
                self.assertNotIn("cargo:rustc-link-lib", result.stdout)
                self.assertNotIn("cargo:rustc-cfg=whale_asmobj", result.stdout)

    def test_plain_cargo_build_needs_neither_compiler_nor_wave_artifact(self):
        result = self.run_guard("missing-metadata", enabled=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("cargo:rustc-link-lib", result.stdout)
        self.assertNotIn("cargo:rustc-cfg=whale_asmobj", result.stdout)

    def test_cross_linked_object_must_match_cargo_target_without_claiming_execution(self):
        for target in helper.TARGETS[1:]:
            with self.subTest(target=target):
                result = self.run_guard(target=target)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn("cargo:rustc-cfg=whale_asmobj", result.stdout)
                archive = "whale_asmobj.lib" if "msvc" in target else "libwhale_asmobj.a"
                self.assertIn(archive, result.stdout)

    def test_cargo_rechecks_warm_source_and_archive_changes(self):
        identity = subprocess.check_output([shutil.which("rustc") or "rustc", "-vV"], text=True)
        hosts = [line.removeprefix("host: ") for line in identity.splitlines()
                 if line.startswith("host: ")]
        self.assertEqual(len(hosts), 1)
        with tempfile.TemporaryDirectory() as temporary:
            manifest, sources, archive_dir, archive, metadata = self.make_project(Path(temporary).resolve(), hosts[0])
            (manifest / "src").mkdir()
            (manifest / "src/lib.rs").write_text("pub fn marker() {}\n")
            (manifest / "Cargo.toml").write_text(
                '[package]\nname="asmobj-guard-probe"\nversion="0.0.0"\nedition="2021"\n')
            shutil.copyfile(ROOT / "toolchain/build.rs", manifest / "build.rs")
            environment = os.environ.copy()
            environment.pop("CARGO_BUILD_TARGET", None)
            environment.update(WHALE_ASMOBJ_DIR=str(archive_dir), CARGO_TARGET_DIR=str(manifest / "target"))
            command = [shutil.which("cargo") or "cargo", "check", "--offline", "--target", hosts[0],
                       "--manifest-path", str(manifest / "Cargo.toml")]

            def check(success, reason=None):
                result = subprocess.run(command, env=environment, capture_output=True, text=True)
                self.assertEqual(result.returncode == 0, success, result.stderr)
                if reason:
                    self.assertIn(reason, result.stderr)

            check(True)
            source = sources / helper.SOURCES[3]
            original = source.read_bytes()
            source.write_bytes(original + b"\n")
            check(False, "stale asmobj archive")
            source.write_bytes(original)
            check(True)
            archive.write_bytes(b"broken")
            check(False, "corrupt asmobj archive")
            archive.write_bytes(b"archive")
            check(True)
            original_metadata = metadata.read_text()
            metadata.write_text(original_metadata.replace("schema=1", "schema=2"))
            check(False, "unsupported asmobj metadata")
            metadata.write_text(original_metadata)
            check(True)


class SourceStructureTests(unittest.TestCase):
    def test_autocrlf_checkout_preserves_wave_source_fingerprint(self):
        # Archives are built on Linux and reused by Windows CI. Git must not
        # rewrite their source bytes before the Cargo fingerprint guard runs.
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            sources = root / "assembler/asmobj"
            sources.mkdir(parents=True)
            shutil.copyfile(ROOT / ".gitattributes", root / ".gitattributes")
            for name in helper.SOURCES:
                shutil.copyfile(ROOT / "assembler/asmobj" / name, sources / name)
            original = helper.source_bytes(root)
            environment = {key: value for key, value in os.environ.items()
                           if not key.startswith("GIT_")}
            environment.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)
            git = shutil.which("git") or "git"
            for arguments in (["init", "--quiet"],
                              ["config", "core.autocrlf", "true"],
                              ["add", ".gitattributes", "assembler/asmobj"]):
                subprocess.run([git, *arguments], cwd=root, env=environment,
                               check=True, capture_output=True)
            for name in helper.SOURCES:
                (sources / name).unlink()
            subprocess.run([git, "checkout-index", "--all", "--force"], cwd=root,
                           env=environment, check=True, capture_output=True)
            checked_out = helper.source_bytes(root)
            self.assertNotIn(b"\r\n", checked_out)
            self.assertEqual(checked_out, original)
            self.assertEqual(helper.fnv64(checked_out), helper.fnv64(original))

    def test_asmobj_is_a_complete_wave_module_with_five_role_files(self):
        module = ROOT / "assembler/asmobj"
        self.assertEqual(sorted(path.name for path in module.glob("*.wave")), sorted(helper.SOURCES))
        self.assertEqual(list(module.rglob("*.rs")), [])
        self.assertFalse((module / "Cargo.toml").exists())
        actual = set()
        for directory in ("assembler", "object", "reverse", "ir", "toolchain", "src"):
            source_root = ROOT / directory
            if source_root.is_dir():
                actual.update(path.relative_to(ROOT).as_posix() for path in source_root.rglob("*.wave"))
                self.assertFalse(any(path.is_dir() and path.name == "wave" for path in source_root.rglob("*")))
        self.assertEqual(actual, {"assembler/asmobj/" + name for name in helper.SOURCES})


if __name__ == "__main__":
    unittest.main()
