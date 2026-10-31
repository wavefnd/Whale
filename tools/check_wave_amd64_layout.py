#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Independent scoped constant, sparse layout and relaxation packet regressions."""
import argparse
import ctypes as C
from pathlib import Path
import random
import re

U = C.c_uint64
NONE = (1 << 64) - 1
ROOT = Path(__file__).resolve().parents[1]
TYPES = {'u64': U, 'i64': C.c_int64, 'u8': C.c_uint8, 'bool': C.c_bool}
for filename in ['assembler/model.wave', 'assembler/ast_model.wave',
                 'assembler/amd64/encoder_model.wave']:
    for name, body in re.findall(r'struct (\w+)\s*\{([^}]+)\}', (ROOT / filename).read_text()):
        if name in TYPES:
            continue
        fields = []
        for field, kind in re.findall(r'(\w+):\s*([^;]+);', body):
            pointer = re.fullmatch(r'ptr<(.+)>', kind)
            fields.append((field, C.POINTER(TYPES[pointer[1]]) if pointer else TYPES[kind]))
        TYPES[name] = type(name, (C.Structure,), {'_fields_': fields})
Name, Expr, Item, Value, Operand, Input, Result = [TYPES[name] for name in
    ['AsmName', 'AsmExpr', 'AsmItem', 'AsmDirectiveValue', 'AsmOperand',
     'AmdEncodeInput', 'AmdEncodeResult']]


class Packet:
    def __init__(self):
        self.names, self.items, self.values, self.operands, self.keep = [], [], [], [], []

    def name(self, text):
        encoded = text.encode()
        data = (C.c_uint8 * len(encoded)).from_buffer_copy(encoded)
        self.keep.append(data)
        self.names.append(Name(data, len(encoded)))
        return len(self.names) - 1

    def number(self, value):
        return Expr(kind=0, low=value & NONE, high=(value >> 64) & NONE)

    def symbol(self, name, addend=0):
        return Expr(kind=1, name=self.name(name), low=addend & NONE, high=(addend >> 64) & NONE)

    def item(self, kind, name, **kwargs):
        self.items.append(Item(kind=kind, name=self.name(name), **kwargs))

    def const(self, name, value):
        self.item(8, name, expression=value)

    def directive(self, name, values):
        first = len(self.values)
        self.values.extend(Value(kind=0, expression=value) for value in values)
        self.item(2, name, first=first, count=len(values))

    def instruction(self, name, label=None):
        first = len(self.operands)
        if label is not None:
            self.operands.append(Operand(kind=2, name=self.name(label)))
        self.item(0, name, first=first, count=len(self.operands)-first)

    def run(self, kernel, limit=1 << 20, iterations=1024):
        capacity = len(self.names) + len(self.items) + len(self.operands) + len(self.values) + 1
        table_capacity = 1 << (capacity * 2 - 1).bit_length()
        p = Input(max_output=limit, max_iterations=iterations, max_work=10000000)
        arrays = {}
        for field, kind, values in [
            ('names', Name, self.names), ('items', Item, self.items),
            ('operands', Operand, self.operands), ('values', Value, self.values),
            ('sections', TYPES['AmdSection'], [TYPES['AmdSection']() for _ in range(len(self.items)+1)]),
            ('symbols', TYPES['AmdSymbol'], [TYPES['AmdSymbol']() for _ in range(capacity)]),
            ('table', U, [0] * table_capacity), ('nodes', TYPES['AmdNode'],
                [TYPES['AmdNode']() for _ in self.items]),
            ('queue', U, [0] * capacity), ('relocations', TYPES['AmdRelocation'],
                [TYPES['AmdRelocation']() for _ in range(len(self.items)+len(self.values)+1)]),
        ]:
            array = (kind * len(values))(*values)
            arrays[field] = array
            setattr(p, field, array)
        p.name_count, p.item_count = len(self.names), len(self.items)
        p.operand_count, p.value_count = len(self.operands), len(self.values)
        p.section_capacity, p.symbol_capacity = len(arrays['sections']), capacity
        p.table_capacity, p.relocation_capacity = table_capacity, len(arrays['relocations'])
        result = Result()
        code = kernel(C.byref(p), 0, C.byref(result))
        assert code == result.status and result.work <= p.max_work
        if code:
            return result, arrays, b''
        before = result.bytes
        data = (C.c_uint8 * before)()
        p.bytes, p.byte_capacity = data, before
        code = kernel(C.byref(p), 1, C.byref(result))
        assert code == result.status == 0, (result.error, result.node, result.argument)
        assert result.bytes == before and result.work <= p.max_work
        return result, arrays, bytes(data)


def fixed(kernel):
    p = Packet()
    p.item(4, '.text')
    p.instruction('nop')
    r, a, data = p.run(kernel)
    assert r.section_count == 1 and data == b'\x90'
    p = Packet()
    p.item(3, 'main')
    p.item(3, '.again')
    p.instruction('loop', '.again')
    r, a, data = p.run(kernel)
    assert data == b'\xe2\xfe' and r.relocation_count == 0
    p = Packet()
    p.instruction('jmp', 'end')
    p.directive('resb', [p.number(127)])
    p.item(3, 'end')
    p.instruction('ret')
    r, a, data = p.run(kernel)
    assert data == b'\xeb\x7f' + b'\0' * 127 + b'\xc3', data[:8]
    p = Packet()
    p.item(4, '.bss')
    p.directive('resb', [p.number(1 << 40)])
    p.directive('align', [p.number(1 << 41)])
    p.item(3, 'end')
    r, a, data = p.run(kernel, limit=0)
    assert data == b'' and a['sections'][1].zero_fill == 1 << 41
    for make, error in [
        (lambda p: (p.const('A', p.number(1)), p.const('A', p.number(2))), 101),
        (lambda p: (p.const('A', p.symbol('B')), p.const('B', p.symbol('A'))), 105),
        (lambda p: (p.const('A', p.number(1)), p.item(3, 'A')), 102),
        (lambda p: p.item(3, '.orphan'), 100),
        (lambda p: p.directive('db', [p.number(256)]), 167),
        (lambda p: p.item(6, 'missing'), 113),
    ]:
        p = Packet()
        make(p)
        r, _, _ = p.run(kernel)
        assert r.status and r.error == error, (error, r.status, r.error)
    p = Packet()
    p.item(7, 'external')
    p.item(7, 'external')
    r, a, _ = p.run(kernel)
    assert r.output_symbols == 1
    output = [s for s in a['symbols'][:r.symbol_count] if s.flags & 6]
    assert len(output) == 1 and output[0].flags & 1 and output[0].order == 0
    # These scale independently of native call-stack depth and exercise worklists.
    p = Packet()
    p.const('C0', p.number(1))
    for index in range(1, 5000):
        p.const(f'C{index}', p.symbol(f'C{index-1}', 1))
    p.directive('dq', [p.symbol('C4999')])
    r, _, data = p.run(kernel)
    assert data == (5000).to_bytes(8, 'little')
    p = Packet()
    for _ in range(65000):
        p.instruction('nop')
    r, _, data = p.run(kernel)
    assert data == b'\x90' * 65000


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--library', type=Path, required=True)
    parser.add_argument('--cases', type=int, default=2000)
    args = parser.parse_args()
    kernel = C.CDLL(str(args.library.resolve())).whale_amd64_encode
    kernel.argtypes = [C.POINTER(Input), U, C.POINTER(Result)]
    fixed(kernel)
    rng = random.Random(0x5748414C454C4159)
    for _ in range(args.cases):
        p = Packet()
        expected = []
        parents = rng.randrange(1, 20)
        for n in range(parents):
            p.item(3, f'parent{n}')
            p.const('.A', p.number(rng.randrange(128)))
            p.const('.B', p.symbol('.A', 1))
            p.directive('db', [p.symbol('.B')])
            expected.append(p.items[-3].expression.low + 1)
            p.item(3, '.end')
        r, a, data = p.run(kernel)
        assert data == bytes(expected)
        assert r.output_symbols == parents * 2
        assert [s.order for s in a['symbols'][:r.symbol_count] if s.flags & 6] == list(range(parents*2))
    print(f'Wave AMD64 layout: {args.cases} independent scoped constant/section packets + branch relaxation, LOOP, sparse BSS, identity and failure regressions passed')


if __name__ == '__main__':
    main()
