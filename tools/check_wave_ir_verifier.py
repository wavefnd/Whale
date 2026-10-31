#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Check real Wave type/cast/literal/signature rules against independent oracles."""

import argparse
import ctypes as C
import random

from wave_ir_verifier_oracle import (FLOATS, INTEGERS, NONE, SCALARS, SIGNED, UNSIGNED,
                                    WIDTHS, Node, Operand, Result, call, load, random_type,
                                    signature_valid, typed_call)


def cast_valid(op, source, destination):
    source_width, destination_width = WIDTHS.get(source, 0), WIDTHS.get(destination, 0)
    if op == 0 and source == 1:
        return destination in INTEGERS and destination != 2
    if op in (0, 1):
        return source in INTEGERS and destination in INTEGERS and source_width < destination_width
    if op == 2:
        return source in INTEGERS and destination in INTEGERS and source_width > destination_width
    if op in (3, 4):
        return source in FLOATS and destination in FLOATS and (
            source_width < destination_width if op == 3 else source_width > destination_width)
    if op == 5:
        return source in SIGNED and destination in FLOATS
    if op == 6:
        return source in UNSIGNED and destination in FLOATS
    if op == 7:
        return source in FLOATS and destination in SIGNED
    if op == 8:
        return source in FLOATS and destination in UNSIGNED
    if op == 9:
        return source in INTEGERS | FLOATS and destination in INTEGERS | FLOATS and source_width == destination_width
    return op == 12 and source in INTEGERS and destination in INTEGERS


def vectors(kernel):
    count = 0
    for op in range(13):
        for source in range(17):
            for destination in range(17):
                status, _ = call(kernel, 2, source, [Operand(7, source, 0, 0, 0)],
                                 sub=op, other=destination)
                assert (status == 0) == cast_valid(op, source, destination), (op, source, destination, status)
                count += 1
    for kind in range(17):
        for actual in range(17):
            for op, valid in [(1, kind in INTEGERS | FLOATS), (7, kind in INTEGERS),
                              (8, kind != 0), (10, kind in FLOATS)]:
                operands = [Operand(1, actual, 0, 0, 0)] * (2 if op == 10 else 1)
                status, _ = call(kernel, op, kind, operands)
                assert status == (1 if not valid else 0 if kind == actual else 3), (op, kind, actual, status)
                count += 1
    nodes = [*SCALARS, Node(17, 6, 0, 0, 0), Node(17, 6, 0, 0, 0), Node(17, 7, 0, 0, 0)]
    for op in (13, 15, 17):
        for align in (0, 1, 2, 3, 4, 8, 4096):
            for actual in (6, 17, 18, 19):
                status, _ = call(kernel, op, 6, [Operand(1, actual, 0, 0, 0)], align=align, nodes=nodes)
                expected = 4 if align == 0 or align & (align - 1) else 0 if actual in (17, 18) else 3
                assert status == expected, (op, align, actual, status)
                count += 1
    assert call(kernel, 1, 6, [Operand(1, NONE, 0, 0, 0)])[0] == 2
    assert call(kernel, 1, 6, [Operand(1, 6, 0, 0, 0)], work=0)[0] == 8
    malformed = [Node(17, 0, 0, 0, 0)]
    assert call(kernel, 33, 0, nodes=malformed)[0] == 7
    # Supported source depth includes 256 nested function-pointer signatures.
    nested = 6
    for _ in range(256):
        nested = ("f", 0, False, nested, [])
    assert typed_call(kernel, 25, nested)[0] == 0
    return count


def literal_vectors(kernel, randomizer, cases):
    count = 0
    for kind in range(17):
        for literal in range(1, 7):
            width = WIDTHS.get(kind, 0)
            values = [0, 1, 2, 1 << 127, (1 << 128) - 1]
            if 0 < width < 128:
                values.extend([(1 << (width - 1)) - 1, 1 << (width - 1), (1 << width) - 1, 1 << width])
            for bits in values:
                check_literal(kernel, kind, literal, bits)
                count += 1
    for _ in range(cases):
        check_literal(kernel, randomizer.randrange(17), randomizer.randrange(1, 7), randomizer.getrandbits(128))
        count += 1
    return count


def check_literal(kernel, kind, literal, payload):
    payload &= (1 << 128) - 1
    signed = payload if payload < 1 << 127 else payload - (1 << 128)
    expected = (kind == 1 and literal == 3) or (kind in FLOATS and literal == kind - 10)
    if literal == 1 and kind in SIGNED:
        expected = -(1 << (WIDTHS[kind] - 1)) <= signed < 1 << (WIDTHS[kind] - 1)
    if literal == 2 and kind in UNSIGNED:
        expected = payload < 1 << WIDTHS[kind]
    status, _ = call(kernel, 24, kind, [Operand(0, 0, literal, payload & NONE, payload >> 64)])
    assert (status == 0) == expected, (kind, literal, payload, status, expected)


def signatures(kernel, randomizer, cases):
    for _ in range(cases):
        signature = ("f", randomizer.randrange(2), randomizer.randrange(7) == 0,
                     random_type(randomizer, 4),
                     [random_type(randomizer, 4) for _ in range(randomizer.randrange(5))])
        status, _ = typed_call(kernel, 25, signature)
        assert (status == 0) == signature_valid(signature), (signature, status)
    for _ in range(max(1, cases // 5)):
        signature = ("f", randomizer.randrange(2), False, randomizer.randrange(17),
                     [randomizer.randrange(1, 17) for _ in range(randomizer.randrange(5))])
        assert typed_call(kernel, 28, signature, other=signature[3], args=signature[4],
                          sub=signature[1], dst=int(signature[3] != 0))[0] == 0
        assert typed_call(kernel, 28, signature, other=signature[3], args=signature[4],
                          sub=1 - signature[1], dst=int(signature[3] != 0))[0] == 13
        if signature[4]:
            arguments = [0, *signature[4][1:]]
            assert typed_call(kernel, 28, signature, other=signature[3], args=arguments,
                              sub=signature[1], dst=int(signature[3] != 0))[0] == 17


def identities(library, cases):
    class Key(C.Structure):
        _fields_ = [("kind", C.c_uint64), ("number", C.c_uint64),
                    ("bytes", C.POINTER(C.c_uint8)), ("count", C.c_uint64)]

    class Slot(C.Structure):
        _fields_ = [*Key._fields_, ("occupied", C.c_uint64)]

    kernel = library.whale_ir_verify_index_insert
    kernel.restype = C.c_int32
    kernel.argtypes = [C.POINTER(Slot), C.c_uint64, C.POINTER(Key), C.c_uint64, C.POINTER(Result)]
    table = (Slot * 8192)()
    seen, borrowed = set(), []
    randomizer = random.Random(0x3502)
    for _ in range(cases):
        kind, number = randomizer.randrange(3), randomizer.randrange(512)
        if kind == 1:
            text = (f"name-{number}-" + "λ" * (number % 13)).encode()
            buffer = (C.c_uint8 * len(text))(*text)
            borrowed.append(buffer)
            key, identity = Key(kind, 0, buffer, len(text)), (kind, text)
        elif kind == 2:
            key, identity = Key(kind, number, None, number % 7), (kind, number, number % 7)
        else:
            key, identity = Key(kind, number, None, 0), (kind, number)
        result = Result()
        status = kernel(table, len(table), C.byref(key), 100000, C.byref(result))
        assert status == result.status and result.work <= 100000
        assert status == int(identity in seen), (identity, status)
        seen.add(identity)
    result = Result()
    key = Key(0, 2048, None, 0)
    assert kernel(table, len(table), C.byref(key), 0, C.byref(result)) == 8

    link = library.whale_ir_verify_link
    link.restype = C.c_int32
    link.argtypes = [C.c_uint64, C.c_uint64, C.POINTER(C.c_uint8), C.c_uint64,
                     C.c_uint64, C.POINTER(Result)]
    for external in (0, 1):
        for present in (0, 1):
            for name in (b"", b"symbol", b"a\0b"):
                buffer = (C.c_uint8 * len(name))(*name)
                expected = ((external == 0 and present == 0)
                            or (external == 1 and present == 1 and name and b"\0" not in name))
                status = link(external, present, buffer, len(buffer), 1000, C.byref(result))
                assert (status == 0) == bool(expected), (external, present, name, status)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", required=True)
    parser.add_argument("--cases", type=int, default=10000)
    args = parser.parse_args()
    assert args.cases >= 0
    library, kernel = load(args.library)
    randomizer = random.Random(0x350)
    fixed = vectors(kernel)
    literals = literal_vectors(kernel, randomizer, args.cases)
    signatures(kernel, randomizer, args.cases)
    identities(library, args.cases)
    print(f"Wave IR verifier: {fixed} primitive vectors, {literals} literals, "
          f"{args.cases} signatures/identities and call/link/transport/work/depth checks passed")


if __name__ == "__main__":
    main()
