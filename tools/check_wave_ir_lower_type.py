#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Independent raw TypeRef-to-canonical-IR/layout conformance for Wave O0 lowering."""
import argparse
import ctypes as C
import random

from check_wave_ir_layout import Storage, Node as LayoutNode, Layout


class Type(C.Structure):
    _fields_ = [(name, C.c_uint64) for name in ("kind", "child", "first", "count", "auxiliary")]


class Name(C.Structure):
    _fields_ = [("data", C.c_void_p), ("length", C.c_uint64)]


class Result(C.Structure):
    _fields_ = [(name, C.c_uint64) for name in ("status", "node", "expected", "actual", "annotation", "detail", "work")]


POINTER_NAMES = {"nodes", "edges", "names", "types", "type_edges", "type_table", "type_map",
                     "temporary", "name_marks", "storage", "layout_nodes", "layouts", "layout_offsets", "result"}


class Context(C.Structure):
    _fields_ = [(name, C.c_void_p if name in POINTER_NAMES else C.c_uint64) for name in (
        "nodes", "node_count", "edges", "edge_count", "names", "name_count", "types", "type_count", "type_capacity",
        "type_edges", "type_edge_count", "type_edge_capacity", "type_table", "type_table_capacity", "type_map",
        "temporary", "temporary_count", "temporary_capacity", "name_marks", "marker", "storage", "layout_nodes",
        "layouts", "layout_offsets", "annotation", "max_depth", "max_work", "work", "result")]


def call(kernel, nodes, edges=(), *, root=None, pointer=64, depth=32, work=1000000):
    root = len(nodes) - 1 if root is None else root
    raw = (C.c_uint64 * (len(nodes) * 12))(*[word for node in nodes for word in (*node, *([0] * (12 - len(node))))])
    links = (C.c_uint64 * len(edges))(*edges)
    names = (Name * 256)(*[Name(None, 2) for _ in range(256)])
    capacity = 17 + len(nodes) * 2 + 32
    edge_capacity = max(64, len(edges) * 4 + 16)
    type_nodes = (Type * capacity)()
    type_edges = (C.c_uint64 * edge_capacity)()
    table_capacity = 1 << ((capacity * 2 - 1).bit_length())
    table = (C.c_uint64 * table_capacity)()
    mapping = (C.c_uint64 * len(nodes))()
    temporary = (C.c_uint64 * (len(nodes) * 2 + 64))()
    marks = (C.c_uint64 * 256)()
    storage = Storage(pointer, pointer // 8, 1, 2, 4, 8, 16, 16, 16, 1)
    layout_nodes = (LayoutNode * capacity)()
    layouts = (Layout * capacity)()
    offsets = (C.c_uint64 * edge_capacity)()
    result = Result()
    p = Context(C.addressof(raw), len(nodes), C.addressof(links), len(edges), C.addressof(names), 256,
                C.addressof(type_nodes), 0, capacity, C.addressof(type_edges), 0, edge_capacity,
                C.addressof(table), table_capacity, C.addressof(mapping), C.addressof(temporary), 0,
                len(temporary), C.addressof(marks), 0, C.addressof(storage), C.addressof(layout_nodes),
                C.addressof(layouts), C.addressof(offsets), 0, depth, work, 0, C.addressof(result))
    output = C.c_uint64(0xdead)
    status = kernel(C.byref(p), root, C.byref(output))
    assert status == result.status and result.work <= work
    if status:
        assert output.value == 0xdead
        return status, None, None
    def shape(index):
        n = type_nodes[index]
        if n.kind <= 16:
            return n.kind
        if n.kind == 17:
            return (17, shape(n.child))
        if n.kind == 19:
            return (19, shape(n.child), n.auxiliary)
        if n.kind == 20:
            return (20, tuple(shape(type_edges[n.first + i]) for i in range(n.count)))
        return (18, shape(n.child), tuple(shape(type_edges[n.first + i]) for i in range(n.count)), n.auxiliary)
    layout = layouts[output.value]
    return 0, shape(output.value), (layout.size, layout.align, layout.allocation_align)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", required=True)
    parser.add_argument("--cases", type=int, default=10000)
    args = parser.parse_args()
    kernel = C.CDLL(args.library).whale_ir_lower_type
    kernel.argtypes = [C.POINTER(Context), C.c_uint64, C.POINTER(C.c_uint64)]
    kernel.restype = C.c_int32
    randomizer = random.Random(0x351)
    scalars = [(101, 1, 1)]
    for signed in (0, 1):
        for width, signed_tag in ((1, 2), (8, 4), (16, 5), (32, 6), (64, 7), (128, 8)):
            tag = signed_tag if signed else (3 if width == 1 else signed_tag + 5)
            scalars.append(((102, width, signed), tag, max(1, width // 8)))
    scalars = [((101,), 1, 1), *scalars[1:]] + [((103, width), tag, width // 8) for width, tag in ((16, 14), (32, 15), (64, 16))]
    for _ in range(args.cases):
        leaf, shape, size = randomizer.choice(scalars)
        pointer = randomizer.choice((32, 64))
        nodes = [leaf]
        expected_size = size
        align = size
        allocation = align
        for _ in range(randomizer.randrange(0, 10)):
            if randomizer.randrange(2):
                nodes.append((104, len(nodes) - 1)); shape = (17, shape)
                expected_size = align = allocation = pointer // 8
            else:
                length = randomizer.randrange(0, 6)
                nodes.append((106, len(nodes) - 1, length)); shape = (19, shape, length)
                expected_size *= length
                allocation = max(align, 16) if expected_size >= 16 else align
        assert call(kernel, nodes, pointer=pointer) == (0, shape, (expected_size, align, allocation))
    # Independent field-name validation precedes nested type mapping. A nested
    # struct reusing x must not hide a duplicate x in its parent.
    nodes = [(102, 32, 1), (108, 0, 0), (107, 0, 1), (104, 2), (108, 0, 3), (108, 0, 0), (107, 1, 2)]
    assert call(kernel, nodes, [1, 4, 5])[0] == 18
    assert call(kernel, [(102, 7, 1)])[0] == 5
    assert call(kernel, [(104, 0)], depth=16)[0] == 2
    assert call(kernel, [(102, 32, 1)], work=1)[0] == 1
    assert call(kernel, [(102, 32, 1)], root=9)[0] == 3
    print(f"Wave O0 type conversion: {args.cases} raw structural/layout cases, duplicate names and limits passed")


if __name__ == "__main__":
    main()
