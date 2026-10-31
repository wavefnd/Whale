#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Independent canonical-byte goldens and transactional native writer checks.

The expected file is composed here from semantic rows, without calling the
native loader, verifier, printer, Rust interpreter, or writer helpers.
"""

import argparse
import ctypes as c
import hashlib
import random
import struct
from pathlib import Path

U64 = c.c_uint64
MISSING = (1 << 64) - 1
STRIDES = (40, 8, 88, 32, 136, 8, 8, 64, 32, 16, 1, 16, 56, 40, 8, 8)
NATIVE_WORDS = (5, 1, 11, 4, 21, 1, 1, 27, 4, 2, 0, 2, 7, 5, 1, 1)
INSTRUCTION_FIELDS = [
    "op",
    "sub",
    "ty",
    "other",
    "dst",
    "a",
    "b",
    "c",
    "align",
    "size",
    "natural",
    "first",
    "count",
    "low",
    "high",
    "literal",
    "signature",
    "flags",
    "global",
    "plan_first",
    "plan_count",
]
PROGRAM_FIELDS = [
    "types",
    "edges",
    "functions",
    "blocks",
    "instructions",
    "operands",
    "value_types",
    "globals",
    "sites",
    "texts",
    "text_bytes",
    "files",
    "originals",
    "mappings",
    "value_ids",
    "constant_globals",
]


class Limits(c.Structure):
    _fields_ = [
        (name, U64)
        for name in [
            "max_input_bytes",
            "max_decoded_bytes",
            "max_records",
            "max_work",
            "max_type_depth",
            "max_workspace_bytes",
        ]
    ]

    def __init__(self, **changes):
        values = (64 << 20, 128 << 20, 1_000_000, 67_108_864, 128, 256 << 20)
        super().__init__(*values)
        for name, value in changes.items():
            setattr(self, name, value)


class Result(c.Structure):
    _fields_ = [
        (name, U64)
        for name in ["status", "code", "table", "index", "work", "decoded_bytes", "scratch_words"]
    ] + [("counts", U64 * 16)]


class Program(c.Structure):
    _fields_ = [
        field for name in PROGRAM_FIELDS for field in ((name, c.c_void_p), (name + "_count", U64))
    ]


def instruction(**fields):
    words = dict.fromkeys(INSTRUCTION_FIELDS, 0)
    words.update(dst=MISSING, a=MISSING, b=MISSING, c=MISSING)
    words.update(fields)
    return tuple(words[name] for name in INSTRUCTION_FIELDS)


def fixture(kind=7, literal=1, low=42, high=0, parameter=False):
    """One sparse-ID function, UTF-8 text and every source metadata table."""
    values = [1, 1] if parameter else [1]
    operation = (
        instruction(op=7, ty=1, dst=1, a=0, b=0)
        if parameter
        else instruction(op=4, ty=1, dst=0, literal=literal, low=low, high=high)
    )
    terminal = instruction(op=34, ty=1, a=len(values) - 1)
    ids = [2_147_483_647, 4_294_967_295] if parameter else [4_294_967_295]
    return [
        [(0, 0, 0, 0, 0), (kind, 0, 0, 0, 0), (18, 0, 0, 2 if parameter else 1, 0)],
        [(1,), (1,)] if parameter else [(1,)],
        [(7, 2, 0, 0, 1, 0, int(parameter), len(values), 0, 1, 0)],
        [(19, 0, 0, 2)],
        [operation, terminal],
        [(0,)] if parameter else [],
        [(ty,) for ty in values],
        [],
        [(7, 19, 0, ids[-1]), (7, 19, 1, MISSING)],
        [(0, 3)],
        b"a\xce\xb2",
        [(5, 0)],
        [(900, 1, 5, 1, 1, 1, 4)],
        [(0, 7, 19, 0, 900), (2, 43, 0, 0, 900)],
        [(value,) for value in ids],
        [(43,)],
    ]


def global_fixture(initialized):
    tables = fixture()
    value = [0] * 27
    value[0] = 55
    value[1] = 1 if initialized else 3
    value[3] = 8
    if initialized:
        value[5:12] = [1, 1, 1, 1, 1, MISSING, MISSING]
        value[26] = 1
    else:
        tables[0].append((19, 1, 0, 4, 0))
    tables[7].append(tuple(value))
    tables[13].append((2, 55, 0, 0, 900))
    return tables


def import_fixture(referenced=False, sysv=False):
    tables = fixture()
    signature = 2
    if sysv:
        signature = 3
        tables[0].append((18, 0, 1, 1, 1))
        tables[1].append((1,))
    tables[2].append((9, signature, MISSING, 1, 0, 0, 0, 0, 1, 0, 1))
    if referenced:
        tables[4][0] = instruction(op=29, sub=int(sysv), ty=1, dst=0, a=1)
    return tables


def expected_image(tables):
    """The file schema intentionally has no native record padding or pointers."""
    bodies = []
    for table, rows in enumerate(tables, 1):
        if table == 11:
            bodies.append(rows)
            continue
        if table == 5:
            rows = [row[:9] + row[11:19] for row in rows]
        if table == 8:
            rows = [
                (row[0], row[1], row[3], row[5], row[6], row[9], row[10], row[11]) for row in rows
            ]
        bodies.append(b"".join(struct.pack("<" + "Q" * len(row), *row) for row in rows))
    size = 576 + sum(map(len, bodies))
    header = struct.pack("<8s7Q", b"WHALEBC\0", 1, 1, 1, size, 16, 0, 576)
    offset = 576
    directory = []
    for table, (rows, body, stride) in enumerate(zip(tables, bodies, STRIDES), 1):
        assert len(body) == len(rows) * stride
        directory.append(struct.pack("<4Q", table, offset, len(rows), stride))
        offset += len(body)
    return header + b"".join(directory) + b"".join(bodies)


class Native:
    def __init__(self, library):
        self.library = c.CDLL(str(library.resolve()))
        self.verify = self.library.whale_ir_bytecode_verify
        self.verify.argtypes = [
            c.POINTER(Program),
            c.POINTER(U64),
            U64,
            c.POINTER(Limits),
            c.POINTER(Result),
        ]
        self.verify.restype = c.c_int32
        self.write = self.library.whale_ir_bytecode_write
        self.write.argtypes = [
            c.POINTER(Program),
            c.POINTER(c.c_uint8),
            U64,
            c.POINTER(U64),
            c.POINTER(U64),
            U64,
            c.POINTER(Limits),
            c.POINTER(Result),
        ]
        self.write.restype = c.c_int32

    def packet(self, tables):
        packet = Program()
        arrays = []
        for name, rows, width in zip(PROGRAM_FIELDS, tables, NATIVE_WORDS):
            if width == 0:
                array = (c.c_uint8 * len(rows)).from_buffer_copy(rows)
            else:
                flat = [value for row in rows for value in row]
                assert len(flat) == len(rows) * width
                array = (U64 * len(flat))(*flat)
            arrays.append(array)
            setattr(packet, name, c.cast(array, c.c_void_p))
            setattr(packet, name + "_count", len(rows))
        return packet, arrays

    def admitted(self, packet, limits):
        result = Result()
        status = self.verify(c.byref(packet), None, 0, c.byref(limits), c.byref(result))
        assert status == result.status == 2 and result.scratch_words > 0, vars_result(result)
        scratch = (U64 * result.scratch_words)()
        status = self.verify(
            c.byref(packet), scratch, len(scratch), c.byref(limits), c.byref(result)
        )
        assert status == result.status == 0, vars_result(result)
        return scratch

    def publish(self, packet, scratch, limits, capacity, *, query=False):
        output = None if query else (c.c_uint8 * capacity)(*([0xA5] * capacity))
        required = U64(MISSING)
        result = Result()
        status = self.write(
            c.byref(packet),
            output,
            capacity,
            c.byref(required),
            scratch,
            len(scratch),
            c.byref(limits),
            c.byref(result),
        )
        assert status == result.status and result.work <= limits.max_work, vars_result(result)
        return status, required.value, bytes(output) if output is not None else None, result


def vars_result(result):
    return {
        name: getattr(result, name)
        for name in ["status", "code", "table", "index", "work", "decoded_bytes", "scratch_words"]
    }


def golden_cases(native, output_dir):
    cases = [
        ("signed64", fixture()),
        ("signed128-min", fixture(8, 1, 0, 1 << 63)),
        ("unsigned128", fixture(13, 2, 0xFEDCBA9876543210, 0x0123456789ABCDEF)),
        ("bool", fixture(1, 3, 1)),
        ("f16-nan", fixture(14, 4, 0x7D55)),
        ("f32-negative-zero", fixture(15, 5, 0x80000000)),
        ("f64-nan", fixture(16, 6, 0xFFF8000000001234)),
        ("sparse-parameter", fixture(parameter=True)),
        ("initialized-global", global_fixture(True)),
        ("uninitialized-aggregate-global", global_fixture(False)),
        ("unused-import", import_fixture()),
    ]
    hashes = {}
    for name, tables in cases:
        packet, keep = native.packet(tables)
        limits = Limits()
        scratch = native.admitted(packet, limits)
        expected = expected_image(tables)
        status, required, _, query = native.publish(packet, scratch, limits, 0, query=True)
        assert status == 0 and required == len(expected), (name, vars_result(query))
        status, required, actual, result = native.publish(packet, scratch, limits, required)
        assert status == 0 and required == len(expected), (name, vars_result(result))
        assert actual == expected, name
        assert keep  # Keep all borrowed native arrays alive through both calls.
        hashes[name] = hashlib.sha256(expected).hexdigest()
        if output_dir:
            output_dir.mkdir(parents=True, exist_ok=True)
            (output_dir / (name + ".wbc")).write_bytes(actual)
        # Deliberately unaligned UTF-8 table: later source records must remain LE.
        text_end = struct.unpack_from("<Q", actual, 64 + 10 * 32 + 8)[0] + 3
        assert text_end % 8 == 3
        for capacity in (0, 1, len(expected) - 1):
            status, needed, unchanged, failure = native.publish(packet, scratch, limits, capacity)
            assert status == 2 and needed == len(expected), vars_result(failure)
            assert unchanged == bytes([0xA5]) * capacity
        # The exact query cost admits verification but cannot admit the complete
        # publish pass. Reject before changing even byte zero of a large buffer.
        work_limits = Limits(max_work=query.work)
        status, _, unchanged, failure = native.publish(packet, scratch, work_limits, len(expected))
        assert status == 3 and failure.work <= query.work, vars_result(failure)
        assert unchanged == bytes([0xA5]) * len(expected)
        short_limits = Limits(max_input_bytes=len(expected) - 1)
        status, _, unchanged, failure = native.publish(packet, scratch, short_limits, len(expected))
        assert status != 0, vars_result(failure)
        assert unchanged == bytes([0xA5]) * len(expected)
    # A fixed digest protects this independently authored fixture from accidental
    # format changes in either native implementation or expected-image code.
    assert hashes["signed64"] == GOLDEN_SHA256, hashes["signed64"]
    return hashes


GOLDEN_SHA256 = "49ddb7169daf8c869879eb31929b012befcfd9a99beb0bdca896a1fe0c4a8225"


def mutations(native, cases):
    randomizer = random.Random(0x363_2026)
    tables = fixture()
    packet, keep = native.packet(tables)
    limits = Limits()
    scratch = native.admitted(packet, limits)
    size = len(expected_image(tables))
    instructions = c.cast(packet.instructions, c.POINTER(U64))
    types = c.cast(packet.types, c.POINTER(U64))
    values = c.cast(packet.value_ids, c.POINTER(U64))
    # Every mutation below is independently known invalid or unsupported. The
    # complete sentinel output proves early rejection is transactional.
    choices = [
        (instructions, 0, 36),
        (instructions, 2, 999),
        (instructions, 4, 99),
        (instructions, 21 + 5, 99),
        (instructions, 0, 5),
        (types, 10 + 4, 3),
        (values, 0, 1 << 32),
    ]
    for _ in range(cases):
        array, index, invalid = randomizer.choice(choices)
        previous = array[index]
        array[index] = invalid
        status, _, unchanged, result = native.publish(packet, scratch, limits, size)
        assert status != 0, (index, invalid, vars_result(result))
        assert unchanged == bytes([0xA5]) * size
        array[index] = previous
    # Runtime preparation may populate these fields between executions. They
    # are host-derived layout facts, and never affect deterministic disk bytes.
    expected = expected_image(tables)
    for index in (9, 10, 19, 20):
        instructions[index] = 0xDEADBEEF01234567
    status, _, actual, result = native.publish(packet, scratch, limits, size)
    assert status == 0 and actual == expected, vars_result(result)
    assert keep
    # An entirely unreferenced function still belongs to the executable file.
    # Undef is valid structural IR, but the VM cannot execute it; reject the
    # whole image rather than silently compiling only entry-reachable bodies.
    tables = fixture()
    tables[2].append((9, 2, 1, 1, 1, 0, 0, 1, 1, 1, 0))
    tables[3].append((29, 1, 2, 2))
    tables[4].extend([instruction(op=5, ty=1, dst=0), instruction(op=34, ty=1, a=0)])
    tables[6].append((1,))
    tables[8].extend([(9, 29, 0, 42), (9, 29, 1, MISSING)])
    tables[14].append((42,))
    for rejected, code, index in (
        (tables, 100, 2),
        (import_fixture(referenced=True), 102, 0),
        (import_fixture(referenced=True, sysv=True), 103, 0),
    ):
        packet, keep = native.packet(rejected)
        query = Result()
        status = native.verify(c.byref(packet), None, 0, c.byref(limits), c.byref(query))
        assert status == 2
        scratch = (U64 * query.scratch_words)()
        size = len(expected_image(rejected))
        status, _, unchanged, result = native.publish(packet, scratch, limits, size)
        assert (status, result.code, result.table, result.index) == (5, code, 5, index), (
            vars_result(result)
        )
        assert unchanged == bytes([0xA5]) * size
        assert keep


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", type=Path, required=True)
    parser.add_argument("--cases", type=int, default=2000)
    parser.add_argument("--output-dir", type=Path)
    args = parser.parse_args()
    if not 0 <= args.cases <= 100_000:
        parser.error("--cases must be between 0 and 100000")
    native = Native(args.library)
    hashes = golden_cases(native, args.output_dir)
    mutations(native, args.cases)
    print(
        f"bytecode writer: {len(hashes)} exact-byte goldens, "
        f"{args.cases} bounded rejected mutations; capacity/work/size atomicity passed"
    )
    for name, digest in hashes.items():
        print(f"{name}: {digest}")


if __name__ == "__main__":
    main()
