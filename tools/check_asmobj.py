#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Compare the compiled asmobj source module with an independent Python oracle.

Build the five explicit Wave sources with the pinned helper, link its object as
a shared library, then pass --library. No Rust asmobj implementation is involved.
"""
from __future__ import annotations

import argparse
import ctypes as c
import itertools
import json
from pathlib import Path
import random

U64 = c.c_uint64
U8P = c.POINTER(c.c_uint8)
NONE = (1 << 64) - 1


class Target(c.Structure):
    _fields_ = [(name, U64) for name in ("format", "machine", "address_bits", "endian")]


class Section(c.Structure):
    _fields_ = [("name", U8P), ("name_len", U64), ("kind", U64), ("alignment", U64),
                ("data", U8P), ("data_len", U64), ("zero_fill", U64)]


class Symbol(c.Structure):
    _fields_ = [("name", U8P), ("name_len", U64), ("kind", U64), ("section", U64),
                ("offset", U64), ("size", U64), ("binding", U64), ("visibility", U64)]


class Relocation(c.Structure):
    _fields_ = [("section", U64), ("offset", U64), ("name", U8P), ("name_len", U64),
                ("kind", U64), ("namespace", U8P), ("namespace_len", U64),
                ("code", U64), ("addend", c.c_int64)]


class Result(c.Structure):
    _fields_ = [(name, U64) for name in ("status", "index", "symbol_count")]


class Fixture:
    def __init__(self, target=(0, 0, 64, 0)):
        self.target = target
        self.sections = []
        self.symbols = []
        self.relocations = []

    def section(self, kind=0, name=b".text", alignment=16, data=b"\x90" * 16, zero=0):
        self.sections.append((name, kind, alignment, data, zero))
        return self

    def symbol(self, name=b"entry", section=0, kind=0, offset=0, size=0, binding=0, visibility=0):
        self.symbols.append((name, kind, section, offset, size, binding, visibility))
        return self

    def relocation(self, name=b"entry", kind=4, namespace=b"", code=0, section=0,
                   offset=0, addend=-4):
        self.relocations.append((section, offset, name, kind, namespace, code, addend))
        return self


def oracle(fixture):
    fmt, machine, bits, endian = fixture.target
    if fmt > 2 or machine > 3 or bits != 64 or endian != 0 or (fmt != 0 and machine > 1):
        return 1, 0, None
    sections = [(name, (0, 1, 3, 2)[kind], align, data, zero)
                for name, kind, align, data, zero in fixture.sections]
    for index, symbol in enumerate(fixture.symbols):
        if symbol[2] != NONE and symbol[2] >= len(sections):
            return 2, index, None
    undefined = {symbol[0]: symbol[2] == NONE for symbol in fixture.symbols}
    known = set(undefined)
    symbols = list(fixture.symbols)
    relocations = []
    for index, (section, offset, name, kind, namespace, code, addend) in enumerate(fixture.relocations):
        result_code = 0
        if kind == 6:
            owner = {b"aarch64": 1, b"riscv64": 2, b"loongarch64": 3}.get(namespace)
            if owner is None:
                return 3, index, None
            if owner != machine:
                return 4, index, None
            if fmt == 0:
                result_code = code
            elif fmt == 2 and owner == 1 and code in (282, 283):
                kind, result_code = 8, 2
            elif fmt == 1 and owner == 1:
                codes = {274: 5, 280: 15, 282: 3, 283: 3}
                if code not in codes:
                    return 5, index, None
                kind, result_code = 7, codes[code]
            else:
                return 6, index, None
        elif kind == 4:
            if fmt == 2:
                kind, result_code = 8, 2
            elif not undefined.get(name, True):
                kind = 3
        relocations.append((section, offset, name, kind, b"", result_code, addend))
        if name not in known:
            known.add(name)
            symbols.append((name, 0, NONE, 0, 0, 1, 0))
    return 0, 0, (sections, symbols, relocations)


def raw_bytes(value):
    return c.string_at(c.addressof(value), c.sizeof(value))


class Checker:
    def __init__(self, library):
        self.library = c.CDLL(str(Path(library).resolve()))
        self.entry = self.library.whale_asmobj
        self.entry.argtypes = [c.POINTER(Target), c.POINTER(Section), U64, c.POINTER(Symbol), U64,
                               c.POINTER(Relocation), U64, c.POINTER(Section), U64,
                               c.POINTER(Symbol), U64, c.POINTER(Relocation), U64,
                               c.POINTER(U64), U64, c.POINTER(Result)]
        self.entry.restype = c.c_int32
        self.count = 0

    def check(self, fixture, capacity=None, expected_status=None, mutate=None):
        keep = []

        def span(data):
            storage = (c.c_uint8 * max(1, len(data)))()
            if data:
                c.memmove(storage, data, len(data))
            keep.append(storage)
            return c.cast(storage, U8P), len(data)

        target = Target(*fixture.target)
        sections = (Section * len(fixture.sections))()
        for i, (name, kind, align, data, zero) in enumerate(fixture.sections):
            name_ptr, name_len = span(name)
            data_ptr, data_len = span(data)
            sections[i] = Section(name_ptr, name_len, kind, align, data_ptr, data_len, zero)
        symbols = (Symbol * len(fixture.symbols))()
        for i, (name, kind, section, offset, size, binding, visibility) in enumerate(fixture.symbols):
            name_ptr, name_len = span(name)
            symbols[i] = Symbol(name_ptr, name_len, kind, section, offset, size, binding, visibility)
        relocs = (Relocation * len(fixture.relocations))()
        for i, (section, offset, name, kind, namespace, code, addend) in enumerate(fixture.relocations):
            name_ptr, name_len = span(name)
            ns_ptr, ns_len = span(namespace)
            relocs[i] = Relocation(section, offset, name_ptr, name_len, kind, ns_ptr, ns_len, code, addend)
        expected, index, converted = oracle(fixture)
        ns, ny, nr = len(sections), len(symbols) + len(relocs), len(relocs)
        out_sections = (Section * max(1, ns))()
        out_symbols = (Symbol * max(1, ny))()
        out_relocs = (Relocation * max(1, nr))()
        for output in (out_sections, out_symbols, out_relocs):
            c.memset(c.addressof(output), 0xA5, c.sizeof(output))
        before_outputs = tuple(raw_bytes(v) for v in (out_sections, out_symbols, out_relocs))
        before_inputs = tuple(raw_bytes(v) for v in (target, sections, symbols, relocs))
        total = len(symbols) + len(relocs)
        words = 8 * (1 << (total - 1).bit_length()) if total else 0
        scratch = (U64 * max(1, words))()
        result = Result(99, 99, 99)
        capacities = list(capacity or (ns, ny, nr))
        call = [c.byref(target), sections, ns, symbols, len(symbols), relocs, nr,
                out_sections, capacities[0], out_symbols, capacities[1], out_relocs,
                capacities[2], scratch, words, c.byref(result)]
        if mutate:
            mutate(call, sections, symbols, relocs, scratch)
            before_inputs = tuple(raw_bytes(v) for v in (target, sections, symbols, relocs))
        status = self.entry(*call)
        expected = expected if expected_status is None else expected_status
        assert status == expected and result.status == status, (fixture.__dict__, status, expected, result.status)
        assert tuple(raw_bytes(v) for v in (target, sections, symbols, relocs)) == before_inputs
        if status:
            assert result.symbol_count == 0
            if expected_status is None:
                assert result.index == index, (status, result.index, index)
            assert tuple(raw_bytes(v) for v in (out_sections, out_symbols, out_relocs)) == before_outputs
        else:
            assert result.index == 0 and result.symbol_count == len(converted[1])

            def data(ptr, length):
                return c.string_at(ptr, length)

            actual_sections = [(data(v.name, v.name_len), v.kind, v.alignment,
                                data(v.data, v.data_len), v.zero_fill) for v in out_sections[:ns]]
            actual_symbols = [(data(v.name, v.name_len), v.kind, v.section, v.offset, v.size,
                               v.binding, v.visibility) for v in out_symbols[:result.symbol_count]]
            actual_relocs = [(v.section, v.offset, data(v.name, v.name_len), v.kind,
                              data(v.namespace, v.namespace_len), v.code, v.addend)
                             for v in out_relocs[:nr]]
            assert (actual_sections, actual_symbols, actual_relocs) == converted
            # Spare rows are never published or overwritten.
            tail = result.symbol_count * c.sizeof(Symbol)
            assert raw_bytes(out_symbols)[tail:] == before_outputs[1][tail:]
        self.count += 1

    def check_emission_work_budget(self, length, repetitions, expected):
        # Keep the independent near-budget regression outside oracle(): a large
        # diagnostic must never stringify a 24 MiB fixture or Python-hash it.
        # The old preflight omitted equality when an empty slot became occupied.
        name = (c.c_uint8 * length)()
        c.memset(name, ord("a"), length)
        section_name = (c.c_uint8 * 5)(*b".text")
        sections = (Section * 1)(Section(section_name, 5, 0, 1, None, 0, 0))
        symbols = (Symbol * 0)()
        relocs = (Relocation * repetitions)(*[
            Relocation(0, 0, name, length, 4, None, 0, 0, -4)
            for _ in range(repetitions)])
        out_sections = (Section * 1)()
        out_symbols = (Symbol * repetitions)()
        out_relocs = (Relocation * repetitions)()
        outputs = (out_sections, out_symbols, out_relocs)
        for output in outputs:
            c.memset(c.addressof(output), 0xA5, c.sizeof(output))
        before = tuple(raw_bytes(v) for v in outputs)
        target = Target(0, 0, 64, 0)
        words = 8 * (1 << (repetitions - 1).bit_length())
        scratch = (U64 * words)()
        result = Result(99, 99, 99)
        status = self.entry(c.byref(target), sections, 1, symbols, 0, relocs, repetitions,
                            out_sections, 1, out_symbols, repetitions, out_relocs,
                            repetitions, scratch, words, c.byref(result))
        assert status == expected and result.status == expected, (length, repetitions, status, result.status)
        if expected:
            assert result.symbol_count == 0
            assert tuple(raw_bytes(v) for v in outputs) == before
        else:
            assert result.symbol_count == 1
            assert out_symbols[0].section == NONE and out_symbols[0].name_len == length
            assert out_symbols[0].binding == 1 and out_symbols[0].kind == 0
            assert c.cast(out_symbols[0].name, c.c_void_p).value == c.addressof(name)
            for relocation in out_relocs:
                assert relocation.kind == 4 and relocation.name_len == length and relocation.addend == -4
            assert raw_bytes(out_symbols)[c.sizeof(Symbol):] == before[1][c.sizeof(Symbol):]
        # Read the borrowed input through a bounded C comparison, with no Python
        # per-byte loop or large temporary expansion.
        expected_name = b"a" * length
        assert c.string_at(name, length) == expected_name
        self.count += 1


def run(checker, cases):
    checker.check(Fixture())
    for fmt, machine in itertools.product(range(4), range(5)):
        for state in ("unknown", "undefined", "defined", "duplicate"):
            fixture = Fixture((fmt, machine, 64, 0)).section()
            if state == "undefined":
                fixture.symbol(section=NONE)
            elif state == "defined":
                fixture.symbol()
            elif state == "duplicate":
                fixture.symbol().symbol(section=NONE)
            for kind in range(6):
                fixture.relocation(kind=kind)
            checker.check(fixture)
    for fmt, machine, namespace, code in itertools.product(
            range(3), range(4), (b"aarch64", b"riscv64", b"loongarch64", b"unknown", b""),
            (0, 16, 17, 64, 66, 274, 280, 282, 283, (1 << 32) - 1)):
        checker.check(Fixture((fmt, machine, 64, 0)).section().relocation(
            kind=6, namespace=namespace, code=code))
    checker.check(Fixture().section(kind=2).section(kind=3, data=b"", zero=1 << 40)
                  .symbol(b"\xce\xbb", section=1, kind=2, size=1 << 40, binding=1, visibility=1)
                  .relocation(name=b"missing").relocation(name=b"missing")
                  .relocation(name=b"second", kind=0, addend=-(1 << 63)))
    checker.check(Fixture().section().symbol(section=999))
    for slot in range(3):
        capacities = [1, 1, 1]
        capacities[slot] = 0
        checker.check(Fixture().section().relocation(), capacity=capacities, expected_status=7)
    checker.check(Fixture().section().relocation(), expected_status=8,
                  mutate=lambda call, *_: call.__setitem__(7, None))
    checker.check(Fixture().section().relocation(), expected_status=7,
                  mutate=lambda call, *_: call.__setitem__(14, 4))
    checker.check(Fixture().section().relocation(), expected_status=8,
                  mutate=lambda call, sections, symbols, relocs, _: setattr(relocs[0], "kind", 99))
    checker.check(Fixture().section().relocation(), expected_status=9,
                  mutate=lambda call, sections, symbols, relocs, _: setattr(relocs[0], "name_len", 67108865))
    # Deliberate linear-probe collisions exercise retained source indices,
    # comparisons, duplicated references and the full-table emission pass.
    colliding = []
    candidate = 0
    while len(colliding) < 64:
        name = f"collision_{candidate}".encode()
        hash_value = 14695981039346656037
        for octet in name:
            hash_value = ((hash_value ^ octet) * 1099511628211) & NONE
        if hash_value & 255 == 0:
            colliding.append(name)
        candidate += 1
    collisions = Fixture().section()
    for name in colliding[:32]:
        collisions.symbol(name)
    for name in colliding:
        collisions.relocation(name)
    for name in colliding[32:]:
        collisions.relocation(name)
    checker.check(collisions)
    checker.check_emission_work_budget(16 * 1024 * 1024 - 64, 1, 0)
    checker.check_emission_work_budget(16 * 1024 * 1024, 1, 9)
    checker.check_emission_work_budget(24 * 1024 * 1024, 1, 9)
    checker.check_emission_work_budget(7 * 1024 * 1024, 2, 0)
    checker.check_emission_work_budget(8 * 1024 * 1024, 2, 9)
    rng = random.Random(0xA5_0B1_EC7)
    names = [b"entry", b"x", b"", b"long_name" * 20, b"\xce\xbb", b"a\0b"]
    for _ in range(cases):
        fixture = Fixture((rng.randrange(3), rng.randrange(4), 64, 0))
        for _ in range(rng.randrange(1, 5)):
            fixture.section(rng.randrange(4), rng.choice(names), 1 << rng.randrange(5),
                            bytes(rng.randrange(256) for _ in range(rng.randrange(20))), rng.randrange(100))
        for _ in range(rng.randrange(10)):
            fixture.symbol(rng.choice(names), rng.choice((NONE, rng.randrange(len(fixture.sections)))),
                           rng.randrange(3), rng.randrange(50), rng.randrange(50), rng.randrange(2), rng.randrange(2))
        for _ in range(rng.randrange(15)):
            fixture.relocation(rng.choice(names), rng.randrange(7),
                               rng.choice((b"aarch64", b"riscv64", b"loongarch64", b"wrong")),
                               rng.choice((0, 16, 17, 64, 65, 66, 274, 280, 282, 283)),
                               rng.randrange(len(fixture.sections)), rng.randrange(20), rng.randrange(-100, 100))
        checker.check(fixture)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", required=True, type=Path)
    parser.add_argument("--cases", type=int, default=10000)
    args = parser.parse_args()
    if args.cases < 0:
        parser.error("--cases must be nonnegative")
    assert [c.sizeof(t) for t in (Target, Section, Symbol, Relocation, Result)] == [32, 56, 64, 72, 24]
    checker = Checker(args.library)
    run(checker, args.cases)
    print(json.dumps({"library": str(args.library), "cases": checker.count, "status": "passed"}))


if __name__ == "__main__":
    main()
