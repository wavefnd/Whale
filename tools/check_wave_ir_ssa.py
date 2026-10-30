#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Check real Wave SSA/phi validation against an independent dominance oracle."""

import argparse
import ctypes as C
import random

from wave_ir_verifier_oracle import NONE, SCALARS, Node


class Block(C.Structure):
    _fields_ = [(name, C.c_uint64) for name in
               ("id", "first", "count", "pred_first", "pred_count", "reachable",
                "start", "end", "terminator_use")]


class Instruction(C.Structure):
    _fields_ = [(name, C.c_uint64) for name in ("phi", "dst", "ty", "first", "count")]


class Usage(C.Structure):
    _fields_ = [(name, C.c_uint64) for name in
               ("value", "ty", "pred", "def_block", "def_position", "defined")]


class Predecessor(C.Structure):
    _fields_ = [(name, C.c_uint64) for name in ("block", "seen")]


class Result(C.Structure):
    _fields_ = [(name, C.c_uint64) for name in
               ("status", "instruction", "usage", "detail", "work", "queries")]


def call(kernel, blocks, instructions, uses, predecessors, capacity=128, work=1_000_000):
    nodes = (Node * len(SCALARS))(*SCALARS)
    edges = (C.c_uint64 * 0)()
    block_array = (Block * len(blocks))(*blocks)
    instruction_array = (Instruction * len(instructions))(*instructions)
    use_array = (Usage * len(uses))(*uses)
    predecessor_array = (C.c_uint64 * len(predecessors))(*predecessors)
    table = (Predecessor * capacity)()
    result = Result()
    status = kernel(nodes, len(nodes), edges, 0, block_array, len(blocks), 0,
                    instruction_array, len(instructions), use_array, len(uses),
                    predecessor_array, len(predecessors), table, capacity, work, C.byref(result))
    assert status == result.status
    assert result.work <= work and result.queries <= result.work
    return status, result


def intervals_from_sets(predecessors):
    """Independent set intersection, unrelated to the Wave immediate-idom kernel."""
    size = len(predecessors)
    dominators = [set(range(size)) for _ in predecessors]
    dominators[0] = {0}
    for block in range(1, size):
        dominators[block] = {block} | set.intersection(*(dominators[p] for p in predecessors[block]))
    parents = [None] + [max(dominators[block] - {block}, key=lambda p: len(dominators[p]))
                        for block in range(1, size)]
    children = [[] for _ in predecessors]
    for block in range(1, size):
        children[parents[block]].append(block)
    intervals, clock = [None] * size, 0

    def visit(block):
        nonlocal clock
        start, clock = clock, clock + 1
        for child in children[block]:
            visit(child)
        intervals[block], clock = (start, clock), clock + 1

    visit(0)
    return dominators, intervals


def random_cases(kernel, cases):
    randomizer = random.Random(0x3501)
    for trial in range(cases):
        size = randomizer.randint(2, 30)
        predecessors = [[] for _ in range(size)]
        for block in range(1, size):
            predecessors[block] = [source for source in range(block)
                                   if source == block - 1 or randomizer.randrange(7) == 0]
        dominators, intervals = intervals_from_sets(predecessors)
        uses, instructions, edges, blocks = [], [], [], []
        expected, expected_usage = 0, None
        for block in range(size):
            first_use = len(uses)
            for _ in range(randomizer.randrange(4)):
                definition = randomizer.randrange(size + 1)
                if definition == size:
                    uses.append(Usage(100, NONE, NONE, NONE, 0, 1))
                    continue
                uses.append(Usage(definition + 1, NONE, NONE, definition, 0, 1))
                if expected == 0 and (definition == block or definition not in dominators[block]):
                    expected, expected_usage = 3, len(uses) - 1
            instructions.append(Instruction(0, block + 1, 0, first_use, len(uses) - first_use))
            first_predecessor = len(edges)
            edges.extend(predecessors[block])
            start, end = intervals[block]
            blocks.append(Block(block + 10, block, 1, first_predecessor,
                                len(predecessors[block]), 1, start, end, NONE))
        status, result = call(kernel, blocks, instructions, uses, edges)
        assert status == expected, (trial, status, expected)
        if status:
            assert result.usage == expected_usage, (trial, result.usage, expected_usage)


def vectors(kernel):
    blocks = [Block(10, 0, 1, 0, 0, 1, 0, 7, NONE),
              Block(11, 1, 1, 0, 1, 1, 1, 2, NONE),
              Block(12, 2, 1, 1, 1, 1, 3, 4, NONE),
              Block(13, 3, 1, 2, 2, 1, 5, 6, NONE)]
    instructions = [Instruction(0, 1, 0, 0, 0), Instruction(0, 2, 0, 0, 0),
                    Instruction(0, 3, 0, 0, 0), Instruction(1, 4, 6, 0, 2)]
    uses = [Usage(2, 6, 1, 1, 0, 1), Usage(3, 6, 2, 2, 0, 1)]
    predecessors = [0, 0, 1, 2]
    assert call(kernel, blocks, instructions, uses, predecessors)[0] == 0
    uses[1].pred = 1
    status, result = call(kernel, blocks, instructions, uses, predecessors)
    assert status == 1 and result.detail == 5
    uses[1].pred = NONE
    status, result = call(kernel, blocks, instructions, uses, predecessors)
    assert status == 1 and result.detail == 3
    uses[1].pred, uses[1].ty = 2, 7
    assert call(kernel, blocks, instructions, uses, predecessors)[0] == 5
    uses[1].ty, uses[1].def_block = 6, 1
    assert call(kernel, blocks, instructions, uses, predecessors)[0] == 3
    uses[1].def_block = 2
    assert call(kernel, blocks, instructions, uses, predecessors, work=0)[0] == 8
    assert call(kernel, blocks, instructions, uses, predecessors, capacity=1)[0] == 7
    uses[1].defined = 0
    assert call(kernel, blocks, instructions, uses, predecessors)[0] == 2
    # Structurally unreachable instructions keep identity/type checks, while
    # executable dominance does not apply to their ordinary uses.
    blocks[3].reachable = 0
    instructions[3].phi = 0
    uses[1].defined = 1
    assert call(kernel, blocks, instructions, uses, predecessors)[0] == 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", required=True)
    parser.add_argument("--cases", type=int, default=10000)
    args = parser.parse_args()
    assert args.cases >= 0
    library = C.CDLL(args.library)
    kernel = library.whale_ir_verify_ssa
    kernel.restype = C.c_int32
    kernel.argtypes = [C.POINTER(Node), C.c_uint64, C.POINTER(C.c_uint64), C.c_uint64,
                       C.POINTER(Block), C.c_uint64, C.c_uint64, C.POINTER(Instruction),
                       C.c_uint64, C.POINTER(Usage), C.c_uint64, C.POINTER(C.c_uint64),
                       C.c_uint64, C.POINTER(Predecessor), C.c_uint64, C.c_uint64,
                       C.POINTER(Result)]
    vectors(kernel)
    random_cases(kernel, args.cases)
    print(f"Wave IR SSA: phi/type/unreachable/resource vectors and {args.cases} graphs passed")


if __name__ == "__main__":
    main()
