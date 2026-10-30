# SPDX-License-Identifier: MPL-2.0
"""Independent transport helpers for native Wave semantic/SSA oracle checks."""

import ctypes as C


NONE = (1 << 64) - 1
SIGNED = {2, 4, 5, 6, 7, 8}
UNSIGNED = {3, 9, 10, 11, 12, 13}
INTEGERS = SIGNED | UNSIGNED
FLOATS = {14, 15, 16}
WIDTHS = {2: 1, 3: 1, 4: 8, 5: 16, 6: 32, 7: 64, 8: 128,
          9: 8, 10: 16, 11: 32, 12: 64, 13: 128, 14: 16, 15: 32, 16: 64}


class Node(C.Structure):
    _fields_ = [(name, C.c_uint64) for name in ("kind", "child", "first", "count", "extra")]


class Operand(C.Structure):
    _fields_ = [(name, C.c_uint64) for name in ("value", "ty", "literal", "low", "high")]


class Operation(C.Structure):
    _fields_ = [(name, C.c_uint64) for name in ("op", "sub", "ty", "other", "dst", "align")]


class Result(C.Structure):
    _fields_ = [(name, C.c_uint64) for name in
               ("status", "index", "expected", "actual", "detail", "work")]


SCALARS = tuple(Node(kind, 0, 0, 0, 0) for kind in range(17))


def load(path):
    library = C.CDLL(str(path))
    kernel = library.whale_ir_verify_semantics
    kernel.argtypes = [C.POINTER(Node), C.c_uint64, C.POINTER(C.c_uint64), C.c_uint64,
                       C.POINTER(Operation), C.POINTER(Operand), C.c_uint64,
                       C.c_uint64, C.POINTER(Result)]
    kernel.restype = C.c_int32
    return library, kernel


def call(kernel, op, ty, operands=(), sub=0, other=0, align=1,
         nodes=SCALARS, edges=(), work=1_000_000, dst=0):
    node_array = (Node * len(nodes))(*nodes)
    operand_array = (Operand * len(operands))(*operands)
    edge_array = (C.c_uint64 * len(edges))(*edges)
    result = Result()
    operation = Operation(op, sub, ty, other, dst, align)
    status = kernel(node_array, len(nodes), edge_array, len(edges), C.byref(operation),
                    operand_array, len(operands), work, C.byref(result))
    assert status == result.status, (status, result.status)
    assert result.work <= work, (result.work, work)
    return status, result


def encode_type(ty, nodes, edges):
    child = first = count = extra = 0
    if isinstance(ty, int):
        kind = ty
    elif ty[0] in ("p", "a"):
        kind = 17 if ty[0] == "p" else 19
        child = encode_type(ty[1], nodes, edges)
        count = ty[2] if ty[0] == "a" else 0
    else:
        kind = {"f": 18, "s": 20, "t": 21}[ty[0]]
        children = [ty[3], *ty[4]] if ty[0] == "f" else ty[1]
        children = [encode_type(item, nodes, edges) for item in children]
        first, count = len(edges), len(children)
        edges.extend(children)
        if ty[0] == "f":
            extra = ty[1] | (int(ty[2]) << 1)
    nodes.append(Node(kind, child, first, count, extra))
    return len(nodes) - 1


def typed_call(kernel, op, ty, other=0, args=(), sub=0, dst=0, work=1_000_000):
    nodes, edges, operands = [], [], []
    root = encode_type(ty, nodes, edges)
    other = encode_type(other, nodes, edges)
    for value, argument in enumerate(args):
        operands.append(Operand(value, encode_type(argument, nodes, edges), 0, 0, 0))
    return call(kernel, op, root, operands, sub=sub, other=other, nodes=nodes,
                edges=edges, dst=dst, work=work)


def signature_type_valid(ty):
    if isinstance(ty, int):
        return ty != 0
    if ty[0] == "f":
        return signature_valid(ty)
    if ty[0] == "p" and ty[1] == 0:
        return True
    if ty[0] in ("p", "a"):
        return signature_type_valid(ty[1])
    return all(signature_type_valid(child) for child in ty[1])


def signature_valid(signature):
    _, convention, variadic, ret, params = signature
    if variadic:
        return False
    if convention == 1 and any(isinstance(ty, tuple) and ty[0] in ("a", "s", "t")
                               for ty in [ret, *params]):
        return False
    return (ret == 0 or signature_type_valid(ret)) and all(map(signature_type_valid, params))


def random_type(randomizer, depth):
    if depth == 0 or randomizer.randrange(2) == 0:
        return randomizer.randrange(17)
    kind = randomizer.choice(("p", "a", "s", "t", "f"))
    if kind == "p":
        return kind, random_type(randomizer, depth - 1)
    if kind == "a":
        return kind, random_type(randomizer, depth - 1), randomizer.randrange(6)
    if kind in ("s", "t"):
        return kind, [random_type(randomizer, depth - 1) for _ in range(randomizer.randrange(3))]
    return (kind, randomizer.randrange(2), randomizer.randrange(5) == 0,
            random_type(randomizer, depth - 1),
            [random_type(randomizer, depth - 1) for _ in range(randomizer.randrange(3))])
