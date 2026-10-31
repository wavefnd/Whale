#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Independent UTF-8 lexer and checked-number checks for native Wave assembly."""
import argparse
import bisect
import ctypes as C
from pathlib import Path
import random
import re

U = C.c_uint64
MASK = (1 << 64) - 1
MIN, MAX = -(1 << 127), (1 << 127) - 1


class Token(C.Structure):
    _fields_ = [(name, U) for name in
                "kind start end line column end_line end_column decoded_start decoded_len low high".split()]


class Result(C.Structure):
    _fields_ = [(name, U) for name in
                "status reason start end line column detail tokens decoded work".split()]


class Wide(C.Structure):
    _fields_ = [("low", U), ("high", U)]


def wide(value):
    return Wide(value & MASK, (value >> 64) & MASK)


def integer(value):
    bits = value.low | value.high << 64
    return bits - (1 << 128) if bits >> 127 else bits


text = (Path(__file__).resolve().parents[1] / "assembler/src/unicode_reference.rs").read_text()
RANGES = [(int(lo), int(hi)) for lo, hi in re.findall(r"\((\d+), (\d+)\)", text)]
STARTS = [lo for lo, _ in RANGES]


def alphanumeric(ch):
    cp = ord(ch)
    at = bisect.bisect_right(STARTS, cp) - 1
    return at >= 0 and cp <= RANGES[at][1]


class Syntax(Exception):
    def __init__(self, reason, start, end, line, column, detail=0):
        self.expected = (1, reason, start, end, line, column, detail)


def reference(source):
    at, byte, line, column, cr = 0, 0, 1, 1, False
    tokens, decoded = [], bytearray()

    def bump():
        nonlocal at, byte, line, column, cr
        ch = source[at]
        at += 1
        byte += len(ch.encode())
        if ch == '\r':
            line, column, cr = line + 1, 1, True
        elif ch == '\n':
            line, column, cr = line + int(not cr), 1, False
        else:
            column, cr = column + 1, False
        return ch

    def fail(reason, mark, detail=0, unconsumed=False):
        start, ln, col = mark
        end = byte if byte > start else byte + (len(source[at].encode()) if at < len(source) else 0)
        if unconsumed:
            end = byte + len(source[at].encode())
        raise Syntax(reason, start, end, ln, col, detail)

    while at < len(source):
        ch = source[at]
        mark = (byte, line, column)
        if ch == ';':
            while at < len(source) and source[at] not in '\r\n':
                bump()
            continue
        if ch in ' \t':
            bump()
            continue
        decoded_start = len(decoded)
        number = 0
        if ch == '"':
            kind = 2
            bump()
            while True:
                escape_mark = (byte, line, column)
                if at == len(source):
                    fail(2, mark)
                ch = bump()
                if ch == '"':
                    break
                if ch != '\\':
                    decoded.extend(ch.encode())
                    continue
                if at == len(source):
                    fail(3, escape_mark)
                ch = bump()
                if ch in {'"', '\\', 'n', 'r', 't', '0'}:
                    decoded.append({'"': 34, '\\': 92, 'n': 10, 'r': 13, 't': 9, '0': 0}[ch])
                elif ch == 'x':
                    value = 0
                    for _ in range(2):
                        if at == len(source):
                            fail(4, escape_mark)
                        ch = bump()
                        if ch not in '0123456789abcdefABCDEF':
                            fail(5, escape_mark)
                        value = value * 16 + int(ch, 16)
                    decoded.append(value)
                else:
                    fail(6, escape_mark)
        elif ch in '0123456789':
            kind, radix = 1, 10
            if source[at:at+2].lower() in ('0x', '0b'):
                radix = 16 if source[at+1].lower() == 'x' else 2
                bump()
                bump()
            digits = at
            alphabet = '0123456789abcdef'[:radix] if radix != 10 else '0123456789'
            while at < len(source) and source[at].lower() in alphabet:
                bump()
            if at == digits:
                fail(7 if radix == 16 else 8, mark)
            number = int(source[digits:at], radix)
            if number > MASK:
                fail(9, mark)
        elif ch in '_.' or alphanumeric(ch):
            kind = 0
            while at < len(source) and (source[at] in '_.' or alphanumeric(source[at])):
                bump()
        else:
            punctuation = {',': 3, ':': 4, '[': 5, ']': 6, '+': 7, '-': 8, '*': 9,
                           '\n': 11, '\r': 11}
            if ch not in punctuation:
                fail(1, mark, ord(ch), unconsumed=True)
            kind = punctuation[ch]
            bump()
            if ch == '\r' and at < len(source) and source[at] == '\n':
                bump()
        tokens.append((kind, mark[0], byte, mark[1], mark[2], line, column,
                       decoded_start, len(decoded) - decoded_start, number, 0))
    return tokens, bytes(decoded)


def check(lex, source):
    encoded = source.encode()
    raw = (C.c_uint8 * len(encoded)).from_buffer_copy(encoded)
    try:
        expected = reference(source)
    except Syntax as error:
        expected = error.expected
    probe = Result()
    code = lex(raw, len(raw), len(raw), len(raw), len(raw) * 2 + 1, 0,
               None, 0, None, 0, None, 0, None, 0, C.byref(probe))
    assert code == probe.status
    if probe.status:
        actual = tuple(getattr(probe, name) for name in
                       'status reason start end line column detail'.split())
        assert actual == expected, (source, actual, expected)
        return
    tokens, decoded = expected
    assert probe.tokens == len(tokens) and probe.decoded == len(decoded), source
    scratch, output = (Token * len(tokens))(), (Token * len(tokens))()
    text, published = (C.c_uint8 * len(decoded))(), (C.c_uint8 * len(decoded))()
    result = Result()
    code = lex(raw, len(raw), len(raw), len(raw), len(raw) * 2 + 1, 1,
               scratch, len(scratch), text, len(text), output, len(output),
               published, len(published), C.byref(result))
    assert code == result.status == 0
    actual = [tuple(getattr(token, name) for name, _ in Token._fields_) for token in output]
    assert actual == tokens and bytes(published) == decoded, (source, actual, tokens)
    assert result.work == probe.work <= len(raw)
    # A failed publication must preserve the caller's output byte-for-byte.
    sentinel = (Token * 1)()
    C.memset(sentinel, 0xA5, C.sizeof(sentinel))
    before = bytes(sentinel)
    result = Result()
    code = lex(raw, len(raw), 0, 0, 0, 1, scratch, len(scratch), text, len(text),
               sentinel, 1, published, len(published), C.byref(result))
    if encoded:
        assert code in (2, 3, 4) and bytes(sentinel) == before


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--library', type=Path, required=True)
    parser.add_argument('--cases', type=int, default=10000)
    args = parser.parse_args()
    library = C.CDLL(str(args.library.resolve()))
    lex = library.whale_asm_lex
    lex.argtypes = [C.POINTER(C.c_uint8), U, U, U, U, U, C.POINTER(Token), U,
                    C.POINTER(C.c_uint8), U, C.POINTER(Token), U,
                    C.POINTER(C.c_uint8), U, C.POINTER(Result)]
    alphabet = 'abcXYZ_.,:[]+-*; \t\r\n0123456789xAbB"\\né한٣²😀\x00\u2003'
    rng = random.Random(0x5748414C4541534D)
    for source in ['', 'nop\r\nret', 'étiquette: db "한\\xFF"', '0x', '0b',
                   '18446744073709551615', '18446744073709551616', '0xfffffffffffffffff',
                   '"\\', '"\\x', '"\\xA', '"\\x한"', '"\\q"', '; 😀\r@', '/']:
        check(lex, source)
    for _ in range(args.cases):
        check(lex, ''.join(rng.choice(alphabet) for _ in range(rng.randrange(100))))
    add, neg, sub = (library.asm_number_add_checked, library.asm_number_neg_checked,
                     library.asm_number_sub_checked)
    for fn in (add, sub):
        fn.argtypes = [C.POINTER(Wide), C.POINTER(Wide), C.POINTER(Wide)]
        fn.restype = C.c_bool
    neg.argtypes = [C.POINTER(Wide), C.POINTER(Wide)]
    neg.restype = C.c_bool
    pairs = [(a, b) for a in [MIN, MIN+1, -1, 0, 1, MAX-1, MAX]
             for b in [MIN, MIN+1, -1, 0, 1, MAX-1, MAX]]
    pairs += [(rng.randrange(MIN, MAX+1), rng.randrange(MIN, MAX+1))
              for _ in range(args.cases)]
    for a, b in pairs:
        for fn, expected in ((add, a+b), (sub, a-b)):
            output = wide(99)
            valid = fn(C.byref(wide(a)), C.byref(wide(b)), C.byref(output))
            assert valid == (MIN <= expected <= MAX)
            assert integer(output) == (expected if valid else 99)
        output = wide(99)
        valid = neg(C.byref(wide(a)), C.byref(output))
        assert valid == (a != MIN) and integer(output) == (-a if valid else 99)
    class Growth(C.Structure):
        _fields_ = [('materialized', U), ('section', U)]
    grow = library.asm_output_grow
    grow.argtypes = [U, U, U, U, U, C.POINTER(Growth)]
    align = library.asm_output_align
    align.argtypes = [U, U, U, U, U, C.POINTER(Growth)]
    for _ in range(args.cases):
        materialized, section, extra, limit = [rng.getrandbits(64) for _ in range(4)]
        logical = rng.randrange(2)
        output = Growth(77, 99)
        expected = 0
        if materialized > limit:
            expected = 3
        elif section + extra > MASK or (not logical and materialized + extra > MASK):
            expected = 1
        elif not logical and materialized + extra > limit:
            expected = 2
        code = grow(materialized, section, extra, limit, logical, C.byref(output))
        assert code == expected
        assert (output.materialized, output.section) == (
            (materialized + (0 if logical else extra), section + extra) if not code else (77, 99))
        output = Growth(77, 99)
        alignment = 1 << rng.randrange(64)
        padded = (section + alignment - 1) & -alignment
        expected = 1 if padded > MASK else (3 if materialized > limit else 0)
        total = materialized + (0 if logical else padded - section)
        if not expected and total > MASK:
            expected = 1
        if not expected and total > limit:
            expected = 2
        code = align(materialized, section, alignment, limit, logical, C.byref(output))
        assert code == expected
        assert (output.materialized, output.section) == ((total, padded) if not code else (77, 99))
    output = Growth()
    assert grow(0, 0, 1 << 40, 1, 1, C.byref(output)) == 0
    assert (output.materialized, output.section) == (0, 1 << 40)
    for encoded in [b'\xc0\x80', b'\xed\xa0\x80', b'\xf4\x90\x80\x80', b'\xe2']:
        raw = (C.c_uint8 * len(encoded)).from_buffer_copy(encoded)
        scratch, published = (Token * 1)(), (Token * 1)()
        C.memset(published, 0xA5, C.sizeof(published))
        before = bytes(published)
        result = Result()
        code = lex(raw, len(raw), 1, 1, 100, 1, scratch, 1, None, 0,
                   published, 1, None, 0, C.byref(result))
        assert code == result.status == 5 and bytes(published) == before
    classifier = library.asm_unicode_alphanumeric
    classifier.argtypes, classifier.restype = [U], C.c_bool
    for cp in range(0x110000):
        assert classifier(cp) == alphanumeric(chr(cp)), cp
    print(f'Wave assembler: {args.cases} independent lexer streams + {args.cases} signed128 + neutral output vectors, all Unicode17 scalar classifications, exact malformed diagnostics and transactional limits passed')


if __name__ == '__main__':
    main()
