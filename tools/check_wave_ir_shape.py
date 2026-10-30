#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Independent structural IR identity and branch-shape Wave vectors."""

import argparse
import ctypes
import random


class Block(ctypes.Structure):
    _fields_ = [(name, ctypes.c_uint64) for name in
                ("id", "terminated", "first_edge", "edge_count")]


class Value(ctypes.Structure):
    _fields_ = [("id", ctypes.c_uint64), ("void_parameter", ctypes.c_uint64)]


class Result(ctypes.Structure):
    _fields_ = [(name, ctypes.c_uint64) for name in ("status", "index", "detail", "work")]


def call(kernel, blocks, entry, values, work=100000):
    raw_blocks = []
    raw_edges = []
    for identity, terminated, targets in blocks:
        raw_blocks.append(Block(identity, terminated, len(raw_edges), len(targets)))
        raw_edges.extend(targets)
    block_array = (Block * len(blocks))(*raw_blocks)
    values_array = (Value * len(values))(*(Value(*value) for value in values))
    edges_array = (ctypes.c_uint64 * len(raw_edges))(*raw_edges)
    block_capacity = 1 << (max(2, 2 * len(blocks)) - 1).bit_length()
    value_capacity = 1 << (max(2, 2 * len(values)) - 1).bit_length()
    block_table = (ctypes.c_uint64 * block_capacity)()
    value_table = (ctypes.c_uint64 * value_capacity)()
    result = Result()
    status = kernel(block_array, len(blocks), entry, values_array, len(values),
                    edges_array, len(raw_edges), work, block_table, block_capacity,
                    value_table, value_capacity, ctypes.byref(result))
    assert status == result.status
    return status, result.index, result.detail, result.work


def reference(blocks, entry, values):
    seen_blocks = set()
    for index, (identity, _, _) in enumerate(blocks):
        if identity in seen_blocks:
            return 1, index, identity
        seen_blocks.add(identity)
    if entry not in seen_blocks:
        return 2, 0, entry
    seen_values = set()
    for index, (identity, void) in enumerate(values):
        if void:
            return 8, index, identity
        if identity in seen_values:
            return 3, index, identity
        seen_values.add(identity)
    for index, (identity, terminated, targets) in enumerate(blocks):
        if not terminated:
            return 4, index, identity
        for target in targets:
            if target not in seen_blocks:
                return 5, index, target
    return 0, 0, 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", required=True)
    parser.add_argument("--cases", type=int, default=10000)
    args = parser.parse_args()
    kernel = ctypes.CDLL(args.library).whale_ir_verify_shape
    kernel.argtypes = [ctypes.c_void_p, ctypes.c_uint64, ctypes.c_uint64,
                       ctypes.c_void_p, ctypes.c_uint64, ctypes.c_void_p,
                       ctypes.c_uint64, ctypes.c_uint64, ctypes.c_void_p,
                       ctypes.c_uint64, ctypes.c_void_p, ctypes.c_uint64,
                       ctypes.POINTER(Result)]
    seeds = [([(0, 1, [1]), (1, 1, [])], 0, [(0, 0)]),
             ([(0, 1, []), (0, 1, [])], 0, []),
             ([(0, 1, [])], 2, []),
             ([(0, 0, [])], 0, [(0, 1)]),
             ([(0, 1, [7])], 0, [(2, 0), (2, 0)])]
    for blocks, entry, values in seeds:
        assert call(kernel, blocks, entry, values)[:3] == reference(blocks, entry, values)
    assert call(kernel, [(0, 1, [])], 0, [], work=0)[0] == 7
    chain = [(index, 1, [index + 1] if index < 59999 else [])
             for index in range(60000)]
    identities = [(index, 0) for index in range(60000)]
    assert call(kernel, chain, 0, identities, work=2000000)[0] == 0
    randomizer = random.Random(0x3480350)
    for index in range(args.cases):
        count = randomizer.randrange(0, 24)
        blocks = [(randomizer.randrange(0, 30), randomizer.randrange(0, 2),
                   [randomizer.randrange(0, 30) for _ in range(randomizer.randrange(0, 4))])
                  for _ in range(count)]
        values = [(randomizer.randrange(0, 30), randomizer.randrange(0, 2))
                  for _ in range(randomizer.randrange(0, 30))]
        entry = randomizer.randrange(0, 30)
        actual = call(kernel, blocks, entry, values)[:3]
        expected = reference(blocks, entry, values)
        assert actual == expected, (index, blocks, entry, values, actual, expected)
    for index in range(args.cases // 2):
        count = randomizer.randrange(1, 40)
        identities = randomizer.sample(range(0, 100000), count)
        blocks = [(identity, 1,
                   [randomizer.choice(identities) for _ in range(randomizer.randrange(0, 4))])
                  for identity in identities]
        values = [(identity, 0) for identity in randomizer.sample(range(0, 100000),
                                                                  randomizer.randrange(0, 40))]
        entry = randomizer.choice(identities)
        if index % 6 == 0 and count > 1:
            blocks[-1] = (blocks[0][0], 1, blocks[-1][2])
        elif index % 6 == 1:
            entry = 100001
        elif index % 6 == 2 and values:
            values.append(values[0])
        elif index % 6 == 3 and values:
            values[-1] = (values[-1][0], 1)
        elif index % 6 == 4:
            blocks[-1] = (blocks[-1][0], 0, blocks[-1][2])
        elif index % 6 == 5:
            blocks[-1][2].append(100001)
        actual = call(kernel, blocks, entry, values)[:3]
        expected = reference(blocks, entry, values)
        assert actual == expected, (index, blocks, entry, values, actual, expected)
    print(f"Wave IR shape: vectors, limits and {args.cases + args.cases // 2} cases passed")


if __name__ == "__main__":
    main()
