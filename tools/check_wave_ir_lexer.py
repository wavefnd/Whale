#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Independent bounded Wave IR lexer vectors and mutation checks."""

import argparse
import ctypes
import random


class Token(ctypes.Structure):
    _fields_ = [(name, ctypes.c_uint64) for name in (
        "kind", "start", "end", "line", "column", "decoded_start", "decoded_len")]


class Result(ctypes.Structure):
    _fields_ = [(name, ctypes.c_uint64) for name in (
        "status", "reason", "offset", "line", "column", "token_count", "decoded_count")]


def call(kernel, source, max_tokens=100, max_work=10000, mode=1,
         token_capacity=None, decoded_capacity=None):
    if token_capacity is None:
        token_capacity = min(max_tokens + 1, len(source) + 1)
    if decoded_capacity is None:
        decoded_capacity = len(source)
    source_buffer = ctypes.create_string_buffer(source)
    scratch = (Token * token_capacity)()
    output = (Token * token_capacity)(*[Token(99, 99, 99, 99, 99, 99, 99)
                                          for _ in range(token_capacity)])
    scratch_decoded = (ctypes.c_uint8 * decoded_capacity)()
    decoded = (ctypes.c_uint8 * decoded_capacity)(*([0xa5] * decoded_capacity))
    result = Result()
    status = kernel(source_buffer, len(source), max_tokens, max_work, mode,
                    scratch, token_capacity, scratch_decoded, decoded_capacity,
                    output, token_capacity, decoded, decoded_capacity,
                    ctypes.byref(result))
    assert status == result.status
    if status:
        assert all(token.kind == 99 for token in output), (source, status)
        assert all(octet == 0xa5 for octet in decoded), (source, status)
    tokens = [(token.kind, source[token.start:token.end], token.line, token.column,
               bytes(decoded[token.decoded_start:token.decoded_start + token.decoded_len]))
              for token in output[:result.token_count]] if status == 0 and mode == 1 else []
    return status, tokens, result.token_count, result.decoded_count


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", required=True)
    parser.add_argument("--cases", type=int, default=10000)
    args = parser.parse_args()
    kernel = ctypes.CDLL(args.library).whale_ir_lex
    kernel.argtypes = [ctypes.c_void_p, ctypes.c_uint64, ctypes.c_uint64,
                       ctypes.c_uint64, ctypes.c_uint64, ctypes.c_void_p,
                       ctypes.c_uint64, ctypes.c_void_p, ctypes.c_uint64,
                       ctypes.c_void_p, ctypes.c_uint64, ctypes.c_void_p,
                       ctypes.c_uint64, ctypes.POINTER(Result)]
    source = 'module {\n target "a\\u{1F40B}\\n" // café\nα β\u2003γ\n}'.encode()
    expected = [(0, b'module', 1, 1, b''), (2, b'{', 1, 8, b''),
                (0, b'target', 2, 2, b''),
                (1, b'"a\\u{1F40B}\\n"', 2, 9, 'a🐋\n'.encode()),
                (0, 'α'.encode(), 3, 1, b''), (0, 'β'.encode(), 3, 3, b''),
                (0, 'γ'.encode(), 3, 5, b''), (2, b'}', 4, 1, b''),
                (3, b'', 4, 2, b'')]
    status, tokens, count, decoded = call(kernel, source)
    assert status == 0 and tokens == expected, (status, tokens)
    assert call(kernel, source, mode=0)[2:] == (count, decoded)
    for malformed in (b'"unterminated', b'"\\u{}"', b'"\\u{d800}"',
                      b'"\\u{110000}"', b'"\\q"', b'"\x00"', b'\xff'):
        assert call(kernel, malformed)[0] == 1, malformed
    assert call(kernel, b'a b', max_tokens=1)[0] == 2
    assert call(kernel, b'"abc"', decoded_capacity=2)[0] == 3
    assert call(kernel, b'a b', max_work=1)[0] == 4
    assert call(kernel, b'a b', token_capacity=1)[0] == 2
    long_source = b'a ' * 60000
    long_status, long_tokens, long_count, _ = call(
        kernel, long_source, max_tokens=60000, max_work=1000000)
    assert long_status == 0 and long_count == 60001
    assert long_tokens[0][1] == b'a' and long_tokens[-1][0] == 3
    randomizer = random.Random(0x7782)
    alphabet = [b'a', b' ', b'\n', b'"', b'\\', b'{', b'}', b'//',
                b'\xff', b'\x00', '🐋'.encode(), 'β'.encode()]
    for index in range(args.cases):
        source = b''.join(randomizer.choice(alphabet)
                          for _ in range(randomizer.randrange(0, 48)))
        pre = call(kernel, source, mode=0)
        published = call(kernel, source)
        assert pre[0] == published[0] and pre[2:] == published[2:], (index, source, pre, published)
    print(f"Wave IR lexer: vectors, limits and {args.cases} mutations passed")


if __name__ == "__main__":
    main()
