#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Independent exact scalar text oracle for the native Wave IR parser."""

import argparse
import ctypes
import random
import re


class Result(ctypes.Structure):
    _fields_ = [(name, ctypes.c_uint64) for name in ("status", "low", "high", "work")]


def reference(text, category, width):
    if category == 0:
        return 8, 0
    if category == 1:
        return (0, int(text == b"true")) if text in (b"true", b"false") else (1, 0)
    if category == 4:
        if not text.startswith(b"0x"):
            return 6, 0
        if len(text) != width // 4 + 2 or re.fullmatch(b"[0-9a-fA-F]+", text[2:]) is None:
            return 7, 0
        return 0, int(text[2:], 16)
    if re.fullmatch(b"-?[0-9]+" if category == 2 else b"[0-9]+", text) is None:
        return (2 if category == 2 else 4), 0
    negative = text.startswith(b"-")
    digits = text[1:] if negative else text
    value = int(digits.lstrip(b"0") or b"0")
    if negative:
        value = -value
    if category == 2:
        if value < -(1 << 127) or value >= (1 << 127):
            return 3, 0
        if value < -(1 << (width - 1)) or value >= (1 << (width - 1)):
            return 9, 0
        return 0, value & ((1 << 128) - 1)
    if value >= (1 << 128):
        return 5, 0
    if value >= (1 << width):
        return 9, 0
    return 0, value


def call(kernel, text, category, width, work=None):
    if work is None:
        work = len(text)
    buffer = ctypes.create_string_buffer(text)
    result = Result()
    status = kernel(buffer, len(text), category, width, work, ctypes.byref(result))
    assert status == result.status
    value = result.low | (result.high << 64)
    if status:
        assert value == 0
    return status, value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", required=True)
    parser.add_argument("--cases", type=int, default=100000)
    args = parser.parse_args()
    kernel = ctypes.CDLL(args.library).whale_ir_parse_literal
    kernel.argtypes = [ctypes.c_void_p, *([ctypes.c_uint64] * 4), ctypes.POINTER(Result)]
    seeds = [b"", b"-", b"+1", b"-0", b"00", b"true", b"false", b"\xff", b"0x1234",
             b"0X1234", b"0xFFFF", b"0x7ff0000000000001", b"0x7F800001", b"0xgggg",
             b"9" * 100 + b"x", str(1 << 128).encode(), str(-(1 << 127)).encode(),
             str((1 << 127) - 1).encode(), b"0" * 60000]
    for category in range(5):
        for width in ((16, 32, 64) if category == 4 else (1, 8, 16, 32, 64, 128)):
            for text in seeds:
                assert call(kernel, text, category, width) == reference(text, category, width), (text[:50], category, width)
    for category in (2, 3):
        for width in (1, 8, 16, 32, 64, 128):
            for power in (0, width - 1, width, 127, 128):
                for offset in (-1, 0, 1):
                    value = (1 << power) + offset
                    for value in (value, -value):
                        text = str(value).encode()
                        assert call(kernel, text, category, width) == reference(text, category, width)
    randomizer = random.Random(0x3490352)
    for case in range(args.cases):
        category = randomizer.randrange(5)
        width = randomizer.choice((16, 32, 64) if category == 4 else (1, 8, 16, 32, 64, 128))
        if category == 4:
            text = ("0x" + format(randomizer.getrandbits(width), f"0{width // 4}x")).encode()
        else:
            value = randomizer.getrandbits(randomizer.randrange(0, 180))
            if randomizer.getrandbits(1):
                value = -value
            text = str(value).encode()
        if case % 3 == 0:
            text += randomizer.choice((b"x", b"\xff", b"_", b"."))
        expected = reference(text, category, width)
        assert call(kernel, text, category, width) == expected, (case, text, category, width, expected)
        if text:
            assert call(kernel, text, category, width, work=len(text) - 1)[0] == 10
    print(f"Wave IR literals: exact widths, lexical precedence, long zeros and {args.cases} independent cases passed")


if __name__ == "__main__":
    main()
