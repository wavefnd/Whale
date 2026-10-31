#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Independent little-endian fixtures and bounded hostile-bytecode admission."""
import argparse
import ctypes as C
import json
import os
import sys

from fuzz_checkpoint import Checkpoint
from pathlib import Path
import random
import struct

U = C.c_uint64
MISSING = (1 << 64) - 1
FIELDS = ('types edges functions blocks instructions operands value_types globals '
          'sites texts text_bytes files originals mappings value_ids constant_globals').split()
STRIDES = [40, 8, 88, 32, 136, 8, 8, 64, 32, 16, 1, 16, 56, 40, 8, 8]
NATIVE_STRIDES = STRIDES.copy()
NATIVE_STRIDES[4] = 168
NATIVE_STRIDES[7] = 216
HEADER = 576


class Program(C.Structure):
    _fields_ = [(field + suffix, kind) for field in FIELDS
                for suffix, kind in (('_data', C.c_void_p), ('_count', U))]


class Limits(C.Structure):
    _fields_ = [(field, U) for field in
                'max_input_bytes max_decoded_bytes max_records max_work max_type_depth max_workspace_bytes'.split()]

    def __init__(self, **changes):
        defaults = dict(max_input_bytes=64 << 20, max_decoded_bytes=64 << 20,
                        max_records=1_000_000, max_work=64 << 20,
                        max_type_depth=128, max_workspace_bytes=128 << 20)
        defaults.update(changes)
        super().__init__(**defaults)


class Result(C.Structure):
    _fields_ = [(field, U) for field in
                'status code table index work decoded_bytes scratch_words estimated_workspace_bytes'.split()] + [('counts', U * 16)]


def words(*values):
    return struct.pack('<' + 'Q' * len(values), *[value & MISSING for value in values])


def instruction(op, **fields):
    names = 'op sub ty other dst a b c align first count low high literal signature flags global'.split()
    values = dict.fromkeys(names, 0)
    values.update(op=op, dst=MISSING)
    values.update(fields)
    return words(*(values[name] for name in names))


def image(tables):
    """Construct file bytes from explicit portable records, without native writer."""
    assert len(tables) == 16
    directory, payload = bytearray(), bytearray()
    for kind, (data, stride) in enumerate(zip(tables, STRIDES), 1):
        assert len(data) % stride == 0
        directory += words(kind, HEADER + len(payload), len(data) // stride, stride)
        payload += data
    return b'WHALEBC\0' + words(1, 1, 1, HEADER + len(payload), 16, 0, HEADER) + directory + payload


def simple_tables(value=42):
    tables = [b''] * 16
    tables[0] = words(0, 0, 0, 0, 0) + words(7, 0, 0, 0, 0) + words(1, 0, 0, 0, 0) + words(18, 0, 0, 1, 0)
    tables[1] = words(1)
    tables[2] = words(7, 3, 0, 0, 1, 0, 0, 1, 0, 1, 0)
    tables[3] = words(11, 0, 0, 2)
    tables[4] = instruction(4, ty=1, dst=0, low=value, literal=1) + instruction(34, ty=1, a=0)
    tables[6] = words(1)
    tables[8] = words(7, 11, 0, 99) + words(7, 11, 1, MISSING)
    tables[14] = words(99)
    return tables


def diamond_tables():
    tables = simple_tables()
    tables[0] = tables[0][:-40] + words(18, 0, 0, 2, 0)
    tables[1] = words(1, 2)
    tables[2] = words(7, 3, 0, 0, 4, 0, 1, 4, 0, 1, 0)
    tables[3] = words(10, 0, 0, 1) + words(11, 0, 1, 2) + words(12, 0, 3, 2) + words(13, 0, 5, 2)
    tables[4] = (instruction(32, a=0, b=1, c=2)
                 + instruction(4, ty=1, dst=1, low=11, literal=1) + instruction(31, a=3)
                 + instruction(4, ty=1, dst=2, low=22, literal=1) + instruction(31, a=3)
                 + instruction(15, ty=1, dst=3, first=1, count=2) + instruction(34, ty=1, a=3))
    tables[5] = words(0, 1, 1, 2, 2)
    tables[6] = words(2, 1, 1, 1)
    tables[8] = (words(7, 10, 0, MISSING) + words(7, 11, 0, 107) + words(7, 11, 1, MISSING)
                 + words(7, 12, 0, 109) + words(7, 12, 1, MISSING)
                 + words(7, 13, 0, 300) + words(7, 13, 1, MISSING))
    tables[14] = words(100, 107, 109, 300)
    return tables


def scale_tables(count):
    tables = simple_tables()
    tables[2] = words(7, 3, 0, 0, 1, 0, 0, count, 0, 1, 0)
    tables[3] = words(11, 0, 0, count + 1)
    tables[4] = b''.join(instruction(4, ty=1, dst=i, low=i, literal=1)
                         for i in range(count)) + instruction(34, ty=1, a=count - 1)
    tables[6] = words(*([1] * count))
    tables[8] = b''.join(words(7, 11, i, 99 + i * 3) for i in range(count)) + words(7, 11, count, MISSING)
    tables[14] = words(*(99 + i * 3 for i in range(count)))
    return tables


def parse_directory(data):
    """Independent structural oracle; does not inspect any native output."""
    assert len(data) >= HEADER and data[:8] == b'WHALEBC\0'
    assert struct.unpack_from('<7Q', data, 8) == (1, 1, 1, len(data), 16, 0, HEADER)
    tables, offset = [], HEADER
    for kind, stride in enumerate(STRIDES, 1):
        actual, start, count, width = struct.unpack_from('<4Q', data, 64 + (kind - 1) * 32)
        assert (actual, start, width) == (kind, offset, stride)
        end = start + count * stride
        assert end <= len(data)
        tables.append(data[start:end])
        offset = end
    assert offset == len(data)
    return tables


def replace_word(data, table, record, field, value):
    result = bytearray(data)
    offset = struct.unpack_from('<Q', data, 64 + (table - 1) * 32 + 8)[0]
    struct.pack_into('<Q', result, offset + record * STRIDES[table - 1] + field * 8, value & MISSING)
    return bytes(result)


def fixtures():
    valid = image(simple_tables())
    cases = {'return42.wbc': (valid, True), 'empty.wbc': (image([b''] * 16), True)}
    for label, offset, value in [('version', 8, 2), ('semantics', 16, 2), ('layout', 24, 2),
                                 ('length', 32, 0), ('table_count', 40, 17), ('flags', 48, 1),
                                 ('header', 56, 0), ('directory_overlap', 72, 0),
                                 ('count_overflow', 80, MISSING), ('stride', 88, 8)]:
        data = bytearray(valid)
        struct.pack_into('<Q', data, offset, value)
        cases['invalid_' + label + '.wbc'] = (bytes(data), False)
    cases['invalid_truncated.wbc'] = (valid[:-1], False)
    cases['invalid_trailing.wbc'] = (valid + b'\0', False)
    for label, table, record, field, value in [
        ('type_kind', 1, 1, 0, 22), ('type_cycle', 1, 3, 0, 17),
        ('signature', 3, 0, 1, MISSING), ('function_id', 3, 0, 0, MISSING),
        ('entry', 3, 0, 2, 10), ('block_owner', 4, 0, 1, 1),
        ('block_count', 4, 0, 3, MISSING), ('opcode', 5, 0, 0, 99),
        ('destination', 5, 0, 4, MISSING), ('return_slot', 5, 1, 5, MISSING),
        ('return_type', 5, 1, 2, 2), ('early_terminal', 5, 0, 0, 34),
        ('site', 9, 0, 3, 0), ('ssa_id', 15, 0, 0, MISSING),
    ]:
        cases['invalid_' + label + '.wbc'] = (replace_word(valid, table, record, field, value), False)
    # A borrowed string descriptor cannot amplify a tiny byte blob through overlap.
    tables = simple_tables()
    tables[9], tables[10] = words(0, 3) + words(0, 3), b'abc'
    cases['invalid_text_overlap.wbc'] = (image(tables), False)
    tables[9], tables[10] = words(0, 2), b'\xc0\x80'
    cases['invalid_utf8.wbc'] = (image(tables), False)
    # Original metadata for a folded constant must remain valid without storage.
    tables = simple_tables()
    tables[9], tables[10] = words(0, 8), b'input.wv'
    tables[11] = words(4, 0)
    tables[12] = words(900, 1, 4, 2, 3, 2, 9)
    tables[13] = words(2, 77, 0, 0, 900)
    tables[15] = words(77)
    cases['constant_source.wbc'] = (image(tables), True)
    diamond = image(diamond_tables())
    cases['phi_diamond.wbc'] = (diamond, True)
    for label, table, record, field, value in [
        ('phi_missing_edge', 5, 5, 10, 1), ('phi_slot', 6, 1, 0, MISSING),
        ('phi_nonpredecessor', 6, 2, 0, 0), ('phi_duplicate_edge', 6, 4, 0, 1),
        ('phi_type', 6, 1, 0, 0), ('nondominating_return', 5, 6, 5, 1),
        ('cross_function_target', 5, 0, 6, 9), ('duplicate_ssa_id', 15, 1, 0, 100),
    ]:
        cases['invalid_' + label + '.wbc'] = (replace_word(diamond, table, record, field, value), False)
    return cases


class Native:
    def __init__(self, path, checkpoint=None):
        self.checkpoint = checkpoint
        self.context = {}
        self.library = C.CDLL(str(path))
        self.probe = self.library.whale_ir_bytecode_probe
        self.probe.argtypes = [C.c_void_p, U, C.POINTER(Limits), C.POINTER(Result)]
        self.probe.restype = C.c_int32
        self.load = self.library.whale_ir_bytecode_load
        self.load.argtypes = [C.c_void_p, U, C.POINTER(Program), C.c_void_p, U,
                              C.POINTER(Limits), C.POINTER(Result)]
        self.load.restype = C.c_int32
        self.verify = self.library.whale_ir_bytecode_verify
        self.verify.argtypes = [C.POINTER(Program), C.c_void_p, U,
                                C.POINTER(Limits), C.POINTER(Result)]
        self.verify.restype = C.c_int32

    def admit(self, data, limits=None):
        limits = limits or Limits()
        if self.checkpoint:
            self.checkpoint.before(data, **self.context,
                                   limits={key: getattr(limits, key) for key, _ in Limits._fields_})
        raw, result = C.create_string_buffer(data), Result()
        status = self.probe(raw, len(data), C.byref(limits), C.byref(result))
        assert status == result.status and result.work <= limits.max_work
        if status:
            assert status != 4, ('file caused transport error', status, result.code)
            return status, result, None
        assert result.estimated_workspace_bytes == result.decoded_bytes + result.scratch_words * 8
        program, buffers = Program(), []
        for name, count, stride in zip(FIELDS, result.counts, NATIVE_STRIDES):
            buffer = (U * max(1, (count * stride + 7) // 8))()
            buffers.append(buffer)
            setattr(program, name + '_data', C.addressof(buffer))
            setattr(program, name + '_count', count)
        scratch = (U * result.scratch_words)()
        status = self.load(raw, len(data), C.byref(program), scratch, len(scratch),
                           C.byref(limits), C.byref(result))
        assert status == result.status and result.work <= limits.max_work
        assert status != 4, ('file caused transport error', status, result.code, result.table, result.index)
        if status == 0:
            assert result.estimated_workspace_bytes == workspace_estimate(data, result)
            assert result.estimated_workspace_bytes <= limits.max_workspace_bytes
        return status, result, (program, buffers, scratch)


def workspace_estimate(data, result):
    tables = parse_directory(data)
    rows = [list(struct.iter_unpack('<' + 'Q' * (stride // 8), table))
            if stride != 1 else table for table, stride in zip(tables, STRIDES)]
    expanded = []
    for kind, child, first, count, _ in rows[0]:
        if kind in (17, 19):
            expanded.append(1 + expanded[child])
        elif kind in (18, 20, 21):
            expanded.append(1 + sum(expanded[rows[1][first + i][0]] for i in range(count)))
        else:
            expanded.append(1)
    owned = 512 * (sum(expanded) + sum(expanded[function[1]] for function in rows[2]))
    owned += len(tables[10]) + sum(rows[9][file[1]][1] for file in rows[11])
    for table, allowance in [(8, 64), (9, 32), (11, 96), (12, 96), (13, 96), (15, 16)]:
        owned += len(rows[table]) * allowance
    return result.decoded_bytes + result.scratch_words * 8 + owned


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--library', type=Path)
    parser.add_argument('--cases', type=int, default=10000)
    parser.add_argument('--scale', type=int, default=0)
    parser.add_argument('--seed', type=int, default=0x57424331)
    parser.add_argument('--artifacts', type=Path,
                        default=None)
    parser.add_argument('--replay', type=Path)
    parser.add_argument('--write-fixtures', type=Path)
    args = parser.parse_args()
    if not 0 <= args.cases <= 100000 or not 0 <= args.scale <= 32768 or not 0 <= args.seed < 2**64:
        parser.error('cases must be 0..100000, scale 0..32768 and seed an unsigned 64-bit integer')
    cases = fixtures()
    if args.write_fixtures:
        args.write_fixtures.mkdir(parents=True, exist_ok=True)
        for name, (data, _) in cases.items():
            (args.write_fixtures / name).write_bytes(data)
    if not args.library:
        assert args.write_fixtures, 'provide --library or --write-fixtures'
        return
    if args.artifacts is None:
        import hashlib
        root = Path(os.environ.get('WHALE_FUZZ_ARTIFACT_DIR', '.ci-artifacts/bytecode'))
        args.artifacts = root / hashlib.sha256(args.library.read_bytes()).hexdigest()[:16]
    args.artifacts = args.artifacts.resolve()
    checkpoint = Checkpoint(args.artifacts, 'admission',
                            [sys.executable, str(Path(__file__).resolve()), '--library', str(args.library.resolve())])
    native, checked = Native(args.library, checkpoint), 0
    if args.replay:
        metadata_path = args.replay.with_suffix('.json')
        record = json.loads(metadata_path.read_text()) if metadata_path.is_file() else {}
        with args.replay.open('rb') as stream:
            data = stream.read((64 << 20) + 1)
        native.context = {key: record[key] for key in ('case', 'seed', 'expected') if key in record}
        status, _, _ = native.admit(data, Limits(**record.get('limits', {})))
        if record.get('expected') is not None:
            assert (status == 0) == record['expected']
        checkpoint.finish(replay=str(args.replay), status=status)
        print(json.dumps(dict(replay=str(args.replay), status=status)))
        return
    for name, (data, expected) in cases.items():
        native.context = dict(case=name, expected=expected)
        status, result, _ = native.admit(data)
        assert (status == 0) == expected, (name, status, result.code, result.table, result.index)
        if expected:
            parse_directory(data)
        checked += 1
    native.context = dict(case='resource-boundaries', expected=None)
    valid = cases['return42.wbc'][0]
    status, result, _ = native.admit(valid)
    assert status == 0
    exact_work = result.work
    exact_workspace = result.estimated_workspace_bytes
    for maximum, expected in [(exact_workspace, 0), (exact_workspace - 1, 2)]:
        status, _, _ = native.admit(valid, Limits(max_workspace_bytes=maximum))
        assert status == expected, ('full workspace estimate', maximum, status, expected)
        checked += 1
    for maximum, expected in [(exact_work, 0), (exact_work - 1, 3), (575, 3), (0, 3)]:
        status, _, _ = native.admit(valid, Limits(max_work=maximum))
        assert status == expected, ('cumulative work', maximum, status, expected)
        checked += 1
    if args.scale:
        status, result, _ = native.admit(image(scale_tables(args.scale)))
        assert status == 0, ('scale', args.scale, status, result.code, result.table, result.index)
        checked += 1
        tables = simple_tables()
        tables[0] += words(7, 0, 0, 0, 0) * min(args.scale * 8, 262144)
        status, result, _ = native.admit(image(tables), Limits(max_workspace_bytes=256 << 20))
        assert status == 0, ('flat type O0 stack bound', status, result.code)
        checked += 1
        tables = simple_tables()
        tables[9], tables[10] = words(0, 2 << 20), b'x' * (2 << 20)
        tables[11] = words(1, 0)
        status, result, _ = native.admit(image(tables), Limits(max_workspace_bytes=8 << 20))
        assert status == 0, ('large UTF8 O0 stack bound', status, result.code)
        checked += 1
    # Small shared type DAGs cannot trigger exponentially expanded host Types.
    tables = simple_tables()
    for node in range(4, 25):
        first = len(tables[1]) // 8
        tables[1] += words(node - 1, node - 1)
        tables[0] += words(20, 0, first, 2, 0)
    status, _, _ = native.admit(image(tables), Limits(max_workspace_bytes=1 << 20))
    assert status == 2, ('expanded type DAG budget', status)
    checked += 1
    # Repeated source-file names consume cumulative clone/work budget.
    tables = simple_tables()
    tables[9], tables[10] = words(0, 4096), b'x' * 4096
    tables[11] = b''.join(words(i, 0) for i in range(128))
    status, _, _ = native.admit(image(tables), Limits(max_workspace_bytes=256 << 10))
    assert status == 2, ('repeated filename budget', status)
    checked += 1
    rng = random.Random(args.seed)
    accepted = 0
    for case in range(args.cases):
        native.context = dict(case=case, seed=args.seed, expected=None)
        data = bytearray(valid)
        for _ in range(rng.randrange(1, 5)):
            at = rng.randrange(len(data))
            data[at] ^= 1 << rng.randrange(8)
        status, result, _ = native.admit(bytes(data))
        if status == 0:
            parse_directory(data)
            accepted += 1
        checked += 1
    checkpoint.finish(checked=checked, mutations=args.cases, seed=args.seed)
    print(json.dumps(dict(status='passed', checked=checked, mutations=args.cases,
                          accepted_mutations=accepted, exact_valid_work=exact_work)))


if __name__ == '__main__':
    main()
