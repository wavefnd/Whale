#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Independent preserved expression-tree oracle for actual Wave grammar."""

import argparse
import ctypes
import random
from check_wave_ir_type import Token, Node as TypeNode, LEAVES, MARKS, generated as generated_type


class Expression(ctypes.Structure):
    _fields_ = [(name, ctypes.c_uint64) for name in
                ("kind", "op", "type_first", "type_count", "low", "high")]


class Result(ctypes.Structure):
    _fields_ = [(name, ctypes.c_uint64) for name in
                ("status", "reason", "at", "expected", "detail", "next",
                 "expressions", "types", "spent", "work")]


BINARY = "add sub mul bit_and bit_or bit_xor div rem shift_left shift_right logical_and logical_or".split()
COMPARE = "eq ne lt le gt ge".split()


def generated(randomizer, depth):
    if depth == 0 or randomizer.randrange(3) == 0:
        if randomizer.randrange(4) == 0:
            words, nodes = generated_type(randomizer, 3)
            identity = randomizer.randrange(1 << 32)
            kind = randomizer.choice((1, 2))
            return ("ref", kind, identity, words, nodes)
        kind = randomizer.randrange(1, 17)
        if kind == 1:
            bits = randomizer.randrange(2)
            spelling = "true" if bits else "false"
        elif kind >= 14:
            width = (16, 32, 64)[kind - 14]
            bits = randomizer.getrandbits(width)
            spelling = "0x" + format(bits, f"0{width // 4}x")
        else:
            width = {2: 1, 3: 1, 4: 8, 5: 16, 6: 32, 7: 64, 8: 128,
                     9: 8, 10: 16, 11: 32, 12: 64, 13: 128}[kind]
            value = randomizer.getrandbits(width)
            if kind == 2 or 4 <= kind <= 8:
                if value & (1 << (width - 1)):
                    value -= 1 << width
            spelling = str(value)
            bits = value & ((1 << 128) - 1)
        return ("literal", kind, spelling, bits)
    choice = randomizer.randrange(4)
    if choice < 2:
        return ("unary", choice, generated(randomizer, depth - 1))
    return ("binary" if choice == 2 else "compare",
            randomizer.randrange(12 if choice == 2 else 6),
            generated(randomizer, depth - 1), generated(randomizer, depth - 1))


def flatten(tree, types, expressions):
    kind = tree[0]
    if kind == "literal":
        _, leaf, spelling, bits = tree
        meta = (len(types), 1)
        types.append((leaf, 0, 0))
        expressions.append((0, 0, *meta, bits & ((1 << 64) - 1), bits >> 64))
        return [LEAVES[leaf], spelling], meta, 2
    if kind == "ref":
        _, category, identity, words, nodes = tree
        meta = (len(types), len(nodes))
        types.extend(nodes)
        expressions.append((category, identity, *meta, 0, 0))
        return [*words, ("@g" if category == 1 else "%v") + str(identity)], meta, 1 + len(nodes)
    left_words, meta, spent = flatten(tree[2], types, expressions)
    if kind == "unary":
        expressions.append((3 + tree[1], 0, *meta, 0, 0))
        return ["bit_not" if tree[1] == 0 else "neg", "(", *left_words, ")"], meta, spent + 1
    right_words, _, right_spent = flatten(tree[3], types, expressions)
    if kind == "compare":
        meta = (len(types), 1)
        types.append((1, 0, 0))
    expressions.append((5 if kind == "binary" else 6, tree[1], *meta, 0, 0))
    op = (BINARY if kind == "binary" else COMPARE)[tree[1]]
    return [op, "(", *left_words, ",", *right_words, ")"], meta, spent + right_spent + 1


def call(kernel, words, *, mode=1, max_depth=128, max_type_depth=128,
         max_nodes=1000000, max_work=16000000, capacity=None, output_capacity=None, version=8):
    capacity = capacity if capacity is not None else len(words) + 1
    output_capacity = output_capacity if output_capacity is not None else capacity
    buffers = [ctypes.create_string_buffer(word.encode()) for word in words]
    tokens = (Token * (len(words) + 1))(*[
        Token(ctypes.cast(buffer, ctypes.c_void_p), len(word.encode()), 2 if word in MARKS else 0)
        for buffer, word in zip(buffers, words)], Token(None, 0, 3))
    type_scratch = (TypeNode * capacity)()
    expression_scratch = (Expression * capacity)()
    types = (TypeNode * output_capacity)(*[TypeNode(99, 99, 99) for _ in range(output_capacity)])
    expressions = (Expression * output_capacity)(*[Expression(99, 99, 99, 99, 99, 99) for _ in range(output_capacity)])
    result = Result()
    status = kernel(tokens, len(tokens), 0, 0, version, max_depth, max_type_depth,
                    max_nodes, max_work, mode, type_scratch, capacity, expression_scratch, capacity,
                    types, output_capacity, expressions, output_capacity, ctypes.byref(result))
    assert status == result.status
    if status or mode == 0:
        assert all(node.kind == 99 for node in types)
        assert all(node.kind == 99 for node in expressions)
        return status, result, [], []
    return status, result, [(n.kind, n.value, n.count) for n in types[:result.types]], [
        (n.kind, n.op, n.type_first, n.type_count, n.low, n.high) for n in expressions[:result.expressions]]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", required=True)
    parser.add_argument("--cases", type=int, default=10000)
    args = parser.parse_args()
    kernel = ctypes.CDLL(args.library).whale_ir_parse_expression
    kernel.argtypes = [ctypes.c_void_p, *([ctypes.c_uint64] * 9),
                       ctypes.c_void_p, ctypes.c_uint64, ctypes.c_void_p, ctypes.c_uint64,
                       ctypes.c_void_p, ctypes.c_uint64, ctypes.c_void_p, ctypes.c_uint64,
                       ctypes.POINTER(Result)]
    assert call(kernel, ["neg", "(", "i32", "1", ")"], version=7)[1].reason == 3
    assert call(kernel, ["bit_not", "(", "i32", "1", ")"], version=4)[1].reason == 2
    assert call(kernel, ["add", "(", "i32", "1", ",", "i32", "2", ")"], version=3)[0] == 0
    assert call(kernel, ["i32", "@g4294967296"])[1].reason == 5
    assert call(kernel, ["i32", "@g4294967296x"])[1].reason == 4
    assert call(kernel, ["i32", "1"], max_nodes=0)[0] == 3
    assert call(kernel, ["i32", "1"], max_work=0)[0] == 4
    assert call(kernel, ["i32", "1"], output_capacity=0)[0] == 5
    assert call(kernel, ["ptr", "<", "u8", ">", "@g0"], max_type_depth=0)[1].reason == 2
    deep = ["neg", "("] * 257 + ["i32", "1"] + [")"] * 257
    assert call(kernel, deep, max_depth=256)[0] == 2
    randomizer = random.Random(0x3490352)
    for case in range(args.cases):
        tree = generated(randomizer, 6)
        expected_types, expected_expressions = [], []
        words, _, spent = flatten(tree, expected_types, expected_expressions)
        status, result, types, expressions = call(kernel, words)
        assert status == 0 and result.next == len(words) and result.spent == spent, (case, words, status, result.reason, result.at)
        assert types == expected_types and expressions == expected_expressions, (case, words, types, expected_types, expressions, expected_expressions)
        pre_status, pre, _, _ = call(kernel, words, mode=0)
        assert pre_status == 0 and (pre.types, pre.expressions, pre.spent, pre.work) == (result.types, result.expressions, result.spent, result.work)
        assert call(kernel, words[:-1])[0] != 0
        assert call(kernel, words, max_work=result.work - 1)[0] != 0
    print(f"Wave IR expression parser: versions, bounds, exact postorder and {args.cases} independent trees passed")


if __name__ == "__main__":
    main()
