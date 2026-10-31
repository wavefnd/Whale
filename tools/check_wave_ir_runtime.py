#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Independent generated call/control streams for the actual Wave runtime."""
import argparse
import ctypes as C
from pathlib import Path
import random
import re

from check_wave_ir_memory import (Memory, Pointer, Request, Part, Allocation,
                                  State, Limits, Fragment, Result, Gep, U, MASK)


def record(name, fields):
    return type(name, (C.Structure,), {"_fields_": [(field, U) for field in fields.split()]})


TYPES = {
    "u64": U, "u8": C.c_uint8,
    "IrVerifyType": record("Type", "kind child first count extra"),
    "IrStorage": record("Storage", "ptr_bits pointer_align scalar_align1 scalar_align2 scalar_align4 scalar_align8 scalar_align16 large_array_threshold large_array_align has_large_array"),
    "IrTypeNode": record("LayoutNode", "kind child first count"),
    "IrTypeLayout": record("Layout", "size align stride allocation_align depth"),
    "IrMemPointer": Pointer,
    "IrMemGep": Gep,
}
# This reads only the record ABI. All generated operations and expected values below
# are independent of Wave instruction implementation and Rust reference execution.
for name, body in re.findall(r"struct (IrRuntime\w+)\s*\{([^}]+)\}",
                             (Path(__file__).resolve().parents[1] / "ir/runtime_model.wave").read_text()):
    fields = []
    for field, kind in re.findall(r"(\w+):\s*([^;]+);", body):
        pointer = re.fullmatch(r"ptr<(.+)>", kind)
        fields.append((field, C.POINTER(TYPES[pointer[1]]) if pointer else TYPES[kind]))
    TYPES[name] = type(name, (C.Structure,), {"_fields_": fields})
V, I, F, B, D, R = [TYPES["IrRuntime" + name] for name in
                     ("Value", "Instruction", "Function", "Block", "Driver", "Result")]


def scalar(value, ty=1):
    return V(kind=1, ty=ty, literal=2, low=value & MASK, origin=1)


class Program:
    def __init__(self, functions, blocks, instructions, operands, value_types, arguments, entry,
                 max_steps=100000, max_depth=128, max_frames=100000, max_values=10000):
        self.keep = []
        self.result = R()
        self.driver = D(entry_id=entry, max_steps=max_steps, max_depth=max_depth,
                        max_frames=max_frames, max_values=max_values, max_work=10000000,
                        scalar_arguments=1, result=C.pointer(self.result))
        d = self.driver
        types = [TYPES["IrVerifyType"](kind=0), TYPES["IrVerifyType"](kind=12),
                 TYPES["IrVerifyType"](kind=1), TYPES["IrVerifyType"](kind=17, child=1),
                 TYPES["IrVerifyType"](kind=18, first=0, count=2)]
        self.array("types", types, TYPES["IrVerifyType"])
        self.array("type_edges", [1, 1], U)
        self.array("functions", functions, F)
        self.array("blocks", blocks, B)
        self.array("instructions", instructions, I)
        self.array("operands", operands, U)
        self.array("value_types", value_types, U)
        self.array("globals", [], TYPES["IrRuntimeGlobal"])
        self.array("global_values", [], V, count_field=None)
        self.array("arguments", [scalar(a) for a in arguments], V)
        for field, count, kind in [
            ("layout_nodes", len(types), TYPES["IrTypeNode"]),
            ("layouts", len(types), TYPES["IrTypeLayout"]), ("layout_offsets", 2, U),
            ("plans", len(operands), Gep), ("ordinals", len(value_types), U),
            ("function_table", 1 << max(1, (len(functions) * 2 - 1).bit_length()), U),
            ("prepared", len(functions), C.c_uint8),
            ("pending_previous", len(functions), U), ("pending_next", len(functions), U),
            ("pending_flags", len(functions), C.c_uint8),
            ("callable_flags", len(functions), C.c_uint8),
            ("callable_targets", len(functions), U),
            ("indirect_signatures", len(instructions), U),
            ("indirect_sites", len(instructions), U),
            ("frames", 0, TYPES["IrRuntimeFrame"]),
            ("slots", 0, TYPES["IrRuntimeSlot"]), ("phis", 0, V),
        ]:
            self.array(field, [kind() for _ in range(count)], kind, count_field=None)
        d.plan_capacity = len(operands)
        d.function_table_capacity = len(self.function_table)
        d.indirect_capacity = len(instructions)
        self.storage = TYPES["IrStorage"](64, 8, 1, 2, 4, 8, 16, 16, 16, 1)
        d.storage = C.pointer(self.storage)

    def array(self, field, values, kind, count_field="auto"):
        data = (kind * len(values))(*values)
        setattr(self, field, data)
        self.keep.append(data)
        setattr(self.driver, field, data)
        if count_field == "auto":
            count_field = {"types": "type_count", "type_edges": "type_edge_count",
                           "functions": "function_count", "blocks": "block_count",
                           "instructions": "instruction_count", "operands": "operand_count",
                           "value_types": "value_type_count", "globals": "global_count",
                           "arguments": "argument_count"}[field]
        if count_field:
            setattr(self.driver, count_field, len(values))

    def run(self, runtime, memory_kernel):
        memory = Memory(memory_kernel)
        d = self.driver
        effects = []
        for _ in range(100000):
            d.memory_allocations = memory.state.allocations
            status = runtime(C.byref(d))
            r = self.result
            assert status == r.status and d.work <= d.max_work and d.steps == r.steps
            if status == 2:
                for field, capacity, kind in [
                    ("frames", r.event.frame_capacity, TYPES["IrRuntimeFrame"]),
                    ("slots", r.event.value_capacity, TYPES["IrRuntimeSlot"]),
                    ("phis", r.event.phi_capacity, V),
                ]:
                    old = getattr(self, field)
                    if capacity > len(old):
                        self.array(field, list(old) + [kind() for _ in range(capacity - len(old))],
                                   kind, count_field=None)
                    setattr(d, {"frames": "frame_capacity", "slots": "slot_capacity",
                                "phis": "phi_capacity"}[field], len(getattr(self, field)))
                d.resume = 2
            elif status == 1:
                e = r.event
                effects.append((e.kind, r.site))
                value = V(ty=e.ty)
                if e.kind == 1:
                    code, pointer = memory.allocate(e.size, global_=bool(e.flag), align=e.align)
                    assert code == 0
                    value.kind, value.pointer = 3, pointer
                elif e.kind == 2:
                    code, _, part = memory.load(e.left.pointer, 12)
                    assert code == 0
                    value = scalar(part.low)
                elif e.kind == 3:
                    assert memory.store(e.left.pointer, 12, e.right.low) == 0
                elif e.kind == 11:
                    assert memory.call(Request(op=11, mode=1, owner=e.owner))[0] == 0
                else:
                    raise AssertionError(("unexpected event", e.kind))
                d.resume_value, d.resume = value, 1
            else:
                return status, r, effects
        raise AssertionError("native runtime failed bounded termination")


def calls(rng, count):
    depth = rng.randint(1, 12)
    ids = rng.sample(range(1, 2**32), depth)
    constants = [rng.getrandbits(64) for _ in range(depth)]
    functions, blocks, instructions, operands, value_types = [], [], [], [], []
    for n in range(depth):
        first = len(instructions)
        parameter = len(operands)
        operands.append(0)
        first_type = len(value_types)
        value_types.extend([1, 1, 1, 1, 3, 1])
        instructions += [I(op=4, ty=1, dst=1, literal=2, low=constants[n]),
                         I(op=7, sub=0, ty=1, dst=2, a=0, b=1)]
        if n + 1 < depth:
            args = len(operands)
            operands.append(2)
            instructions += [I(op=29, ty=1, dst=3, a=n + 1, first=args, count=1),
                             I(op=34, ty=1, dst=MASK, a=3)]
        else:
            # Distinct volatile sites are issued once, then resumed after the effect.
            instructions += [I(op=18, ty=1, dst=4, align=8),
                             I(op=20, ty=1, dst=MASK, a=2, b=4, align=8),
                             I(op=19, ty=1, dst=5, a=4, align=8),
                             I(op=34, ty=1, dst=MASK, a=5)]
        blocks.append(B(id=rng.getrandbits(32), function=n, first=first,
                        count=len(instructions) - first))
        functions.append(F(id=ids[n], signature=4, entry=n, first_block=n, block_count=1,
                           first_parameter=parameter, parameter_count=1, value_count=6,
                           first_value_type=first_type, has_body=1))
    argument = rng.getrandbits(64)
    expected = (argument + sum(constants)) & MASK
    program = Program(functions, blocks, instructions, operands, value_types, [argument], ids[0])
    return program, expected, depth


def loop(rng):
    count = rng.randrange(0, 40)
    argument = rng.getrandbits(64)
    instructions = [
        I(op=4, ty=1, dst=1, literal=2, low=0),
        I(op=4, ty=1, dst=2, literal=2, low=1),
        I(op=4, ty=1, dst=3, literal=2, low=count),
        I(op=7, ty=1, sub=0, dst=9, a=0, b=2),
        I(op=31, ty=0, dst=MASK, a=1),
        I(op=15, ty=1, dst=4, first=1, count=2),
        I(op=15, ty=1, dst=5, first=5, count=2),
        I(op=15, ty=1, dst=6, first=9, count=2),
        I(op=11, ty=1, sub=2, dst=7, a=4, b=3),
        I(op=32, ty=0, dst=MASK, a=7, b=2, c=3),
        I(op=7, ty=1, sub=0, dst=8, a=4, b=2),
        I(op=31, ty=0, dst=MASK, a=1),
        I(op=34, ty=1, dst=MASK, a=5),
    ]
    operands = [0, 1, 0, 8, 2, 0, 0, 6, 2, 9, 0, 5, 2]
    blocks = [B(id=100+n, function=0, first=first, count=size)
              for n, (first, size) in enumerate([(0, 5), (5, 5), (10, 2), (12, 1)])]
    function = F(id=77, signature=4, entry=0, first_block=0, block_count=4,
                 first_parameter=0, parameter_count=1, value_count=10,
                 first_value_type=0, has_body=1)
    program = Program([function], blocks, instructions, operands,
                      [1, 1, 1, 1, 1, 1, 1, 2, 1, 1], [argument], 77)
    return program, (argument + count % 2) & MASK, 11 + 7 * count


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--library", type=Path, required=True)
    parser.add_argument("--cases", type=int, default=2000)
    args = parser.parse_args()
    library = C.CDLL(str(args.library.resolve()))
    memory = library.whale_ir_memory
    memory.argtypes = [C.POINTER(State), C.POINTER(Limits), C.POINTER(Allocation), U,
                       C.POINTER(Request), C.POINTER(Fragment), U, C.POINTER(Part), U,
                       C.POINTER(Result)]
    runtime = library.whale_ir_runtime_step
    runtime.argtypes = [C.POINTER(D)]
    rng = random.Random(0x5748414C455254)
    for case in range(args.cases):
        program, expected, depth = calls(rng, case)
        status, result, effects = program.run(runtime, memory)
        assert status == 0, (case, status, result.error, result.site, result.detail)
        assert result.value.kind == 1 and result.value.low == expected
        assert len(effects) == depth + 3, (case, effects)
        assert [kind for kind, _ in effects[:3]] == [1, 3, 2]
        assert result.steps == 4 * depth + 2
        # Admission limits fail at the call before issuing any leaf memory effect.
        if depth > 1 and case % 16 == 0:
            limited, _, _ = calls(random.Random(case), case)
            limited.driver.max_depth = 0
            status, result, effects = limited.run(runtime, memory)
            assert status == 3 and result.error == 7 and not effects
    for case in range(args.cases):
        program, expected, steps = loop(rng)
        status, result, effects = program.run(runtime, memory)
        assert status == 0 and result.value.low == expected, (case, status, result.error)
        assert result.steps == steps and len(effects) == 1
    program, _, _ = calls(rng, 0)
    assert runtime(C.byref(program.driver)) == 2
    assert runtime(C.byref(program.driver)) == 4
    print(f"Wave runtime: {args.cases} generated call/volatile + {args.cases} independent phi/control streams passed")


if __name__ == "__main__":
    main()
