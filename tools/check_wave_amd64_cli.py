#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Compare AMD64 CLI encodings with NASM and execute static Linux fixtures."""

import argparse
import hashlib
import json
from pathlib import Path
import platform
import random
import shutil
import struct
import subprocess


def run(command, *, expected=0):
    result = subprocess.run(list(map(str, command)), capture_output=True, timeout=30)
    if result.returncode != expected:
        raise ValueError(f"command failed: {command!r}: {result.stderr.decode(errors='replace')}")
    return result


def text_section(path):
    data = path.read_bytes()
    if data[:6] != b"\x7fELF\x02\x01":
        raise ValueError("oracle requires ELF64 little endian")
    offset, = struct.unpack_from("<Q", data, 40)
    width, count, names = struct.unpack_from("<HHH", data, 58)
    if width != 64 or names >= count or offset + width * count > len(data):
        raise ValueError("invalid oracle section table")
    sections = [struct.unpack_from("<IIQQQQIIQQ", data, offset + width * i) for i in range(count)]
    names_offset, names_size = sections[names][4:6]
    strings = data[names_offset:names_offset + names_size]
    for section in sections:
        name = strings[section[0]:].split(b"\0", 1)[0]
        if name == b".text":
            at, size = section[4:6]
            if at + size > len(data):
                raise ValueError("truncated oracle text section")
            return data[at:at + size]
    raise ValueError("oracle has no text section")


def instructions(seed, count):
    randomizer = random.Random(seed)
    gprs = ("rax", "rcx", "rdx", "rbx", "rsi", "rdi", "r8", "r9", "r10", "r11", "r12", "r13", "r14", "r15", "rsp", "rbp")
    for _ in range(count):
        kind = randomizer.randrange(4)
        base = randomizer.choice(gprs)
        if kind == 0:
            yield f"{randomizer.choice(('not', 'neg', 'div', 'idiv'))} {base}"
        elif kind == 1:
            index = randomizer.choice(tuple(reg for reg in gprs if reg != "rsp"))
            scale = randomizer.choice((2, 4, 8))
            displacement = randomizer.choice((-129, -128, 1, 127, 128, 65536))
            yield f"lea {randomizer.choice(gprs)},[{base}+{index}*{scale}{displacement:+d}]"
        elif kind == 2:
            op = randomizer.choice(("movss", "movsd", "addss", "addsd", "subss", "subsd",
                                    "mulss", "mulsd", "divss", "divsd"))
            yield f"{op} xmm{randomizer.randrange(16)},xmm{randomizer.randrange(16)}"
        else:
            op = randomizer.choice(("movss", "movsd", "addss", "addsd", "subss", "subsd"))
            yield f"{op} xmm{randomizer.randrange(16)},[{base}+16]"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--cases", type=int, default=2000)
    parser.add_argument("--seed", type=int, default=0x357356)
    parser.add_argument("--nasm", default="nasm")
    args = parser.parse_args()
    if not 1 <= args.cases <= 100000:
        parser.error("--cases must be between 1 and 100000")
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        parser.error("native static execution requires Linux x86_64")
    nasm = shutil.which(args.nasm)
    if nasm is None:
        parser.error("NASM is required for the independent oracle")
    binary = args.binary.resolve(strict=True)
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    source = output / "vectors.asm"
    source.write_text("section .text\n" + "\n".join(instructions(args.seed, args.cases)) + "\n")
    whale_object, oracle_object = output / "whale.o", output / "nasm.o"
    run([binary, "asm", "--amd64", source, "-o", whale_object])
    run([nasm, "-O0", "-f", "elf64", source, "-o", oracle_object])
    actual, expected = text_section(whale_object), text_section(oracle_object)
    (output / "whale.text").write_bytes(actual)
    (output / "nasm.text").write_bytes(expected)
    if actual != expected:
        first = next((i for i, pair in enumerate(zip(actual, expected)) if pair[0] != pair[1]),
                     min(len(actual), len(expected)))
        raise ValueError(f"NASM encoding divergence at byte {first}; evidence retained in {output}")

    sources = {
        "leaf": "section .text\nglobal _start\n_start: mov edi,42\nmov eax,60\nsyscall\n",
        "start": ("section .text\nglobal _start\nextern helper\nextern payload\n"
                  "_start: lea rsi,[rel payload]\nmov eax,[rsi]\ncall helper\n"
                  "mov edi,eax\nmov eax,60\nsyscall\n"),
        "helper": ("section .text\nglobal helper\nhelper: ret\nsection .data\n"
                   "global payload\npayload: dd 42\nsection .bss\nresb 64\n"),
    }
    objects = {}
    for name, contents in sources.items():
        source = output / (name + ".asm")
        source.write_text(contents)
        objects[name] = output / (name + ".o")
        run([binary, "asm", "--amd64", source, "-o", objects[name]])
    for name, inputs in (("leaf", [objects["leaf"]]), ("cross-object", [objects["start"], objects["helper"]])):
        executable = output / name
        run([binary, "link", *inputs, "-o", executable])
        run([executable], expected=42)
    report = {"seed": args.seed, "instructions": args.cases, "bytes": len(actual),
              "text_sha256": hashlib.sha256(actual).hexdigest(),
              "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "nasm": run([nasm, "-v"]).stdout.decode().strip(),
              "leaf_exit": 42, "cross_object_exit": 42}
    (output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"NASM: {args.cases} AMD64 integer/address/SSE encodings; leaf/cross-object exit 42")


if __name__ == "__main__":
    main()
