#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Independent exact-type and transactional builder vectors for the Wave IR module."""
import argparse
import ctypes as c
import random


class Node(c.Structure):
    _fields_ = [(key, c.c_uint64) for key in ("kind", "depth", "children", "auxiliary")]


class Value(c.Structure):
    _fields_ = [(key, c.c_uint64) for key in ("id", "first_type", "type_count")]


class Block(c.Structure):
    _fields_ = [(key, c.c_uint64) for key in ("id", "terminated", "non_phi")]


class State(c.Structure):
    _fields_ = [(key, c.c_uint64) for key in
               ("next_value", "next_block", "type_count", "instruction_count", "namespace_nodes",
                "namespace_bytes", "next_function", "next_global", "work")]


class Namespace(c.Structure):
    _fields_ = [("kind", c.c_uint64), ("id", c.c_uint64), ("flags", c.c_uint64),
                ("name", c.c_void_p), ("name_count", c.c_uint64),
                ("link", c.c_void_p), ("link_count", c.c_uint64),
                ("types", c.c_void_p), ("type_count", c.c_uint64),
                ("defined", c.c_uint64), ("align", c.c_uint64),
                ("initializer", c.c_uint64), ("low", c.c_uint64), ("high", c.c_uint64)]


class Result(c.Structure):
    _fields_ = [(key, c.c_uint64) for key in ("status", "detail", "value", "work")]


def reference(nodes, depth=128, limit=1000000, work=1000000):
    if not nodes:
        return 1
    if len(nodes) > limit:
        return 3
    if len(nodes) > work:
        return 4
    pending = [0]
    for index, (kind, actual_depth, children, auxiliary) in enumerate(nodes):
        if not pending or actual_depth != pending.pop():
            return 1
        if actual_depth > min(depth, 256):
            return 2
        if kind > 21 or (kind <= 16 and children):
            return 1
        if kind in (17, 19) and children != 1:
            return 1
        if kind == 18 and (children == 0 or auxiliary > 3):
            return 1
        if children > len(nodes) - index - 1:
            return 1
        if children and actual_depth == min(depth, 256):
            return 2
        pending.extend([actual_depth + 1] * children)
    return 1 if pending else 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", required=True)
    parser.add_argument("--cases", type=int, default=30000)
    args = parser.parse_args()
    library = c.CDLL(args.library)
    check = library.whale_ir_builder_type
    check.argtypes = [c.c_void_p, c.c_uint64, c.c_uint64, c.c_uint64, c.c_uint64, c.POINTER(Result)]
    allocate = library.whale_ir_builder_allocate
    allocate.argtypes = [c.c_void_p, c.c_uint64, c.c_void_p] + [c.c_uint64] * 5 + [c.c_void_p, c.c_uint64] * 3 + [c.POINTER(Result)]
    order = library.whale_ir_builder_order
    order.argtypes = [c.c_void_p, c.c_uint64, c.c_void_p] + [c.c_uint64] * 6 + [c.c_void_p, c.c_uint64, c.c_void_p] + [c.c_uint64] * 3 + [c.POINTER(Result)]

    def check_types(nodes, depth=128, limit=1000000, work=1000000):
        array = (Node * len(nodes))(*(Node(*node) for node in nodes))
        result = Result()
        status = check(array, len(nodes), depth, limit, work, c.byref(result))
        assert status == result.status
        return status

    seeds = [[(kind, 0, 0, 0)] for kind in range(17)]
    seeds += [[(17, 0, 1, 0), (6, 1, 0, 0)],
              [(20, 0, 3, 0), (21, 1, 0, 0), (19, 1, 1, 7), (7, 2, 0, 0), (1, 1, 0, 0)],
              [(18, 0, 3, 3), (0, 1, 0, 0), (17, 1, 1, 0), (9, 2, 0, 0), (16, 1, 0, 0)],
              [], [(22, 0, 0, 0)], [(17, 0, 0, 0)], [(6, 0, 1, 0)],
              [(20, 0, 1, 0)], [(18, 0, 1, 4), (0, 1, 0, 0)]]
    rng = random.Random(348)

    def tree(depth=0):
        kind = rng.choice(list(range(17)) + [17, 18, 19, 20, 21]) if depth < 7 else rng.randrange(17)
        count = 1 if kind in (17, 19) else rng.randrange(1, 5) if kind == 18 else rng.randrange(5) if kind >= 20 else 0
        nodes = [(kind, depth, count, rng.randrange(4) if kind == 18 else rng.randrange(100) if kind == 19 else 0)]
        for _ in range(count):
            nodes.extend(tree(depth + 1))
        return nodes

    for nodes in seeds:
        assert check_types(nodes) == reference(nodes), nodes
    for _ in range(args.cases):
        nodes = tree()
        if rng.randrange(2):
            index = rng.randrange(len(nodes))
            node = list(nodes[index])
            field = rng.randrange(4)
            node[field] = rng.randrange(30)
            nodes[index] = tuple(node)
        depth = rng.randrange(10)
        # Invalid over-wide child lists are detected before following children.
        expected = reference(nodes, depth)
        actual = check_types(nodes, depth)
        assert actual == expected, (nodes, depth, expected, actual)
    assert check_types([(6, 0, 0, 0)], limit=0) == 3
    assert check_types([(6, 0, 0, 0)], work=0) == 4
    deep = [(17, depth, 1, 0) for depth in range(256)] + [(6, 256, 0, 0)]
    assert check_types(deep, depth=256) == 0
    assert check_types(deep, depth=255) == 2

    capacity = 60001
    types = (Node * capacity)()
    values = (Value * capacity)()
    blocks = (Block * capacity)()
    state = State()
    scalar = (Node * 1)(Node(6, 0, 0, 0))

    def reserve(kind, nodes=scalar, mode=1, node_limit=1000000, work=1000000,
                type_capacity=capacity, value_capacity=capacity, block_capacity=capacity):
        before = bytes(state), bytes(types[:1][0]), bytes(values[:1][0]), bytes(blocks[:1][0])
        result = Result()
        status = allocate(c.byref(state), kind, nodes, len(nodes), 128, node_limit, work, mode,
                          types, type_capacity, values, value_capacity, blocks, block_capacity, c.byref(result))
        assert status == result.status
        if status or mode == 0:
            assert before == (bytes(state), bytes(types[:1][0]), bytes(values[:1][0]), bytes(blocks[:1][0]))
        return status, result.value

    assert reserve(1) == (0, 0)
    assert reserve(0, mode=0) == (0, 0)
    assert reserve(0, type_capacity=0)[0] == 1
    assert reserve(0, node_limit=2)[0] == 3
    assert reserve(0, work=3)[0] == 4
    assert reserve(0) == (0, 0)

    def operation(op, operands=(), targets=(), query=0, first_block=0, first_value=0,
                  block=0, limit=1000000, work=1000000):
        before = bytes(state), bytes(blocks[block])
        operands = (c.c_uint64 * len(operands))(*operands)
        targets = (c.c_uint64 * len(targets))(*targets)
        result = Result()
        status = order(c.byref(state), limit, blocks, state.next_block, first_block, first_value,
                       state.next_value, block, op, operands, len(operands), targets, len(targets),
                       query, work, c.byref(result))
        assert status == result.status
        if status or query:
            assert before == (bytes(state), bytes(blocks[block]))
        return status

    assert operation(15, (0,), (0,), query=1) == 0
    assert operation(4, query=1) == 0
    assert operation(4) == 0
    assert operation(15, (0,), (0,)) == 9
    assert operation(7, (0,)) == 1
    assert operation(7, (0, 1)) == 8
    assert operation(31, targets=(1,)) == 6
    assert operation(7, (0, 0), first_value=1) == 8
    assert operation(4, limit=4) == 3
    assert operation(33, (0,)) == 0
    assert operation(4) == 7
    assert operation(35, query=1, work=state.work) == 7
    assert operation(34, query=1, work=state.work) == 0
    assert operation(36) == 0
    assert blocks[0].terminated == 1
    # Allocation stress uses fixed-size data and should remain linear at O0/O2.
    for index in range(1, 60000):
        assert reserve(0) == (0, index)
    assert state.next_value == 60000
    assert values[59999].first_type == 59999
    assert types[59999].kind == 6
    # Corrupt identity counters must never wrap or publish output.
    state.next_value = 0xffffffff
    assert reserve(0, node_limit=0xffffffffffffffff)[0] == 5
    namespace = library.whale_ir_builder_namespace
    namespace.argtypes = [c.c_void_p, c.c_void_p, c.c_uint64, c.c_void_p] + [c.c_uint64] * 6 + [c.c_void_p, c.c_uint64, c.POINTER(Result)]
    namespace_state = State()
    entries = (Namespace * 128)()
    payloads = []
    count = 0

    def entry(kind, name, nodes, link=None, readonly=False, align=4, literal=0, low=0, high=0,
              legacy=False, limit=1000000, byte_limit=1000000):
        nonlocal count
        name_raw = name.encode()
        link_raw = None if link is None else link.encode()
        name_buffer = c.create_string_buffer(name_raw)
        link_buffer = None if link_raw is None else c.create_string_buffer(link_raw)
        types = (Node * len(nodes))(*(Node(*node) for node in nodes))
        candidate = Namespace(kind, 0, (3 if link is not None else 0) | (4 if readonly else 0),
                              c.cast(name_buffer, c.c_void_p), len(name_raw),
                              None if link_buffer is None else c.cast(link_buffer, c.c_void_p),
                              0 if link_raw is None else len(link_raw), c.cast(types, c.c_void_p),
                              len(nodes), 0, align, literal, low, high)
        before = bytes(namespace_state), bytes(entries)
        result = Result()
        status = namespace(c.byref(namespace_state), entries, count, c.byref(candidate), 0,
                           int(legacy), 128, limit, byte_limit, 100000000, None, 0, c.byref(result))
        assert status == result.status
        assert before == (bytes(namespace_state), bytes(entries))
        if status:
            return status, result.value
        status = namespace(c.byref(namespace_state), entries, count, c.byref(candidate), 1,
                           int(legacy), 128, limit, byte_limit, 100000000, entries, 128, c.byref(result))
        assert status == result.status == 0
        if result.detail == 0:
            payloads.append((name_buffer, link_buffer, types))
            count += 1
        return status, result.value

    def signature(parameter=6, convention=0):
        return [(18, 0, 2, convention), (0, 1, 0, 0), (parameter, 1, 0, 0)]

    assert entry(0, "foo", signature(), limit=2)[0] == 3
    assert entry(0, "foo", signature(), byte_limit=2)[0] == 19
    assert entry(0, "foo", signature()) == (0, 0)
    assert entry(0, "foo", signature()) == (0, 0)
    assert entry(0, "foo", signature(7))[0] == 12
    assert entry(0, "bar", signature(), link="symbol") == (0, 1)
    assert entry(0, "baz", signature(), link="symbol")[0] == 11
    assert entry(0, "bad", signature(0))[0] == 17
    assert entry(0, "bad", signature(convention=2))[0] == 17
    assert entry(0, "bad", signature(), link="a\0b")[0] == 16
    assert entry(2, "data", [(6, 0, 0, 0)], literal=1, low=7) == (0, 0)
    assert entry(1, "constant", [(6, 0, 0, 0)], legacy=True) == (0, 1)
    assert entry(2, "constant", [(6, 0, 0, 0)])[0] == 10
    assert entry(2, "other", [(6, 0, 0, 0)], link="symbol")[0] == 11
    assert entry(2, "unaligned", [(6, 0, 0, 0)], align=3)[0] == 25
    assert entry(2, "readonly", [(6, 0, 0, 0)], readonly=True)[0] == 26
    assert entry(2, "range", [(2, 0, 0, 0)], literal=1, low=1)[0] == 27
    assert entry(2, "negative", [(2, 0, 0, 0)], literal=1,
                 low=0xffffffffffffffff, high=0xffffffffffffffff) == (0, 2)
    reference_functions = {}
    for _ in range(3000):
        name = "generated_" + str(rng.randrange(20))
        parameter = rng.randrange(1, 17)
        if name in reference_functions and reference_functions[name][0] != parameter:
            assert entry(0, name, signature(parameter))[0] == 12
        else:
            status, identity = entry(0, name, signature(parameter))
            assert status == 0
            if name in reference_functions:
                assert identity == reference_functions[name][1]
            reference_functions[name] = parameter, identity
    assert namespace_state.next_function == 22
    assert namespace_state.next_global == 3
    assert count == 25
    print(f"Wave IR builder: {args.cases} differential type cases, transaction negatives, 3000 namespace cases and 60000 allocations passed")


if __name__ == "__main__":
    main()
