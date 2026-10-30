#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Independent generated instruction records for the native Wave grammar."""
import argparse
import ctypes as c
import random
import re
from check_wave_ir_type import Token, Node as TypeNode, Result as TypeResult


class State(c.Structure):
    _fields_ = [(key, c.c_uint64) for key in
                ("phase", "at", "format", "mode", "op", "sub", "op_at", "has_dst", "dst",
                 "annotation_kind", "type_kind", "a", "b", "c", "d", "e", "name", "aux", "nodes", "work")]


class Event(c.Structure):
    _fields_ = [(key, c.c_uint64) for key in
                ("status", "reason", "at", "expected", "event", "role", "op", "sub", "has_dst", "dst",
                 "a", "b", "c", "d", "e", "name", "aux", "next", "nodes", "work")]


BINARY = "add sub mul udiv sdiv urem srem fadd fsub fmul fdiv frem and or xor shl lshr ashr".split()
CAST = "checked_int zext sext trunc fext ftrunc itof_s itof_u ftoi_s ftoi_u bitcast ptrtoint inttoptr".split()
CHECKED = "uadd_chk usub_chk umul_chk sadd_chk ssub_chk smul_chk".split()
PREDICATES = ["eq ne slt sle sgt sge ult ule ugt uge feq fne flt fle fgt fge".split(),
              "eq ne ult ule ugt uge slt sle sgt sge".split(),
              "oeq one olt ole ogt oge ord uno ueq une ult ule ugt uge".split()]
MARKS = {bytes((value,)) for value in b":=,[]()<>"} | {b"=>", b"->"}
TOKEN = re.compile(rb'"(?:\\.|[^"\\])*"|=>|->|[\[\]()<>:,=]|[^\s\[\]()<>:,="]+')


def decode_type(nodes):
    stack = []
    for node in nodes:
        if node.kind <= 16:
            stack.append((node.kind,))
        elif node.kind in (17, 19):
            child = stack.pop()
            stack.append((17, child) if node.kind == 17 else (19, node.value, child))
        elif node.kind == 18:
            ret = stack.pop()
            params = tuple(stack[-node.count:]) if node.count else ()
            if node.count:
                del stack[-node.count:]
            stack.append((18, node.value, params, ret))
        else:
            fields = tuple(stack[-node.count:]) if node.count else ()
            if node.count:
                del stack[-node.count:]
            stack.append((node.kind, fields))
    assert len(stack) == 1
    return stack[0]


def generated(rng, choice=None):
    op = rng.randrange(36) if choice is None else choice
    a, b, cc, dst = [rng.randrange(1 << 32) for _ in range(4)]
    prefix = lambda ty: f"%v{dst}: {ty} = "
    fields = {"op": op}
    phi, values, cases = [], [], []
    term = op >= 31
    if not term:
        fields.update(has_dst=int(op not in (1, 20, 22, 23, 25, 26, 30)), dst=dst)
    if op == 0:
        text = prefix("ptr<i32>") + f"global_addr @g{a}"; fields["a"] = a
    elif op == 1:
        text = f"lifetime_end %v{a}"; fields["a"] = a
    elif op == 2:
        text = prefix("i128") + f"ptrdiff %v{a}, %v{b}"; fields.update(a=a, b=b)
    elif op == 3:
        text = prefix("i32") + 'const_decl "name" i32 7 => const i32 7'
    elif op == 4:
        text = prefix("i32") + f"const i32 {rng.randrange(-(1 << 31), 1 << 31)}"
    elif op == 5:
        text = prefix("i32") + "undef i32"
    elif op in (6, 8, 9):
        text = prefix("i32") + f"{ {6: 'mov', 8: 'neg', 9: 'not'}[op]} i32 %v{a}"; fields["a"] = a
    elif op == 7:
        sub = rng.randrange(len(BINARY)); fields.update(sub=sub, a=a, b=b)
        text = prefix("i32") + f"{BINARY[sub]} i32 %v{a}, %v{b}"
    elif op in (10, 11, 12):
        predicates = PREDICATES[op - 10]; sub = rng.randrange(len(predicates))
        fields.update(sub=sub, a=a, b=b)
        text = prefix("bool") + f"{['cmp', 'icmp', 'fcmp'][op - 10]} {predicates[sub]} i32 %v{a}, %v{b}"
    elif op == 13:
        text = prefix("i32") + f"select bool %v{a}, i32 %v{b}, i32 %v{cc}"; fields.update(a=a, b=b, c=cc)
    elif op == 14:
        sub = rng.randrange(len(CAST)); fields.update(sub=sub, a=a)
        text = prefix("i64") + f"{CAST[sub]} i32 %v{a} to i64"
    elif op == 15:
        phi = [(rng.randrange(1 << 32), rng.randrange(1 << 32)) for _ in range(rng.randrange(20))]
        text = prefix("i32") + "phi i32 " + ", ".join(f"[%v{v}, %b{bb}]" for v, bb in phi)
    elif op == 16:
        index = rng.randrange(1 << 32); fields.update(a=a, d=index)
        text = prefix("i32") + f"extract %v{a}, {index}"
    elif op == 17:
        sub = rng.randrange(len(CHECKED)); fields.update(sub=sub, a=a, b=b)
        text = prefix("tuple<i32, bool>") + f"{CHECKED[sub]} i32 %v{a}, %v{b}"
    elif op == 18:
        fields["d"] = 4; text = prefix("ptr<i32>") + "alloca i32, align 4"
    elif op in (19, 20, 21, 22, 23):
        name = {19: "volatile_load", 20: "volatile_store", 21: "load", 22: "uninit", 23: "store"}[op]
        fields.update(b=b, d=4)
        if op in (19, 21): text = prefix("i32") + f"{name} i32, ptr<i32> %v{b}, align 4"
        elif op == 22: text = f"{name} i32, ptr<i32> %v{b}, align 4"
        else: fields["a"] = a; text = f"{name} i32 %v{a}, ptr<i32> %v{b}, align 4"
    elif op == 24:
        values = [rng.randrange(1 << 32) for _ in range(rng.randrange(20))]
        fields["a"] = a; text = prefix("ptr<i32>") + f"gep %v{a}" + "".join(f", %v{v}" for v in values)
    elif op in (25, 26):
        fields.update(a=a, b=b, c=cc, d=1)
        text = f"{'memcpy' if op == 25 else 'memset'} ptr<u8> %v{a}, {'ptr<u8>' if op == 25 else 'u8'} %v{b}, u64 %v{cc}, align 1"
    elif op in (27, 28):
        text = prefix("fnptr<whale () -> void>") + ("null_function" if op == 27 else f"function_addr @f{a}")
        if op == 28: fields["a"] = a
    elif op == 29:
        values = [rng.randrange(1 << 32) for _ in range(rng.randrange(20))]
        returns = bool(rng.randrange(2)); indirect = rng.randrange(2); sub = rng.randrange(2)
        fields.update(a=a, e=indirect, sub=sub, has_dst=int(returns))
        text = (prefix("i32") if returns else "") + f"call {'whale' if sub == 0 else 'sysv64'} {'i32' if returns else 'void'} " + (f"indirect %v{a}" if indirect else f"@f{a}") + "(" + ", ".join(f"%v{v}" for v in values) + ")"
    elif op == 30:
        text = f'trap_if bool %v{a}, reason = "trap"'; fields["a"] = a
    elif op == 31:
        text = f"br label %b{a}"; fields["a"] = a
    elif op == 32:
        text = f"cbr bool %v{a}, label %b{b}, label %b{cc}"; fields.update(a=a, b=b, c=cc)
    elif op == 33:
        cases = [(rng.randrange(-(1 << 31), 1 << 31), rng.randrange(1 << 32)) for _ in range(rng.randrange(20))]
        fields.update(a=a, b=b)
        text = f"switch i32 %v{a}, label %b{b} [" + ", ".join(f"{v}: %b{bb}" for v, bb in cases) + "]"
    elif op == 34:
        returns = bool(rng.randrange(2)); fields["aux"] = int(returns)
        if returns: fields["a"] = a
        text = f"ret i32 %v{a}" if returns else "ret void"
    else:
        text = 'trap reason = "trap"'
    if not term and fields["has_dst"] == 0:
        fields.pop("dst", None)
    return text, term, fields, phi, values, cases


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", required=True)
    parser.add_argument("--cases", type=int, default=10000)
    args = parser.parse_args()
    library = c.CDLL(args.library)
    kernel = library.whale_ir_instruction_step
    kernel.argtypes = [c.c_void_p, c.c_uint64, c.POINTER(State), c.c_uint64, c.c_uint64, c.POINTER(Event)]
    type_kernel = library.whale_ir_parse_type
    type_kernel.argtypes = [c.c_void_p] + [c.c_uint64] * 7 + [c.c_void_p, c.c_uint64, c.c_void_p, c.c_uint64, c.POINTER(TypeResult)]

    def call(case, max_nodes=1000000, max_work=16000000):
        text, term, expected, phi, values, cases = case
        # Borrow one stable byte buffer. Wide fixtures must not allocate a
        # ctypes owner/string per token or expand every token as an argument.
        encoded = text.encode("ascii")
        token_count = sum(1 for _ in TOKEN.finditer(encoded))
        source = c.create_string_buffer(encoded)
        source_address = c.addressof(source)
        raw = (Token * (token_count + 1))()
        for index, match in enumerate(TOKEN.finditer(encoded)):
            token = raw[index]
            token.data = source_address + match.start()
            token.length = match.end() - match.start()
            token.kind = 1 if encoded[match.start()] == 34 else 2 if match.group() in MARKS else 0
        raw[token_count].kind = 3

        def word(index):
            token = raw[index]
            start = token.data - source_address
            return encoded[start:start + token.length].decode("ascii")
        state = State(format=8, mode=int(term))
        types = [None, None, None]
        expression_type = None
        got_phi, got_values, got_cases = [], [], []
        literal = None
        used_nodes = used_work = 0

        def ty(start):
            scratch, output = (TypeNode * 256)(), (TypeNode * 256)()
            result = TypeResult()
            status = type_kernel(raw, len(raw), start, 0, 128, max_nodes - used_nodes,
                                 max_work - used_work, 1, scratch, 256, output, 256, c.byref(result))
            assert status == result.status == 0, (text, status, result.reason)
            return result, decode_type(output[:result.nodes])

        for _ in range(token_count * 2 + 100):
            event = Event()
            status = kernel(raw, len(raw), c.byref(state), max_nodes - used_nodes, max_work - used_work, c.byref(event))
            assert status == event.status
            used_nodes += event.nodes; used_work += event.work
            if status:
                return status
            if event.event == 100:
                result, parsed = ty(state.at)
                state.at = result.next; state.type_kind = parsed[0]; types[event.role] = parsed
                used_nodes += result.nodes; used_work += result.work
            elif event.event == 101:
                spelling = word(state.at)
                literal = int(spelling)
                assert types[1] == (6,) and -(1 << 31) <= literal < 1 << 31
                state.at += 1
            elif event.event == 102:
                result, expression_type = ty(state.at)
                state.at = result.next + 1  # generated preserved expression is i32 7
                assert word(result.next) == "7"
                used_nodes += result.nodes + 1; used_work += result.work + 1
            elif event.event == 110:
                left, right = {
                    0: (types[0], types[1]), 1: (types[0], (1,)), 2: (types[0], (8,)),
                    3: (types[0], (17, types[1])), 4: (types[0], (21, (types[1], (1,)))),
                    5: (types[0], types[2]), 6: (types[1], types[2]),
                    7: ((17, types[1]), types[2]), 8: (types[1], expression_type),
                    9: (types[0], expression_type),
                }[event.role]
                assert left == right, (text, event.role, left, right)
            elif event.event == 200:
                got_phi.append((event.a, event.b))
            elif event.event == 201:
                got_values.append(event.b)
            elif event.event == 202:
                got_cases.append((literal, event.c))
            elif event.event == 1:
                assert state.at == token_count, (text, state.at, token_count)
                assert all(getattr(event, key) == value for key, value in expected.items()), (text, expected)
                assert (got_phi, got_values, got_cases) == (phi, values, cases), text
                return 0
            else:
                raise AssertionError((text, event.event))
        raise AssertionError("instruction parser failed to make bounded progress")

    rng = random.Random(349)
    for choice in range(36):
        assert call(generated(rng, choice)) == 0
    for _ in range(args.cases):
        assert call(generated(rng)) == 0
    count = 60000
    phi = [(i, i) for i in range(count)]
    text = "%v2: i32 = phi i32 " + ", ".join(f"[%v{v}, %b{bb}]" for v, bb in phi)
    assert call((text, False, {"op": 15, "has_dst": 1, "dst": 2}, phi, [], [])) == 0
    assert call(("lifetime_end %v0", False, {"op": 1}, [], [], []), max_work=0) == 4
    print(f"Wave IR instruction grammar: {args.cases} generated records, full inventory and 60000-incoming stress passed")


if __name__ == "__main__":
    main()
