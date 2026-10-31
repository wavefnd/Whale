#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Native Wave IEEE checks against exact Fraction and retained mathematical vectors."""
import argparse
import ctypes as C
from fractions import Fraction
from pathlib import Path
import random

from generate_float_vectors import arithmetic, decode, encode, shape

MASK64 = (1 << 64) - 1
MASK128 = (1 << 128) - 1
KINDS = {'i1': 2, 'u1': 3, 'i8': 4, 'i16': 5, 'i32': 6, 'i64': 7, 'i128': 8,
         'u8': 9, 'u16': 10, 'u32': 11, 'u64': 12, 'u128': 13,
         'f16': 14, 'f32': 15, 'f64': 16}
FLOAT_KIND = {16: 14, 32: 15, 64: 16}
TAG = {16: 4, 32: 5, 64: 6}
OPS = ('fadd', 'fsub', 'fmul', 'fdiv', 'frem')
CASTS = ('fext', 'ftrunc', 'itof_s', 'itof_u', 'ftoi_s', 'ftoi_u')


class Request(C.Structure):
    _fields_ = [(n, C.c_uint64) for n in
                'mode operation source destination left_kind left_low left_high right_kind right_low right_high'.split()]


class Result(C.Structure):
    _fields_ = [(n, C.c_uint64) for n in 'status low high flag'.split()]


def scalar(ty, value):
    if ty.startswith('f'):
        return TAG[int(ty[1:])], value, 0
    return (1 if ty.startswith('i') else 2), value & MASK64, (value & MASK128) >> 64


class Kernel:
    def __init__(self, library):
        self.run = C.CDLL(library).whale_ir_float
        self.run.argtypes = [C.POINTER(Request), C.POINTER(Result)]
        self.run.restype = C.c_int32

    def call(self, mode, op, src, dst=0, left=(0, 0, 0), right=(0, 0, 0)):
        request = Request(mode, op, src, dst, *left, *right)
        result = Result(*([MASK64] * 4))
        status = self.run(C.byref(request), C.byref(result))
        assert status == result.status
        if status:
            assert (result.low, result.high, result.flag) == (0, 0, 0)
            return None
        assert result.flag <= 1
        return result.low | (result.high << 64), result.flag

    def arithmetic(self, width, op, a, b):
        value = self.call(0, OPS.index(op), FLOAT_KIND[width], FLOAT_KIND[width],
                          (TAG[width], a, 0), (TAG[width], b, 0))
        assert value is not None and value[1] == 0
        return value[0]

    def convert(self, src, dst, op, value):
        answer = self.call(3, CASTS.index(op), KINDS[src], KINDS[dst], scalar(src, value))
        return None if answer is None else answer[0]


def integer_range(ty):
    width = int(ty[1:])
    if ty.startswith('i'):
        return -(1 << (width - 1)), (1 << (width - 1)) - 1
    return 0, (1 << width) - 1


def conversion_oracle(src, dst, value):
    if src.startswith('f'):
        decoded = decode(int(src[1:]), value)
    else:
        decoded = Fraction(value)
    if dst.startswith('f'):
        negative_zero = src.startswith('f') and bool(value >> (int(src[1:]) - 1)) and decoded == 0
        return encode(int(dst[1:]), decoded, negative_zero)
    if isinstance(decoded, str):
        return None
    magnitude = abs(decoded.numerator) // decoded.denominator
    truncated = -magnitude if decoded < 0 else magnitude
    low, high = integer_range(dst)
    return truncated & MASK128 if low <= truncated <= high else None


def retained(kernel, root):
    rows = 0
    for line in (root / 'ir/tests/fixtures/float-arithmetic.txt').read_text().splitlines():
        if not line or line.startswith('#'):
            continue
        w, op, a, b, result = line.split()
        assert kernel.arithmetic(int(w), op, int(a, 16), int(b, 16)) == int(result, 16), line
        rows += 1
    for line in (root / 'ir/tests/fixtures/float-conversions.txt').read_text().splitlines():
        if not line or line.startswith('#'):
            continue
        src, dst, op, source, expected = line.split()
        value = int(source, 16 if src.startswith('f') else 10)
        want = None if expected == 'trap' else int(expected, 16 if dst.startswith('f') else 10) & MASK128
        actual = kernel.convert(src, dst, op, value)
        assert actual == want, (line, actual, want)
        rows += 1
    return rows


def half_extension(bits, width, numeric=True):
    fraction, bias = shape(width)
    sign = (bits >> 15) << (width - 1)
    exponent = (bits >> 10) & 31
    mantissa = bits & 1023
    if exponent == 31:
        if mantissa:
            return (((2 * bias + 1) << fraction) | (1 << (fraction - 1))) if numeric else sign | ((2 * bias + 1) << fraction) | (mantissa << (fraction - 10))
        return sign | ((2 * bias + 1) << fraction)
    if exponent:
        return sign | ((exponent - 15 + bias) << fraction) | (mantissa << (fraction - 10))
    if not mantissa:
        return sign
    highest = mantissa.bit_length() - 1
    return sign | ((highest - 24 + bias) << fraction) | ((mantissa - (1 << highest)) << (fraction - highest))


def exhaustive_half(kernel):
    for bits in range(1 << 16):
        nan = (bits & 0x7c00) == 0x7c00 and bits & 1023
        for width in (32, 64):
            want = half_extension(bits, width)
            actual = kernel.convert('f16', f'f{width}', 'fext', bits)
            assert actual == want, (bits, width, actual, want)
            back = kernel.convert(f'f{width}', 'f16', 'ftrunc', actual)
            expected = 0x7e00 if nan else bits
            assert back == expected, (bits, width, back, expected)
        value = kernel.call(4, 0, 14, left=(4, bits, 0))[0]
        expected = half_extension(bits, 64, numeric=False)
        assert value == expected, (bits, value, expected)


def compare_oracle(width, a, b, predicate):
    x, y = decode(width, a), decode(width, b)
    ordered = x != 'nan' and y != 'nan'
    order = 0
    if ordered:
        if x == y:
            order = 0
        elif x == '-inf' or y == 'inf':
            order = -1
        elif x == 'inf' or y == '-inf':
            order = 1
        else:
            order = -1 if x < y else 1
    eq, lt, gt = order == 0, order < 0, order > 0
    return (ordered and eq, ordered and not eq, ordered and lt, ordered and not gt,
            ordered and gt, ordered and not lt, ordered, not ordered,
            not ordered or eq, not ordered or not eq, not ordered or lt,
            not ordered or not gt, not ordered or gt, not ordered or not lt)[predicate]


def generated(kernel, count):
    rng = random.Random(0x353f10a7)
    integer_types = [ty for ty in KINDS if not ty.startswith('f')]
    for i in range(count):
        width = (16, 32, 64)[i % 3]
        a, b = rng.getrandbits(width), rng.getrandbits(width)
        op = OPS[i % 5]
        want = arithmetic(width, op, a, b)
        actual = kernel.arithmetic(width, op, a, b)
        assert actual == want, (width, op, hex(a), hex(b), hex(actual), hex(want))
        neg = kernel.call(1, 0, FLOAT_KIND[width], left=(TAG[width], a, 0))
        assert neg == (a ^ (1 << (width - 1)), 0)
        pred = i % 14
        compared = kernel.call(2, pred, FLOAT_KIND[width], FLOAT_KIND[width],
                               (TAG[width], a, 0), (TAG[width], b, 0))
        assert compared == (0, int(compare_oracle(width, a, b, pred)))
        int_ty = rng.choice(integer_types)
        low, high = integer_range(int_ty)
        integer = rng.randrange(low, high + 1)
        src = int_ty
        dst = f'f{width}'
        op = 'itof_s' if int_ty.startswith('i') else 'itof_u'
        assert kernel.convert(src, dst, op, integer) == conversion_oracle(src, dst, integer)
        op = 'ftoi_s' if int_ty.startswith('i') else 'ftoi_u'
        assert kernel.convert(dst, src, op, a) == conversion_oracle(dst, src, a)
        other_width = rng.choice([w for w in (16, 32, 64) if w != width])
        op = 'fext' if other_width > width else 'ftrunc'
        assert kernel.convert(f'f{width}', f'f{other_width}', op, a) == conversion_oracle(f'f{width}', f'f{other_width}', a)
        decoded = decode(width, a)
        if width == 64:
            want64 = a
        elif width == 16:
            want64 = half_extension(a, 64, numeric=False)
        elif decoded == 'nan':
            want64 = ((a >> 31) << 63) | (0x7ff << 52) | ((a & 0x7fffff) << 29) | (1 << 51)
        else:
            want64 = encode(64, decoded, decoded == 0 and bool(a >> 31))
        assert kernel.call(4, 0, FLOAT_KIND[width], left=(TAG[width], a, 0)) == (want64, 0)
        host_bits = rng.getrandbits(64)
        decoded = decode(64, host_bits)
        if width == 64:
            wanted = host_bits
        elif decoded == 'nan':
            fraction, bias = shape(width)
            sign = (host_bits >> 63) << (width - 1)
            wanted = sign | ((2 * bias + 1) << fraction) | (1 << (fraction - 1))
            if width == 32:
                wanted |= (host_bits & ((1 << 52) - 1)) >> 29
        else:
            wanted = encode(width, decoded, decoded == 0 and bool(host_bits >> 63))
        assert kernel.call(5, 0, 16, FLOAT_KIND[width], left=(6, host_bits, 0)) == (wanted, 0)


def invalid(kernel):
    for mode, op, src, dst, left, right in [
        (0, 99, 15, 15, (5, 0x7fc00000, 0), (5, 0, 0)),
        (0, 0, 14, 15, (4, 0, 0), (5, 0, 0)),
        (0, 0, 14, 14, (4, 1 << 16, 0), (4, 0, 0)),
        (1, 0, 15, 0, (5, 0, 1), (0, 0, 0)),
        (3, 0, 16, 15, (6, 0, 0), (0, 0, 0)),
        (3, 2, 9, 15, (2, 1, 0), (0, 0, 0)),
        (3, 3, 4, 15, (1, 1, 0), (0, 0, 0)),
        (3, 2, 4, 15, (1, 128, 0), (0, 0, 0)),
        (2, 14, 15, 15, (5, 0, 0), (5, 0, 0)),
    ]:
        assert kernel.call(mode, op, src, dst, left, right) is None
    output = Result(7, 7, 7, 7)
    assert kernel.run(None, C.byref(output)) == 9
    assert (output.status, output.low, output.high, output.flag) == (9, 0, 0, 0)
    request = Request()
    assert kernel.run(C.byref(request), None) == 9
    for address in (MASK64, MASK64 - 79):
        output = Result(7, 7, 7, 7)
        assert kernel.run(C.cast(C.c_void_p(address), C.POINTER(Request)), C.byref(output)) == 9
        assert (output.status, output.low, output.high, output.flag) == (9, 0, 0, 0)
    for mode in (6, MASK64):
        request.mode = mode
        output = Result()
        assert kernel.run(C.byref(request), C.byref(output)) == 9


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--library', required=True)
    parser.add_argument('--cases', type=int, default=10000)
    args = parser.parse_args()
    kernel = Kernel(args.library)
    rows = retained(kernel, Path(__file__).resolve().parents[1])
    exhaustive_half(kernel)
    generated(kernel, args.cases)
    invalid(kernel)
    print(f'Wave IEEE: {rows} retained Fraction/conversion vectors, all 65536 binary16 patterns, {args.cases} independent arithmetic/cast/compare cases and transport failures passed')


if __name__ == '__main__':
    main()
