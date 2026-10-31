#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Independent native AST O0 identity, instruction-order and budget checks."""
import argparse
import ctypes as C
import random

from check_wave_ir_layout import Storage, Node as LayoutNode, Layout
from check_wave_ir_lower_type import Context, Name, Result, Type

NONE = (1 << 64) - 1


def record(name, fields):
    return type(name, (C.Structure,), {"_fields_": [(f, C.c_uint64) for f in fields.split()]})


Instruction = record("Instruction", "tag dst ty a b c d e first count next annotation source")
Block = record("Block", "id function name first last allocation_last instruction_count term_kind term_a term_b term_c term_ty term_annotation term_source")
Constant = record("Constant", "kind operation ty low high left right reference annotation")
Declaration = record("Declaration", "name signature linkage link_name defined source")
Function = record("Function", "declaration first_param param_count first_block block_count entry source")
Global = record("Global", "name ty expression align annotation source")
Binding = record("Binding", "name ty value kind constant")
Field = record("Field", "name ordinal previous used")
Loop = record("Loop", "condition exit")
Annotation = record("Annotation", "original has_source file line column end_line end_column")
Request = record("Request", "expression kind identity annotation")
POINTERS = set("core instructions output_edges blocks constants declarations functions globals global_bindings bindings loops annotations original_table source_order source_marks type_sizes value_types declaration_map field_slots field_bindings local_slots global_slots mapping_order requests".split())


class Driver(C.Structure):
    _fields_ = [(f, C.c_void_p if f in POINTERS else C.c_uint64) for f in (
        "core mode expansion max_expansion instructions instruction_count instruction_capacity output_edges output_edge_count output_edge_capacity blocks block_count block_capacity constants declarations declaration_count declaration_capacity functions function_count function_capacity globals global_count global_capacity global_bindings global_binding_count bindings binding_count binding_capacity loops loop_count loop_capacity annotations annotation_count original_table original_capacity source_order source_count source_capacity source_marks type_sizes value_types value_capacity next_value declaration_map field_slots field_bindings field_binding_count field_binding_capacity current_function current_block entry return_type local_slots global_slots mapping_order mapping_count mapping_capacity source_work max_source_work error_index requests request_count request_capacity"
    ).split()]


class Fixture:
    def __init__(self):
        self.nodes = []
        self.edges = []
        self.strings = [C.create_string_buffer(s.encode()) for s in ("entry", "x", "saved")]
        self.keep = []

    def node(self, *words):
        self.nodes.append((*words, *([0] * (12 - len(words)))))
        return len(self.nodes) - 1

    def links(self, values):
        start = len(self.edges)
        self.edges.extend(values)
        return start, len(values)

    def literal(self, width, signed, value):
        b = C.create_string_buffer(str(value).encode())
        self.keep.append(b)
        return self.node(17, width, signed, C.addressof(b), len(str(value)))

    def finish(self, ty, body, params=()):
        first, count = self.links(params)
        statements, length = self.links(body)
        f = self.node(203, 0, first, count, ty, statements, length, 0, 0, NONE)
        functions, count = self.links([f])
        return self.node(200, 0, 0, 0, 0, functions, count)


def run(kernel, fixture, root, *, work=1000000, expansion=1000000, depth=32, source_work=1000000, annotations=()):
    n = len(fixture.nodes)
    capacity = 17 + 4 * n
    edge_capacity = 4 * n + 64
    table_capacity = 1 << (2 * capacity - 1).bit_length()
    keep = []

    def alloc(kind, count):
        a = (kind * count)()
        keep.append(a)
        return a

    nodes = alloc(C.c_uint64, n * 12)
    nodes[:] = [word for row in fixture.nodes for word in row]
    edges = alloc(C.c_uint64, len(fixture.edges))
    edges[:] = fixture.edges
    names = alloc(Name, len(fixture.strings))
    for i, b in enumerate(fixture.strings):
        names[i] = Name(C.addressof(b), len(b.value))
    storage = Storage(64, 8, 1, 2, 4, 8, 16, 16, 16, 1)
    result = Result()
    core = Context()
    for field, value in {
        "nodes": nodes, "edges": edges, "names": names,
        "types": alloc(Type, capacity), "type_edges": alloc(C.c_uint64, edge_capacity),
        "type_table": alloc(C.c_uint64, table_capacity), "type_map": alloc(C.c_uint64, n),
        "temporary": alloc(C.c_uint64, edge_capacity), "name_marks": alloc(C.c_uint64, len(names)),
        "layout_nodes": alloc(LayoutNode, capacity), "layouts": alloc(Layout, capacity),
        "layout_offsets": alloc(C.c_uint64, edge_capacity), "storage": storage, "result": result,
    }.items():
        setattr(core, field, C.addressof(value))
    core.node_count, core.edge_count, core.name_count = n, len(edges), len(names)
    core.type_capacity, core.type_edge_capacity = capacity, edge_capacity
    core.type_table_capacity, core.temporary_capacity = table_capacity, edge_capacity
    core.max_depth, core.max_work = depth, work
    d = Driver()
    d.core, d.max_expansion, d.max_source_work = C.addressof(core), expansion, source_work
    buffers = {}
    for field, kind, count in (
        ("blocks", Block, 4 * n + 8), ("constants", Constant, n),
        ("declarations", Declaration, n), ("functions", Function, n), ("globals", Global, n),
        ("global_bindings", Binding, n), ("bindings", Binding, n + 8), ("loops", Loop, n + 8),
        ("type_sizes", C.c_uint64, capacity), ("declaration_map", C.c_uint64, len(names)),
        ("field_slots", C.c_uint64, len(names)), ("field_bindings", Field, n + 8),
        ("local_slots", C.c_uint64, len(names)), ("global_slots", C.c_uint64, len(names)),
        ("requests", Request, n),
    ):
        buffers[field] = alloc(kind, count)
        setattr(d, field, C.addressof(buffers[field]))
    for field, kind in (("annotations", Annotation), ("source_marks", C.c_uint64),
                        ("source_order", C.c_uint64), ("mapping_order", C.c_uint64)):
        buffers[field] = alloc(kind, len(annotations))
        setattr(d, field, C.addressof(buffers[field]))
    for i, annotation in enumerate(annotations):
        buffers["annotations"][i] = Annotation(*annotation)
    d.annotation_count = d.source_capacity = d.mapping_capacity = len(annotations)
    for field, value in {"block_capacity": 4 * n + 8, "declaration_capacity": n,
                         "function_capacity": n, "global_capacity": n, "binding_capacity": n + 8,
                         "loop_capacity": n + 8, "field_binding_capacity": n + 8,
                         "request_capacity": n}.items():
        setattr(d, field, value)
    status = kernel(C.byref(d), root)
    assert status == result.status and core.work == result.work <= work
    if status:
        return status, result, None
    counts = d.instruction_count, d.output_edge_count, d.next_value
    for field, kind, count in (("instructions", Instruction, counts[0]), ("output_edges", C.c_uint64, counts[1]),
                               ("value_types", C.c_uint64, counts[2])):
        buffers[field] = alloc(kind, count)
        setattr(d, field, C.addressof(buffers[field]))
    d.mode, d.instruction_capacity, d.output_edge_capacity, d.value_capacity = 1, *counts
    status = kernel(C.byref(d), root)
    assert status == result.status == 0 and counts == (d.instruction_count, d.output_edge_count, d.next_value)
    assert core.work == result.work <= work
    # Keep every native pointee alive until all independent assertions finish.
    buffers["_keep"] = keep
    return status, result, (d, buffers, C.cast(core.types, C.POINTER(Type)))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", required=True)
    parser.add_argument("--cases", type=int, default=10000)
    args = parser.parse_args()
    library = C.CDLL(args.library)
    kernel = library.whale_ir_lower_o0
    kernel.argtypes = [C.POINTER(Driver), C.c_uint64]
    kernel.restype = C.c_int32
    randomizer = random.Random(0x351_000)
    for _ in range(args.cases):
        width = randomizer.choice((1, 8, 16, 32, 64, 128))
        signed = randomizer.randrange(2)
        opcode = randomizer.randrange(10)
        payload = randomizer.randrange(0, 1 << min(width - signed, 127))
        if signed and randomizer.randrange(2):
            payload = -payload
        f = Fixture()
        ty = f.node(102, width, signed)
        param = f.node(204, 1, ty)
        load = f.node(15, 1)
        literal = f.literal(width, signed, payload)
        operation = f.node(19, load, opcode, literal)
        ret = f.node(51, operation)
        root = f.finish(ty, [ret], [param])
        status, _, output = run(kernel, f, root, source_work=0)
        assert status == 0
        d, b, types = output
        assert (d.function_count, d.declaration_count, d.block_count, d.next_value) == (1, 1, 1, 5)
        instructions = b["instructions"]
        assert [i.tag for i in instructions] == [18, 23, 21, 4, 7]
        assert [i.next for i in instructions] == [1, 2, 3, 4, NONE]
        block = b["blocks"][0]
        assert (block.first, block.last, block.instruction_count, block.term_kind, block.term_a) == (0, 4, 5, 3, 4)
        expected = {0: 0, 1: 1, 2: 2, 3: 12, 4: 13, 5: 14, 6: 4 if signed else 3,
                    7: 6 if signed else 5, 8: 15, 9: 17 if signed else 16}[opcode]
        assert (instructions[4].dst, instructions[4].a, instructions[4].b, instructions[4].c) == (4, expected, 2, 3)
        bits = payload & ((1 << 128) - 1)
        assert (instructions[3].b, instructions[3].c, instructions[3].d) == (bits & NONE, bits >> 64, 2 if signed else 3)
        assert instructions[0].dst == 1 and instructions[1].a == 0 and instructions[1].b == 1
        assert types[b["value_types"][1]].kind == 17
    # Later allocations are hoisted among entry allocas without moving source
    # ownership onto unrelated operations, while all SSA identities stay stable.
    f = Fixture()
    ty = f.node(102, 32, 1)
    first = f.node(55, 1, ty, f.literal(32, 1, 1))
    first = f.node(50, 0, first)
    second = f.node(55, 2, ty, f.literal(32, 1, 3))
    root = f.finish(ty, [first, second, f.node(51, f.node(15, 1))])
    status, _, output = run(kernel, f, root, annotations=[(100, 0, 0, 0, 0, 0, 0)])
    assert status == 0
    d, b, _ = output
    body = []
    index = b["blocks"][0].first
    while index != NONE:
        body.append(index)
        index = b["instructions"][index].next
    assert body == [1, 4, 0, 2, 3, 5, 6]
    assert [b["instructions"][i].annotation for i in body] == [0, NONE, 0, 0, NONE, NONE, NONE]
    assert d.source_count == d.mapping_count == 1 and b["source_order"][0] == b["mapping_order"][0] == 0
    assert run(kernel, f, root, source_work=0, annotations=[(100, 0, 0, 0, 0, 0, 0)])[0] == 22

    # Lazy logical operations preserve their skipped RHS in a separate block
    # and produce a phi with exact predecessor identities.
    for opcode in (10, 11):
        f = Fixture()
        ty = f.node(101)
        param = f.node(204, 1, ty)
        expression = f.node(19, f.node(15, 1), opcode, f.node(16, 1))
        root = f.finish(ty, [f.node(51, expression)], [param])
        status, _, output = run(kernel, f, root)
        assert status == 0
        d, b, _ = output
        blocks, instructions = b["blocks"], b["instructions"]
        assert d.block_count == 3 and [blocks[i].name for i in range(3)] == [0, 8, 9]
        assert (blocks[0].term_kind, blocks[0].term_a) == (2, 2)
        assert (blocks[0].term_b, blocks[0].term_c) == ((1, 2) if opcode == 10 else (2, 1))
        assert (blocks[1].term_kind, blocks[1].term_a) == (1, 2)
        phi = instructions[blocks[2].first]
        assert phi.tag == 15 and list(b["output_edges"])[phi.first:phi.first + phi.count] == [2, 0, 3, 1]
        assert (blocks[2].term_kind, blocks[2].term_a) == (3, 4)

    f = Fixture()
    ty = f.node(102, 32, 1)
    value = f.literal(32, 1, 42)
    root = f.finish(ty, [f.node(51, value)])
    assert run(kernel, f, root, work=1)[0] == 1
    assert run(kernel, f, root, expansion=1)[0] == 23
    assert run(kernel, f, root, depth=0)[0] == 2
    assert run(kernel, f, len(f.nodes) + 1)[0] == 3
    # Cycles cannot recurse past the explicit AST ceiling.
    f.nodes[value] = (10, value, *([0] * 10))
    assert run(kernel, f, root, depth=8)[0] == 2
    print(f"Wave AST O0 lowering: {args.cases} independent opcode/literal/SSA vectors, hoisting, source mapping, lazy CFG and limits passed")


if __name__ == "__main__":
    main()
