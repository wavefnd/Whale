# SPDX-License-Identifier: MPL-2.0
"""Exercise the real std-only Cargo guards for every Wave/Rust binding."""

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from wave_modules import ASSEMBLERS, MODULES, ROOT, fnv64, input_hashes, inputs


class GuardTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory()
        cls.directory = Path(cls.temporary.name).resolve()
        cls.binaries = {}
        for module in MODULES:
            output = cls.directory / module
            subprocess.run(
                [
                    "rustc",
                    "--edition=2021",
                    "-Dwarnings",
                    str(ROOT / "bindings/rust" / module / "build.rs"),
                    "-o",
                    str(output),
                ],
                check=True,
                capture_output=True,
            )
            cls.binaries[module] = output

    @classmethod
    def tearDownClass(cls):
        cls.temporary.cleanup()

    def fixture(self, root, module, target="x86_64-unknown-linux-gnu"):
        directory, library, variable = MODULES[module]
        for relative in (
            (directory, "assembler") if module in ASSEMBLERS else (directory,)
        ):
            source = root / relative
            (source / "src/nested").mkdir(parents=True, exist_ok=True)
            (source / "vex.ws").write_bytes(b"module manifest\n")
            (source / "src/lib.wave").write_bytes(b"// source entry\n")
            (source / "src/nested/role.wave").write_bytes(b"// role\n")
        for name in ("vex.ws", "vex.lock"):
            (root / name).write_bytes(name.encode())
        binding = root / "bindings/rust" / module
        (binding / "src").mkdir(parents=True)
        (binding / "src/lib.rs").write_bytes(b"// typed data bridge\n")
        (binding / "Cargo.toml").write_bytes(b"binding manifest\n")
        output = root / "artifacts"
        output.mkdir()
        archive = output / (
            library + ".lib" if "msvc" in target else "lib" + library + ".a"
        )
        archive.write_bytes(b"!<arch>\n")
        provenance = output / "provenance.json"
        provenance.write_bytes(b"verified compiler/std identities\n")
        sources, abi = inputs(module, root)
        fields = dict(
            schema="3",
            target=target,
            wave_revision="a" * 40,
            opt_level="0",
            source_fnv64=input_hashes(sources, root)[0],
            abi_fnv64=input_hashes(abi, root)[0],
            archive_fnv64=fnv64([archive.read_bytes()]),
            provenance_fnv64=fnv64([provenance.read_bytes()]),
        )
        metadata = output / "module.meta"
        metadata.write_text(
            "".join(f"{key}={value}\n" for key, value in fields.items())
        )
        environment = {
            key: value
            for key, value in os.environ.items()
            if not key.startswith("WHALE_") and key != "CARGO_FEATURE_RUST_REFERENCE"
        }
        environment.update(CARGO_MANIFEST_DIR=str(binding), TARGET=target)
        environment[variable] = str(output)
        if module in ASSEMBLERS:
            environment["WHALE_WAVE_ASSEMBLER_DIR"] = str(output)
        return environment, sources, abi, archive, provenance, metadata

    def execute(self, module, environment):
        return subprocess.run(
            [str(self.binaries[module])], env=environment, capture_output=True
        )

    def test_matching_modules_watch_all_role_and_binding_inputs(self):
        for module in MODULES:
            with (
                self.subTest(module=module),
                tempfile.TemporaryDirectory() as temporary,
            ):
                root = Path(temporary).resolve()
                environment, sources, abi, archive, provenance, metadata = self.fixture(
                    root, module
                )
                result = self.execute(module, environment)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn(b"rustc-link-lib=static=", result.stdout)
                for path in [*sources, *abi, archive, provenance, metadata]:
                    self.assertIn(str(path).encode(), result.stdout)

    def test_source_abi_archive_and_provenance_changes_fail_before_link_selection(self):
        for module in MODULES:
            with (
                self.subTest(module=module),
                tempfile.TemporaryDirectory() as temporary,
            ):
                environment, sources, abi, archive, provenance, metadata = self.fixture(
                    Path(temporary).resolve(), module
                )
                for path in [*sources, *abi, archive, provenance]:
                    previous = path.read_bytes()
                    try:
                        path.write_bytes(previous + b"changed")
                        result = self.execute(module, environment)
                        self.assertNotEqual(result.returncode, 0, str(path))
                        self.assertNotIn(b"rustc-link-lib", result.stdout)
                    finally:
                        path.write_bytes(previous)

    def test_added_empty_role_invalidates_the_source_fingerprint(self):
        for module in MODULES:
            with (
                self.subTest(module=module),
                tempfile.TemporaryDirectory() as temporary,
            ):
                root = Path(temporary).resolve()
                environment, *_ = self.fixture(root, module)
                path = root / MODULES[module][0] / "src/new.wave"
                path.write_bytes(b"")
                result = self.execute(module, environment)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(b"stale Wave source", result.stderr)

    def test_malformed_metadata_and_foreign_host_mismatch_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            environment, *_, metadata = self.fixture(
                Path(temporary).resolve(), "asmobj"
            )
            original = metadata.read_text()
            for text in (
                original.replace("schema=3", "schema=4"),
                original + "schema=3\n",
                original.replace("opt_level=0", "opt_level=1"),
                "x" * 4097,
                original.replace(
                    "x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"
                ),
                original.replace("a" * 40, "A" * 40),
            ):
                with self.subTest(text=text[:60]):
                    metadata.write_text(text)
                    result = self.execute("asmobj", environment)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertNotIn(b"rustc-link-lib", result.stdout)

    def test_missing_module_requires_explicit_reference_and_asmobj_reports_unavailable(
        self,
    ):
        for module in MODULES:
            with (
                self.subTest(module=module),
                tempfile.TemporaryDirectory() as temporary,
            ):
                environment, *_ = self.fixture(Path(temporary).resolve(), module)
                environment.pop(MODULES[module][2])
                plain = self.execute(module, environment)
                self.assertEqual(plain.returncode == 0, module == "asmobj")
                self.assertNotIn(b"rustc-link-lib", plain.stdout)
                environment["CARGO_FEATURE_RUST_REFERENCE"] = "1"
                self.assertEqual(self.execute(module, environment).returncode, 0)

    def test_cross_object_selection_uses_the_consuming_target_without_executing_it(
        self,
    ):
        for target in (
            "aarch64-unknown-linux-gnu",
            "x86_64-pc-windows-msvc",
            "aarch64-apple-darwin",
        ):
            with (
                self.subTest(target=target),
                tempfile.TemporaryDirectory() as temporary,
            ):
                environment, *_ = self.fixture(
                    Path(temporary).resolve(), "asmobj", target
                )
                result = self.execute("asmobj", environment)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn(b"rustc-link-lib=static=whale_asmobj", result.stdout)

    def test_actual_warm_cargo_build_rechecks_changed_source_and_archive(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            environment, sources, abi, archive, provenance, metadata = self.fixture(
                root, "asmobj"
            )
            binding = root / "bindings/rust/asmobj"
            (binding / "Cargo.toml").write_text(
                '[package]\nname="guard-probe"\nversion="0.1.0"\nedition="2021"\n'
            )
            (binding / "src/lib.rs").write_text("pub fn value() -> u64 { 42 }\n")
            shutil.copyfile(
                ROOT / "bindings/rust/asmobj/build.rs", binding / "build.rs"
            )
            shutil.copyfile(
                ROOT / "bindings/rust/build_support.rs",
                binding.parent / "build_support.rs",
            )
            text = metadata.read_text()
            old = next(
                line for line in text.splitlines() if line.startswith("abi_fnv64=")
            )
            metadata.write_text(
                text.replace(old, "abi_fnv64=" + input_hashes(abi, root)[0])
            )
            command = [
                "cargo",
                "build",
                "--offline",
                "--manifest-path",
                str(binding / "Cargo.toml"),
                "--target-dir",
                str(root / "cargo"),
            ]
            first = subprocess.run(command, env=environment, capture_output=True)
            self.assertEqual(first.returncode, 0, first.stderr)
            for path in (sources[-1], archive):
                previous = path.read_bytes()
                path.write_bytes(previous + b"changed")
                result = subprocess.run(command, env=environment, capture_output=True)
                self.assertNotEqual(result.returncode, 0, result.stderr)
                path.write_bytes(previous)


if __name__ == "__main__":
    unittest.main()
