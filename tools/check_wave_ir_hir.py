#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Independent conformance vectors for the real Wave resolved-HIR adapter."""
import argparse
import ctypes as C
import random


class Node(C.Structure):
    _fields_ = [("words", C.c_uint64 * 8)]


class Pair(C.Structure):
    _fields_ = [("a", C.c_uint64), ("b", C.c_uint64)]


class Result(C.Structure):
    _fields_ = [(name, C.c_uint64) for name in ("status", "node", "detail", "work")]


def call(kernel, nodes, edges, *, work=100000, depth=32):
    records = (Node * len(nodes))(*[Node((C.c_uint64 * 8)(*node, *([0] * (8 - len(node)))))
                                   for node in nodes])
    links = (C.c_uint64 * len(edges))(*edges)
    output = (Pair * len(nodes))()
    bindings = (Pair * len(nodes))()
    table = (Pair * 64)()
    result = Result()
    status = kernel(records, len(nodes), links, len(edges), len(edges) - 1, 1,
                    output, bindings, table, 64, depth, work, C.byref(result))
    assert status == result.status and result.work <= work
    return status, result, [(entry.a, entry.b) for entry in output]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", required=True)
    parser.add_argument("--cases", type=int, default=10000)
    args = parser.parse_args()
    kernel = C.CDLL(args.library).whale_ir_adapt_wave_hir
    kernel.argtypes = [C.POINTER(Node), C.c_uint64, C.POINTER(C.c_uint64), C.c_uint64,
                       C.c_uint64, C.c_uint64, C.POINTER(Pair), C.POINTER(Pair),
                       C.POINTER(Pair), C.c_uint64, C.c_uint64, C.c_uint64, C.POINTER(Result)]
    kernel.restype = C.c_int32
    randomizer = random.Random(0x149)
    widths = (1, 8, 16, 32, 64, 128)
    target_ops = (0, 1, 2, 6, 7, 3, 4, 5, 8, 9, 10, 11, 32, 33, 34, 35, 36, 37)
    for index in range(args.cases):
        width = randomizer.choice(widths)
        signed = randomizer.randrange(2)
        operation = randomizer.randrange(18)
        boolean = operation in (10, 11)
        compare = operation >= 12
        maximum = (1 << (width - signed)) - 1
        minimum = -(1 << (width - 1)) if signed else 0
        a = randomizer.randint(minimum, maximum)
        b = randomizer.randint(minimum, maximum)
        left = C.create_string_buffer(str(a).encode())
        right = C.create_string_buffer(str(b).encode())
        # Different type records with equivalent resolved facts prove that the
        # adapter uses semantic facts, rather than relying on transport identity.
        operand = (2, 1) if boolean else (2, 2, width, signed)
        result_type = (2, 1) if compare or boolean else operand
        nodes = [operand, operand, result_type,
                 (4, 0, 0, a & 1) if boolean else (3, 0, 0, C.addressof(left), len(str(a))),
                 (4, 1, 1, b & 1) if boolean else (3, 1, 1, C.addressof(right), len(str(b))),
                 (6, 2, 2, operation, 3, 4), (11, 5), (0, 0, 0, 0, 2, 0, 1, 0)]
        edges = [6, 7]
        status, _, output = call(kernel, nodes, edges)
        assert status == 0, (index, width, signed, operation, status)
        assert output[5][1] == target_ops[operation]
        # A contradictory return fact must be rejected, even for dead source.
        bad = list(nodes)
        bad[7] = (0, 0, 0, 0, 1 if compare or boolean else 2, 0, 1, 0)
        if compare or boolean:
            # Keep the RHS operand scalar facts intact by adding a fresh return.
            bad[7] = (0, 0, 0, 0, 8, 0, 1, 0)
            bad.append((2, 2, 8, 1))
        else:
            bad[2] = (2, 1)
        assert call(kernel, bad, edges)[0] == 4, (index, operation, call(kernel,bad,edges)[0], bad)
        bad = list(nodes)
        bad[4] = tuple([nodes[4][0], 0, *nodes[4][2:]])
        assert call(kernel, bad, edges)[0] == 6
        assert call(kernel, nodes, edges, work=1)[0] == 1
    for category in (1, 2, 3, 4):
        nodes = [(2, 0), (12, category), (0, 0, 0, 0, 0, 0, 1, 0)]
        status, result, _ = call(kernel, nodes, [1, 2])
        assert status == 5 and result.detail == category
    print(f"Wave typed-HIR adapter: {args.cases} resolved scalar/operator vectors, contradictory facts, duplicate identities and limits passed")


if __name__ == "__main__":
    main()
