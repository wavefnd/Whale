use ir::*;
const HEADER: &str = "module {\nformat_version 3\nsemantics_version 1\ntarget \"x86_64-whale-linux\"\ndatalayout { ptr=64, endian=little }\n";
fn function(params: &str, body: &str) -> String {
    let mut types = Vec::new();
    for param in params.split(';').filter(|s| !s.is_empty()) {
        types.push(param.split_once(':').unwrap().1.trim());
    }
    let params = params.replace(';', ",");
    format!("{HEADER}declare @f7 \"test\": whale ({}) -> void, linkage internal\nfn @f7 \"test\"({params}) -> void, entry %b11 {{\n%b11 \"entry\":\n{body}\nret void\n}}\n}}\n", types.join(", "))
}
fn roundtrip(text: &str) -> Module {
    let parsed = parse_module(text).unwrap_or_else(|e| panic!("{e}\n{text}"));
    verify_module(&parsed).unwrap();
    let canonical = print_module(&parsed);
    assert_eq!(print_module(&parse_module(&canonical).unwrap()), canonical);
    parsed
}
#[test]
fn canonical_fixtures_preserve_scoped_sparse_ids_block_order_and_wave_output() {
    for text in [
        include_str!("fixtures/calls-v3.wir"),
        include_str!("fixtures/text-identities-v3.wir"),
        include_str!("fixtures/wave-control-v3.wir"),
        include_str!("fixtures/wave-casts-v3.wir"),
    ] {
        assert_eq!(print_module(&roundtrip(text)), text);
    }
    let m = parse_module(include_str!("fixtures/text-identities-v3.wir")).unwrap();
    assert_eq!(m.functions[0].entry, BlockId(10));
    assert_eq!(m.functions[0].blocks[0].id, BlockId(40));
    assert_eq!(m.functions[0].params[0].id, m.functions[1].params[0].id);
    assert_eq!(m.globals[0].name, m.functions[0].name);
}
#[test]
fn every_scalar_opcode_and_predicate_reads_its_printed_form() {
    for (ty, ops) in [
        ("i32", "add sub mul sdiv srem and or xor shl lshr ashr"),
        ("u32", "add sub mul udiv urem and or xor shl lshr lshr"),
        ("f32", "fadd fsub fmul fdiv frem"),
    ] {
        for op in ops.split_whitespace() {
            roundtrip(&function(
                &format!("%v0 \"a\": {ty};%v1 \"b\": {ty}"),
                &format!("%v5: {ty} = {op} {ty} %v0, %v1"),
            ));
        }
    }
    for (opcode, ty, preds) in [
        ("cmp", "i32", "eq ne slt sle sgt sge"),
        ("cmp", "u32", "eq ne ult ule ugt uge"),
        ("cmp", "f64", "feq fne flt fle fgt fge"),
        ("icmp", "i64", "eq ne slt sle sgt sge"),
        ("icmp", "u64", "ult ule ugt uge"),
        (
            "fcmp",
            "f16",
            "oeq one olt ole ogt oge ord uno ueq une ult ule ugt uge",
        ),
    ] {
        for pred in preds.split_whitespace() {
            roundtrip(&function(
                &format!("%v0 \"a\": {ty};%v1 \"b\": {ty}"),
                &format!("%v5: bool = {opcode} {pred} {ty} %v0, %v1"),
            ));
        }
    }
    for (op, source, destination) in [
        ("zext", "bool", "u1"),
        ("zext", "u8", "i32"),
        ("sext", "i8", "i32"),
        ("trunc", "u128", "i8"),
        ("fext", "f16", "f64"),
        ("ftrunc", "f64", "f32"),
        ("itof_s", "i64", "f32"),
        ("itof_u", "u64", "f64"),
        ("ftoi_s", "f32", "i32"),
        ("ftoi_u", "f64", "u32"),
        ("bitcast", "f32", "u32"),
        ("bitcast", "ptr<i32>", "ptr<u8>"),
        ("ptrtoint", "ptr<u8>", "u64"),
        ("inttoptr", "u64", "ptr<u8>"),
    ] {
        roundtrip(&function(
            &format!("%v0 \"source\": {source}"),
            &format!("%v5: {destination} = {op} {source} %v0 to {destination}"),
        ));
    }
    for (ty, ops) in [
        ("i32", "sadd_chk ssub_chk smul_chk"),
        ("u128", "uadd_chk usub_chk umul_chk"),
    ] {
        for op in ops.split_whitespace() {
            roundtrip(&function(&format!("%v0 \"a\": {ty};%v1 \"b\": {ty}"),&format!("%v5: tuple<{ty}, bool> = {op} {ty} %v0, %v1\n%v6: {ty} = extract %v5, 0\n%v7: bool = extract %v5, 1")));
        }
    }
}
#[test]
fn memory_dataflow_function_addresses_and_dead_o0_instructions_are_retained() {
    let text = function(
        "%v0 \"c\": bool",
        r#"%v1: i32 = undef i32
%v2: i32 = mov i32 %v1
%v3: i32 = not i32 %v2
%v4: i32 = select bool %v0, i32 %v2, i32 %v3
%v5: ptr<i32> = alloca i32, align 4
store i32 %v4, ptr<i32> %v5, align 4
%v6: i32 = load i32, ptr<i32> %v5, align 4
%v7: u64 = const u64 0
%v8: ptr<i32> = gep %v5, %v7
%v9: ptr<u8> = alloca u8, align 1
%v10: u8 = const u8 0
memcpy ptr<u8> %v9, ptr<u8> %v9, u64 %v7, align 1
memset ptr<u8> %v9, u8 %v10, u64 %v7, align 1
%v11: fnptr<whale (bool) -> void> = function_addr @f7
%v12: fnptr<whale (bool) -> void> = null_function
call whale void indirect %v11(%v0)
trap_if bool %v0, reason="한글 λ\"\\\n\r\t\0\u{85}\u{2028}""#,
    );
    let m = roundtrip(&text);
    assert_eq!(m.functions[0].blocks[0].instructions.len(), 17);
    assert!(print_module(&m).contains("undef i32"));
    // Whole aggregate types occur in memory and pointer signatures, even though
    // aggregate literal values and SysV aggregate ABI lowering are unsupported.
    roundtrip(&function("%v0 \"base\": ptr<array<struct{i32, tuple<u8, bool>}, 4>>", "%v1: u64 = const u64 0\n%v2: u64 = const u64 1\n%v3: ptr<tuple<u8, bool>> = gep %v0, %v1, %v1, %v2"));
}
#[test]
fn exact_literals_constant_trees_forward_references_and_escapes() {
    let text = format!(
        "{HEADER}{}\n}}",
        r#"global @g9 "max": u128 = const u128 340282366920938463463374607431768211455, align 16, init_expr u128 340282366920938463463374607431768211455
global @g7 "min": i128 = const i128 -170141183460469231731687303715884105728, align 16, init_expr i128 -170141183460469231731687303715884105728
global @g5 "wrap": i8 = const i8 -128, align 1, init_expr add(i8 @g6, i8 1)
global @g6 "next": i8 = const i8 127, align 1, init_expr i8 127
global @g3 "half": f16 = const f16 0x8000, align 2, init_expr f16 0x8000
global @g2 "nan": f32 = const f32 0x7fa01234, align 4, init_expr f32 0x7fa01234
global @g1 "double": f64 = const f64 0x8000000000000000, align 8, init_expr f64 0x8000000000000000"#
    );
    let m = roundtrip(&text);
    assert_eq!(m.globals[0].init, ConstValue::U(u128::MAX));
    assert_eq!(m.globals[1].init, ConstValue::I(i128::MIN));
    assert_eq!(m.globals[5].init, ConstValue::F(FloatBits::F32(0x7fa01234)));
    for op in ["add", "sub", "mul", "eq", "ne", "lt", "le", "gt", "ge"] {
        let (ty, value) = match op {
            "add" => ("i32", "5"),
            "sub" => ("i32", "-1"),
            "mul" => ("i32", "6"),
            "eq" | "gt" | "ge" => ("bool", "false"),
            _ => ("bool", "true"),
        };
        roundtrip(&function("",&format!("%v0: {ty} = const_decl \"typed\" {op}(i32 2, i32 3) => const {ty} {value}\n%v1: {ty} = const_decl \"ref\" {ty} %v0 => const {ty} {value}")));
    }
    let escaped = r#"한글 λ\"\\\n\r\t\0\u{1}\u{7f}\u{85}\u{2028}\u{2029}"#;
    let text = function("", "").replace("\"test\"", &format!("\"{escaped}\""));
    let m = roundtrip(&text);
    assert_eq!(
        m.functions[0].name,
        "한글 λ\"\\\n\r\t\0\u{1}\u{7f}\u{85}\u{2028}\u{2029}"
    );
}
#[test]
fn strict_syntax_and_semantic_failures_have_locations() {
    let good = function("%v0 \"x\": i32", "%v5: i32 = const i32 7");
    let cases = [
        good.replace("format_version 3", "format_version 2"),
        good.replace("semantics_version 1", "semantics_version 2"),
        good.replace("format_version 3", ""),
        good.replace("ptr=64", "ptr=32"),
        good.replace("ret void", "ret i32 %v5"),
        good.replace("entry %b11", "entry %b42"),
        good.replace("const i32 7", "const i32 2147483648"),
        good.replace("%v5: i32", "%v5: u32"),
        good.replace("const i32 7", "mystery i32 7"),
        good.replace("const i32 7", "const i32 +7"),
        good.replace("%v5:", "%v4294967296:"),
        good.replace("%v5:", "%name:"),
        good.replace("const i32 7", "mov i32 %v999"),
        good.replace("ret void", "ret void\n%v9: i32 = const i32 1"),
        good.replace("const i32 7", "const i32 7, unknown 1"),
        format!("{good} extra"),
        good.replace("\"test\"", r#""bad\q""#),
        good.replace("\"test\"", r#""bad\u{d800}""#),
        good.replace("\"test\"", r#""bad\u{110000}""#),
        good.replace("\"test\"", "\"bad\nname\""),
        good.replace("%v0 \"x\": i32", "%v0 \"x\": i32, %v0 \"y\": i32"),
        good.replace(
            "%v5: i32 = const i32 7",
            "%v5: i32 = const i32 7\n%v5: i32 = const i32 8",
        ),
    ];
    for text in cases {
        let e = parse_module(&text).unwrap_err();
        assert!(e.location.line >= 1 && e.location.column >= 1);
        assert!(e.location.byte_offset <= text.len());
    }
    let e = parse_module(&good.replace("const i32 7", "unknown i32 7")).unwrap_err();
    assert_eq!(e.location.line, 9);
    let e = parse_module(&good.replace("format_version 3", "format_version 99")).unwrap_err();
    assert_eq!(e.location.line, 2);
    for text in ["", "module", "module {", "\"", "💧", "// only a comment"] {
        assert!(parse_module(text).is_err());
    }
    roundtrip(&format!("// comment\n{good}// end\n"));
    assert_eq!(
        print_module(&roundtrip(&good.replace("\n", "\r\n"))),
        print_module(&roundtrip(&good))
    );
}
#[test]
fn parser_resource_limits_and_truncated_inputs_never_panic() {
    let good = function("%v0 \"x\": i32", "%v5: i32 = const i32 7");
    for limits in [
        IrLimits {
            max_input_bytes: good.len() - 1,
            ..IrLimits::default()
        },
        IrLimits {
            max_tokens: 4,
            ..IrLimits::default()
        },
        IrLimits {
            max_nodes: 1,
            ..IrLimits::default()
        },
    ] {
        assert!(matches!(
            parse_module_with_limits(&good, limits).unwrap_err().kind,
            ParseErrorKind::ResourceLimit(_)
        ));
    }
    let ty = format!("{}i32{}", "ptr<".repeat(50000), ">".repeat(50000));
    let bad=format!("{HEADER}declare @f0 \"deep\": whale ({ty}) -> void, linkage external, link_name \"deep\"\n}}");
    assert!(matches!(
        parse_module(&bad).unwrap_err().kind,
        ParseErrorKind::ResourceLimit(_)
    ));
    let expression = format!("{}i32 0{}", "add(".repeat(50000), ", i32 0)".repeat(50000));
    let bad = format!(
        "{HEADER}global @g0 \"deep\": i32 = const i32 0, align 4, init_expr {expression}\n}}"
    );
    assert!(matches!(
        parse_module(&bad).unwrap_err().kind,
        ParseErrorKind::ResourceLimit(_)
    ));
    let sample = include_str!("fixtures/calls-v3.wir");
    for (i, _) in sample.char_indices() {
        let _ = parse_module(&sample[..i]);
    }
    // Deterministic single-token corruption probes error handling and verifies
    // any mutation that still describes a valid module.
    for i in (0..sample.len()).step_by(13) {
        let mut bytes = sample.as_bytes().to_vec();
        bytes[i] = b'?';
        let text = String::from_utf8(bytes).unwrap();
        if let Ok(m) = parse_module(&text) {
            verify_module(&m).unwrap();
        }
    }
}
