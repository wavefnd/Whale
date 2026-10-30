#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Independent generated type trees and bounded native Wave parser checks."""

import argparse
import ctypes
import random


class Token(ctypes.Structure):
    _fields_ = [("data", ctypes.c_void_p), ("length", ctypes.c_uint64),
                ("kind", ctypes.c_uint64)]


class Node(ctypes.Structure):
    _fields_ = [(name, ctypes.c_uint64) for name in ("kind", "value", "count")]


class Result(ctypes.Structure):
    _fields_ = [(name, ctypes.c_uint64) for name in
                ("status", "reason", "at", "expected", "next", "nodes", "work")]


LEAVES = "void bool i1 u1 i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f16 f32 f64".split()
MARKS = {"<", ">", "{", "}", "(", ")", ",", "->"}


def call(kernel, words, *, mode=1, capacity=None, output_capacity=None,
         max_depth=128, max_nodes=100000, max_work=1000000):
    if capacity is None:
        capacity = len(words)
    if output_capacity is None:
        output_capacity = capacity
    buffers = [ctypes.create_string_buffer(word.encode()) for word in words]
    raw = (Token * (len(words) + 1))(*[
        Token(ctypes.cast(buffer, ctypes.c_void_p), len(word.encode()),
              2 if word in MARKS else 0)
        for buffer, word in zip(buffers, words)], Token(None, 0, 3))
    scratch = (Node * capacity)()
    output = (Node * output_capacity)(*[Node(99, 99, 99) for _ in range(output_capacity)])
    result = Result()
    status = kernel(raw, len(raw), 0, 0, max_depth, max_nodes, max_work,
                    mode, scratch, capacity, output, output_capacity, ctypes.byref(result))
    assert status == result.status
    if status or mode == 0:
        assert all((node.kind, node.value, node.count) == (99, 99, 99) for node in output)
    nodes = [(node.kind, node.value, node.count) for node in output[:result.nodes]] if status == 0 and mode == 1 else []
    return status, result, nodes


def generated(randomizer, depth):
    kind = randomizer.randrange(17 if depth == 0 else 22)
    if kind < 17:
        return [LEAVES[kind]], [(kind, 0, 0)]
    if kind == 17:
        words, nodes = generated(randomizer, depth - 1)
        return ["ptr", "<", *words, ">"], [*nodes, (kind, 0, 1)]
    if kind == 19:
        words, nodes = generated(randomizer, depth - 1)
        count = randomizer.randrange(1 << 64)
        return ["array", "<", *words, ",", str(count), ">"], [*nodes, (kind, count, 1)]
    fields = [generated(randomizer, depth - 1) for _ in range(randomizer.randrange(4))]
    words = []
    nodes = []
    for index, (field_words, field_nodes) in enumerate(fields):
        if index:
            words.append(",")
        words.extend(field_words)
        nodes.extend(field_nodes)
    if kind == 18:
        ret_words, ret_nodes = generated(randomizer, depth - 1)
        convention = randomizer.randrange(2)
        return (["fnptr", "<", "whale" if convention == 0 else "sysv64", "(",
                 *words, ")", "->", *ret_words, ">"],
                [*nodes, *ret_nodes, (kind, convention, len(fields))])
    opening, closing = ("{", "}") if kind == 20 else ("<", ">")
    return (["struct" if kind == 20 else "tuple", opening, *words, closing],
            [*nodes, (kind, 0, len(fields))])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", required=True)
    parser.add_argument("--cases", type=int, default=10000)
    args = parser.parse_args()
    kernel = ctypes.CDLL(args.library).whale_ir_parse_type
    kernel.argtypes = [ctypes.c_void_p, *([ctypes.c_uint64] * 7), ctypes.c_void_p,
                       ctypes.c_uint64, ctypes.c_void_p, ctypes.c_uint64,
                       ctypes.POINTER(Result)]
    for index, word in enumerate(LEAVES):
        assert call(kernel, [word])[2] == [(index, 0, 0)]
    for words, status in [
        (["ptr", "<", "i32"], 1), (["struct", "{", "i32", ",", "}"], 1),
        (["array", "<", "i8", ",", "18446744073709551616", ">"], 1),
        (["array", "<", "i8", ",", "18446744073709551616x", ">"], 1),
        (["fnptr", "<", "unknown", "(", ")", "->", "i32", ">"], 1),
    ]:
        assert call(kernel, words)[0] == status
    assert call(kernel, ["ptr", "<", "i32", ">"], max_depth=0)[0] == 2
    assert call(kernel, ["i32"], max_nodes=0)[0] == 3
    assert call(kernel, ["i32"], max_work=0)[0] == 4
    assert call(kernel, ["i32"], capacity=0)[0] == 5
    assert call(kernel, ["i32"], output_capacity=0)[0] == 5
    wide = ["struct", "{"]
    for index in range(60000):
        if index:
            wide.append(",")
        wide.append("i32")
    wide.append("}")
    status, result, nodes = call(kernel, wide)
    assert status == 0 and result.nodes == 60001 and nodes[-1] == (20, 0, 60000)
    deep = ["ptr", "<"] * 257 + ["i32"] + [">"] * 257
    assert call(kernel, deep, max_depth=256)[0] == 2
    randomizer = random.Random(0x3480349)
    for case in range(args.cases):
        words, expected = generated(randomizer, 5)
        status, result, actual = call(kernel, words)
        assert status == 0 and result.next == len(words) and actual == expected, (case, words, status, actual, expected)
        pre_status, pre, _ = call(kernel, words, mode=0)
        assert pre_status == 0 and pre.nodes == result.nodes and pre.work == result.work
        assert call(kernel, words, capacity=len(expected) - 1)[0] == 5
        assert call(kernel, words, max_work=result.work - 1)[0] == 4
    print(f"Wave IR type parser: full grammar, transactional limits, wide/deep cases and {args.cases} independent trees passed")


if __name__ == "__main__":
    main()
