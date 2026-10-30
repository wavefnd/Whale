#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Independent native AST admission, numeric syntax and source-table oracle."""

import argparse
import ctypes as C
import random

MASK = (1 << 64) - 1


class State(C.Structure):
    _fields_ = [(name, C.c_uint64) for name in
               ("nodes", "strings", "features", "numbers", "sources", "work")]


class Event(C.Structure):
    _fields_ = [(name, C.c_uint64) for name in ("tag", "a", "b", "c", "d", "e", "f")]
    _fields_ += [("bytes", C.POINTER(C.c_uint8)), ("length", C.c_uint64)]


class Slot(C.Structure):
    _fields_ = [("id", C.c_uint64), ("occupied", C.c_uint64)]


class Result(C.Structure):
    _fields_ = [("status", C.c_uint64)]


def fields(value):
    return tuple(getattr(value, name) for name, _ in State._fields_)


def mix(value):
    value = ((value ^ (value >> 30)) * 13787848793156543929) & MASK
    value = ((value ^ (value >> 27)) * 10723151780598845931) & MASK
    return value ^ (value >> 31)


class Kernel:
    def __init__(self, library):
        self.library = C.CDLL(library)
        self.step = self.library.whale_ir_ast_inspect_step
        self.step.restype = C.c_int32
        self.step.argtypes = [C.POINTER(State), C.POINTER(Event), C.c_uint64,
                              C.c_uint64, C.c_uint64, C.c_uint64,
                              C.POINTER(Slot), C.c_uint64, C.POINTER(Result)]
        self.rehash = self.library.whale_ir_ast_inspect_rehash
        self.rehash.restype = C.c_int32
        self.rehash.argtypes = [C.POINTER(State), C.POINTER(Slot), C.c_uint64,
                                C.POINTER(Slot), C.c_uint64, C.c_uint64,
                                C.POINTER(Result)]

    def event(self, state, tag, a=0, b=0, c=0, d=0, e=0, f=0, text=b"",
              slots=None, depth=32, nodes=1000000, strings=1000000, work=10000000,
              null_bytes=False):
        data = (C.c_uint8 * len(text))(*text)
        event = Event(tag, a, b, c, d, e, f,
                      None if null_bytes else data, len(text))
        result = Result(MASK)
        status = self.step(C.byref(state), C.byref(event), depth, nodes, strings,
                           work, slots, len(slots) if slots is not None else 0,
                           C.byref(result))
        assert status == result.status, (tag, status, result.status)
        return status

    def grow(self, state, old, capacity, work=10000000):
        output = (Slot * capacity)()
        result = Result(MASK)
        status = self.rehash(C.byref(state), old, len(old) if old is not None else 0,
                             output, capacity, work, C.byref(result))
        assert status == result.status
        return status, output


def numeric_oracle(tag, width, text):
    valid = width in ((1, 8, 16, 32, 64, 128) if tag in (4, 6) else (16, 32, 64))
    if tag == 6:
        digits = text[1:] if text.startswith(b"-") else text
        valid = valid and bool(digits) and all(48 <= byte <= 57 for byte in digits)
    if tag == 7:
        valid = (valid and len(text) == 2 + width // 4 and text.startswith(b"0x")
                 and all(byte in b"0123456789abcdefABCDEF" for byte in text[2:]))
    return valid


def numeric_cases(kernel, count):
    fixed = [b"", b"-", b"+1", b"1", b"-0", b"000", b"-999", b" 1", b"1_0",
             b"0x10", b"1\0", "１２".encode(), b"0xFFFF", b"0xffff", b"0XFFFF",
             b"0x00000000", b"0x0000000000000000", b"0xgggg", b"0x123"]
    rng = random.Random(0x335_a57)
    for index in range(count + len(fixed) * 4 * 12):
        if index < len(fixed) * 4 * 12:
            text = fixed[index % len(fixed)]
            tag = 4 + (index // len(fixed)) % 4
            width = (0, 1, 7, 8, 16, 24, 32, 64, 80, 128, 256, MASK)[index // (len(fixed) * 4)]
        else:
            tag = rng.randrange(4, 8)
            width = rng.choice((0, 1, 8, 16, 32, 64, 128, MASK))
            text = bytes(rng.choice(b"-+0123456789abcdefABCDEFxX_g \0") for _ in range(rng.randrange(30)))
        state = State(numbers=1)
        assert kernel.event(state, tag, width, text=text) == 0
        assert bool(state.numbers) == numeric_oracle(tag, width, text), (tag, width, text)
        assert state.strings == (len(text) if tag >= 6 else 0)
        assert state.nodes == state.sources == state.work == state.features == 0
    state = State(numbers=1)
    assert kernel.event(state, 6, 32, text=b"bad") == 0 and state.numbers == 0
    assert kernel.event(state, 6, 32, text=b"123") == 0 and state.numbers == 0
    for tag in (6, 7):
        state = State(numbers=1)
        assert kernel.event(state, tag, 32, text=b"12345", strings=4) == 4
        assert state.strings == 0
        assert kernel.event(State(numbers=1), tag, 32, text=b"1", null_bytes=True) == 8


def basic_cases(kernel):
    state = State(numbers=1)
    assert kernel.event(state, 0, nodes=2) == 0
    assert kernel.event(state, 1, 32, nodes=2) == 0
    assert kernel.event(state, 0, nodes=2) == 3 and state.nodes == 2
    for depth, actual in ((0, 1), (3, 4), (32, 33), (MASK, 33)):
        state = State(numbers=1)
        assert kernel.event(state, 1, actual, depth=depth) == 2
        assert state.nodes == 0
    for length in (0, 1, 17, 1000):
        state = State(numbers=1)
        assert kernel.event(state, 2, text=b"x" * length, strings=length) == 0
        assert state.strings == length
        assert kernel.event(state, 2, text=b"!", strings=length) == 4
        assert state.strings == length
    state = State(numbers=1)
    for form in (1, 2, 3):
        assert kernel.event(state, 3, form) == 0 and state.features == 4
    assert kernel.event(state, 3, 4) == 0 and state.features == 5
    for op in range(12):
        state = State(numbers=1)
        assert kernel.event(state, 3, 5, op) == 0
        assert state.features == (0 if op < 3 else 1)
    for form, op in ((0, 0), (6, 0), (5, 12), (5, MASK)):
        assert kernel.event(State(numbers=1), 3, form, op) == 8
    assert kernel.event(State(numbers=1), 9) == 8
    for name, value in (("numbers", 2), ("features", 8), ("nodes", 1000001),
                        ("strings", 1000001), ("work", 10000001)):
        state = State(numbers=1)
        setattr(state, name, value)
        assert kernel.event(state, 0) == 8, name


def insert_reference(table, identity):
    at = mix(identity) & (len(table) - 1)
    probes = 1
    while table[at] is not None and table[at] != identity:
        at = (at + 1) & (len(table) - 1)
        probes += 1
    duplicate = table[at] is not None
    table[at] = identity
    return probes, duplicate


def source_cases(kernel, count=60000):
    state = State(numbers=1)
    slots = None
    reference = []
    expected_work = 0
    ids = []
    for index in range(count):
        identity = ((index + 1) * 0x9e3779b97f4a7c15) & MASK
        ids.append(identity)
        needed = 2
        while needed < (index + 1) * 2:
            needed *= 2
        if needed > len(reference):
            old = reference
            reference = [None] * needed
            expected_work += needed
            for entry in old:
                if entry is not None:
                    probes, duplicate = insert_reference(reference, entry)
                    assert not duplicate
                    expected_work += probes
            status, slots = kernel.grow(state, slots, needed)
            assert status == 0
            assert state.work == expected_work
            assert [(slot.id if slot.occupied else None) for slot in slots] == reference
        assert kernel.event(state, 0) == 0
        probes, duplicate = insert_reference(reference, identity)
        assert not duplicate
        expected_work += probes
        assert kernel.event(state, 8, identity, slots=slots) == 0
        assert (state.sources, state.nodes, state.features, state.work) == (index + 1, index + 1, 2, expected_work)
    assert state.sources == count
    before = state.sources
    probes, duplicate = insert_reference(reference, ids[count // 2])
    assert duplicate
    expected_work += probes
    assert kernel.event(state, 8, ids[count // 2], slots=slots) == 1
    assert state.sources == before and state.work == expected_work
    assert [(slot.id if slot.occupied else None) for slot in slots] == reference

    # A growth and the following insert share one cumulative work account.
    state = State(numbers=1)
    status, slots = kernel.grow(state, None, 4, work=5)
    assert status == 0 and state.work == 4
    assert kernel.event(state, 0, work=5) == 0
    assert kernel.event(state, 8, 1, slots=slots, work=5) == 0
    assert state.sources == 1 and state.work == 5
    assert kernel.event(state, 0, work=5) == 0
    assert kernel.event(state, 8, 2, slots=slots, work=5) == 7
    assert state.sources == 1 and state.work == 5
    failed = State(numbers=1)
    assert kernel.grow(failed, None, 8, work=7)[0] == 7
    assert failed.work == 7

    # Filename bytes spend the same cumulative source budget as rehash/probes.
    for available in (5, 6):
        state = State(numbers=1, nodes=1)
        status, slots = kernel.grow(state, None, 2, work=available)
        assert status == 0 and state.work == 2
        status = kernel.event(state, 8, 1, 1, 1, 1, 1, 1, text=b"abc", slots=slots, work=available)
        assert status == (7 if available == 5 else 0)
        assert state.work == (3 if available == 5 else 6)
    state = State(numbers=1, nodes=1)
    slots = (Slot * 2)()
    assert kernel.event(state, 8, 1, 1, 1, 1, 1, 1, text=b"x\0y", slots=slots) == 1
    assert state.work == 4

    # Force multiple colliding identities to exercise exact bounded probing.
    bucket = mix(0) & 7
    collisions = [identity for identity in range(1000) if mix(identity) & 7 == bucket][:4]
    state = State(numbers=1)
    status, slots = kernel.grow(state, None, 8)
    assert status == 0
    for ordinal, identity in enumerate(collisions):
        assert kernel.event(state, 0) == 0
        before = state.work
        assert kernel.event(state, 8, identity, slots=slots) == 0
        assert state.work - before == ordinal + 1


def source_errors(kernel):
    spans = [(b"a.wave", 1, 1, 1, 1, 0), (b"a.wave", 1, 9, 2, 1, 0),
             (b"", 1, 1, 1, 1, 1), (b"x\0y", 1, 1, 1, 1, 1),
             (b"a", 0, 1, 1, 1, 1), (b"a", 1, 0, 1, 1, 1),
             (b"a", 1, 1, 0, 1, 1), (b"a", 1, 1, 1, 0, 1),
             (b"a", 2, 1, 1, 9, 1), (b"a", 1, 9, 1, 1, 1)]
    for text, line, column, end_line, end_column, status in spans:
        state = State(numbers=1, nodes=1)
        slots = (Slot * 2)()
        assert kernel.event(state, 8, MASK, 1, line, column, end_line, end_column,
                            text=text, slots=slots) == status
    for capacity in (0, 1, 3):
        state = State(numbers=1)
        assert kernel.event(state, 8, 1, slots=(Slot * capacity)()) == 8
        assert kernel.grow(state, None, capacity)[0] == 8
    state = State(numbers=1, nodes=1)
    assert kernel.event(state, 8, 1, 2, slots=(Slot * 4)()) == 8
    assert kernel.event(State(numbers=1), 8, 1, 1, 1, 1, 1, 1, text=b"a",
                        slots=(Slot * 4)(), null_bytes=True) == 8
    bad = (Slot * 2)(Slot(1, 2), Slot())
    assert kernel.grow(State(numbers=1), bad, 4)[0] == 8
    duplicate = (Slot * 2)(Slot(3, 1), Slot(3, 1))
    assert kernel.grow(State(numbers=1, sources=2), duplicate, 4)[0] == 8
    assert kernel.grow(State(numbers=1, sources=1), None, 4)[0] == 8
    assert kernel.grow(State(numbers=1), (Slot * 8)(), 4)[0] == 8
    # Capacity and null pointer validation must happen before dereferencing.
    result = Result()
    assert kernel.rehash(None, None, 0, None, 4, 100, C.byref(result)) == 8
    assert kernel.step(None, None, 32, 100, 100, 100, None, 0, C.byref(result)) == 8


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", required=True)
    parser.add_argument("--cases", type=int, default=10000)
    args = parser.parse_args()
    if args.cases < 0:
        parser.error("--cases must be nonnegative")
    kernel = Kernel(args.library)
    basic_cases(kernel)
    numeric_cases(kernel, args.cases)
    source_errors(kernel)
    source_cases(kernel)
    print(f"Wave AST admission: {args.cases} numeric mutations, widths/features/limits, "
          "source errors and 60000 independently rehashed source identities passed")


if __name__ == "__main__":
    main()
