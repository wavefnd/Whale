#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Independent byte-cell/capability conformance for the Wave memory kernel."""
import argparse
import ctypes as C
import random

U = C.c_uint64
MASK = (1 << 64) - 1


def record(name, fields):
    return type(name, (C.Structure,), {'_fields_': [(key, U) for key in fields.split()]})


Pointer = record('Pointer', 'address present arena allocation generation lower upper offset read write')
class Authority(C.Structure):
    _fields_ = [('kind', U), ('pointer', Pointer), ('signature', U), ('target_kind', U), ('target', U)]
class Fragment(C.Structure):
    _fields_ = [('occupied', U), ('offset', U), ('ordinal', U), ('authority', Authority)]
class Allocation(C.Structure):
    _fields_ = [(n, U) for n in 'kind address generation alive read write length'.split()] + [('data', C.POINTER(C.c_uint8)), ('initialized', C.POINTER(C.c_uint8)), ('fragments', C.POINTER(Fragment)), ('fragment_capacity', U), ('fragment_count', U)]
State = record('State', 'arena allocations bytes fragments work next_address')
Limits = record('Limits', 'bytes allocations fragments work')
class Part(C.Structure):
    _fields_ = [(n, U) for n in 'offset kind low high'.split()] + [('authority', Authority)]
Gep = record('Gep', 'kind amount low high signed')
class Request(C.Structure):
    _fields_ = [(n, U) for n in 'op mode size natural align flag owner'.split()] + [('pointer', Pointer), ('source', Pointer), ('parts', C.POINTER(Part)), ('part_count', U), ('geps', C.POINTER(Gep)), ('gep_count', U)]
class Result(C.Structure):
    _fields_ = [(n, U) for n in 'status detail extra work allocation start end fragments fragment_capacity low high'.split()] + [('pointer', Pointer)]


def clone(value):
    return type(value).from_buffer_copy(bytes(value))


class Memory:
    def __init__(self, kernel, byte_limit=1000000, allocation_limit=1000, fragments=10000, work=10000000):
        self.kernel = kernel
        self.state = State(arena=17, next_address=4096)
        self.limits = Limits(byte_limit, allocation_limit, fragments, work)
        self.allocations = (Allocation * allocation_limit)()
        self.buffers = []
        self.fragment_tables = {}

    def call(self, request, table=None):
        output = (Part * 2)()
        result = Result()
        status = self.kernel(C.byref(self.state), C.byref(self.limits), self.allocations, len(self.allocations), C.byref(request), table, len(table) if table is not None else 0, output, 2, C.byref(result))
        assert status == result.status, (status, result.status)
        assert self.state.work <= self.limits.work and result.work <= self.limits.work
        return status, result, output

    def allocate(self, size, global_=False, align=1):
        q = Request(op=0, size=size, natural=align, align=align, flag=int(global_))
        status, probe, _ = self.call(q)
        if status:
            return status, None
        data = (C.c_uint8 * size)()
        initialized = (C.c_uint8 * size)()
        index = self.state.allocations
        self.allocations[index].data = data
        self.allocations[index].initialized = initialized
        q.mode = 1
        status, result, _ = self.call(q)
        assert status == 0 and result.pointer.address == probe.pointer.address
        self.buffers.append((data, initialized))
        return status, clone(result.pointer)

    def mutate(self, q):
        before = [(bytes(data), bytes(init)) for data, init in self.buffers]
        status, probe, _ = self.call(q)
        if status:
            assert before == [(bytes(data), bytes(init)) for data, init in self.buffers]
            return status
        table = (Fragment * probe.fragment_capacity)()
        q.mode = 1
        status, result, _ = self.call(q, table)
        if status:
            assert before == [(bytes(data), bytes(init)) for data, init in self.buffers]
        elif result.allocation != MASK:
            assert result.fragment_capacity == len(table) == probe.fragment_capacity
            self.fragment_tables[result.allocation] = table
        return status

    def store(self, pointer, kind, low, high=0, authority=None):
        part = Part(kind=kind, low=low, high=high, authority=authority or Authority())
        parts = (Part * 1)(part)
        size = {1: 1, 2: 1, 3: 1, 4: 1, 5: 2, 6: 4, 7: 8, 8: 16, 9: 1, 10: 2, 11: 4, 12: 8, 13: 16, 14: 2, 15: 4, 16: 8, 17: 8, 18: 8}[kind]
        return self.mutate(Request(op=4, pointer=pointer, size=size, align=1, parts=parts, part_count=1))

    def load(self, pointer, kind, signature=0):
        parts = (Part * 1)(Part(kind=kind, authority=Authority(signature=signature)))
        size = {1: 1, 2: 1, 3: 1, 4: 1, 5: 2, 6: 4, 7: 8, 8: 16, 9: 1, 10: 2, 11: 4, 12: 8, 13: 16, 14: 2, 15: 4, 16: 8, 17: 8, 18: 8}[kind]
        status, result, out = self.call(Request(op=3, mode=1, pointer=pointer, size=size, align=1, parts=parts, part_count=1))
        return status, result, clone(out[0])


def offset(pointer, amount, *, read=True, write=True, lower=0, upper=None):
    p = clone(pointer)
    p.address += amount
    p.offset += amount
    p.read, p.write = int(read), int(write)
    p.lower = lower
    if upper is not None:
        p.upper = upper
    return p


def fixed(kernel):
    m = Memory(kernel)
    _, object_ = m.allocate(8, align=8)
    _, src = m.allocate(32, align=8)
    _, dst = m.allocate(32, align=8)
    assert m.load(src, 12)[0] == 8
    authority = Authority(kind=1, pointer=object_)
    assert m.store(src, 17, object_.address, authority=authority) == 0
    assert m.mutate(Request(op=6, pointer=dst, source=src, size=4, align=1)) == 0
    assert m.load(dst, 17)[0] == 8
    assert m.mutate(Request(op=6, pointer=offset(dst, 4), source=offset(src, 4), size=4, align=1)) == 0
    status, _, part = m.load(dst, 17)
    assert status == 0 and bytes(part.authority.pointer) == bytes(object_)
    assert m.store(offset(dst, 7), 9, 0) == 0
    assert m.load(dst, 17)[2].authority.kind == 0
    for target in (0, 7, 2**32 - 1):
        bits = target + 1
        authority = Authority(kind=2, signature=9, target_kind=1, target=target)
        assert m.store(src, 18, bits, authority=authority) == 0
        assert m.load(src, 18, signature=9)[2].authority.target == target
        assert m.load(src, 18, signature=10)[2].authority.kind == 0
        assert m.load(src, 17)[2].authority.kind == 0
    assert m.mutate(Request(op=6, pointer=offset(src, 1), source=src, size=8, align=1)) == 11
    assert m.mutate(Request(op=7, pointer=Pointer(), size=0, align=8)) == 0
    assert m.mutate(Request(op=6, pointer=Pointer(address=999), source=Pointer(), size=0, align=8)) == 0
    assert m.mutate(Request(op=7, pointer=offset(dst, 0, write=False), size=1, align=1)) == 6
    assert m.mutate(Request(op=7, pointer=offset(dst, 31), size=2, align=1)) == 4
    assert m.mutate(Request(op=7, pointer=offset(dst, 1), size=1, align=8)) == 7
    assert m.mutate(Request(op=7, pointer=dst, flag=2, size=1, align=1)) == 0
    assert m.load(dst, 1)[0] == 9
    q = Request(op=2, mode=1, pointer=object_, owner=0)
    assert m.call(q)[0] == 0
    assert m.load(object_, 9)[0] == 3
    # Source bounds precede an expired destination in memcpy diagnostics.
    assert m.mutate(Request(op=6, pointer=object_, source=offset(src, 33), size=1, align=1)) == 4
    assert m.call(Request(op=2, mode=1, pointer=object_, owner=0))[0] == 3
    _, glob = m.allocate(1, global_=True)
    assert m.mutate(Request(op=7, pointer=glob, flag=77, size=1, align=1)) == 0
    status, seal, _ = m.call(Request(op=1, mode=1, pointer=glob, flag=1))
    assert status == 0
    glob = clone(seal.pointer)
    assert m.mutate(Request(op=7, pointer=glob, flag=0, size=1, align=1)) == 6
    assert m.call(Request(op=2, mode=1, pointer=glob, owner=0))[0] == 13
    assert m.call(Request(op=11, mode=1, owner=0))[0] == 0
    assert m.load(glob, 9)[2].low == 77
    assert m.load(src, 9)[0] == 3


def generated(kernel, cases):
    rng = random.Random(0x354_128)
    types = [(2, 1, True), (3, 1, False), (4, 8, True), (9, 8, False), (5, 16, True), (10, 16, False), (6, 32, True), (11, 32, False), (7, 64, True), (12, 64, False), (8, 128, True), (13, 128, False), (14, 16, False), (15, 32, False), (16, 64, False)]
    m = Memory(kernel, work=100000000)
    _, src = m.allocate(32)
    _, dst = m.allocate(32)
    for _ in range(cases):
        kind, width, signed = rng.choice(types)
        bits = rng.getrandbits(width)
        expected = bits - (1 << width) if signed and bits >> (width - 1) else bits
        raw = expected % (1 << 128)
        assert m.store(src, kind, raw & MASK, raw >> 64) == 0
        size = max(1, width // 8)
        assert m.mutate(Request(op=6, pointer=dst, source=src, size=size, align=1)) == 0
        status, _, output = m.load(dst, kind)
        assert status == 0 and output.low | (output.high << 64) == raw
        # Scalar stores erase authority even when bytes happen to match.
        assert output.authority.kind == 0
    # A separate byte-cell model computes initialization/copy/overwrite independently.
    m = Memory(kernel, work=100000000)
    _, p = m.allocate(24)
    _, q = m.allocate(24)
    bases = [p, q]
    data = [bytearray(24), bytearray(24)]
    init = [bytearray(24), bytearray(24)]
    for _ in range(cases):
        code, a, b = rng.randrange(4), rng.randrange(2), rng.randrange(2)
        at, source, n = rng.randrange(25), rng.randrange(25), rng.randrange(25)
        valid = at + n <= 24 and (code != 2 or source + n <= 24)
        overlap = code == 2 and a == b and at < source + n and source < at + n
        if n == 0 and code in (0, 2):
            valid, overlap = True, False
        expected = 0 if valid and not overlap else 11 if valid else 4
        if code == 0:
            octet = rng.randrange(256)
            result = m.mutate(Request(op=7, pointer=offset(bases[a], at), size=n, flag=octet, align=1))
            if expected == 0:
                data[a][at:at+n] = bytes([octet]) * n
                init[a][at:at+n] = bytes([1]) * n
        elif code == 1:
            result = m.mutate(Request(op=5, pointer=offset(bases[a], at), size=n, align=1))
            if expected == 0:
                init[a][at:at+n] = bytes(n)
        elif code == 2:
            result = m.mutate(Request(op=6, pointer=offset(bases[a], at), source=offset(bases[b], source), size=n, align=1))
            if expected == 0:
                data[a][at:at+n] = data[b][source:source+n]
                init[a][at:at+n] = init[b][source:source+n]
        else:
            status, _, value = m.load(offset(bases[a], at), 9)
            expected = 4 if at == 24 else 8 if not init[a][at] else 0
            assert status == expected
            if not status:
                assert value.low == data[a][at]
            continue
        assert result == expected, (code, a, at, b, source, n, result, expected)
        for index in range(2):
            assert bytes(m.buffers[index][0]) == data[index]
            assert bytes(m.buffers[index][1]) == init[index]


def limits(kernel):
    for count in (0, 7, 8, 15, 16):
        m = Memory(kernel, byte_limit=17, allocation_limit=3, fragments=count)
        _, obj = m.allocate(1)
        _, src = m.allocate(8)
        _, dst = m.allocate(8)
        assert m.allocate(1)[0] == 102
        assert m.mutate(Request(op=7, pointer=dst, size=8, flag=255, align=1)) == 0
        authority = Authority(kind=1, pointer=obj)
        result = m.store(src, 17, obj.address, authority=authority)
        if count < 8:
            assert result == 103
        else:
            assert result == 0
            result = m.mutate(Request(op=6, pointer=dst, source=src, size=8, align=1))
            assert result == (103 if count < 16 else 0)
            if count < 16:
                assert m.load(dst, 12)[2].low == MASK
        before = bytes(m.buffers[2][0])
        m.limits.work = m.state.work
        assert m.mutate(Request(op=7, pointer=dst, size=8, align=1)) == 104
        assert bytes(m.buffers[2][0]) == before
    m = Memory(kernel)
    _, p = m.allocate(8)
    for n in (0, 8, -1, 9, -(1 << 127), (1 << 128)-1):
        signed = int(n < 0)
        raw = n % (1 << 128)
        g = (Gep * 1)(Gep(kind=0, amount=1, low=raw & MASK, high=raw >> 64, signed=signed))
        status, result, _ = m.call(Request(op=8, pointer=p, geps=g, gep_count=1))
        expected = 0 if n in (0, 8) else 4 if n in (-1, 9) else 5
        assert status == expected, (n, status)
        if status == 0:
            assert result.pointer.address == p.address + n
    assert m.call(Request(op=200))[0] == 200
    assert m.call(Request(op=4, pointer=p, size=8, align=1, part_count=1))[0] == 200


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--library', required=True)
    parser.add_argument('--cases', type=int, default=10000)
    args = parser.parse_args()
    kernel = C.CDLL(args.library).whale_ir_memory
    kernel.argtypes = [C.POINTER(State), C.POINTER(Limits), C.POINTER(Allocation), U, C.POINTER(Request), C.POINTER(Fragment), U, C.POINTER(Part), U, C.POINTER(Result)]
    assert C.sizeof(Pointer) == 80 and C.sizeof(Authority) == 112 and C.sizeof(Fragment) == 136
    fixed(kernel)
    generated(kernel, args.cases)
    limits(kernel)
    print(f'Wave memory: {args.cases} integer/IEEE storage vectors + {args.cases} independent byte streams, authority/lifetime/global/resource regressions passed')


if __name__ == '__main__':
    main()
