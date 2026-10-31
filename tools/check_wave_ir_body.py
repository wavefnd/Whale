#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Native complete body inventory, hoisting and transactional mutation oracle."""

import argparse
import ctypes as C
import random

NONE = (1 << 64) - 1


class Atom(C.Structure):
    _fields_ = [(name, C.c_uint64) for name in ("kind", "a", "b", "c")]


class Record(C.Structure):
    _fields_ = [(name, C.c_uint64) for name in
               ("op", "block", "first", "count", "byte_first", "byte_count", "next")]


class GlobalRecord(C.Structure):
    _fields_ = [(name, C.c_uint64) for name in ("id", "first", "count", "byte_first", "byte_count")]


class Block(C.Structure):
    _fields_ = [(name, C.c_uint64) for name in ("head", "tail", "count", "terminator", "prefix_tail", "prefix_count")]


class State(C.Structure):
    _fields_ = [(name, C.c_uint64) for name in ("atoms", "bytes", "records", "globals", "work")]


class Result(C.Structure):
    _fields_ = [(name, C.c_uint64) for name in ("status", "detail", "ordinal", "work")]


def uint(value=1):
    return (0, value, 0, 0)


def ty(kind=6, children=0, auxiliary=0):
    return (3, kind, children, auxiliary)


def const(value=7):
    return (1, 1, value & NONE, (value >> 64) & NONE)


def fixture(op):
    u, t = uint(), ty()
    literal = const()
    text = "λ🐋\0 reason".encode()
    string = (2, 0, len(text), 0)
    expression = [(4, 5, 0, 2), t, (4, 0, 0, 0), t, const(3), (4, 0, 0, 0), t, const(4)]
    signature = [ty(18, 2, 1), ty(0), ty(17, 1), ty(0)]
    samples = {
        0: [u, u, t], 1: [u], 2: [u, u, u], 3: [u, string, *expression, literal],
        4: [u, t, literal], 5: [u, t], 6: [u, t, u],
        7: [u, uint(17), t, u, u], 8: [u, t, u], 9: [u, t, u],
        10: [u, uint(15), t, u, u], 11: [u, uint(9), t, u, u],
        12: [u, uint(13), ty(14), u, u], 13: [u, t, u, u, u],
        14: [u, uint(12), ty(8), ty(13), u],
        15: [u, t, (5, 1, 2, 0), u, uint(0), u, uint(4)],
        16: [u, t, u, uint((1 << 32) - 1)], 17: [u, uint(5), t, u, u],
        18: [u, ty(20, 2), ty(19, 1, NONE), ty(9), *signature, uint(64)],
        19: [u, t, u, uint(16)], 20: [t, u, u, uint(16)],
        21: [u, t, u, uint(16)], 22: [t, u, uint(16)], 23: [t, u, u, uint(16)],
        24: [u, ty(17, 1), t, u, (5, 0, 2, 0), uint(0), uint(5)],
        25: [u, u, u, uint(8)], 26: [u, u, u, uint(8)],
        27: [u, *signature], 28: [u, u, *signature],
        29: [uint(1), u, t, uint(1), uint(1), u, (5, 0, 2, 0), u, uint(7)],
        30: [u, string], 31: [uint(4)], 32: [u, uint(0), uint(4)],
        33: [ty(13), u, uint(0), (5, 2, 2, 0), (1, 2, NONE, NONE), uint(4), (1, 2, 0, 0), uint(2)],
        34: [t, uint(1), u], 35: [string],
    }
    return samples[op], text if op in (3, 30, 35) else b""


class Store:
    def __init__(self, kernel, atom_capacity=2048, byte_capacity=4096, record_capacity=128):
        self.kernel = kernel
        self.state = State()
        self.blocks = (Block * 5)(*(Block(NONE, NONE, 0, NONE, NONE, 0) for _ in range(5)))
        self.atoms = (Atom * atom_capacity)()
        self.bytes = (C.c_uint8 * byte_capacity)()
        self.records = (Record * record_capacity)()
        self.globals = (GlobalRecord * record_capacity)()

    def snapshot(self):
        return tuple(bytes(value) for value in
                     (self.state, self.blocks, self.atoms, self.bytes, self.records, self.globals))

    def append(self, op, atoms, text=b"", block=0, hoist=0, mode=1, work=1_000_000,
               nodes=100000, byte_limit=100000, capacity=None):
        atoms = (Atom * len(atoms))(*(Atom(*atom) for atom in atoms))
        text = (C.c_uint8 * len(text))(*text)
        result = Result()
        before = self.snapshot()
        status = self.kernel(C.byref(self.state), self.blocks, len(self.blocks), block,
                             op, hoist, atoms, len(atoms), text, len(text), 257, 256,
                             nodes, byte_limit, work, mode,
                             self.atoms, len(self.atoms) if capacity is None else capacity,
                             self.bytes, len(self.bytes), self.records, len(self.records),
                             C.byref(result))
        assert status == result.status
        if status or mode == 0:
            assert self.snapshot() == before, (op, status, "partial publication")
        else:
            old = State.from_buffer_copy(before[0])
            assert self.state.atoms == old.atoms + len(atoms)
            assert self.state.bytes == old.bytes + len(text)
            assert self.state.records == old.records + 1
            assert self.state.work == old.work + result.work <= work
            for index, atom in enumerate(atoms):
                expected = (atom.kind, atom.a + old.bytes if atom.kind == 2 else atom.a, atom.b, atom.c)
                actual = self.atoms[old.atoms + index]
                assert tuple(getattr(actual, name) for name, _ in Atom._fields_) == expected
            assert bytes(self.bytes[old.bytes:self.state.bytes]) == bytes(text)
            record = self.records[old.records]
            assert (record.op, record.block, record.first, record.count,
                    record.byte_first, record.byte_count) == (op, block, old.atoms, len(atoms), old.bytes, len(text))
        return status, result


def global_cases(library, count):
    kernel = library.whale_ir_builder_global
    kernel.restype = C.c_int32
    kernel.argtypes = [C.POINTER(State), C.c_uint64, C.POINTER(GlobalRecord), C.c_uint64,
                       C.POINTER(Atom), C.c_uint64, C.POINTER(C.c_uint8), C.c_uint64,
                       *([C.c_uint64] * 6), C.POINTER(Atom), C.c_uint64,
                       C.POINTER(C.c_uint8), C.c_uint64, C.POINTER(Result)]
    def append(store, identity, payload, text, mode=1, nodes=100000, byte_limit=100000,
               work=1000000, capacity=None):
        atoms = (Atom * len(payload))(*(Atom(*atom) for atom in payload))
        data = (C.c_uint8 * len(text))(*text)
        result = Result()
        before = store.snapshot()
        status = kernel(C.byref(store.state), identity, store.globals,
                        len(store.globals) if capacity is None else capacity,
                        atoms, len(atoms), data, len(data), 257, 256, nodes, byte_limit, work,
                        mode, store.atoms, len(store.atoms), store.bytes, len(store.bytes), C.byref(result))
        assert status == result.status
        if status or mode == 0:
            assert store.snapshot() == before, (status, "partial global publication")
        else:
            old = State.from_buffer_copy(before[0])
            assert (store.state.atoms, store.state.bytes, store.state.records, store.state.globals,
                    store.state.work) == (old.atoms + len(atoms), old.bytes + len(data), old.records,
                                          old.globals + 1, old.work + result.work)
            record = store.globals[old.globals]
            assert (record.id, record.first, record.count, record.byte_first, record.byte_count) == (
                identity, old.atoms, len(atoms), old.bytes, len(data))
            for index, atom in enumerate(atoms):
                expected = (atom.kind, atom.a + old.bytes if atom.kind == 2 else atom.a, atom.b, atom.c)
                actual = store.atoms[old.atoms + index]
                assert tuple(getattr(actual, name) for name, _ in Atom._fields_) == expected
            assert bytes(store.bytes[old.bytes:store.state.bytes]) == text
        return status
    payload, text = fixture(3)
    payload = [*payload[1:], uint(32)]
    store = Store(library.whale_ir_builder_body)
    assert append(store, 0, payload, text, mode=0) == 0
    assert append(store, 0, payload, text) == 0
    assert append(store, 2, payload, text) == 0
    assert append(store, 2, payload, text) == 1
    assert append(store, 1, payload, text) == 1
    assert append(store, NONE, payload, text) == 1
    assert append(Store(None), 0, payload, text, work=0) == 4
    assert append(Store(None), 0, payload, text, nodes=0) == 3
    assert append(Store(None), 0, payload, text, byte_limit=0) == 10
    assert append(Store(None), 0, payload, text, capacity=0) == 1
    for end in range(len(payload)):
        assert append(Store(None), 0, payload[:end], text) != 0
    rng = random.Random(0x348_610b)
    for _ in range(count):
        mutation = [list(atom) for atom in payload]
        mutation[rng.randrange(len(mutation))][rng.randrange(4)] ^= 1 << rng.randrange(64)
        append(Store(None, atom_capacity=128, byte_capacity=64, record_capacity=4),
               0, mutation, text, work=10000)


def large_hoists(kernel, count=60000):
    # No repeated array snapshots: this exercises the actual persistent body
    # links with a work ceiling that excludes any quadratic prefix rescanning.
    state = State()
    blocks = (Block * 1)(Block(NONE, NONE, 0, NONE, NONE, 0))
    output = (Atom * (count * 3 + 3))()
    records = (Record * (count + 2))()
    result = Result()
    def append(op, payload, hoist=0):
        atoms = (Atom * len(payload))(*(Atom(*a) for a in payload))
        status = kernel(C.byref(state), blocks, 1, 0, op, hoist, atoms, len(atoms),
                        None, 0, 257, 256, count * 4, 0, count * 16 + 64, 1,
                        output, len(output), None, 0, records, len(records), C.byref(result))
        assert status == result.status == 0, (op, status, state.work)
        return result.ordinal, result.work
    append(5, [uint(), ty()])
    append(31, [uint(0)])
    cost = None
    for index in range(count):
        ordinal, work = append(18, [uint(index), ty(), uint(4)], hoist=1)
        assert ordinal == index
        if cost is None:
            cost = work
        assert cost == work
    assert (blocks[0].prefix_count, blocks[0].count, blocks[0].prefix_tail,
            blocks[0].head, blocks[0].tail, blocks[0].terminator) == (count, count + 1, count + 1, 2, 0, 1)
    at = blocks[0].head
    for index in range(count):
        assert at == index + 2 and records[at].op == 18
        at = records[at].next
    assert at == 0 and records[0].next == NONE
    assert state.work <= count * 16 + 64


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", required=True)
    parser.add_argument("--cases", type=int, default=10000)
    args = parser.parse_args()
    assert args.cases >= 0
    library = C.CDLL(args.library)
    kernel = library.whale_ir_builder_body
    kernel.restype = C.c_int32
    kernel.argtypes = [C.POINTER(State), C.POINTER(Block), C.c_uint64, C.c_uint64,
                       C.c_uint64, C.c_uint64, C.POINTER(Atom), C.c_uint64,
                       C.POINTER(C.c_uint8), C.c_uint64, C.c_uint64, C.c_uint64,
                       C.c_uint64, C.c_uint64, C.c_uint64, C.c_uint64,
                       C.POINTER(Atom), C.c_uint64, C.POINTER(C.c_uint8), C.c_uint64,
                       C.POINTER(Record), C.c_uint64, C.POINTER(Result)]
    large_hoists(kernel)
    global_cases(library, args.cases)
    store = Store(kernel)
    for op in range(31):
        atoms, text = fixture(op)
        assert store.append(op, atoms, text)[0] == 0, op
    for block, op in enumerate(range(31, 36)):
        atoms, text = fixture(op)
        assert store.append(op, atoms, text, block=block)[0] == 0, op
    atoms, text = fixture(18)
    status, result = store.append(18, atoms, text, hoist=1)
    assert status == 0 and result.ordinal == 0
    chain, index = [], store.blocks[0].head
    while index != NONE:
        chain.append(store.records[index].op)
        index = store.records[index].next
        assert len(chain) <= 32
    assert chain == [18, *range(31)]
    assert store.records[store.blocks[0].terminator].op == 31

    for op in range(36):
        atoms, text = fixture(op)
        assert Store(kernel).append(op, atoms, text, mode=0)[0] == 0
        assert Store(kernel).append(op, atoms, text, work=0)[0] == 4
        assert Store(kernel).append(op, atoms, text, nodes=0)[0] == 3
        assert Store(kernel).append(op, atoms, text, capacity=0)[0] == 1
        for end in range(len(atoms)):
            assert Store(kernel).append(op, atoms[:end], text)[0] != 0
    atoms, _ = fixture(35)
    assert Store(kernel).append(35, atoms, b"\xff" * atoms[0][2])[0] == 1
    assert Store(kernel).append(35, atoms, b"x" * atoms[0][2], byte_limit=0)[0] == 10
    randomizer = random.Random(0x348_b0d1)
    for _ in range(args.cases):
        op = randomizer.randrange(36)
        atoms, text = fixture(op)
        atoms = [list(atom) for atom in atoms]
        selected = randomizer.randrange(len(atoms))
        field = randomizer.randrange(4)
        atoms[selected][field] ^= 1 << randomizer.randrange(64)
        Store(kernel, atom_capacity=128, byte_capacity=64, record_capacity=4).append(op, atoms, text, work=10000)
    # Normal appended allocas extend only the initial allocation prefix.
    mixed = Store(kernel)
    alloca = [uint(), ty(), uint(4)]
    assert mixed.append(18, alloca)[0] == 0
    assert mixed.append(18, alloca, hoist=1)[0] == 0
    assert mixed.append(5, [uint(), ty()])[0] == 0
    assert mixed.append(18, alloca)[0] == 0
    status, result = mixed.append(18, alloca, hoist=1)
    assert status == 0 and result.ordinal == 2 and mixed.blocks[0].prefix_count == 3
    print(f"Wave IR body: complete 36-form inventory, 60000 bounded hoists, global constexpr records, atomic limits and {args.cases} body/global mutations passed")


if __name__ == "__main__":
    main()
