#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Exercise real Wave canonical output independently, including bounded resume."""
import argparse
import ctypes as C
import random


class State(C.Structure):
    _fields_ = [(name, C.c_void_p if name in ("nodes", "edges", "stack") else C.c_uint64)
                for name in ("nodes", "node_count", "edges", "edge_count", "stack",
                             "stack_capacity", "stack_len", "output_limit", "total", "work",
                             "depth_limit", "status", "initialized")]


def render(kernel, nodes, edges=(), *, root=None, chunk=7, output_limit=1_000_000,
           work=10_000_000, stack_capacity=1024, depth=256):
    raw = (C.c_uint64 * (len(nodes) * 12))(*[word for node in nodes
                                          for word in (*node, *((0,) * (12 - len(node))))])
    edge_array = (C.c_uint64 * len(edges))(*edges)
    stack = (C.c_uint64 * (stack_capacity * 10))()
    state = State(C.addressof(raw), len(nodes), C.addressof(edge_array), len(edges),
                  C.addressof(stack), stack_capacity, 0, output_limit, 0, work, depth, 0, 0)
    root = len(nodes) - 1 if root is None else root
    result = bytearray()
    while True:
        # Canaries prove that every step respects the caller's exact span.
        buffer = (C.c_ubyte * (chunk + 2))(*([0xAA] * (chunk + 2)))
        written = C.c_uint64()
        status = kernel(C.byref(state), root, C.byref(buffer, 1), chunk, C.byref(written))
        assert written.value <= chunk
        assert buffer[0] == buffer[chunk + 1] == 0xAA
        result.extend(buffer[1:1 + written.value])
        assert len(result) <= output_limit and state.total == len(result)
        if status != 6:
            return status, bytes(result)
        assert chunk > 0


def escaped(text):
    result = '"'
    for character in text:
        cp = ord(character)
        if character in ('\\', '"'):
            result += '\\' + character
        elif cp in (0, 9, 10, 13):
            result += {0: '\\0', 9: '\\t', 10: '\\n', 13: '\\r'}[cp]
        elif cp < 32 or 127 <= cp <= 159 or cp in (0x2028, 0x2029):
            result += f'\\u{{{cp:x}}}'
        else:
            result += character
    return (result + '"').encode()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", required=True)
    parser.add_argument("--cases", type=int, default=10000)
    args = parser.parse_args()
    kernel = C.CDLL(args.library).whale_ir_print_step
    kernel.argtypes = [C.POINTER(State), C.c_uint64, C.c_void_p, C.c_uint64,
                       C.POINTER(C.c_uint64)]
    kernel.restype = C.c_int32
    randomizer = random.Random(0x349)
    values = [0, 1, (1 << 64) - 1, 1 << 64, (1 << 127) - 1, 1 << 127, (1 << 128) - 1]
    values += [randomizer.getrandbits(128) for _ in range(args.cases)]
    for value in values:
        low, high = value & ((1 << 64) - 1), value >> 64
        for tag, integer in ((32, value if value < 1 << 127 else value - (1 << 128)),
                             (33, value)):
            expected = str(integer).encode()
            assert render(kernel, [(tag, low, high)], chunk=1 + value % 41) == (0, expected)
            assert render(kernel, [(tag, low, high)], output_limit=len(expected)) == (0, expected)
            status, prefix = render(kernel, [(tag, low, high)], output_limit=len(expected) - 1)
            assert status == 2 and prefix == expected[:-1]
        width = randomizer.choice((16, 32, 64))
        bits = low & ((1 << width) - 1)
        expected = f'0x{bits:0{width // 4}x}'.encode()
        assert render(kernel, [(34, bits, width)]) == (0, expected)
    alphabet = [chr(cp) for cp in range(160)] + ['\u2028', '\u2029', '가', '😀', '\U0010ffff']
    strings = [''.join(alphabet), '', '\\"\n', 'abc' * 20000]
    strings += [''.join(randomizer.choices(alphabet, k=randomizer.randrange(50)))
                for _ in range(args.cases // 10)]
    for text in strings:
        data = C.create_string_buffer(text.encode())
        assert render(kernel, [(30, C.addressof(data), len(text.encode()))], chunk=13) == (0, escaped(text))
    assert render(kernel, [(18, 0)], depth=32)[0] == 5  # cycle
    assert render(kernel, [(18, 1)])[0] == 1           # invalid child
    assert render(kernel, [(1,)], work=0)[0] == 4
    assert render(kernel, [(1,)], stack_capacity=0)[0] == 3
    assert render(kernel, [(51, 0, 2, 0)], edges=[9, 9])[0] == 1
    assert render(kernel, [(51, 0, 1, 0)])[0] == 1
    malformed = C.create_string_buffer(b'\xff')
    assert render(kernel, [(30, C.addressof(malformed), 1)])[0] == 1
    nodes = [(1,), (51, 0, 60000, 0)]
    assert render(kernel, nodes, [0] * 60000, chunk=1024, stack_capacity=32) == (0, b'void' * 60000)
    print(f"Wave IR printer: {len(values)} 128-bit vectors, float bits, {len(strings)} UTF-8 strings, limits and 60000-node stream passed")


if __name__ == "__main__":
    main()
