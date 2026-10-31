#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Independent native vectors for the Wave-owned versioned IR header."""

import argparse
import ctypes
import random


class Token(ctypes.Structure):
    _fields_ = [("data", ctypes.c_void_p), ("length", ctypes.c_uint64),
                ("kind", ctypes.c_uint64)]


class Result(ctypes.Structure):
    _fields_ = [(name, ctypes.c_uint64) for name in
                ("status", "at", "expected", "version", "ptr_bits", "endian",
                 "target_index", "next")]


HEADER = [b"module", b"{", b"format_version", b"8", b"semantics_version",
          b"1", b"target", b"x86_64-whale-linux", b"datalayout", b"{",
          b"ptr", b"=", b"64", b",", b"endian", b"=", b"little", b"}"]
KINDS = [0, 2, 0, 0, 0, 0, 0, 1, 0, 2, 0, 2, 0, 2, 0, 2, 0, 2]


def call(kernel, words, kinds=KINDS):
    buffers = [ctypes.create_string_buffer(word) for word in words]
    tokens = (Token * len(words))(*(
        Token(ctypes.cast(buffer, ctypes.c_void_p), len(word), kind)
        for buffer, word, kind in zip(buffers, words, kinds)))
    result = Result()
    status = kernel(tokens, len(words), 8, 1, ctypes.byref(result))
    assert status == result.status, (status, result.status)
    if status:
        assert (result.version, result.ptr_bits, result.endian,
                result.target_index, result.next) == (0, 0, 0, 0, 0)
    return status, result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", required=True)
    parser.add_argument("--cases", type=int, default=10000)
    args = parser.parse_args()
    kernel = ctypes.CDLL(args.library).whale_ir_header
    kernel.argtypes = [ctypes.c_void_p, ctypes.c_uint64, ctypes.c_uint64,
                       ctypes.c_uint64, ctypes.POINTER(Result)]
    status, result = call(kernel, HEADER)
    assert status == 0 and (result.version, result.ptr_bits, result.endian,
                            result.target_index, result.next) == (8, 64, 0, 7, 18)
    for version in range(3, 9):
        words = HEADER.copy()
        words[3] = str(version).encode()
        assert call(kernel, words)[0] == 0
    for index, replacement, expected in [
        (3, b"2", 4), (3, b"9", 4), (3, b"4294967296", 3),
        (3, b"-1", 2), (5, b"2", 5), (12, b"4294967296", 3),
        (16, b"middle", 7),
    ]:
        words = HEADER.copy()
        words[index] = replacement
        assert call(kernel, words)[0] == expected
    for index in range(18):
        words = HEADER.copy()
        words[index] = b"wrong"
        if index == 7:
            assert call(kernel, words)[0] == 0
        else:
            assert call(kernel, words)[0] != 0
        assert call(kernel, HEADER[:index])[0] != 0
    randomizer = random.Random(0x3490349)
    for case in range(args.cases):
        words = HEADER.copy()
        kinds = KINDS.copy()
        index = randomizer.randrange(18)
        if index in (3, 5, 12):
            words[index] = str(randomizer.randrange(0, 1 << 36)).encode()
        else:
            words[index] = randomizer.choice((b"", b"bad", b"\xff", b"123"))
        if index == 7 and case % 2 == 0:
            kinds[index] = 0
        status, result = call(kernel, words, kinds)
        assert status != 0 or (index in (3, 5, 7, 12) and result.next == 18), (case, index)
    print(f"Wave IR header: versions, truncation, transaction and {args.cases} mutations passed")


if __name__ == "__main__":
    main()
