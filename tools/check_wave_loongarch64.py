#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Independent LLVM fixtures and mathematical LoongArch64 integer encodings."""
import argparse
import ctypes as C
from pathlib import Path
import random
import re

ROOT = Path(__file__).resolve().parents[1]
U = C.c_uint64
NONE = (1 << 64) - 1
TYPES = {'u64': U, 'i64': C.c_int64, 'u8': C.c_uint8, 'bool': C.c_bool}
for name, body in re.findall(r'struct (\w+)\s*\{([^}]+)\}',
                             (ROOT / 'assembler/fixed_model.wave').read_text()):
    fields = []
    for field, kind in re.findall(r'(\w+):\s*([^;]+);', body):
        pointer = re.fullmatch(r'ptr<(.+)>', kind)
        fields.append((field, C.POINTER(TYPES[pointer[1]]) if pointer else TYPES[kind]))
    TYPES[name] = type(name, (C.Structure,), {'_fields_': fields})
Text, Instruction, Context, Result = [TYPES[n] for n in
    ['FixedText', 'FixedInstruction', 'FixedContext', 'FixedResult']]


def text(value):
    data = value.encode()
    buffer = (C.c_uint8 * len(data)).from_buffer_copy(data)
    return Text(buffer, len(data)), buffer


def execute(kernel, source, fold=0, max_work=10000000):
    mnemonic, _, tail = source.partition(' ')
    operands = [operand.strip() for operand in tail.split(',')] if tail.strip() else []
    name, keep = text(mnemonic)
    values = [text(operand) for operand in operands]
    array = (Text * len(values))(*(value for value, _ in values))
    instruction = Instruction(name, array, len(array), fold)
    namespace, namespace_keep = text('loongarch64')
    table = (U * 2)(NONE, NONE)
    relocations = (TYPES['FixedRelocation'] * 4)()
    context = Context(namespace=namespace, table=table, table_capacity=2,
                      relocations=relocations, relocation_capacity=4)
    result = Result(max_work=max_work)
    code = kernel(C.byref(instruction), C.byref(context), C.byref(result))
    assert code == result.status and result.work <= result.max_work
    return result


def assemble(library, source, limit=1000000):
    encoded, keep = text(source)
    namespace, namespace_keep = text('loongarch64')
    result = Result(max_work=256 * 1024 * 1024)
    probe = library.whale_fixed_probe
    probe.argtypes = [C.POINTER(Text), U, C.POINTER(Result)]
    code = probe(C.byref(encoded), 1, C.byref(result))
    assert code == result.status
    if code:
        return result, {}, b''
    values = {}
    packet = TYPES['FixedInput'](source=encoded, namespace=namespace, hash_comments=1,
                                max_output=limit, max_passes=1024,
                                max_work=result.max_work-result.work)
    symbols = result.symbols
    table_capacity = 1 << max(1, (symbols * 2 - 1).bit_length())
    for field, name, count in [
        ('sections', 'FixedSection', result.sections), ('symbols', 'FixedSymbol', symbols),
        ('globals', 'FixedGlobal', symbols), ('records', 'FixedRecord', result.records),
        ('operands', 'FixedText', result.operands),
        ('relocations', 'FixedRelocation', result.relocations),
    ]:
        array = (TYPES[name] * count)()
        values[field] = array
        setattr(packet, field, array)
        setattr(packet, field[:-1] + '_capacity', count)
    values['table'], values['order'] = (U * table_capacity)(), (U * symbols)()
    packet.table, packet.table_capacity = values['table'], table_capacity
    packet.order, packet.order_capacity = values['order'], symbols
    entry = library.whale_loongarch64_assemble
    entry.argtypes = [C.POINTER(TYPES['FixedInput']), C.POINTER(Result)]
    result = Result()
    code = entry(C.byref(packet), C.byref(result))
    assert code == result.status
    if code:
        return result, values, b''
    size = result.bytes
    values['bytes'] = (C.c_uint8 * size)()
    packet.bytes, packet.byte_capacity, packet.mode = values['bytes'], size, 1
    packet.max_work -= result.work
    code = entry(C.byref(packet), C.byref(result))
    assert code == result.status == 0, (source, code, result.error)
    return result, values, bytes(values['bytes'])


def expected_branch(opcode, register_a, register_b, displacement, bits):
    assert displacement % 4 == 0
    immediate = (displacement // 4) & ((1 << bits) - 1)
    if bits == 26:
        return opcode | (immediate & 0xFFFF) << 10 | immediate >> 16
    if bits == 21:
        return opcode | register_a << 5 | (immediate & 0xFFFF) << 10 | immediate >> 16
    return opcode | register_b | register_a << 5 | immediate << 10


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--library', type=Path, required=True)
    parser.add_argument('--cases', type=int, default=10000)
    args = parser.parse_args()
    library = C.CDLL(str(args.library.resolve()))
    kernel = library.whale_loongarch64_encode
    kernel.argtypes = [C.POINTER(Instruction), C.POINTER(Context), C.POINTER(Result)]
    fixtures = 0
    for line in (ROOT / 'assembler/loongarch64/tests/fixtures/encodings.tsv').read_text().splitlines():
        if not line or line.startswith('#'):
            continue
        encoded, source = line.split('\t')
        result = execute(kernel, source)
        assert result.status == 0, (source, result.error)
        assert result.word == int.from_bytes(bytes.fromhex(encoded), 'little'), source
        fixtures += 1
    for source, error in [
        ('ld.d BAD,$r0,99999', 3000), ('slli.d BAD,$r0,99999', 3000),
        ('lu12i.w BAD,524288', 3001), ('jirl BAD,BAD,18446744073709551616', 3002),
        ('li.d BAD,99999', 3000), ('ADD.D $r0,$r0,$r0', 3003),
    ]:
        result = execute(kernel, source)
        assert result.status and result.error == error, (source, result.error, error)
    assert execute(kernel, 'ADD.D $R0,$R1,$R2', fold=1).word == 0x00108820
    for register in ['r+1', 'r-1', 'r00', 'r01', 'r031', 'r32', '$$sp', 'r１', '']:
        result = execute(kernel, f'move {register},$r0')
        assert result.status and result.error == 3000, (register, result.error)
    for offset in [-(1 << 27), -4, 0, (1 << 27) - 4]:
        result = execute(kernel, f'b {offset}')
        assert result.status == 0 and result.word == expected_branch(0x50000000, 0, 0, offset, 26)
    for opcode, name, bits, registers in [
        (0x50000000, 'b', 26, ''), (0x54000000, 'bl', 26, ''),
        (0x58000000, 'beq', 16, '$r1,$r2,'),
        (0x40000000, 'beqz', 21, '$r1,'),
        (0x4C000000, 'jirl', 16, '$r2,$r1,'),
    ]:
        for offset in [-(1 << (bits+1))-4, 1 << (bits+1), -2, 2]:
            result = execute(kernel, f'{name} {registers}{offset}')
            assert result.status, (name, offset)
    assert execute(kernel, 'nop', max_work=0).status
    for source, expected in [
        ('NOP\nret', bytes.fromhex('000040032000004c')),
        ('.data\n.byte -1,255\n.short 65535\n.long 4294967295\n.quad -1', b'\xff' * 16),
        ('here: b here', bytes.fromhex('00000050')),
        ('.data\n.byte 1,2\n.short 0x1234\n.long 0x12345678\n.quad 0x0123456789abcdef', bytes.fromhex('0102341278563412efcdab8967452301')),
        ('.bss\n.zero 1099511627776\n.balign 4096\n.text\nret', bytes.fromhex('2000004c')),
    ]:
        result, arrays, data = assemble(library, source)
        assert result.status == 0 and data == expected, (source, result.error, data)
    for source, error in [('.global missing\nINVALID', 74), ('here: here: nop', 71),
                          ('.text\n.byte 0\nnop', 35), ('.bss\n.word 0', 34),
                          ('.data\n.byte 256', 79), ('.quad missing', 13)]:
        result, arrays, data = assemble(library, source)
        assert result.status and result.error == error, (source, result.error, error)
    result, arrays, data = assemble(library, '.global start\nstart: nop\nbl external+8\n.extern external\n.data\n.quad start+4')
    assert result.status == 0 and data == bytes.fromhex('0000400300000054') + b'\0' * 8
    assert result.relocations == 2
    branch, absolute = arrays['relocations'][:2]
    assert (branch.section, branch.offset, branch.code, branch.addend) == (0, 4, 66, 8)
    assert (absolute.section, absolute.offset, absolute.code, absolute.addend) == (1, 0, NONE-1, 4)
    numeric = '.word ' + ','.join(['0'] * 60000)
    raw, raw_keep = text(numeric)
    probe_result = Result(max_work=256 * 1024 * 1024)
    assert library.whale_fixed_probe(C.byref(raw), 1, C.byref(probe_result)) == 0
    assert (probe_result.symbols, probe_result.records, probe_result.operands, probe_result.relocations) == (0, 1, 60000, 0)
    result, arrays, data = assemble(library, numeric)
    assert result.status == 0 and data == b'\0' * 240000
    raw, raw_keep = text('nop\n' * 300000)
    probe_result = Result(max_work=256 * 1024 * 1024)
    assert library.whale_fixed_probe(C.byref(raw), 1, C.byref(probe_result)) == 0
    assert probe_result.records == probe_result.relocations == 262145
    result, arrays, data = assemble(library, 'nop\n' * 300000, limit=2000000)
    assert result.status and result.error == 78
    result, arrays, data = assemble(library, 'nop', limit=0)
    assert result.status and result.error == 77
    result, arrays, data = assemble(library, '.bss\n.zero 1099511627776', limit=0)
    assert result.status == 0 and result.bytes == 0 and arrays['sections'][1].size == 1 << 40
    for index, alias in enumerate(['zero', 'ra', 'tp', 'sp', 'a0', 'a1', 'a2', 'a3', 'a4', 'a5', 'a6', 'a7', 't0', 't1', 't2', 't3', 't4', 't5', 't6', 't7', 't8', 'u0', 'fp', 's0', 's1', 's2', 's3', 's4', 's5', 's6', 's7', 's8']):
        for spelling in [alias, '$'+alias, '$'+alias.upper()]:
            result = execute(kernel, f'move {spelling},$r0')
            assert result.status == 0 and result.word == 0x00150000 | index
    rng = random.Random(0x5748414C454C4136)
    for _ in range(args.cases):
        rd, rj, rk = [rng.randrange(32) for _ in range(3)]
        displacement = rng.randrange(-(1 << 25), 1 << 25) * 4
        source = f'b {displacement}'
        result = execute(kernel, source)
        assert result.status == 0 and result.word == expected_branch(0x50000000, 0, 0, displacement, 26)
        displacement = rng.randrange(-(1 << 15), 1 << 15) * 4
        result = execute(kernel, f'beq $r{rj},$r{rd},{displacement}')
        assert result.status == 0 and result.word == expected_branch(0x58000000, rj, rd, displacement, 16)
        displacement = rng.randrange(-(1 << 20), 1 << 20) * 4
        result = execute(kernel, f'bnez $r{rj},{displacement}')
        assert result.status == 0 and result.word == expected_branch(0x44000000, rj, 0, displacement, 21)
        immediate = rng.randrange(-2048, 2048)
        result = execute(kernel, f'addi.d $r{rd},$r{rj},{immediate}')
        assert result.status == 0 and result.word == 0x02C00000 | rd | rj << 5 | (immediate & 4095) << 10
        for mnemonic, opcode in [('li.w', 0x02800000), ('li.d', 0x02C00000)]:
            result = execute(kernel, f'{mnemonic} $r{rd},{immediate}')
            assert result.status == 0 and result.word == opcode | rd | (immediate & 4095) << 10
        result = execute(kernel, f'xor $r{rd},$r{rj},$r{rk}')
        assert result.status == 0 and result.word == 0x00158000 | rd | rj << 5 | rk << 10
    print(f'Wave LoongArch64: {fixtures} LLVM fixtures + {args.cases * 7} independent split-field/integer vectors, invalid registers, exact validation precedence and work limits passed')


if __name__ == '__main__':
    main()
