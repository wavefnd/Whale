"""Check versioned JSON Schema/decoder agreement and semantic-only lower errors.

Install tools/requirements-schema.txt, build --all-features --example schema_probe,
then pass its executable with --binary. No network/schema retrieval occurs.
"""
import argparse
import copy
import json
from pathlib import Path
import subprocess
import tempfile


def cases(version):
    integer = {"Int": {"bits": 8, "signed": False}}
    literal = {"Lit": {"Int": {"bits": 8, "signed": False, "value": "42"}}}
    program = {"declarations": [], "globals": [], "functions": [{
        "name": "answer", "convention": "Whale", "linkage": "Internal", "link_name": None,
        "parameters": [], "return_type": integer, "body": [{"Return": literal}],
    }]}
    base = {"format_version": version, "semantics_version": 1, "features": [], "program": program}
    cast = copy.deepcopy(base)
    cast["program"]["functions"][0]["body"] = [{"Return": {"Cast": {"value": literal, "ty": integer, "kind": "Checked"}}}]
    yield "integer-cast", cast, version >= 5, version >= 5
    for kind in ("Numeric", "Checked"):
        tagged_cast = copy.deepcopy(cast)
        tagged_cast["program"]["functions"][0]["body"][0]["Return"]["Cast"]["kind"] = {kind: None}
        yield "integer-cast-unit-object-" + kind, tagged_cast, version >= 5, version >= 5
    neg = copy.deepcopy(base)
    neg["program"]["functions"][0]["body"] = [{"Return": {"Neg": literal}}]
    yield "unary-negation", neg, version >= 5, version >= 5
    aggregate = copy.deepcopy(base)
    record = {"Struct": {"fields": [{"name": "first", "ty": integer}, {"name": "second", "ty": integer}]}}
    array_type = {"Array": {"elem": integer, "len": 2}}
    index = {"Index": {"base": {"Var": "a"}, "index": literal}}
    zero = {"Lit": {"Int": {"bits": 8, "signed": False, "value": "0"}}}
    index["Index"]["index"] = zero
    selected = {"Field": {"base": {"Var": "s"}, "layout": record, "name": "first"}}
    aggregate["program"]["functions"][0]["body"] = [
        {"VarDecl": {"name": "s", "ty": record, "init": {"Struct": {"ty": record, "fields": [{"name": "first", "value": literal}, {"name": "second", "value": literal}]}}}},
        {"VarDecl": {"name": "a", "ty": array_type, "init": {"Array": {"ty": array_type, "elements": [literal, literal]}}}},
        {"Store": {"place": selected, "value": literal}},
        {"Store": {"place": index, "value": selected}},
        {"Return": {"Deref": {"PointerOffset": {"base": {"AddressOf": index}, "index": zero}}}},
    ]
    yield "aggregate-pointer-storage", aggregate, version >= 5, version >= 5
    volatile = copy.deepcopy(base)
    address = {"AddressOf": {"Var": "x"}}
    volatile["program"]["functions"][0]["body"] = [
        {"VarDecl": {"name": "x", "ty": integer, "init": None}},
        {"VolatileStore": {"pointer": address, "value": literal}},
        {"Return": {"VolatileLoad": address}},
    ]
    yield "volatile-scalar-storage", volatile, version >= 5, version >= 5
    yield "ordinary-null-link-name", base, True, True
    missing = copy.deepcopy(base); del missing["program"]["functions"][0]["link_name"]
    yield "missing-optional-link-name", missing, True, True
    unit = copy.deepcopy(base); unit["program"]["functions"][0].update(return_type={"Void": None}, body=[{"Return": None}])
    yield "unit-variant-object", unit, True, True
    for bits, payload in [(16, "0xfe01"), (32, "0x80000000"), (64, "0x7ff8000000000001")]:
        case = copy.deepcopy(base)
        case["program"]["functions"][0].update(return_type={"Float": {"bits": bits}}, body=[{"Return": {"Lit": {"Float": {"bits": bits, "value": payload}}}}])
        yield f"exact-float-{bits}", case, True, True
        bad = copy.deepcopy(case); bad["program"]["functions"][0]["body"][0]["Return"]["Lit"]["Float"]["value"] = payload + "00"
        yield f"bad-float-width-{bits}", bad, False, False
    for label, field, value in [("bad-int-width", "bits", 7), ("decimal-spelling", "value", "+1"), ("wrong-numeric-category", "value", 42)]:
        case = copy.deepcopy(base); case["program"]["functions"][0]["body"][0]["Return"]["Lit"]["Int"][field] = value
        yield label, case, False, False
    for value in ["256", "-1", "340282366920938463463374607431768211456"]:
        case = copy.deepcopy(base); case["program"]["functions"][0]["body"][0]["Return"]["Lit"]["Int"]["value"] = value
        yield f"semantic-int-range-{value}", case, True, False
    mismatch = copy.deepcopy(base); mismatch["program"]["functions"][0]["return_type"] = "Bool"
    yield "semantic-return-type", mismatch, True, False
    declaration = {"name": "emit", "signature": {"params": [], "ret": "Void", "convention": "Whale", "variadic": False}, "linkage": "External", "link_name": None}
    case = copy.deepcopy(base); case["program"]["declarations"] = [declaration]
    yield "semantic-external-needs-link-name", case, True, False
    case = copy.deepcopy(base); case["features"] = ["unknown"]
    yield "unknown-feature", case, False, False
    case = copy.deepcopy(base); case["program"]["functions"][0]["extra"] = 0
    yield "unknown-field", case, False, False
    case = copy.deepcopy(base); case["program"]["functions"][0]["body"][0]["Return"] = {"Binary": {"left": literal, "op": "BitXor", "right": literal}}
    yield "versioned-extended-op", case, version >= 3, version >= 3
    located = copy.deepcopy(base)
    located["program"]["functions"][0]["body"][0]["Return"] = {"Located": {"annotation": {"id": 1, "source": {"file": "source.wave", "line": 1, "column": 1, "end_line": 1, "end_column": 5}}, "expression": literal}}
    yield "versioned-expression-wrapper", located, version >= 4, version >= 4
    statement = copy.deepcopy(base)
    statement["program"]["functions"][0]["body"][0] = {"Located": {"annotation": {"id": 2, "source": None}, "statement": {"Return": literal}}}
    yield "versioned-statement-wrapper", statement, version >= 4, version >= 4
    if version >= 4:
        nested = copy.deepcopy(statement)
        nested["program"]["functions"][0]["body"][0]["Located"]["statement"] = {"Return": located["program"]["functions"][0]["body"][0]["Return"]}
        yield "nested-expression-statement-wrappers", nested, True, True
        void_call = copy.deepcopy(statement)
        void_call["program"]["functions"][0]["body"].insert(0, {"Located": {"annotation": {"id": 3}, "statement": {"ExprStmt": {"Located": {"annotation": {"id": 4, "source": None}, "expression": {"Located": {"annotation": {"id": 5}, "expression": {"Call": {"callee": {"Direct": "side"}, "args": []}}}}}}}}})
        void_call["program"]["functions"].append({"name": "side", "convention": "Whale", "linkage": "Internal", "parameters": [], "return_type": "Void", "body": [{"Return": None}]})
        yield "located-discarded-void-call", void_call, True, True
        bad = copy.deepcopy(located)
        bad["program"]["functions"][0]["body"][0]["Return"]["Located"]["annotation"]["source"]["line"] = 0
        yield "schema-source-zero-coordinate", bad, False, False
        bad = copy.deepcopy(located)
        bad["program"]["functions"][0]["body"][0]["Return"]["Located"]["annotation"]["source"].update(line=10, end_line=1)
        # Cross-field ranges and program-wide node identity are semantic checks
        # performed by the bounded decoder guard, not structural JSON Schema.
        yield "semantic-source-range", bad, True, False, False
        bad = copy.deepcopy(located)
        wrapper = bad["program"]["functions"][0]["body"][0]["Return"]
        bad["program"]["functions"][0]["body"][0] = {"Located": {"annotation": {"id": 1, "source": None}, "statement": {"Return": wrapper}}}
        yield "semantic-original-node-identity", bad, True, False, False


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    args = parser.parse_args()
    from jsonschema import Draft202012Validator
    root = Path(__file__).resolve().parents[1]
    binary = args.binary.resolve()
    validators = {}
    for path in sorted((root / "ir/schema").glob("ast-v*.schema.json")):
        schema = json.loads(path.read_text(encoding="utf-8"))
        version = schema["properties"]["format_version"]["const"]
        Draft202012Validator.check_schema(schema)
        validators[version] = Draft202012Validator(schema)
    if not validators:
        raise AssertionError("empty schema set")
    count = 0
    with tempfile.TemporaryDirectory() as directory:
        fixture = Path(directory) / "case.json"

        def check(label, instance, schema_ok, lower_ok, raw=None, decode_ok=None):
            nonlocal count
            if raw is None:
                version = instance["format_version"]
                actual_schema = validators.get(version, validators[max(validators)]).is_valid(instance)
                assert actual_schema == schema_ok, (label, "schema", list(validators.get(version, validators[max(validators)]).iter_errors(instance)))
                raw = json.dumps(instance, ensure_ascii=False)
            fixture.write_text(raw, encoding="utf-8")
            result = subprocess.run([str(binary), str(fixture)], check=True, capture_output=True, text=True, timeout=10)
            if decode_ok is None:
                decode_ok = schema_ok
            expected_decode = "accept" if decode_ok else "reject"
            expected_lower = "accept" if lower_ok else ("reject" if decode_ok else "skip")
            assert f"decode={expected_decode} lower={expected_lower}" in result.stdout, (label, result.stdout, result.stderr)
            count += 1

        for version in sorted(validators):
            for record in cases(version):
                label, instance, valid, lower, *decode = record
                check(f"v{version}-{label}", instance, valid, lower, decode_ok=decode[0] if decode else None)
        accepted_fixtures = sorted((root / "bindings/rust/ir/tests/fixtures").glob("ast-v*.json"))
        accepted_fixtures += sorted((root / "fuzz/corpus/ast").glob("located-v*.json"))
        for path in accepted_fixtures:
            check(path.name, json.loads(path.read_text(encoding="utf-8")), True, True)
        # JSON Schema sees parsed objects; it cannot observe discarded duplicate
        # keys. Keep these raw-input checks explicitly in the decoder column.
        for escaped in ["globals", "glob\\u0061ls"]:
            raw = '{"format_version":2,"semantics_version":1,"features":[],"program":{"declarations":[],"globals":[],"' + escaped + '":[],"functions":[]}}'
            check("raw-duplicate-" + escaped, None, False, False, raw=raw)
    print(f"AST schema/decoder agreement passed: versions={sorted(validators)}, {count} cases")


if __name__ == "__main__":
    main()
