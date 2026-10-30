#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Independent Wave IR CFG traversal vectors and bounded random graphs."""

import argparse
import ctypes
import random


class Block(ctypes.Structure):
    _fields_ = [("first", ctypes.c_uint64), ("count", ctypes.c_uint64)]


class Result(ctypes.Structure):
    _fields_ = [(name, ctypes.c_uint64) for name in ("status", "index", "count", "work")]


def call(kernel, graph, entry=0, work=100000, capacity=None):
    count = len(graph)
    if capacity is None:
        capacity = count
    blocks = []
    edges = []
    for targets in graph:
        blocks.append(Block(len(edges), len(targets)))
        edges.extend(targets)
    block_array = (Block * count)(*blocks)
    edge_array = (ctypes.c_uint64 * len(edges))(*edges)
    seen = (ctypes.c_uint8 * capacity)()
    stack_block = (ctypes.c_uint64 * capacity)()
    stack_next = (ctypes.c_uint64 * capacity)()
    scratch = (ctypes.c_uint64 * capacity)()
    output = (ctypes.c_uint64 * capacity)(*([0xdead] * capacity))
    result = Result()
    status = kernel(block_array, count, edge_array, len(edges), entry, work,
                    seen, capacity, stack_block, stack_next, capacity,
                    scratch, capacity, output, capacity, ctypes.byref(result))
    assert status == result.status
    if status:
        assert all(value == 0xdead for value in output), (graph, status)
    return status, result.index, list(output[:result.count]), result.work


def reference(graph, entry):
    for source, targets in enumerate(graph):
        if entry in targets:
            return 1, source, []
    seen = set()
    postorder = []

    def visit(block):
        if block in seen:
            return
        seen.add(block)
        for target in graph[block]:
            visit(target)
        postorder.append(block)

    visit(entry)
    return 0, 0, list(reversed(postorder))


def call_idom(kernel, graph, order, entry=0, work=100000, capacity=None):
    count = len(graph)
    if capacity is None:
        capacity = count
    predecessors = [[] for _ in graph]
    for source, targets in enumerate(graph):
        for target in dict.fromkeys(targets):
            predecessors[target].append(source)
    blocks = []
    edges = []
    for sources in predecessors:
        blocks.append(Block(len(edges), len(sources)))
        edges.extend(sources)
    block_array = (Block * count)(*blocks)
    edge_array = (ctypes.c_uint64 * len(edges))(*edges)
    order_array = (ctypes.c_uint64 * len(order))(*order)
    rank = (ctypes.c_uint64 * capacity)()
    scratch = (ctypes.c_uint64 * capacity)()
    output = (ctypes.c_uint64 * capacity)(*([0xdead] * capacity))
    result = Result()
    status = kernel(block_array, count, edge_array, len(edges), order_array,
                    len(order), entry, work, rank, capacity, scratch, capacity,
                    output, capacity, ctypes.byref(result))
    assert status == result.status
    if status:
        assert all(value == 0xdead for value in output), (graph, status)
    return status, [None if value == (1 << 64) - 1 else value
                    for value in output[:result.count]]


def reference_idom(graph, entry):
    reachable = {entry}
    pending = [entry]
    while pending:
        for target in graph[pending.pop()]:
            if target not in reachable:
                reachable.add(target)
                pending.append(target)
    predecessors = [[] for _ in graph]
    for source in reachable:
        for target in graph[source]:
            if target in reachable:
                predecessors[target].append(source)
    dom = {block: ({entry} if block == entry else set(reachable)) for block in reachable}
    changed = True
    while changed:
        changed = False
        for block in reachable - {entry}:
            common = set(reachable)
            for pred in predecessors[block]:
                common &= dom[pred]
            next_dom = common | {block}
            if next_dom != dom[block]:
                dom[block] = next_dom
                changed = True
    result = [None] * len(graph)
    result[entry] = entry
    for block in reachable - {entry}:
        strict = dom[block] - {block}
        result[block] = max(strict, key=lambda candidate: len(dom[candidate]))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", required=True)
    parser.add_argument("--cases", type=int, default=10000)
    args = parser.parse_args()
    kernel = ctypes.CDLL(args.library).whale_ir_cfg_rpo
    kernel.argtypes = [ctypes.c_void_p, ctypes.c_uint64, ctypes.c_void_p,
                       ctypes.c_uint64, ctypes.c_uint64, ctypes.c_uint64,
                       ctypes.c_void_p, ctypes.c_uint64, ctypes.c_void_p,
                       ctypes.c_void_p, ctypes.c_uint64, ctypes.c_void_p,
                       ctypes.c_uint64, ctypes.c_void_p, ctypes.c_uint64,
                       ctypes.POINTER(Result)]
    dominance = ctypes.CDLL(args.library).whale_ir_cfg_idom
    dominance.argtypes = [ctypes.c_void_p, ctypes.c_uint64, ctypes.c_void_p,
                          ctypes.c_uint64, ctypes.c_void_p, ctypes.c_uint64,
                          ctypes.c_uint64, ctypes.c_uint64, ctypes.c_void_p,
                          ctypes.c_uint64, ctypes.c_void_p, ctypes.c_uint64,
                          ctypes.c_void_p, ctypes.c_uint64, ctypes.POINTER(Result)]
    cases = [([[1, 2], [3], [3], []], 0),
             ([[1], [2], [3], []], 0),
             ([[1], [2], [1], []], 0),
             ([[1], [], [0]], 0),
             ([[1, 1], []], 0),
             ([[]], 0)]
    for graph, entry in cases:
        status, source, order, _ = call(kernel, graph, entry)
        assert (status, source, order) == reference(graph, entry)
        if status == 0:
            assert call_idom(dominance, graph, order, entry) == (0, reference_idom(graph, entry))
    assert call(kernel, [[1], []], work=1)[0] == 3
    assert call(kernel, [[1], []], capacity=1)[0] == 2
    assert call(kernel, [[2], []])[0] == 5
    assert call(kernel, [[1], []], entry=3)[0] == 5
    assert call_idom(dominance, [[1], []], [0, 1], work=1)[0] == 3
    assert call_idom(dominance, [[1], []], [0, 1], capacity=1)[0] == 2
    assert call_idom(dominance, [[1], []], [1, 0])[0] == 5
    chain = [[index + 1] for index in range(59999)] + [[]]
    status, _, order, _ = call(kernel, chain, work=2000000)
    assert status == 0 and len(order) == 60000 and order[0] == 0 and order[-1] == 59999
    status, parents = call_idom(dominance, chain, order, work=2000000)
    assert status == 0 and parents[0] == 0 and parents[-1] == 59998
    randomizer = random.Random(0x349f)
    for index in range(args.cases):
        count = randomizer.randrange(1, 40)
        entry = randomizer.randrange(count)
        graph = [[randomizer.randrange(count)
                  for _ in range(randomizer.randrange(0, 5))]
                 for _ in range(count)]
        if index % 2 == 0:
            graph = [[target for target in targets if target != entry] for targets in graph]
        status, source, order, consumed = call(kernel, graph, entry)
        expected = reference(graph, entry)
        assert (status, source, order) == expected, (index, graph, entry, status, order, expected)
        assert consumed <= 100000
        if status == 0 and index % 4 == 0:
            actual = call_idom(dominance, graph, order, entry)
            oracle = reference_idom(graph, entry)
            assert actual == (0, oracle), (index, graph, entry, actual, oracle)
    print(f"Wave IR CFG: vectors, limits and {args.cases} graphs passed")


if __name__ == "__main__":
    main()
