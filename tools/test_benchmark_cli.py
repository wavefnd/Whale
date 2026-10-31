# SPDX-License-Identifier: MPL-2.0
"""Correctness and publication gates for actual CLI performance evidence."""

import contextlib
import io
import struct
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

import benchmark_cli as bench


def elf_object(code, machine=62):
    """A small independently constructed ELF64 object with just section headers."""
    strings = b"\0.shstrtab\0.text\0"
    names = 64 + len(code)
    table = (names + len(strings) + 7) & ~7
    data = bytearray(table + 3 * 64)
    data[:7] = b"\x7fELF\x02\x01\x01"
    struct.pack_into("<HHIQQQIHHHHHH", data, 16, 1, machine, 1, 0, 0, table, 0, 64, 0, 0, 64, 3, 1)
    data[64 : 64 + len(code)] = code
    data[names : names + len(strings)] = strings
    struct.pack_into("<IIQQQQIIQQ", data, table + 64, 1, 3, 0, 0, names, len(strings), 0, 0, 1, 0)
    struct.pack_into("<IIQQQQIIQQ", data, table + 128, 11, 1, 6, 0, 64, len(code), 0, 0, 1, 0)
    return bytes(data)


class CliBenchmarkChecks(unittest.TestCase):
    def test_timer_rejects_unsupported_hosts_and_missing_executable(self):
        for platform, available in (("darwin", True), ("win32", True), ("linux", False)):
            with (
                self.subTest(platform=platform, available=available),
                patch("benchmark_cli.sys.platform", platform),
                patch.object(Path, "is_file", return_value=available),
                self.assertRaisesRegex(ValueError, "Linux with GNU"),
            ):
                bench.benchmark_timer()

    def test_reference_mixed_and_incomplete_products_are_not_native_evidence(self):
        modules = ["ir", "assembler", "asmobj", "amd64"]
        info = {
            "schema_version": 1,
            "product_mode": "wave",
            "assemblers": ["amd64"],
            "socket_cli": True,
            "implementations": dict.fromkeys(modules, "wave"),
            "wave_modules": {module: {} for module in modules},
        }
        bench.validate_product(info, ["amd64"])
        for mode in ("rust-reference", "mixed"):
            with self.subTest(mode=mode), self.assertRaises(ValueError):
                bench.validate_product(dict(info, product_mode=mode), ["amd64"])
        for changed in (
            dict(info, implementations={"ir": "wave"}),
            dict(info, socket_cli=False),
            dict(info, wave_modules={}),
        ):
            with self.subTest(changed=changed), self.assertRaises(ValueError):
                bench.validate_product(changed, ["amd64"])
        with self.assertRaisesRegex(ValueError, "--isa"):
            bench.validate_product(info, ["aarch64"])

    def test_object_gate_rejects_wrong_machine_or_instruction_bytes(self):
        code = bytes.fromhex("b82a00000083c000c3")
        bench.check_assembly(elf_object(code), "amd64", 1)
        for data in (elf_object(code, 183), elf_object(code[:-1] + b"\x90"), elf_object(code)[:-1]):
            with self.subTest(data=data[:24]), self.assertRaises(ValueError):
                bench.check_assembly(data, "amd64", 1)

    def test_fold_gate_rejects_wrong_values_dropped_ids_and_changed_return(self):
        text = "%v0: i32 = const i32 42\n%v1: i32 = const i32 0\n%v2: i32 = const i32 42\nret i32 %v2\n"
        bench.check_folded(text.encode(), 1)
        for wrong in (
            text.replace("const i32 42", "const i32 41", 1),
            text.replace("%v2: ", "%v3: "),
            text.replace("ret i32 %v2", "ret i32 %v0"),
        ):
            with self.subTest(wrong=wrong), self.assertRaises(ValueError):
                bench.check_folded(wrong.encode(), 1)

    def test_lowering_gate_requires_unused_o0_loads_in_source_order(self):
        loads = [f"%v{index}: i32 = load i32, ptr<i32> %v1, align 4\n" for index in range(2, 5)]
        text = "".join(loads) + "ret i32 %v4\n"
        bench.check_lowered(text.encode(), 2)
        for wrong in (
            text.replace(loads[1], ""),
            loads[1] + loads[0] + loads[2] + "ret i32 %v4\n",
            text.replace("ret i32 %v4", "ret i32 %v3"),
        ):
            with self.subTest(wrong=wrong), self.assertRaises(ValueError):
                bench.check_lowered(wrong.encode(), 2)

    def test_phase_deletes_outputs_checks_warmups_and_uses_measured_samples(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, output = root / "source", root / "output"
            source.write_bytes(b"input")
            output.write_bytes(b"stale")
            times = iter([999, 1, 3, 2])

            def execute(*_args):
                self.assertFalse(output.exists())
                output.write_bytes(b"validated")
                return b"status", next(times), 1024

            args = SimpleNamespace(runs=3, warmups=1, timeout=1)
            with patch("benchmark_cli.execute", side_effect=execute) as invoked:
                result = bench.run_phase(
                    "phase",
                    1,
                    ["cli"],
                    source,
                    output,
                    lambda data: self.assertEqual(data, b"validated"),
                    args,
                    "timer",
                )
            self.assertEqual(invoked.call_count, 4)
            self.assertEqual(len(result["samples"]), 3)
            self.assertEqual(result["median"]["phase_seconds"], 2)
            self.assertEqual(result["fingerprint"]["result_sha256"], bench.digest(b"validated"))

    def test_missing_output_cannot_reuse_an_earlier_success(self):
        with tempfile.TemporaryDirectory() as directory:
            source, output = Path(directory) / "source", Path(directory) / "output"
            source.write_bytes(b"input")
            output.write_bytes(b"old valid output")
            with (
                patch("benchmark_cli.execute", return_value=(b"", 1, 1024)),
                self.assertRaises(FileNotFoundError),
            ):
                bench.run_phase(
                    "phase",
                    1,
                    ["cli"],
                    source,
                    output,
                    lambda _: None,
                    SimpleNamespace(runs=1, warmups=0, timeout=1),
                    "timer",
                )

    def test_changed_validated_output_and_input_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source"
            source.write_bytes(b"input")
            args = SimpleNamespace(runs=1, warmups=1, timeout=1)
            with (
                patch(
                    "benchmark_cli.execute",
                    side_effect=[(b"first", 1, 1024), (b"different", 1, 1024)],
                ),
                self.assertRaisesRegex(ValueError, "non-deterministic"),
            ):
                bench.run_phase("phase", 1, ["cli"], source, None, lambda _: None, args, "timer")

            def changed_input(*_args):
                source.write_bytes(b"changed")
                return b"output", 1, 1024

            with (
                patch("benchmark_cli.execute", side_effect=changed_input),
                self.assertRaisesRegex(ValueError, "changed input"),
            ):
                bench.run_phase("phase", 1, ["cli"], source, None, lambda _: None, args, "timer")

    def test_failed_command_preserves_an_existing_report(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary, report = root / "whale", root / "report.json"
            binary.write_bytes(b"selected executable")
            report.write_bytes(b"previous complete report")
            with (
                patch("benchmark_cli.benchmark_timer", return_value=root / "timer"),
                patch("benchmark_cli.text_command", return_value="test version"),
                patch("benchmark_cli.native_modules", return_value={}),
                patch("benchmark_cli.product_info", return_value={"wave_modules": {}}),
                patch("benchmark_cli.benchmark_size", side_effect=ValueError("bad artifact")),
                contextlib.redirect_stderr(io.StringIO()),
            ):
                status = bench.main(
                    [
                        "--output",
                        str(report),
                        "--cli",
                        str(binary),
                        "--skip-build",
                        "--small",
                        "1",
                        "--large",
                        "2",
                        "--runs",
                        "1",
                        "--warmups",
                        "0",
                    ]
                )
            self.assertEqual(status, 1)
            self.assertEqual(report.read_bytes(), b"previous complete report")


if __name__ == "__main__":
    unittest.main()
