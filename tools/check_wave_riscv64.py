#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Independent RV64I native scalar, operand and instruction conformance."""

import argparse
import ctypes as c
import itertools
import random


class Text(c.Structure):
    _fields_ = [("data", c.POINTER(c.c_uint8)), ("count", c.c_uint64)]


class Instruction(c.Structure):
    _fields_ = [
        ("mnemonic", Text),
        ("operands", c.POINTER(Text)),
        ("count", c.c_uint64),
        ("fold", c.c_uint64),
    ]


class Symbol(c.Structure):
    _fields_ = [("name", Text)] + [
        (field, c.c_uint64) for field in ("section", "offset", "start", "end", "line", "flags")
    ]


class Relocation(c.Structure):
    _fields_ = [(field, c.c_uint64) for field in ("section", "offset", "symbol", "code")] + [
        ("addend", c.c_int64)
    ]


class Context(c.Structure):
    _fields_ = [
        ("source", Text),
        ("namespace", Text),
        ("symbols", c.POINTER(Symbol)),
        ("symbol_count", c.c_uint64),
        ("table", c.POINTER(c.c_uint64)),
        ("table_capacity", c.c_uint64),
        ("section", c.c_uint64),
        ("offset", c.c_uint64),
        ("relocations", c.POINTER(Relocation)),
        ("relocation_count", c.c_uint64),
        ("relocation_capacity", c.c_uint64),
    ]


class Result(c.Structure):
    _fields_ = [
        (field, c.c_uint64)
        for field in (
            "status",
            "error",
            "start",
            "end",
            "line",
            "previous_start",
            "previous_end",
            "previous_line",
        )
    ]
    _fields_ += [("text", Text)]
    _fields_ += [
        (field, c.c_uint64)
        for field in (
            "detail",
            "display",
            "word",
            "sections",
            "symbols",
            "relocations",
            "records",
            "operands",
            "bytes",
            "work",
            "max_work",
        )
    ]


ALIASES = [
    "zero",
    "ra",
    "sp",
    "gp",
    "tp",
    "t0",
    "t1",
    "t2",
    "s0",
    "s1",
    "a0",
    "a1",
    "a2",
    "a3",
    "a4",
    "a5",
    "a6",
    "a7",
    "s2",
    "s3",
    "s4",
    "s5",
    "s6",
    "s7",
    "s8",
    "s9",
    "s10",
    "s11",
    "t3",
    "t4",
    "t5",
    "t6",
]
REGISTER = {
    "add": (51, 0, 0),
    "sub": (51, 0, 32),
    "sll": (51, 1, 0),
    "slt": (51, 2, 0),
    "sltu": (51, 3, 0),
    "xor": (51, 4, 0),
    "srl": (51, 5, 0),
    "sra": (51, 5, 32),
    "or": (51, 6, 0),
    "and": (51, 7, 0),
    "addw": (59, 0, 0),
    "subw": (59, 0, 32),
    "sllw": (59, 1, 0),
    "srlw": (59, 5, 0),
    "sraw": (59, 5, 32),
}
IMMEDIATE = {
    "addi": (19, 0),
    "slti": (19, 2),
    "sltiu": (19, 3),
    "xori": (19, 4),
    "ori": (19, 6),
    "andi": (19, 7),
    "addiw": (27, 0),
}
BRANCH = {"beq": 0, "bne": 1, "blt": 4, "bge": 5, "bltu": 6, "bgeu": 7}
LOAD = {"lb": 0, "lh": 1, "lw": 2, "ld": 3, "lbu": 4, "lhu": 5, "lwu": 6}
STORE = {"sb": 0, "sh": 1, "sw": 2, "sd": 3}
MASK32 = (1 << 32) - 1


def r(opcode, f3, f7, rd, rs1, rs2):
    return opcode | rd << 7 | f3 << 12 | rs1 << 15 | rs2 << 20 | f7 << 25


def i(opcode, f3, rd, rs1, immediate):
    return opcode | rd << 7 | f3 << 12 | rs1 << 15 | (immediate & 4095) << 20


def jump(rd, displacement):
    value = displacement & ((1 << 21) - 1)
    return (
        111
        | rd << 7
        | (value >> 20) << 31
        | ((value >> 1) & 1023) << 21
        | ((value >> 11) & 1) << 20
        | ((value >> 12) & 255) << 12
    )


def branch(f3, rs1, rs2, displacement):
    value = displacement & 8191
    return (
        99
        | ((value >> 11) & 1) << 7
        | ((value >> 1) & 15) << 8
        | f3 << 12
        | rs1 << 15
        | (rs2 << 20)
        | ((value >> 5) & 63) << 25
        | (value >> 12) << 31
    )


class Native:
    def __init__(self, path):
        self.lib = c.CDLL(path)
        self.lib.whale_riscv64_instruction.argtypes = [
            c.POINTER(Instruction),
            c.POINTER(Context),
            c.POINTER(Result),
        ]
        self.lib.whale_riscv64_operand.argtypes = [c.c_uint64, c.POINTER(Text), c.POINTER(Result)]
        self.lib.whale_riscv64_branch.argtypes = [c.c_uint64] * 4 + [c.c_int64, c.POINTER(Result)]
        self.count = 0

    @staticmethod
    def text(value):
        data = value.encode()
        buffer = (c.c_uint8 * len(data))(*data)
        return Text(buffer, len(data)), buffer

    def instruction(self, mnemonic, operands, expected, fold=0):
        texts = [self.text(value) for value in [mnemonic, *operands]]
        raw = (Text * len(operands))(*(text for text, _ in texts[1:]))
        instruction = Instruction(texts[0][0], raw, len(operands), fold)
        table = (c.c_uint64 * 2)((1 << 64) - 1, (1 << 64) - 1)
        relocs = (Relocation * 2)()
        context = Context(table=table, table_capacity=2, relocations=relocs, relocation_capacity=2)
        result = Result(max_work=1_000_000)
        status = self.lib.whale_riscv64_instruction(
            c.byref(instruction), c.byref(context), c.byref(result)
        )
        self.count += 1
        if expected is None:
            assert status != 0 and result.status == status, (mnemonic, operands, result.error)
        else:
            assert status == 0 and result.status == 0 and result.word == expected & MASK32, (
                mnemonic,
                operands,
                status,
                result.error,
                hex(result.word),
                hex(expected),
            )

    def operand(self, mode, value, expected):
        text, buffer = self.text(value)
        result = Result()
        status = self.lib.whale_riscv64_operand(mode, c.byref(text), c.byref(result))
        self.count += 1
        if expected is None:
            assert status != 0 and result.status == status, (mode, value, result.error)
        else:
            assert status == 0 and (result.word, result.detail) == expected, (
                mode,
                value,
                status,
                result.error,
                result.word,
                result.detail,
            )
        assert buffer is not None


def boundaries(native):
    for code, alias in enumerate(ALIASES):
        for name in [alias, alias.upper(), f"x{code}", f"X{code}", f"\u2003{alias}\u0085"]:
            native.operand(0, name, (code, 0))
    native.operand(0, "fp", (8, 0))
    for name in ["x00", "x01", "x031", "x+1", "x-0", "x-1", "x32", "x１", "$ra", "", "a8"]:
        native.operand(0, name, None)
    for offset in [-2049, -2048, -1, 0, 2047, 2048]:
        native.operand(1, f"{offset}(sp)", (2, offset & 4095) if -2048 <= offset <= 2047 else None)
    native.operand(1, "(sp)", (2, 0))
    for memory in ["4((sp))", "4(sp))", "4(sp", "4(x32)", "4(x+1)", "4(x01)", "4(sp) "]:
        native.operand(1, memory, None)
    for length in range(5):
        for flags in itertools.permutations("iorw", length):
            spelling = "".join(flags)
            mask = sum({"i": 8, "o": 4, "r": 2, "w": 1}[flag] for flag in flags)
            native.operand(2, spelling, (mask, 0))
    native.operand(2, "0", (0, 0))
    for bad in ["rr", "ii", "rwz", "RW", "0r", "r0", "00", "+0", "-0", "0x0"]:
        native.operand(2, bad, None)
    for mnemonic, f3 in BRANCH.items():
        for value in [-4098, -4096, -2, -1, 0, 1, 4094, 4096]:
            native.instruction(
                mnemonic,
                ["x1", "x31", str(value)],
                branch(f3, 1, 31, value) if -4096 <= value <= 4094 and value % 2 == 0 else None,
            )
    for value in [-(1 << 20) - 2, -(1 << 20), -2, -1, 0, 1, (1 << 20) - 2, 1 << 20]:
        expected = jump(1, value) if -(1 << 20) <= value < 1 << 20 and value % 2 == 0 else None
        native.instruction("jal", ["ra", str(value)], expected)
    for mnemonic, (op, f3) in IMMEDIATE.items():
        for value in [-2049, -2048, -1, 0, 2047, 2048, 1 << 127]:
            native.instruction(
                mnemonic,
                ["a0", "a1", str(value)],
                i(op, f3, 10, 11, value) if -2048 <= value <= 2047 else None,
            )
    for mnemonic, opcode in [("lui", 55), ("auipc", 23)]:
        for value in [-1, 0, 1048575, 1048576]:
            native.instruction(
                mnemonic,
                ["a0", str(value)],
                opcode | 10 << 7 | value << 12 if 0 <= value < 1 << 20 else None,
            )
    for mnemonic in ["slli", "srli", "srai", "slliw", "srliw", "sraiw"]:
        word = mnemonic.endswith("w")
        for count in [-1, 0, 31, 32, 63, 64]:
            expected = None
            if 0 <= count < (32 if word else 64):
                immediate = count | (1024 if mnemonic.startswith("sra") else 0)
                expected = i(
                    27 if word else 19, 1 if mnemonic.startswith("sll") else 5, 10, 11, immediate
                )
            native.instruction(mnemonic, ["a0", "a1", str(count)], expected)
    for name, expected in [("nop", 19), ("ret", 32871), ("ecall", 115), ("ebreak", 1048691)]:
        native.instruction(name, [], expected)
        native.instruction(name, ["x1"], None)
    for name, expected in [
        ("mv", i(19, 0, 10, 11, 0)),
        ("not", i(19, 4, 10, 11, 4095)),
        ("neg", r(51, 0, 32, 10, 0, 11)),
        ("negw", r(59, 0, 32, 10, 0, 11)),
    ]:
        native.instruction(name, ["a0", "a1"], expected)
    native.instruction("fence", [], 0x0FF0000F)
    native.instruction("fence", ["rw", "0"], 0x0300000F)
    native.instruction("fence", ["RW", "0"], None)
    native.instruction("ADD", ["a0", "a1", "a2"], r(51, 0, 0, 10, 11, 12), fold=1)
    native.instruction("ADD", ["a0", "a1", "a2"], None)
    for extension in ["mul", "fadd.d", "c.add", "lr.d", "amoadd.d", "vadd.vv"]:
        native.instruction(extension, ["a0", "a1", "a2"], None)


def generated(native, count):
    rng = random.Random(0x359)
    for _ in range(count):
        rd, rs1, rs2 = (rng.randrange(32) for _ in range(3))
        family = rng.randrange(5)
        if family == 0:
            mnemonic = rng.choice(list(REGISTER))
            op, f3, f7 = REGISTER[mnemonic]
            native.instruction(
                mnemonic, [f"x{rd}", f"x{rs1}", f"x{rs2}"], r(op, f3, f7, rd, rs1, rs2)
            )
        elif family == 1:
            mnemonic = rng.choice(list(IMMEDIATE))
            op, f3 = IMMEDIATE[mnemonic]
            immediate = rng.randrange(-2048, 2048)
            native.instruction(
                mnemonic, [f"x{rd}", f"x{rs1}", str(immediate)], i(op, f3, rd, rs1, immediate)
            )
        elif family == 2:
            mnemonic = rng.choice(list(LOAD))
            immediate = rng.randrange(-2048, 2048)
            native.instruction(
                mnemonic,
                [f"x{rd}", f"{immediate}(x{rs1})"],
                i(3, LOAD[mnemonic], rd, rs1, immediate),
            )
        elif family == 3:
            mnemonic = rng.choice(list(STORE))
            immediate = rng.randrange(-2048, 2048) & 4095
            expected = (
                35
                | (immediate & 31) << 7
                | STORE[mnemonic] << 12
                | rs1 << 15
                | rs2 << 20
                | (immediate >> 5) << 25
            )
            native.instruction(
                mnemonic,
                [f"x{rs2}", f"{immediate if immediate < 2048 else immediate - 4096}(x{rs1})"],
                expected,
            )
        else:
            mnemonic = rng.choice(list(BRANCH))
            displacement = rng.randrange(-2048, 2048) * 2
            native.instruction(
                mnemonic,
                [f"x{rs1}", f"x{rs2}", str(displacement)],
                branch(BRANCH[mnemonic], rs1, rs2, displacement),
            )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", required=True)
    parser.add_argument("--cases", type=int, default=100_000)
    args = parser.parse_args()
    native = Native(args.library)
    boundaries(native)
    generated(native, args.cases)
    print(f"Wave RV64I: {native.count} independent instruction/operand mathematical vectors passed")


if __name__ == "__main__":
    main()
