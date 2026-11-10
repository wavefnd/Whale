"""Regenerate scalar casts using the independent exact-rational float oracle."""
from fractions import Fraction
from pathlib import Path
import random
from generate_float_vectors import decode, encode, shape


def main():
    rng = random.Random(0x43415354)
    lines = ["# src dst op input expected (decimal integers / hex float bits / trap)"]
    integers = [(f"{sign}{bits}", bits, sign == "i")
                for bits in (1, 8, 16, 32, 64, 128) for sign in ("i", "u")]
    for name, size, signed in integers:
        low = -(1 << (size - 1)) if signed else 0
        high = (1 << (size - int(signed))) - 1
        values = {low, high, 0}
        values.update(x for x in (-1, 1, 2049, 16777217, (1 << 53) + 1)
                      if low <= x <= high)
        for width in (16, 32, 64):
            for value in sorted(values):
                want = encode(width, Fraction(value))
                lines.append(f"{name} f{width} itof_{'s' if signed else 'u'} {value} {want:x}")
    for width in (16, 32, 64):
        frac, bias = shape(width)
        inf = (2 * bias + 1) << frac
        sign = 1 << (width - 1)
        values = {0, sign, 1, sign | 1, inf, sign | inf, inf | 1,
                  inf | (1 << (frac - 1)), inf - 1, sign | (inf - 1)}
        for _, size, signed in integers:
            for edge in (-(1 << (size - 1)), (1 << (size - int(signed))) - 1,
                         1 << (size - int(signed)), -1, 0, 1):
                for half in (Fraction(-1, 2), Fraction(0), Fraction(1, 2)):
                    raw = encode(width, Fraction(edge) + half)
                    values.update((x & ((1 << width) - 1)) for x in (raw-1, raw, raw+1))
        values.update(rng.getrandbits(width) for _ in range(128))
        for raw in sorted(values):
            value = decode(width, raw)
            for name, size, signed in integers:
                low = -(1 << (size - 1)) if signed else 0
                high = (1 << (size - int(signed))) - 1
                want = "trap"
                if not isinstance(value, str):
                    truncated = abs(value.numerator) // value.denominator
                    if value < 0:
                        truncated = -truncated
                    if low <= truncated <= high:
                        want = str(truncated)
                lines.append(f"f{width} {name} ftoi_{'s' if signed else 'u'} {raw:x} {want}")
            for dest in (16, 32, 64):
                if dest != width:
                    want = encode(dest, value, raw == sign)
                    lines.append(f"f{width} f{dest} {'fext' if width < dest else 'ftrunc'} {raw:x} {want:x}")
    path = Path(__file__).resolve().parent.parent / "bindings/rust/ir/tests/fixtures/float-conversions.txt"
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"Generated {len(lines) - 1} exact conversion vectors")


if __name__ == "__main__":
    main()
