#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Independent source identity, span, mapping and bounded-work vectors."""
import argparse
import ctypes as c
import random


class File(c.Structure):
    _fields_ = [('id', c.c_uint64), ('bytes', c.c_void_p), ('length', c.c_uint64)]


def record(name, fields):
    return type(name, (c.Structure,), {'_fields_': [(key, c.c_uint64) for key in fields.split()]})


Original = record('Original', 'id has_span file line column end_line end_column')
Block = record('Block', 'function block instructions terminated')
Mapping = record('Mapping', 'kind function block index original')
Slot = record('Slot', 'kind a b c value extra')
Result = record('Result', 'status index work')


def expected(files, nodes, blocks, globals_, mappings):
    seen = set()
    for ident, name in files:
        if not name or b'\0' in name or ident in seen:
            return 1
        seen.add(ident)
    originals = set()
    for ident, present, file, line, column, end_line, end_column in nodes:
        if ident in originals:
            return 2
        originals.add(ident)
        if present and (file not in seen or min(line, column, end_line, end_column) == 0
                        or (line, column) > (end_line, end_column)):
            return 3
    block_map = {(f, b): (n, t) for f, b, n, t in blocks}
    seen_targets = set()
    for kind, f, b, index, ident in mappings:
        key = (kind, f, b, index)
        if ident not in originals or key in seen_targets:
            return 4
        seen_targets.add(key)
        if kind == 2:
            if f not in globals_:
                return 5
        elif (f, b) not in block_map or not (
                index < block_map[f, b][0] if kind == 0 else block_map[f, b][1]):
            return 5
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--library', required=True)
    parser.add_argument('--cases', type=int, default=10000)
    args = parser.parse_args()
    kernel = c.CDLL(args.library).whale_ir_verify_sources
    kernel.argtypes = [c.c_void_p, c.c_uint64] * 6 + [c.c_uint64, c.POINTER(Result)]

    def run(files, nodes, blocks, globals_, mappings, work=1000000, capacity=None):
        buffers = [c.create_string_buffer(name) for _, name in files]
        raw_files = (File * len(files))(*(File(ident, c.cast(buf, c.c_void_p), len(name))
            for (ident, name), buf in zip(files, buffers)))
        raw_nodes = (Original * len(nodes))(*(Original(*n) for n in nodes))
        raw_blocks = (Block * len(blocks))(*(Block(*b) for b in blocks))
        raw_globals = (c.c_uint64 * len(globals_))(*globals_)
        raw_maps = (Mapping * len(mappings))(*(Mapping(*m) for m in mappings))
        count = len(files) + len(nodes) + len(blocks) + len(globals_) + len(mappings)
        capacity = capacity if capacity is not None else 1 << (max(1, count) * 2 - 1).bit_length()
        slots = (Slot * capacity)()
        result = Result()
        status = kernel(raw_files, len(files), raw_nodes, len(nodes), raw_blocks, len(blocks),
                        raw_globals, len(globals_), raw_maps, len(mappings), slots, capacity,
                        work, c.byref(result))
        assert status == result.status and result.work <= work, (status, result.status, result.work)
        return status

    rng = random.Random(350_348)
    for case in range(args.cases):
        n = rng.randrange(1, 30)
        files = [(i, ('경로%d.wave' % i).encode()) for i in range(n)]
        nodes = [(i, rng.randrange(2), i, 1, 3, 1, 7) for i in range(n)]
        blocks = [(i, i + 1, rng.randrange(3), rng.randrange(2)) for i in range(n)]
        globals_ = list(range(n))
        mappings = [(kind, rng.randrange(n + 1), rng.randrange(n + 2) if kind < 2 else 0,
                     rng.randrange(3) if kind == 0 else 0, rng.randrange(n + 1))
                    for kind in (rng.randrange(3) for _ in range(rng.randrange(30)))]
        mutation = case % 10
        if mutation == 0:
            files.append(files[-1])
        elif mutation == 1:
            files[-1] = (files[-1][0], b'')
        elif mutation == 2:
            files[-1] = (files[-1][0], b'a\0b')
        elif mutation == 3:
            nodes.append(nodes[-1])
        elif mutation == 4:
            nodes[-1] = (n - 1, 1, n - 1, 0, 0, 0, 0)
        elif mutation == 5:
            nodes[-1] = (n - 1, 1, n - 1, 1, 9, 1, 3)
        elif mutation == 6:
            mappings = [(1, 0, 1, 0, 0)] * 2
        elif mutation == 7:
            mappings = [(0, i, i + 1, 0, i) for i in range(n) if blocks[i][2] > 0]
        elif mutation == 8:
            mappings = [(2, i, 0, 0, i) for i in range(n)]
        else:
            files[-1] = (2**64 - 1, files[-1][1])
            nodes[-1] = (2**64 - 1, 1, 2**64 - 1, 1, 1, 2**32 - 1, 1)
            mappings = [(2, 0, 0, 0, 2**64 - 1)]
        assert run(files, nodes, blocks, globals_, mappings) == expected(files, nodes, blocks, globals_, mappings), case
    valid = ([(0, b'a')], [(0, 1, 0, 1, 1, 1, 1)], [(0, 0, 1, 1)], [0], [(0, 0, 0, 0, 0)])
    assert run(*valid, work=0) == 7
    assert run(*valid, capacity=2) == 6
    assert run(*valid, capacity=3) == 6
    # A duplicated block key must preserve the latest body just like the reference map.
    assert run(valid[0], valid[1], [(0, 0, 1, 1), (0, 0, 0, 0)], valid[3], valid[4]) == 5
    count = 60000
    wide = ([(0, b'wide.wave')], [(i, 0, 0, 0, 0, 0, 0) for i in range(count)],
            [(0, 0, count, 1)], [], [(0, 0, 0, i, i) for i in range(count)])
    assert run(*wide, work=10000000) == 0
    assert run(*wide, work=1000) == 7
    print(f'Wave IR source metadata: {args.cases} independent vectors, 60000 mappings and work limits passed')


if __name__ == '__main__':
    main()
