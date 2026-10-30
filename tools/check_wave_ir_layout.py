#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Run the real Wave IR layout kernel against independent storage vectors."""

import argparse
import ctypes
import random


class Node(ctypes.Structure):
    _fields_ = [(name, ctypes.c_uint64) for name in ("kind", "child", "first", "count")]


class Storage(ctypes.Structure):
    _fields_ = [(name, ctypes.c_uint64) for name in (
        "ptr_bits", "pointer_align", "scalar_align1", "scalar_align2", "scalar_align4",
        "scalar_align8", "scalar_align16", "large_array_threshold", "large_array_align",
        "has_large_array")]


class Layout(ctypes.Structure):
    _fields_ = [(name, ctypes.c_uint64) for name in (
        "size", "align", "stride", "allocation_align", "depth")]


class Result(ctypes.Structure):
    _fields_ = [("status", ctypes.c_uint64), ("index", ctypes.c_uint64)]


def call(kernel, nodes, edges=(), root=None, depth=128, work=None, storage=None):
    if root is None:
        root = len(nodes) - 1
    if work is None:
        work = max(1024, 64 * (len(nodes) + len(edges)))
    if storage is None:
        storage = Storage(64, 8, 1, 2, 4, 8, 16, 16, 16, 1)
    node_array = (Node * len(nodes))(*nodes)
    edge_array = (ctypes.c_uint64 * len(edges))(*edges)
    scratch = (Layout * len(nodes))()
    output = (Layout * len(nodes))(*[Layout(0xdead, 0xbeef, 3, 5, 7) for _ in nodes])
    scratch_offsets = (ctypes.c_uint64 * len(edges))()
    output_offsets = (ctypes.c_uint64 * len(edges))(*([0xdead] * len(edges)))
    result = Result()
    status = kernel(ctypes.byref(storage), node_array, len(nodes), edge_array, len(edges), root,
                    depth, work, scratch, len(nodes), scratch_offsets, len(edges),
                    output, len(nodes), output_offsets, len(edges), ctypes.byref(result))
    assert status == result.status, (status, result.status)
    if status:
        assert all(item.size == 0xdead and item.align == 0xbeef for item in output)
        assert all(offset == 0xdead for offset in output_offsets)
    return status, [tuple(getattr(item, name) for name, _ in Layout._fields_) for item in output], list(output_offsets)


def align_up(value, align):
    return (value + align - 1) & -align


def reference(nodes, edges, storage, max_depth=128):
    layouts = []
    offsets = [0] * len(edges)
    limit = (1 << storage.ptr_bits) - 1
    scalar = {1: (1, storage.scalar_align1), 2: (2, storage.scalar_align2),
              3: (4, storage.scalar_align4), 4: (8, storage.scalar_align8),
              5: (16, storage.scalar_align16), 6: (storage.ptr_bits // 8, storage.pointer_align),
              7: (storage.ptr_bits // 8, storage.pointer_align)}
    for index, node in enumerate(nodes):
        if node.kind == 0:
            current = (0, 0, 0, 0, 0)
        elif node.kind in scalar:
            size, align = scalar[node.kind]
            current = (size, align, 0, align, 0)
        elif node.kind == 8:
            if node.child >= index:
                raise ValueError("graph")
            child_size, align, _, _, child_depth = layouts[node.child]
            if not align:
                raise ValueError("void")
            if child_depth >= max_depth:
                raise ValueError("depth")
            size = child_size * node.count
            allocation_align = align
            if storage.has_large_array and size >= storage.large_array_threshold:
                allocation_align = max(align, storage.large_array_align)
            current = (size, align, child_size, allocation_align, child_depth + 1)
        elif node.kind in (9, 10):
            size, align, depth = 0, 1, 1
            for position in range(node.first, node.first + node.count):
                child = edges[position]
                if child >= index or layouts[child][1] == 0:
                    raise ValueError("graph")
                child_size, child_align, _, _, child_depth = layouts[child]
                if child_depth >= max_depth:
                    raise ValueError("depth")
                size = align_up(size, child_align)
                offsets[position] = size
                size += child_size
                align = max(align, child_align)
                depth = max(depth, child_depth + 1)
            current = (align_up(size, align), align, 0, align, depth)
        else:
            raise ValueError("kind")
        if current[0] > limit:
            raise OverflowError("size")
        layouts.append(current)
    return layouts, offsets


def exercise(kernel, seed, cases):
    storage = Storage(64, 8, 1, 2, 4, 8, 16, 16, 16, 1)
    nodes = [Node(1, 0, 0, 0), Node(4, 0, 0, 0), Node(2, 0, 0, 0),
             Node(9, 0, 0, 3), Node(8, 3, 0, 3), Node(3, 0, 0, 0),
             Node(10, 0, 3, 3)]
    edges = [0, 1, 2, 0, 4, 5]
    status, layouts, offsets = call(kernel, nodes, edges)
    assert status == 0 and layouts[6][:2] == (88, 8), (status, layouts)
    assert offsets == [0, 8, 16, 0, 8, 80], offsets
    assert call(kernel, nodes, edges, work=39)[0] == 6
    assert call(kernel, [Node(1, 0, 0, 0), Node(9, 0, 0, 10)], [0] * 10, work=36)[0] == 6
    assert call(kernel, [Node(1, 0, 0, 0), Node(8, 0, 0, (1 << 64) - 1),
                         Node(9, 0, 0, 2)], [1, 0])[0] == 3
    assert call(kernel, [Node(0, 0, 0, 0), Node(6, 0, 0, 0)])[0] == 0
    assert call(kernel, [Node(6, 9, 0, 0)])[0] == 1
    assert call(kernel, [Node(1, 0, 0, 0), Node(8, 0, 0, 1)], depth=0)[0] == 2
    assert call(kernel, [Node(9, 0, 0, 0)])[1][0][:2] == (0, 1)
    assert call(kernel, [Node(9, 0, 0, 0), Node(8, 0, 0, (1 << 64) - 1)])[1][1][:3] == (0, 1, 0)
    storage32 = Storage(32, 4, 1, 2, 4, 8, 16, 16, 16, 1)
    assert call(kernel, [Node(4, 0, 0, 0), Node(8, 0, 0, 1 << 32)], storage=storage32)[0] == 3
    invalid = Storage(64, 3, 1, 2, 4, 8, 16, 16, 16, 1)
    assert call(kernel, [Node(1, 0, 0, 0)], storage=invalid)[0] == 5
    rng = random.Random(seed)
    for case in range(cases):
        nodes = []
        edges = []
        for index in range(rng.randrange(1, 24)):
            kind = rng.choice((1, 2, 3, 4, 5, 6, 8, 9, 10)) if index else rng.randrange(1, 6)
            if kind == 8:
                nodes.append(Node(kind, rng.randrange(index), 0, rng.randrange(0, 9)))
            elif kind in (9, 10):
                first = len(edges)
                count = rng.randrange(0, 5)
                edges.extend(rng.randrange(index) for _ in range(count))
                nodes.append(Node(kind, 0, first, count))
            else:
                nodes.append(Node(kind, 0, 0, 0))
        try:
            expected, expected_offsets = reference(nodes, edges, storage)
        except OverflowError:
            assert call(kernel, nodes, edges)[0] == 3, case
        else:
            status, actual, actual_offsets = call(kernel, nodes, edges)
            assert status == 0 and actual[-1] == expected[-1], (case, status, actual[-1], expected[-1])
            root = nodes[-1]
            if root.kind in (9, 10):
                span = slice(root.first, root.first + root.count)
                assert actual_offsets[span] == expected_offsets[span], case


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", required=True)
    parser.add_argument("--seed", type=int, default=149)
    parser.add_argument("--cases", type=int, default=2000)
    args = parser.parse_args()
    library = ctypes.CDLL(args.library)
    kernel = library.whale_ir_layout
    kernel.restype = ctypes.c_int
    kernel.argtypes = [ctypes.POINTER(Storage), ctypes.POINTER(Node), ctypes.c_uint64,
                       ctypes.POINTER(ctypes.c_uint64), ctypes.c_uint64, ctypes.c_uint64,
                       ctypes.c_uint64, ctypes.c_uint64, ctypes.POINTER(Layout), ctypes.c_uint64,
                       ctypes.POINTER(ctypes.c_uint64), ctypes.c_uint64, ctypes.POINTER(Layout),
                       ctypes.c_uint64, ctypes.POINTER(ctypes.c_uint64), ctypes.c_uint64,
                       ctypes.POINTER(Result)]
    exercise(kernel, args.seed, args.cases)
    print(f"Wave IR layout: {args.cases} independent cases passed")


if __name__ == "__main__":
    main()
