#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Measure actual selected-ISA, IR and bytecode CLI processes on Linux."""

import argparse
import hashlib
import json
import math
import os
import platform
import re
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

import benchmark
from benchmark import (
    compare_reports,
    execute,
    host_metadata,
    sha256,
    summarize,
    text_command,
)

ARCHITECTURES = {
    "amd64": (
        62,
        "section .text\nglobal bench\nbench:\nmov eax,42\n",
        "add eax,0\n",
        "ret\n",
        bytes.fromhex("b82a000000"),
        bytes.fromhex("83c000"),
        bytes.fromhex("c3"),
    ),
    "aarch64": (
        183,
        ".text\n.global bench\nbench:\nmov x0,#42\n",
        "add x0,x0,#0\n",
        "ret\n",
        bytes.fromhex("400580d2"),
        bytes.fromhex("00000091"),
        bytes.fromhex("c0035fd6"),
    ),
    "riscv64": (
        243,
        ".text\n.global bench\nbench:\naddi a0,zero,42\n",
        "addi a0,a0,0\n",
        "ret\n",
        bytes.fromhex("1305a002"),
        bytes.fromhex("13050500"),
        bytes.fromhex("67800000"),
    ),
    "loongarch64": (
        258,
        ".text\n.global bench\nbench:\naddi.d $a0,$zero,42\n",
        "addi.d $a0,$a0,0\n",
        "ret\n",
        bytes.fromhex("04a8c002"),
        bytes.fromhex("8400c002"),
        bytes.fromhex("2000004c"),
    ),
}
STRIDES = (40, 8, 88, 32, 136, 8, 8, 64, 32, 16, 1, 16, 56, 40, 8, 8)
FORMAT_VERSION = 1
GENERATOR = "scalar-cli-pipelines-v1"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def ir_source(count):
    lines = [
        "module {",
        " format_version 8",
        " semantics_version 1",
        ' target "whale-bytecode-v1"',
        " datalayout { ptr=64, endian=little }",
        ' declare @f0 "bench": whale () -> i32, linkage internal',
        ' fn @f0 "bench"() -> i32, entry %b0 {',
        ' %b0 "entry":',
        " %v0: i32 = const i32 42",
        " %v1: i32 = const i32 0",
    ]
    for index in range(count):
        previous = index + 1 if index else 0
        lines.append(f" %v{index + 2}: i32 = add i32 %v{previous}, %v1")
    lines.extend([f" ret i32 %v{count + 1 if count else 0}", " }", "}"])
    return "\n".join(lines) + "\n"


def ast_source(count):
    # Flat unused parameter references preserve O0 loads while fitting the
    # existing AST node/depth budgets even at 32768 operations.
    kind = {"Int": {"bits": 32, "signed": True}}
    body = [{"ExprStmt": {"Var": "x"}} for _ in range(count)]
    body.append({"Return": {"Var": "x"}})
    program = {
        "declarations": [],
        "globals": [],
        "functions": [
            {
                "name": "bench",
                "parameters": [{"name": "x", "ty": kind}],
                "return_type": kind,
                "body": body,
                "convention": "Whale",
                "linkage": "Internal",
                "link_name": None,
            }
        ],
    }
    return (
        json.dumps(
            {"format_version": 5, "semantics_version": 1, "features": [], "program": program},
            separators=(",", ":"),
        )
        + "\n"
    )


def elf_text(data, machine):
    if len(data) < 64 or data[:7] != b"\x7fELF\x02\x01\x01":
        raise ValueError("expected little-endian ELF64")
    word16 = lambda at: struct.unpack_from("<H", data, at)[0]
    word32 = lambda at: struct.unpack_from("<I", data, at)[0]
    word64 = lambda at: struct.unpack_from("<Q", data, at)[0]
    if word16(16) != 1 or word16(18) != machine:
        raise ValueError("wrong ELF object type/machine")
    offset, stride, count, names = word64(40), word16(58), word16(60), word16(62)
    if stride != 64 or not 0 < names < count or offset + count * stride > len(data):
        raise ValueError("invalid ELF section table")
    header = offset + names * stride
    first, size = word64(header + 24), word64(header + 32)
    if first + size > len(data):
        raise ValueError("invalid ELF section names")
    strings = data[first : first + size]
    code = []
    for index in range(1, count):
        header = offset + index * stride
        name = word32(header)
        if name >= len(strings) or b"\0" not in strings[name:]:
            raise ValueError("invalid ELF section name")
        if strings[name:].split(b"\0", 1)[0] == b".text":
            first, size = word64(header + 24), word64(header + 32)
            if word64(header + 8) != 6 or first + size > len(data):
                raise ValueError("invalid ELF code section")
            code.append(data[first : first + size])
    if len(code) != 1:
        raise ValueError("expected exactly one ELF code section")
    return code[0]


def check_assembly(data, architecture, count):
    machine, _, _, _, head, body, tail = ARCHITECTURES[architecture]
    if elf_text(data, machine) != head + body * count + tail:
        raise ValueError("ELF text differs from independent ISA encoding")


def check_folded(data, count):
    text = data.decode()
    constants = re.findall(r"^\s*%v(\d+): i32 = const i32 (-?\d+)\s*$", text, re.MULTILINE)
    expected = [(str(index), "0" if index == 1 else "42") for index in range(count + 2)]
    if constants != expected or "= add " in text:
        raise ValueError("folded values/IDs differ from independent expected constants")
    if f"ret i32 %v{count + 1 if count else 0}\n" not in text:
        raise ValueError("folded return identity changed")


def check_lowered(data, count):
    text = data.decode()
    loads = re.findall(r"^\s*%v(\d+): i32 = load i32,", text, re.MULTILINE)
    if loads != [str(index) for index in range(2, count + 3)]:
        raise ValueError("lowering lost unused O0 loads or their source order")
    if f"ret i32 %v{count + 2}\n" not in text:
        raise ValueError("lowered return identity changed")


def check_bytecode(data, count):
    if len(data) < 576 or struct.unpack_from("<8s7Q", data) != (
        b"WHALEBC\0",
        1,
        1,
        1,
        len(data),
        16,
        0,
        576,
    ):
        raise ValueError("wrong canonical bytecode header")
    tables = []
    end = 576
    for table, stride in enumerate(STRIDES, 1):
        kind, offset, amount, width = struct.unpack_from("<4Q", data, 64 + (table - 1) * 32)
        if (kind, offset, width) != (table, end, stride) or amount > (len(data) - end) // stride:
            raise ValueError("invalid canonical bytecode directory")
        tables.append(data[offset : offset + amount * stride])
        end += amount * stride
    if end != len(data) or len(tables[4]) != (count + 3) * 136:
        raise ValueError("wrong bytecode instruction count")
    instructions = list(struct.iter_unpack("<17Q", tables[4]))
    if [row[0] for row in instructions] != [4, 4] + [7] * count + [34]:
        raise ValueError("bytecode lost O0 instructions")
    if [(row[11], row[12], row[13]) for row in instructions[:2]] != [(42, 0, 1), (0, 0, 1)]:
        raise ValueError("bytecode literal bits changed")
    for index, row in enumerate(instructions[2:-1]):
        if (row[1], row[4], row[5], row[6]) != (0, index + 2, index + 1 if index else 0, 1):
            raise ValueError("bytecode arithmetic or SSA identity changed")
    if list(struct.iter_unpack("<Q", tables[14])) != [(index,) for index in range(count + 2)]:
        raise ValueError("bytecode did not retain original SSA IDs")
    if instructions[-1][5] != (count + 1 if count else 0):
        raise ValueError("bytecode return identity changed")


def invoke(cli, arguments, timeout):
    return execute([cli, *arguments], timeout)[0]


def validate_product(info, architectures):
    required = ["ir", "assembler", "asmobj", *architectures]
    if info.get("schema_version") != 1 or info.get("product_mode") != "wave":
        raise ValueError("native benchmarks require a Wave product with build-info schema 1")
    if set(info.get("assemblers", [])) != set(architectures):
        raise ValueError("--isa selections must match the product's build-info assemblers")
    if any(info.get("implementations", {}).get(module) != "wave" for module in required):
        raise ValueError("every measured product module must select its Wave implementation")
    if info.get("socket_cli") is not True:
        raise ValueError("IR lowering benchmarks require socket-cli")
    if any(module not in info.get("wave_modules", {}) for module in required):
        raise ValueError("build-info is missing selected Wave archive provenance")


def product_info(cli, architectures, timeout):
    info = json.loads(invoke(cli, ["build-info", "--json"], timeout))
    if not isinstance(info, dict):
        raise TypeError("build-info must be a JSON object")
    validate_product(info, architectures)
    return info


def run_phase(name, count, command, source, output, validator, args, timer):
    samples, expected = [], None
    source_hash = sha256(source)
    for number in range(args.warmups + args.runs):
        if output:
            output.unlink(missing_ok=True)
        stdout, wall, peak = execute(command, args.timeout, timer)
        payload = output.read_bytes() if output else stdout
        if sha256(source) != source_hash:
            raise ValueError(f"benchmark command changed input: {source}")
        canonical = validator(payload)
        semantic = payload if canonical is None else canonical
        fingerprint = {
            "count": count,
            "input_sha256": sha256(source),
            "result_sha256": digest(semantic),
            "result_bytes": len(semantic),
        }
        if expected is not None and expected != fingerprint:
            raise ValueError(f"non-deterministic validated output: {name}")
        expected = fingerprint
        if number >= args.warmups:
            samples.append(
                {"phase_seconds": wall, "process_wall_seconds": wall, "peak_rss_bytes": peak}
            )
    return {
        "name": name,
        "command": list(map(str, command)),
        "fingerprint": expected,
        "output_sha256": digest(payload),
        "output_bytes": len(payload),
        "samples": samples,
        "median": summarize(samples),
    }


def prepare(directory, count, architectures):
    directory.mkdir(parents=True, exist_ok=True)
    sources = {"input.wir": ir_source(count), "input.json": ast_source(count)}
    for architecture in architectures:
        _, head, body, tail, *_ = ARCHITECTURES[architecture]
        sources[architecture + ".s"] = head + body * count + tail
    for name, text in sources.items():
        (directory / name).write_text(text)
    return {name: sha256(directory / name) for name in sources}


def benchmark_size(cli, directory, count, architectures, args, timer):
    inputs = prepare(directory, count, architectures)
    source, json_source = directory / "input.wir", directory / "input.json"
    image = directory / "input.wbc"
    invoke(cli, ["ir", "emit", source, "-o", image], args.timeout)
    check_bytecode(image.read_bytes(), count)
    expected_run = b"i32 42\n"

    def require_run(arguments):
        if invoke(cli, arguments, args.timeout) != expected_run:
            raise ValueError("generated program did not return independent expected i32 42")

    require_run(["bytecode", "run", image, "--function", "@f0"])
    measurements = []
    for architecture in architectures:
        measurements.append(
            run_phase(
                f"asm/{architecture}",
                count,
                [
                    cli,
                    "asm",
                    "--arch",
                    architecture,
                    directory / (architecture + ".s"),
                    "-o",
                    directory / (architecture + ".o"),
                ],
                directory / (architecture + ".s"),
                directory / (architecture + ".o"),
                lambda data, isa=architecture: check_assembly(data, isa, count),
                args,
                timer,
            )
        )

    def verify_stdout(data, kind, path):
        if data != f"Verified {kind} {path}\n".encode():
            raise ValueError("verifier did not confirm input")
        # Verifier stdout contains a filesystem path. Retain its actual hash in
        # the report but compare the verified content across output locations.
        return path.read_bytes()

    def lower(data):
        check_lowered(data, count)
        require_run(["ir", "run", directory / "lowered.wir", "--function", "@f0", "--arg", "42"])

    def folded(data):
        check_folded(data, count)
        require_run(["ir", "run", directory / "folded.wir", "--function", "@f0"])
        repeated = directory / "folded-twice.wir"
        invoke(cli, ["ir", "fold", directory / "folded.wir", "-o", repeated], args.timeout)
        if repeated.read_bytes() != data:
            raise ValueError("folding is not idempotent")

    def compiled(data):
        check_bytecode(data, count)
        if data != image.read_bytes():
            raise ValueError("bytecode compilation is not deterministic")
        require_run(["bytecode", "run", directory / "actual.wbc", "--function", "@f0"])

    def run(data):
        if data != expected_run:
            raise ValueError("bytecode returned wrong result")

    phases = [
        (
            "ir/verify",
            ["ir", "verify", source],
            source,
            None,
            lambda data: verify_stdout(data, "IR", source),
        ),
        (
            "ir/lower",
            [
                "ir",
                "lower",
                json_source,
                "--target",
                "whale-bytecode-v1",
                "-o",
                directory / "lowered.wir",
            ],
            json_source,
            directory / "lowered.wir",
            lower,
        ),
        (
            "ir/fold",
            ["ir", "fold", source, "-o", directory / "folded.wir"],
            source,
            directory / "folded.wir",
            folded,
        ),
        (
            "bytecode/compile",
            ["ir", "emit", source, "--target", "whale-bytecode-v1", "-o", directory / "actual.wbc"],
            source,
            directory / "actual.wbc",
            compiled,
        ),
        (
            "bytecode/verify",
            ["bytecode", "verify", image],
            image,
            None,
            lambda data: verify_stdout(data, "bytecode", image),
        ),
        ("bytecode/run", ["bytecode", "run", image, "--function", "@f0"], image, None, run),
    ]
    for name, arguments, source, output, validator in phases:
        measurements.append(
            run_phase(name, count, [cli, *arguments], source, output, validator, args, timer)
        )
    inputs["input.wbc"] = sha256(image)
    return inputs, measurements


def build_environment():
    names = (
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "RUSTUP_TOOLCHAIN",
        "RUSTC",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
    )
    environment = {name: os.environ.get(name) for name in names}
    environment.update(
        {name: value for name, value in os.environ.items() if name.startswith("CARGO_PROFILE_")}
    )
    environment.update(CARGO_INCREMENTAL="0", CARGO_BUILD_JOBS="2")
    return environment


def native_modules():
    modules = {}
    for name in (
        "WHALE_ASMOBJ_DIR",
        "WHALE_WAVE_IR_DIR",
        "WHALE_WAVE_ASSEMBLER_DIR",
        *("WHALE_WAVE_" + isa.upper() + "_DIR" for isa in ARCHITECTURES),
    ):
        directory = os.environ.get(name)
        if directory is None:
            modules[name] = None
            continue
        path = Path(directory).resolve(strict=True)
        artifacts = [
            file
            for pattern in ("*.meta", "*.a", "*.lib", "provenance.json")
            for file in sorted(path.glob(pattern))
        ]
        modules[name] = {
            "directory": str(path),
            "sha256": {file.name: sha256(file) for file in artifacts},
            "metadata": {
                file.name: dict(
                    line.split("=", 1) for line in file.read_text().splitlines() if "=" in line
                )
                for file in artifacts
                if file.suffix == ".meta"
            },
        }
    return modules


def benchmark_timer():
    timer = Path("/usr/bin/time")
    if not sys.platform.startswith("linux") or not timer.is_file():
        raise ValueError("Linux with GNU /usr/bin/time is required")
    return timer


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--target-dir", type=Path, default=Path("target/benchmark-cli"))
    parser.add_argument("--profile", choices=("debug", "release"), default="release")
    parser.add_argument("--cli", type=Path)
    parser.add_argument("--compare", type=Path)
    parser.add_argument("--skip-build", action="store_true")
    parser.add_argument("--isa", action="append", choices=tuple(ARCHITECTURES))
    parser.add_argument("--small", type=int, default=64)
    parser.add_argument("--large", type=int, default=32768)
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--warmups", type=int, default=1)
    parser.add_argument("--timeout", type=float, default=60)
    args = parser.parse_args(argv)
    if not (
        0 <= args.small < args.large <= 32768
        and 1 <= args.runs <= 100
        and 0 <= args.warmups <= 20
        and math.isfinite(args.timeout)
        and args.timeout > 0
    ):
        parser.error("require 0 <= small < large <= 32768, runs 1..100, warmups 0..20, timeout > 0")
    architectures = list(dict.fromkeys(args.isa or ARCHITECTURES))
    try:
        timer = benchmark_timer()
        target = args.target_dir.resolve()
        features = ",".join(["socket-cli", *("asm-" + isa for isa in architectures)])
        command = [
            "cargo",
            "build",
            "--locked",
            "--offline",
            "--jobs",
            "2",
            "--no-default-features",
            "--features",
            features,
            "--bin",
            "whale",
            "--target-dir",
            str(target),
        ]
        if args.profile == "release":
            command.append("--release")
        if not args.skip_build:
            execute(command, 600, env=dict(os.environ, CARGO_INCREMENTAL="0"))
        cli = (args.cli or target / args.profile / "whale").resolve(strict=True)
        product = product_info(cli, architectures, args.timeout)
        environment = build_environment()
        environment["product_configuration"] = {
            "reference_feature": product.get("reference_feature"),
            "native_linker": product.get("native_linker"),
            "wave_compilers": {
                module: {
                    key: data.get(key)
                    for key in (
                        "wave_revision",
                        "target",
                        "opt_level",
                        "compiler_sha256",
                        "std_sha256",
                    )
                }
                for module, data in product["wave_modules"].items()
            },
        }
        report = {
            "format_version": FORMAT_VERSION,
            "generator": GENERATOR,
            "revision": text_command(["git", "rev-parse", "HEAD"]),
            "dirty": bool(text_command(["git", "status", "--porcelain"])),
            "rustc": text_command(["rustc", "-vV"]),
            "cargo": text_command(["cargo", "-V"]),
            "python": platform.python_version(),
            "host": host_metadata(),
            "profile": args.profile,
            "build_command": command,
            "build_executed": not args.skip_build,
            "binaries_built_by_command": not args.skip_build and args.cli is None,
            "cli_sha256": sha256(cli),
            "product_build_info": product,
            "driver_sha256": sha256(Path(__file__)),
            "shared_driver_sha256": sha256(Path(benchmark.__file__)),
            "runs": args.runs,
            "warmups": args.warmups,
            "counts": [args.small, args.large],
            "architectures": architectures,
            "cli_measured": True,
            "build_environment": environment,
            "native_modules": native_modules(),
            "rss_method": text_command([str(timer), "--version"]).splitlines()[0]
            + "; %M KiB converted to bytes; whole child-process high-water RSS",
            "workloads": [],
            "measurements": [],
        }
        args.output.parent.mkdir(parents=True, exist_ok=True)
        inputs = args.output.parent / (args.output.stem + "-inputs")
        for size, count in (("small", args.small), ("large", args.large)):
            hashes, measurements = benchmark_size(
                cli, (inputs / size).resolve(), count, architectures, args, timer
            )
            report["workloads"].append({"size": size, "count": count, "sha256": hashes})
            for measurement in measurements:
                measurement["name"] = size + "/" + measurement["name"]
            report["measurements"].extend(measurements)
        if sha256(cli) != report["cli_sha256"]:
            raise ValueError("CLI executable changed during measurements")
        if args.compare:
            report["comparison"] = compare_reports(json.loads(args.compare.read_text()), report)
        with tempfile.NamedTemporaryFile(
            mode="w", encoding="utf-8", dir=args.output.parent, delete=False
        ) as stream:
            temporary = Path(stream.name)
            try:
                json.dump(report, stream, indent=2)
                stream.write("\n")
                stream.flush()
                os.fsync(stream.fileno())
                stream.close()
                os.replace(temporary, args.output)
            finally:
                temporary.unlink(missing_ok=True)
        for measurement in report["measurements"]:
            sample = measurement["median"]
            print(
                f"{measurement['name']}: {sample['phase_seconds'] * 1000:.3f} ms; "
                f"{sample['peak_rss_bytes'] / 1048576:.2f} MiB RSS"
            )
        print(f"saved {args.output}")
        return 0
    except (
        ValueError,
        OSError,
        KeyError,
        TypeError,
        struct.error,
        subprocess.SubprocessError,
    ) as error:
        print(f"CLI benchmark failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
