#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Independent native pass-controller/effect/constant-propagation conformance."""

import argparse
import ctypes as c
import random


def record(name, fields):
    return type(
        name, (c.Structure,), {"_fields_": [(field, c.c_uint64) for field in fields.split()]}
    )


Node = record("Node", "op sub scope id kind literal low high first count")
Value = record("Value", "known kind low high replace")
Slot = record("Slot", "key index present")
User = record("User", "index next")
Pending = record("Pending", "head queued")
Output = record("Output", "status changed folded work")
State = record("State", "phase epoch0 epoch1 epoch2 work changed mask failure_stage failure_detail")
Reply = record("Reply", "action failed changed mask")
PassOutput = record(
    "PassOutput", "status action stage detail changed mask epoch0 epoch1 epoch2 work"
)


class Input(c.Structure):
    _fields_ = [
        ("nodes", c.POINTER(Node)),
        ("count", c.c_uint64),
        ("references", c.POINTER(c.c_uint64)),
        ("reference_count", c.c_uint64),
        ("values", c.POINTER(Value)),
        ("slots", c.POINTER(Slot)),
        ("capacity", c.c_uint64),
        ("users", c.POINTER(User)),
        ("pending", c.POINTER(Pending)),
        ("queue", c.POINTER(c.c_uint64)),
        ("max_work", c.c_uint64),
    ]


WIDTHS = {
    1: 1,
    2: 1,
    3: 1,
    4: 8,
    5: 16,
    6: 32,
    7: 64,
    8: 128,
    9: 8,
    10: 16,
    11: 32,
    12: 64,
    13: 128,
}
SIGNED = {2, 4, 5, 6, 7, 8}
MASK128 = (1 << 128) - 1


def packed(value, kind):
    width = WIDTHS[kind]
    value &= (1 << width) - 1
    if kind in SIGNED and value >> (width - 1):
        value -= 1 << width
    return value & MASK128


def arithmetic(op, a, b, kind):
    width = WIDTHS[kind]
    mask = (1 << width) - 1
    a, b = a & mask, b & mask
    signed = lambda value: value - (1 << width) if value >> (width - 1) else value
    if op in (3, 4, 5, 6) and b == 0:
        return None
    if op == 0:
        result = a + b
    elif op == 1:
        result = a - b
    elif op == 2:
        result = a * b
    elif op in (3, 5):
        result = a // b if op == 3 else a % b
    elif op in (4, 6):
        sa, sb = signed(a), signed(b)
        q = abs(sa) // abs(sb) * (-1 if (sa < 0) != (sb < 0) else 1)
        result = q if op == 4 else sa - q * sb
    elif op == 12:
        result = a & b
    elif op == 13:
        result = a | b
    elif op == 14:
        result = a ^ b
    elif op == 15:
        result = a << (b % width)
    elif op == 16:
        result = a >> (b % width)
    else:
        result = signed(a) >> (b % width)
    return packed(result, kind)


def fold(library, nodes, references, work=1_000_000):
    count = len(nodes)
    capacity = 1 << max(1, (2 * count - 1).bit_length())
    raw_nodes = (Node * count)(*nodes)
    raw_refs = (c.c_uint64 * len(references))(*references)
    values = (Value * count)()
    slots = (Slot * capacity)()
    users = (User * len(references))()
    pending = (Pending * count)()
    queue = (c.c_uint64 * count)()
    input_ = Input(
        raw_nodes,
        count,
        raw_refs,
        len(references),
        values,
        slots,
        capacity,
        users,
        pending,
        queue,
        work,
    )
    output = Output()
    status = library.whale_ir_fold_integer(c.byref(input_), c.byref(output))
    return status, output, values


def check_folds(library, cases):
    rng = random.Random(0x3550118)
    operations = [0, 1, 2, 3, 4, 5, 6, 12, 13, 14, 15, 16, 17]
    for case in range(cases):
        kind = rng.choice(list(range(2, 14)))
        logical = []
        expected = []
        for index in range(64):
            node = Node(scope=case & 0xFFFFFFFF, id=0xFFFFFFFF - index * 7919, kind=kind)
            if index < 4 or rng.randrange(6) == 0:
                node.op = 4
                node.literal = 1 if kind in SIGNED else 2
                value = packed(rng.getrandbits(WIDTHS[kind]), kind)
                node.low, node.high = value & ((1 << 64) - 1), value >> 64
                refs = []
                expected.append(value)
            else:
                lhs, rhs = rng.sample(range(index), 2)
                node.op = 7
                node.sub = rng.choice(operations)
                refs = [logical[lhs][0].id, logical[rhs][0].id]
                a, b = expected[lhs], expected[rhs]
                expected.append(
                    None if a is None or b is None else arithmetic(node.sub, a, b, kind)
                )
            logical.append((node, refs))
        order = list(range(len(logical)))
        rng.shuffle(order)
        nodes, references = [], []
        for index in order:
            node, refs = logical[index]
            node.first, node.count = len(references), len(refs)
            nodes.append(node)
            references.extend(refs)
        status, output, values = fold(library, nodes, references)
        assert status == 0 and output.status == 0, (case, status, output.work)
        folded = 0
        for index, value in zip(order, values):
            answer = expected[index]
            assert value.known == int(answer is not None), (case, index)
            if answer is not None:
                assert value.low | (value.high << 64) == answer, (case, index)
                assert value.kind == kind
            replace = answer is not None and logical[index][0].op != 4
            assert value.replace == int(replace), (case, index)
            folded += replace
        assert output.folded == folded and output.changed == int(folded > 0)
        # Replacing every folded node makes the same physical graph idempotent.
        for node, value in zip(nodes, values):
            if value.replace:
                node.op, node.sub = 4, 0
                node.literal = 1 if kind in SIGNED else 2
                node.low, node.high = value.low, value.high
        references = []
        for node, index in zip(nodes, order):
            node.first = len(references)
            refs = logical[index][1] if node.op == 7 else []
            node.count = len(refs)
            references.extend(refs)
        status, output, _ = fold(library, nodes, references)
        assert status == 0 and output.folded == 0 and output.changed == 0
    # Bool comparisons feed select; equal phi inputs propagate while distinct
    # constants and unknown parameters remain unknown, without losing traps.
    nodes = [
        Node(op=4, id=0, kind=9, literal=2, low=42),
        Node(op=4, id=1, kind=9, literal=2, low=7),
        Node(op=11, id=2, kind=9, sub=0, count=2),
        Node(op=13, id=3, kind=9, first=2, count=3),
        Node(op=15, id=4, kind=9, first=5, count=2),
        Node(op=15, id=5, kind=9, first=7, count=2),
        Node(op=31, id=6, kind=9, first=9),
        Node(op=4, id=7, kind=1, literal=3, low=0, first=9),
        Node(op=13, id=8, kind=9, first=9, count=3),
        Node(op=11, id=9, kind=1, sub=1, first=12, count=2),
    ]
    references = [0, 1, 2, 0, 1, 0, 0, 0, 1, 7, 6, 0, 2, 7]
    status, output, values = fold(library, nodes, references)
    assert status == 0 and output.folded == 5
    assert values[2].kind == 1 and values[2].low == 0
    assert values[3].low == 7 and values[4].low == 42
    assert values[5].known == values[6].known == 0
    assert values[8].low == 42 and values[9].low == 0
    assert fold(library, [], [])[0] == 0
    # Cross-function duplicate IDs remain separate; literal declarations never replaced.
    nodes = [
        Node(op=3, scope=0, id=0xFFFFFFFF, kind=9, literal=2, low=42),
        Node(op=4, scope=1, id=0xFFFFFFFF, kind=9, literal=2, low=7),
        Node(op=6, scope=0, id=1, kind=9, count=1),
        Node(op=6, scope=1, id=1, kind=9, first=1, count=1),
    ]
    status, _, values = fold(library, nodes, [0xFFFFFFFF, 0xFFFFFFFF])
    assert status == 0 and values[2].low == 42 and values[3].low == 7
    assert values[0].replace == 0
    status, output, _ = fold(library, nodes, [0xFFFFFFFF, 0xFFFFFFFF], work=1)
    assert status == 1 and output.changed == 0 and output.folded == 0
    nodes[1].scope = 0
    assert fold(library, nodes, [0xFFFFFFFF, 0xFFFFFFFF])[0] == 2


def pass_run(library, before, after, changed, mask, epochs=(0, 0, 0), failed=0, work=1000):
    state = State(epoch0=epochs[0], epoch1=epochs[1], epoch2=epochs[2])
    reply = Reply()
    first = (c.c_uint8 * len(before))(*before)
    second = (c.c_uint8 * len(after))(*after)
    actions = []
    for _ in range(12):
        result = PassOutput()
        status = library.whale_ir_pass_step(
            c.byref(state),
            c.byref(reply),
            first,
            len(before),
            second,
            len(after),
            work,
            c.byref(result),
        )
        assert status == 0 and result.status == 0
        actions.append(result.action)
        if result.action in (9, 10):
            return result, actions
        reply = Reply(
            action=result.action,
            failed=int(result.action == failed),
            changed=changed if result.action == 3 else 0,
            mask=mask if result.action == 3 else 0,
        )
    raise AssertionError("controller did not terminate")


def check_passes(library):
    for changed, before, after in [
        (0, b"same", b"same"),
        (1, b"same", b"edit"),
        (1, b"same", b"longer"),
    ]:
        result, actions = pass_run(library, before, after, changed, 7 * changed)
        assert actions == [1, 2, 3, 4, 5, 7, 9]
        assert result.epoch0 == result.epoch1 == result.epoch2 == changed
        result, actions = pass_run(library, before, after, 1 - changed, 7 * (1 - changed))
        assert actions[-2:] == [8, 10] and result.stage == 3 and result.detail == 1
    for action, stage in [(1, 0), (2, 0), (3, 1), (4, 2), (5, 3)]:
        result, actions = pass_run(library, b"same", b"edit", 1, 7, failed=action)
        assert result.action == 10 and result.stage == stage
        assert (8 in actions) == (action >= 3)
    result, actions = pass_run(library, b"same", b"edit", 1, 7, epochs=(3, (1 << 64) - 1, 9))
    assert result.detail == 2 and actions[-2:] == [8, 10]
    assert (result.epoch0, result.epoch1, result.epoch2) == (3, (1 << 64) - 1, 9)
    result, _ = pass_run(library, b"0123456789", b"0123456789", 0, 0, work=1)
    assert result.detail == 3 and result.stage == 3
    for kind in range(3):
        for identity, epoch, expected in [(11, 2, 1), (12, 2, 0), (11, 1, 0)]:
            assert library.whale_ir_analysis_current(identity, epoch, 11, 2, 2, 2, kind) == expected
    assert library.whale_ir_analysis_current(11, 2, 11, 2, 2, 2, 3) == 2


def check_effects(library):
    fixed = {
        19: 21,
        20: 22,
        21: 17,
        22: 18,
        23: 18,
        26: 18,
        25: 19,
        18: 48,
        1: 48,
        29: 27,
        24: 16,
        2: 16,
        30: 16,
        5: 16,
    }
    for op in range(31):
        for sub in range(18 if op == 7 else 13 if op == 14 else 1):
            for pointer in (0, 1):
                expected = fixed.get(op, 0)
                if (
                    op == 7
                    and 3 <= sub <= 6
                    or op == 14
                    and sub in (0, 8, 9)
                    or op in (10, 11)
                    and pointer
                ):
                    expected = 16
                output = c.c_uint64(0xFFFFFFFF)
                assert library.whale_ir_effects(op, sub, pointer, 0, c.byref(output)) == 0
                assert output.value == expected
    for mask in range(65):
        assert library.whale_ir_effects_removable(mask) == (
            1 if mask == 0 else 2 if mask > 63 else 0
        )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--library", required=True)
    parser.add_argument("--cases", type=int, default=2000)
    args = parser.parse_args()
    library = c.CDLL(args.library)
    library.whale_ir_fold_integer.argtypes = [c.POINTER(Input), c.POINTER(Output)]
    library.whale_ir_pass_step.argtypes = [
        c.POINTER(State),
        c.POINTER(Reply),
        c.POINTER(c.c_uint8),
        c.c_uint64,
        c.POINTER(c.c_uint8),
        c.c_uint64,
        c.c_uint64,
        c.POINTER(PassOutput),
    ]
    library.whale_ir_analysis_current.argtypes = [c.c_uint64] * 7
    check_effects(library)
    check_passes(library)
    check_folds(library, args.cases)
    print(
        f"Wave passes: full effects inventory, transactional rollback/cache cases, "
        f"{args.cases} sparse reordered 64-node mathematical fold DAGs and idempotence passed"
    )


if __name__ == "__main__":
    main()
