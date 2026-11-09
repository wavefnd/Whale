"""Regenerate arithmetic vectors with exact Fraction arithmetic and binary search.

The oracle searches representable neighbors, independently of the production
significand/guard/sticky implementation. No host floating point is used.
"""
from fractions import Fraction
from pathlib import Path
import random


def shape(width):
    return {16: (10, 15), 32: (23, 127), 64: (52, 1023)}[width]


def decode(width, bits):
    frac, bias = shape(width)
    sign = bool(bits >> (width - 1))
    exponent = (bits >> frac) & (2 * bias + 1)
    mantissa = bits & ((1 << frac) - 1)
    if exponent == 2 * bias + 1:
        return "nan" if mantissa else ("-inf" if sign else "inf")
    significand = mantissa if exponent == 0 else mantissa + (1 << frac)
    power = max(1, exponent) - bias - frac
    value = Fraction(significand) * (Fraction(2) ** power)
    return -value if sign else value


def encode(width, value, negative_zero=False):
    frac, bias = shape(width)
    infinity = (2 * bias + 1) << frac
    sign = 1 << (width - 1)
    if value == "nan":
        return infinity | (1 << (frac - 1))
    if isinstance(value, str):
        return infinity | (sign if value == "-inf" else 0)
    negative = value < 0 or (value == 0 and negative_zero)
    value = abs(value)
    # Overflow threshold is half one top-binade ULP above largest finite.
    maximum = decode(width, infinity - 1)
    threshold = maximum + Fraction(2) ** (bias - frac - 1)
    if value >= threshold:
        result = infinity
    elif value > maximum:
        result = infinity - 1
    else:
        low, high = 0, infinity - 1
        while low < high:
            middle = (low + high + 1) // 2
            if decode(width, middle) <= value:
                low = middle
            else:
                high = middle - 1
        result = low
        if low < infinity - 1:
            left = value - decode(width, low)
            right = decode(width, low + 1) - value
            if right < left or (left == right and low & 1):
                result += 1
    return result | (sign if negative else 0)


def arithmetic(width, op, a, b):
    x, y = decode(width, a), decode(width, b)
    sign_a, sign_b = bool(a >> (width - 1)), bool(b >> (width - 1))
    if x == "nan" or y == "nan":
        return encode(width, "nan")
    if op == "fsub":
        y = ({"inf": "-inf", "-inf": "inf"}[y] if isinstance(y, str) else -y)
        sign_b = not sign_b
    inf_a, inf_b = isinstance(x, str), isinstance(y, str)
    negative = sign_a != sign_b
    inf = "-inf" if negative else "inf"
    if op in ("fadd", "fsub"):
        if inf_a or inf_b:
            value = "nan" if inf_a and inf_b and sign_a != sign_b else (x if inf_a else y)
        else:
            value = x + y
        return encode(width, value, sign_a and sign_b)
    if op == "fmul":
        value = "nan" if (inf_a and y == 0) or (inf_b and x == 0) else (inf if inf_a or inf_b else x * y)
    elif op == "fdiv":
        value = "nan" if (inf_a and inf_b) or (x == 0 and y == 0) else (inf if inf_a or y == 0 else (Fraction(0) if inf_b else x / y))
    else:
        if inf_a or y == 0:
            value = "nan"
        elif inf_b:
            value = x
        else:
            quotient = abs(x / y)
            quotient = quotient.numerator // quotient.denominator
            if (x < 0) != (y < 0):
                quotient = -quotient
            value = x - quotient * y
        negative = sign_a
    return encode(width, value, negative)


def main():
    rng = random.Random(0x5748414C45)
    lines = ["# width op lhs rhs expected; exact Fraction neighbor-search oracle"]
    for width in (16, 32, 64):
        frac, bias = shape(width)
        inf = (2 * bias + 1) << frac
        sign = 1 << (width - 1)
        one = bias << frac
        boundaries = [0, sign, 1, sign | 1, (1 << frac) - 1, 1 << frac,
                      one, one + 1, one - 1, one | sign, inf - 1, sign | (inf - 1),
                      inf, inf | sign, inf | 1, inf | (1 << (frac - 1))]
        pairs = [(a, b) for a in boundaries for b in boundaries]
        pairs += [(rng.getrandbits(width), rng.getrandbits(width)) for _ in range(128)]
        for a, b in pairs:
            for op in ("fadd", "fsub", "fmul", "fdiv", "frem"):
                expected = arithmetic(width, op, a, b)
                lines.append(f"{width} {op} {a:x} {b:x} {expected:x}")
    output = Path(__file__).resolve().parent.parent / "bindings/rust/ir/tests/fixtures/float-arithmetic.txt"
    output.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"Generated {len(lines) - 1} exact arithmetic vectors")


if __name__ == "__main__":
    main()
