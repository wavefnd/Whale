#!/usr/bin/env python3
"""Check the actual Wave AMD64 grammar ABI with independent bounded fixtures."""
import argparse
import ctypes as c
import json
import random

U = c.c_uint64
P = c.c_void_p
MISSING = (1 << 64) - 1
MIN128 = -(1 << 127)
MAX128 = (1 << 127) - 1


class Name(c.Structure):
    _fields_ = [("bytes", P), ("count", U)]


class Expr(c.Structure):
    _fields_ = [(field, U) for field in ("kind", "name", "low", "high")]


class Operand(c.Structure):
    _fields_ = [(field, U) for field in (
        "kind", "name", "low", "high", "mode", "width", "base", "index", "scale", "disp", "symbol")]


class Item(c.Structure):
    _fields_ = [(field, U) for field in ("kind", "name", "aux", "flags")]
    _fields_ += [("expression", Expr), ("first", U), ("count", U)]


class Directive(c.Structure):
    _fields_ = [("kind", U), ("expression", Expr), ("bytes", P), ("count", U)]


class Token(c.Structure):
    _fields_ = [("kind", U), ("position", U), ("end", U), ("text", Name), ("low", U), ("high", U)]


class Span(c.Structure):
    _fields_ = [("start", U), ("end", U)]


class Input(c.Structure):
    _fields_ = [("tokens", P), ("count", U), ("mode", U), ("max_work", U)]


class Output(c.Structure):
    _fields_ = [(field, ty) for name in ("item", "span", "operand", "value", "name", "byte")
               for field, ty in ((name + "s", P), (name + "_capacity", U))]


class Result(c.Structure):
    _fields_ = [(field, U) for field in (
        "status", "error", "index", "detail", "items", "operands", "values", "names", "bytes", "work")]


PUNCTUATION = {",": 3, ":": 4, "[": 5, "]": 6, "+": 7, "-": 8, "*": 9, "/": 10, "\n": 11}


def signed(low, high):
    value = low | (high << 64)
    return value - (1 << 128) if high >> 63 else value


class Packet:
    def __init__(self, library, atoms):
        self.buffers = []
        self.tokens = (Token * len(atoms))()
        position = 0
        payload = 0
        for token, atom in zip(self.tokens, atoms):
            token.position = position
            spelling = str(atom).encode("utf-8")
            position += len(spelling)
            token.end = position
            position += 1
            if isinstance(atom, int):
                token.kind = 1
                bits = atom & ((1 << 128) - 1)
                token.low = bits & MISSING
                token.high = bits >> 64
            elif atom in PUNCTUATION:
                token.kind = PUNCTUATION[atom]
            else:
                token.kind = 0
                buffer = c.create_string_buffer(spelling)
                self.buffers.append(buffer)
                token.text = Name(c.addressof(buffer), len(spelling))
                payload += len(spelling)
        self.input = Input(c.addressof(self.tokens), len(atoms), 0, 16 * (len(atoms) + payload + 1))
        self.result = Result()
        empty = Output()
        status = library.whale_amd64_parse(c.byref(self.input), c.byref(empty), c.byref(self.result))
        assert status == self.result.status, (status, self.result.status)
        assert self.result.work <= self.input.max_work
        if status:
            assert status == 1, (status, atoms)
            return
        assert max(self.result.items, self.result.operands, self.result.values, self.result.names) <= len(atoms)
        assert self.result.bytes <= payload
        self.items = (Item * self.result.items)()
        self.spans = (Span * self.result.items)()
        self.operands = (Operand * self.result.operands)()
        self.values = (Directive * self.result.values)()
        self.names = (Name * self.result.names)()
        self.bytes = (c.c_ubyte * self.result.bytes)()
        output = Output(
            c.addressof(self.items), len(self.items), c.addressof(self.spans), len(self.spans),
            c.addressof(self.operands), len(self.operands), c.addressof(self.values), len(self.values),
            c.addressof(self.names), len(self.names), c.addressof(self.bytes), len(self.bytes))
        published = Result()
        self.input.mode = 1
        status = library.whale_amd64_parse(c.byref(self.input), c.byref(output), c.byref(published))
        assert status == 0 and tuple(getattr(published, field) for field, _ in Result._fields_) == tuple(
            getattr(self.result, field) for field, _ in Result._fields_)

    def name(self, index):
        name = self.names[index]
        return c.string_at(name.bytes, name.count).decode("utf-8")


def check(library, cases):
    library.whale_amd64_parse.argtypes = [c.POINTER(Input), c.POINTER(Output), c.POINTER(Result)]
    library.whale_amd64_parse.restype = c.c_int
    assert [c.sizeof(record) for record in (Name, Expr, Operand, Item, Directive, Token, Span, Input, Output, Result)] == [16, 32, 88, 80, 56, 56, 16, 32, 96, 80]
    randomizer = random.Random(0x357_128)
    integers = [MIN128, MIN128 + 1, -(1 << 63), -1, 0, 1, (1 << 64) - 1, MAX128]
    for iteration in range(cases):
        if iteration < 64:
            a, b = randomizer.choice(integers), randomizer.choice(integers)
        elif iteration % 3 == 0:
            a, b = (randomizer.randrange(MIN128, MAX128 + 1) for _ in range(2))
        elif iteration % 3 == 1:
            a, b = (randomizer.randrange(-(1 << 63), 1 << 64) for _ in range(2))
        else:
            a = randomizer.randrange(MIN128 + (1 << 64), MAX128 - (1 << 64))
            b = -a + randomizer.randrange(-(1 << 63), 1 << 64)
        packet = Packet(library, ["mov", a, "+", b])
        total = a + b
        if not MIN128 <= total <= MAX128:
            assert (packet.result.status, packet.result.error, packet.result.index) == (1, 28, 3)
        elif not -(1 << 63) <= total <= (1 << 64) - 1:
            assert (packet.result.status, packet.result.error, packet.result.index) == (1, 34, 3)
        else:
            assert packet.result.status == 0 and len(packet.items) == 1
            operand = packet.operands[0]
            assert operand.kind == 1 and signed(operand.low, operand.high) == total
        symbolic = Packet(library, ["mov", "Target", "+", a, "+", b])
        if MIN128 <= total <= MAX128:
            assert symbolic.result.status == 0
            operand = symbolic.operands[0]
            assert operand.kind == (2 if total == 0 else 3)
            assert symbolic.name(operand.name) == "Target"
            assert signed(operand.low, operand.high) == total
        else:
            assert (symbolic.result.status, symbolic.result.error, symbolic.result.index) == (1, 35, 5)
    registers = ["rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi"] + [f"r{index}" for index in range(8, 16)]
    memory_cases = 0
    for base in registers:
        for index in registers:
            for scale in (1, 2, 4, 8):
                packet = Packet(library, ["MOV", "qword", "[", base.upper(), "+", index, "*", scale, "+", "Data", "-", 7, "]"])
                assert packet.result.status == 0
                memory = packet.operands[0]
                assert memory.kind == 4 and memory.mode == 0 and memory.width == 64
                assert packet.name(memory.base) == base and packet.name(memory.index) == index
                assert packet.name(memory.symbol) == "Data" and memory.scale == scale
                assert memory.disp == (1 << 64) - 7
                memory_cases += 1
    malformed = Input(None, MISSING, 0, 1)
    result = Result()
    output = Output()
    assert library.whale_amd64_parse(c.byref(malformed), c.byref(output), c.byref(result)) == 2
    assert result.status == 2 and result.error == 0
    return {"numeric_cases": cases * 2, "memory_cases": memory_cases, "malformed_spans": 1}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", required=True)
    parser.add_argument("--cases", type=int, default=10000)
    args = parser.parse_args()
    if not 1 <= args.cases <= 100000:
        parser.error("--cases must be between 1 and 100000")
    print(json.dumps(check(c.CDLL(args.library), args.cases), sort_keys=True))


if __name__ == "__main__":
    main()
