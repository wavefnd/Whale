#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Independent mathematical conformance for Wave fixed-width and two-word math."""

import argparse
import ctypes as C
import random

MASK = (1 << 128) - 1
SHAPES = {
    1: (1, False),
    2: (1, True),
    3: (1, False),
    **{kind: (width, True) for kind, width in zip(range(4, 9), (8, 16, 32, 64, 128))},
    **{kind: (width, False) for kind, width in zip(range(9, 14), (8, 16, 32, 64, 128))},
}
BINARY = (0, 1, 2, 3, 4, 5, 6, 12, 13, 14, 15, 16, 17)


class Result(C.Structure):
    _fields_ = [(name, C.c_uint64) for name in ("status", "low", "high", "flag")]


def signed(value, width):
    value &= (1 << width) - 1
    return value - (1 << width) if value >= (1 << (width - 1)) else value


def packed(value, kind):
    width, negative = SHAPES[kind]
    return (signed(value, width) if negative else value & ((1 << width) - 1)) & MASK


def trunc_div(a, b):
    return (abs(a) // abs(b)) * (-1 if (a < 0) != (b < 0) else 1)


def oracle(mode, op, source, destination, a, b):
    if source not in SHAPES or destination not in SHAPES or mode > 6:
        return 4, 0, 0
    width, is_signed = SHAPES[source]
    dest_width, dest_signed = SHAPES[destination]
    mask = (1 << width) - 1
    a, b = a & mask, b & mask
    value, flag = 0, 0
    if mode == 0:
        if source == 1 or destination != source or op not in BINARY:
            return 4, 0, 0
        if 3 <= op <= 6 and b == 0:
            return (1 if op <= 4 else 2), 0, 0
        count = b % width
        if op == 0:
            value = a + b
        elif op == 1:
            value = a - b
        elif op == 2:
            value = a * b
        elif op == 3:
            value = a // b
        elif op == 4:
            value = trunc_div(signed(a, width), signed(b, width))
        elif op == 5:
            value = a % b
        elif op == 6:
            sa, sb = signed(a, width), signed(b, width)
            value = sa - trunc_div(sa, sb) * sb
        elif op == 12:
            value = a & b
        elif op == 13:
            value = a | b
        elif op == 14:
            value = a ^ b
        elif op == 15:
            value = a << count
        elif op == 16:
            value = a >> count
        else:
            value = signed(a, width) >> count
    elif mode == 1:
        if source == 1 or destination != source or op > 5:
            return 4, 0, 0
        aa, bb = (a, b) if op < 3 else (signed(a, width), signed(b, width))
        value = aa + bb if op % 3 == 0 else aa - bb if op % 3 == 1 else aa * bb
        minimum, maximum = (0, mask) if op < 3 else (-(1 << (width - 1)), (1 << (width - 1)) - 1)
        flag = int(not minimum <= value <= maximum)
    elif mode == 2:
        valid = (
            destination != 1
            and op <= 3
            and (
                (
                    op == 0
                    and ((source == 1 and destination != 2) or (source != 1 and width < dest_width))
                )
                or (op == 1 and source != 1 and width < dest_width)
                or (op == 2 and source != 1 and width > dest_width)
                or (op == 3 and source != 1 and width == dest_width)
            )
        )
        if not valid:
            return 4, 0, 0
        value = signed(a, width) if op == 1 else a
    elif mode == 3:
        if op > 9 or destination != 1 or (source == 1 and op > 1):
            return 4, 0, 0
        aa, bb = (signed(a, width), signed(b, width)) if op >= 6 else (a, b)
        value = int(
            (
                aa == bb,
                aa != bb,
                aa < bb,
                aa <= bb,
                aa > bb,
                aa >= bb,
                aa < bb,
                aa <= bb,
                aa > bb,
                aa >= bb,
            )[op]
        )
    elif mode == 4:
        if op != 0 or source == 1 or destination == 1:
            return 4, 0, 0
        value = signed(a, width) if is_signed else a
        minimum = -(1 << (dest_width - 1)) if dest_signed else 0
        maximum = (1 << (dest_width - int(dest_signed))) - 1
        if not minimum <= value <= maximum:
            return 3, 0, 0
    elif mode == 5:
        if op != 0 or source != destination:
            return 4, 0, 0
        value = a
    else:
        if op > 1 or source == 1 or destination != source:
            return 4, 0, 0
        value = -a if op == 0 else ~a
    return 0, packed(value, destination), flag


class Kernel:
    def __init__(self, path):
        lib = C.CDLL(path)
        self.scalar = lib.whale_ir_integer
        self.scalar.argtypes = [C.c_uint64] * 8 + [C.POINTER(Result)]
        self.scalar.restype = C.c_int32
        self.wide = lib.whale_ir_wide
        self.wide.argtypes = [C.c_uint64] * 6 + [C.POINTER(Result)]
        self.wide.restype = C.c_int32
        self.bits = lib.whale_ir_integer_bits
        self.bits.argtypes = [C.c_uint64] * 4 + [C.POINTER(Result)]
        self.bits.restype = C.c_int32

    def call(self, mode, op, source, destination, a, b):
        result = Result(99, 99, 99, 99)
        code = self.scalar(
            mode,
            op,
            source,
            destination,
            a & ((1 << 64) - 1),
            (a >> 64) & ((1 << 64) - 1),
            b & ((1 << 64) - 1),
            (b >> 64) & ((1 << 64) - 1),
            C.byref(result),
        )
        assert code == result.status
        return code, result.low | (result.high << 64), result.flag


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", required=True)
    parser.add_argument("--cases", type=int, default=100000)
    args = parser.parse_args()
    kernel = Kernel(args.library)
    rng = random.Random(0x352_128)
    fixed = [0, 1, 2, 3, (1 << 64) - 1, 1 << 64, MASK]
    fixed += [
        (1 << bit) + delta
        for bit in (1, 7, 8, 15, 16, 31, 32, 63, 64, 126, 127)
        for delta in (-1, 0, 1)
    ]
    count = 0
    for source in SHAPES:
        for mode, operations in ((0, BINARY), (1, range(6)), (3, range(10)), (6, range(2))):
            destination = 1 if mode == 3 else source
            for op in operations:
                for a in fixed:
                    for b in (0, 1, MASK, a, 1 << 127):
                        actual = kernel.call(mode, op, source, destination, a, b)
                        expected = oracle(mode, op, source, destination, a, b)
                        assert actual == expected, (
                            mode,
                            op,
                            source,
                            destination,
                            a,
                            b,
                            actual,
                            expected,
                        )
                        count += 1
        for destination in SHAPES:
            for mode, operations in ((2, range(4)), (4, (0,))):
                for op in operations:
                    for a in fixed:
                        actual = kernel.call(mode, op, source, destination, a, 0)
                        expected = oracle(mode, op, source, destination, a, 0)
                        assert actual == expected, (
                            mode,
                            op,
                            source,
                            destination,
                            a,
                            actual,
                            expected,
                        )
                        count += 1
    for _ in range(args.cases):
        source, destination = rng.randrange(1, 14), rng.randrange(1, 14)
        mode = rng.randrange(7)
        if mode in (0, 1, 5, 6):
            destination = source
        if mode == 3:
            destination = 1
        op = (
            rng.choice(BINARY)
            if mode == 0
            else rng.randrange(6)
            if mode == 1
            else rng.randrange(4)
            if mode == 2
            else rng.randrange(10)
            if mode == 3
            else rng.randrange(2)
            if mode == 6
            else 0
        )
        a, b = rng.getrandbits(128), rng.getrandbits(128)
        assert kernel.call(mode, op, source, destination, a, b) == oracle(
            mode, op, source, destination, a, b
        ), (mode, op, source, destination, a, b)
    for _ in range(args.cases):
        a, b, shift, op = (
            rng.getrandbits(rng.randrange(129)),
            rng.getrandbits(rng.randrange(129)),
            rng.randrange(260),
            rng.randrange(12),
        )
        result = Result()
        status = kernel.wide(
            op, a & ((1 << 64) - 1), a >> 64, b & ((1 << 64) - 1), b >> 64, shift, C.byref(result)
        )
        expected, flag = 0, 0
        if op == 0:
            expected, flag = (a + b) & MASK, int(a + b > MASK)
        elif op == 1:
            expected, flag = (a - b) & MASK, int(a < b)
        elif op == 2:
            expected, flag = (a * b) & MASK, int(a * b > MASK)
        elif op == 3:
            expected = a // b if b else 0
        elif op == 4:
            expected = a % b if b else 0
        elif op == 5:
            expected = (a << shift) & MASK
        elif op == 6:
            expected = a >> shift
        elif op == 7:
            expected = int(a > b) - int(a < b) + 1
        elif op == 8:
            expected = 128 - a.bit_length()
        elif op == 9:
            expected = (a >> shift) | int(bool(a & ((1 << shift) - 1)))
        elif op == 10:
            q, tail = divmod(a, 1 << shift)
            expected = q + int(
                shift > 0 and (tail > (1 << (shift - 1)) or (tail == (1 << (shift - 1)) and q & 1))
            )
        else:
            expected = (-a) & MASK
        assert status == result.status == (1 if op in (3, 4) and not b else 0)
        assert (result.low | (result.high << 64), result.flag) == (expected, flag), (
            op,
            a,
            b,
            shift,
            expected,
            result.low | result.high << 64,
            flag,
            result.flag,
        )
    for width in range(1, 129):
        for is_signed in (0, 1):
            for a in fixed:
                result = Result()
                status = kernel.bits(
                    a & ((1 << 64) - 1), a >> 64, width, is_signed, C.byref(result)
                )
                expected = (signed(a, width) if is_signed else a & ((1 << width) - 1)) & MASK
                assert status == result.status == 0 and result.low | (result.high << 64) == expected
    for parameters in [(7, 0, 4, 4), (0, 7, 4, 4), (0, 0, 0, 4), (0, 0, 4, 14)]:
        assert kernel.call(*parameters, 1, 1) == (4, 0, 0)
    assert kernel.scalar(0, 0, 4, 4, 0, 0, 0, 0, None) == 4
    print(
        f"Wave integers: {count} fixed boundaries, {args.cases} mathematical scalar vectors, {args.cases} two-word vectors and exact raw normalization passed"
    )


if __name__ == "__main__":
    main()
