# SPDX-License-Identifier: MPL-2.0
"""Explicit internal Wave packages and their Rust data bindings."""

import hashlib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MODULES = {
    "asmobj": ("assembler/asmobj", "whale_asmobj", "WHALE_ASMOBJ_DIR"),
    "assembler": ("assembler", "whale_wave_assembler", "WHALE_WAVE_ASSEMBLER_DIR"),
    "amd64": ("assembler/amd64", "whale_wave_amd64", "WHALE_WAVE_AMD64_DIR"),
    "aarch64": ("assembler/aarch64", "whale_wave_aarch64", "WHALE_WAVE_AARCH64_DIR"),
    "riscv64": ("assembler/riscv64", "whale_wave_riscv64", "WHALE_WAVE_RISCV64_DIR"),
    "loongarch64": (
        "assembler/loongarch64",
        "whale_wave_loongarch64",
        "WHALE_WAVE_LOONGARCH64_DIR",
    ),
    "ir": ("ir", "whale_wave_ir", "WHALE_WAVE_IR_DIR"),
}
ASSEMBLERS = ("amd64", "aarch64", "riscv64", "loongarch64")


def inputs(module, root=ROOT):
    directory = MODULES[module][0]
    directories = [directory, "assembler"] if module in ASSEMBLERS else [directory]
    sources = [root / "vex.ws", root / "vex.lock"]
    for directory in directories:
        sources += [
            root / directory / "vex.ws",
            *(root / directory / "src").rglob("*.wave"),
        ]
    binding = root / "bindings/rust" / module
    abi = [binding / "Cargo.toml", *(binding / "src").rglob("*.rs")]
    return sorted(set(sources)), sorted(set(abi))


def input_bytes(paths, root=ROOT):
    for path in sorted(set(paths)):
        yield path.relative_to(root).as_posix().encode() + b"\0"
        yield path.read_bytes()
        yield b"\0"


def fnv64(chunks):
    value = 0xCBF29CE484222325
    for chunk in chunks:
        for byte in chunk:
            value = ((value ^ byte) * 0x100000001B3) & ((1 << 64) - 1)
    return f"{value:016x}"


def input_hashes(paths, root=ROOT):
    sha = hashlib.sha256()
    for chunk in input_bytes(paths, root):
        sha.update(chunk)
    return fnv64(input_bytes(paths, root)), sha.hexdigest()
