#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Independent scalar, operand, probe and generic fixed-width driver checks."""
import argparse
import ctypes as C
from pathlib import Path
import random
import re

ROOT = Path(__file__).resolve().parents[1]
U, NONE = C.c_uint64, (1 << 64) - 1
MIN, MAX = -(1 << 127), (1 << 127) - 1
SPACE = '\t\n\v\f\r \u0085\u00a0\u1680' + ''.join(chr(n) for n in range(0x2000, 0x200B)) + '\u2028\u2029\u202f\u205f\u3000'
TYPES = {'u64': U, 'i64': C.c_int64, 'u8': C.c_uint8, 'bool': C.c_bool}
for name, body in re.findall(r'struct (\w+)\s*\{([^}]+)\}',
                             (ROOT / 'assembler/fixed_model.wave').read_text()):
    fields = []
    for field, kind in re.findall(r'(\w+):\s*([^;]+);', body):
        pointer = re.fullmatch(r'ptr<(.+)>', kind)
        fields.append((field, C.POINTER(TYPES[pointer[1]]) if pointer else TYPES[kind]))
    TYPES[name] = type(name, (C.Structure,), {'_fields_': fields})
Text, Result = TYPES['FixedText'], TYPES['FixedResult']


def text(value):
    data = value.encode()
    owner = (C.c_uint8 * len(data)).from_buffer_copy(data)
    return Text(owner, len(data)), owner


def integer_reference(source):
    value = source.strip(SPACE)
    if value.startswith('#'):
        value = value[1:].strip(SPACE)
    negative = value.startswith('-')
    if negative or value.startswith('+'):
        value = value[1:]
    value = value.strip(SPACE)
    if not value or value[0] not in '0123456789':
        return 2, None
    clean = value.replace('_', '')
    radix, digits = 10, clean
    if clean[:2].lower() == '0x':
        radix, digits = 16, clean[2:]
    elif clean[:2].lower() == '0b':
        radix, digits = 2, clean[2:]
    raw = digits[1:] if digits[:1] in ('+', '-') else digits
    alphabet = '0123456789abcdef'[:radix]
    if not raw or any(ch.lower() not in alphabet for ch in raw):
        return 2, None
    result = int(digits, radix)
    if not MIN <= result <= MAX:
        return 2, None
    if negative:
        result = -result
        if not MIN <= result <= MAX:
            return 3, None
    return 0, result


def operands_reference(source):
    if not source.strip(SPACE):
        return 0, []
    stack, parts, start = [], [], 0
    for at, ch in enumerate(source):
        if ch in '[(':
            stack.append(ch)
        elif ch in '])':
            expected = '[' if ch == ']' else '('
            if not stack or stack.pop() != expected:
                return 15, None
        elif ch == ',' and not stack:
            if len(parts) >= 65536:
                return 17, None
            parts.append(source[start:at].strip(SPACE))
            start = at + 1
    if stack:
        return 16, None
    parts.append(source[start:].strip(SPACE))
    if len(parts) > 65536:
        return 17, None
    if any(not part for part in parts):
        return 18, None
    return 0, parts


def scalar_checks(library, cases, rng):
    scalar = library.whale_fixed_scalar
    scalar.argtypes = [U, C.POINTER(Text), U, C.c_int64, C.c_int64, C.POINTER(Result)]
    retained = ['0', '-0', '# +0_xf', '0x-1', '-0x-1', '0x_+_f', '0b-1',
                '0x-80000000000000000000000000000000', '-0x-80000000000000000000000000000000',
                '-170141183460469231731687303715884105728', '0x+', '0b2', '0x+-1',
                '_1', '# #1', '\u2003- 128\u3000', '\x1c1', '１２', '0__x10', '1__2_3']
    sources = list(retained)
    for _ in range(cases):
        number = rng.randrange(MIN-2, MAX+3)
        radix = rng.choice([2, 10, 16])
        digits = format(abs(number), {2: 'b', 10: 'd', 16: 'x'}[radix])
        prefix = {2: '0b', 10: '', 16: '0x'}[radix]
        if rng.randrange(2):
            digits = '_'.join(digits)
        sign = '-' if number < 0 else rng.choice(['', '+'])
        if radix != 10 and rng.randrange(2):
            source = prefix + sign + digits
        else:
            source = sign + prefix + digits
        if rng.randrange(4) == 0:
            source = '\u2003# ' + source + '\u3000'
        sources.append(source)
    for source in sources:
        raw, keep = text(source)
        expected_error, value = integer_reference(source)
        result = Result(max_work=256 * 1024 * 1024)
        status = scalar(0, C.byref(raw), 0, 0, 0, C.byref(result))
        assert status == result.status and result.work <= result.max_work
        assert result.error == expected_error, (source, result.error, expected_error)
        if not expected_error:
            assert result.word | result.detail << 64 == value & ((1 << 128)-1), source
    for _ in range(cases):
        number = rng.randrange(-(1 << 40), 1 << 40)
        bits = rng.randrange(0, 35)
        raw, keep = text(str(number))
        for mode in [1, 2]:
            expected = 4 if bits == 0 or bits > 32 else 0
            if not expected and mode == 1 and not -(1 << (bits-1)) <= number < 1 << (bits-1):
                expected = 5
            if not expected and mode == 2 and not 0 <= number < 1 << bits:
                expected = 6
            result = Result(max_work=256 * 1024 * 1024)
            assert scalar(mode, C.byref(raw), bits, 0, 0, C.byref(result)) == result.status
            assert result.error == expected
            if not expected:
                assert result.word == number & ((1 << bits)-1)
        alignment = rng.choice([-1, 0, 1, 2, 4, 8])
        result = Result(max_work=256 * 1024 * 1024)
        expected = 7 if bits == 0 or bits > 32 or alignment <= 0 else 0
        if not expected and number % alignment:
            expected = 8
        if not expected and not -(1 << (bits-1)) <= number // alignment < 1 << (bits-1):
            expected = 9
        assert scalar(3, C.byref(raw), bits, number, alignment, C.byref(result)) == result.status
        assert result.error == expected
        if not expected:
            assert result.word == (number // alignment) & ((1 << bits)-1)
    raw, keep = text('1')
    result = Result(max_work=0)
    assert scalar(0, C.byref(raw), 0, 0, 0, C.byref(result)) == 1 and result.error == 93


def operand_checks(library, cases, rng):
    parse = library.whale_fixed_operands
    parse.argtypes = [C.POINTER(Text), C.POINTER(Text), U, C.POINTER(U), C.POINTER(Result)]
    sources = ['', 'a,b', '[a,(b,c)],d', 'a,,b', '(a,b', 'a],b', ',' + '(' * 90,
               '[' * 100 + 'a' + ']' * 100, '[' * 100 + 'a' + ')' * 100,
               ','.join(['a'] * 65536), ','.join(['a'] * 65537)]
    alphabet = 'abé한0123[](), \t\u2003'
    sources += [''.join(rng.choice(alphabet) for _ in range(rng.randrange(80)))
                for _ in range(cases)]
    for source in sources:
        expected, parts = operands_reference(source)
        raw, keep = text(source)
        result, count = Result(max_work=256 * 1024 * 1024), U()
        status = parse(C.byref(raw), None, 0, C.byref(count), C.byref(result))
        assert status == result.status and result.error == expected, (source[:100], result.error, expected)
        if expected:
            continue
        assert count.value == len(parts)
        output = (Text * count.value)()
        result = Result(max_work=256 * 1024 * 1024)
        assert parse(C.byref(raw), output, len(output), C.byref(count), C.byref(result)) == 0
        actual = [C.string_at(value.data, value.count).decode() for value in output]
        assert actual == parts, source
        if parts:
            sentinel = (Text * 1)()
            C.memset(sentinel, 0xA5, C.sizeof(sentinel))
            before = bytes(sentinel)
            result = Result(max_work=256 * 1024 * 1024)
            assert parse(C.byref(raw), sentinel, 0, C.byref(count), C.byref(result)) == 2
            assert bytes(sentinel) == before


    for source in [' ' * 100, '\u2003' * 100]:
        raw, keep = text(source)
        result, count = Result(max_work=0), U()
        assert parse(C.byref(raw), None, 0, C.byref(count), C.byref(result)) == 1
        assert result.error == 93


def probe_checks(library):
    probe = library.whale_fixed_probe
    probe.argtypes = [C.POINTER(Text), U, C.POINTER(Result)]
    def inspect(source, comments=1):
        raw, keep = text(source)
        result = Result(max_work=256 * 1024 * 1024)
        assert probe(C.byref(raw), comments, C.byref(result)) == 0
        return result
    result = inspect('.word ' + ','.join(['0'] * 60000))
    assert (result.symbols, result.records, result.operands, result.relocations) == (0, 1, 60000, 0)
    result = inspect('nop\n' * 300000)
    assert result.records == result.relocations == 262145 and result.symbols == 0
    assert inspect('; a,b,c\n# x:y,z\n').records == 0
    assert inspect('add x0,x1,#1\n', comments=0).operands >= 3
    result = inspect('.global foo,bar\nfoo: nop\n.extern bar\n.quad foo,0,bar+4')
    assert result.symbols >= 4 and result.relocations == 3
    result = inspect('left:right:nop\n.section custom\n.data\n.word 0')
    assert result.sections == 3 and result.symbols >= 2 and result.records == 2
    raw, keep = text(';' * (8 * 1024 * 1024 + 1))
    result = Result(max_work=256 * 1024 * 1024)
    assert probe(C.byref(raw), 1, C.byref(result)) == 1 and result.error == 41


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--library', type=Path, required=True)
    parser.add_argument('--cases', type=int, default=10000)
    args = parser.parse_args()
    library = C.CDLL(str(args.library.resolve()))
    rng = random.Random(0x5748414C45465836)
    scalar_checks(library, args.cases, rng)
    operand_checks(library, args.cases, rng)
    probe_checks(library)
    print(f'Wave fixed core: {args.cases} integer literals + {args.cases * 3} immediate/displacement vectors + {args.cases} operand streams, radix-sign quirks, Unicode trim, probe/resource and transactional-capacity regressions passed')


if __name__ == '__main__':
    main()
