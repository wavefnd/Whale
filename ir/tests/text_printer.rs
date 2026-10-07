use ir::*;

fn flow(id: u32, name: &str) -> (FunctionDecl, Function) {
    let ty = Type::I32;
    let block = |id, instructions, terminator| BasicBlock {
        id: BlockId(id),
        name: "repeat".into(),
        instructions,
        terminator: Some(terminator),
    };
    let f = Function {
        id: FunctionId(id),
        name: name.into(),
        params: vec![
            Param {
                id: ValueId(40),
                name: "x".into(),
                ty: ty.clone(),
            },
            Param {
                id: ValueId(41),
                name: "c".into(),
                ty: Type::Bool,
            },
        ],
        ret_ty: ty.clone(),
        entry: BlockId(10),
        value_types: vec![
            (ValueId(40), ty.clone()),
            (ValueId(41), Type::Bool),
            (ValueId(42), ty.clone()),
        ],
        blocks: vec![
            block(
                40,
                vec![Instruction::Phi {
                    dst: ValueId(42),
                    ty: ty.clone(),
                    incomings: vec![(ValueId(40), BlockId(20)), (ValueId(40), BlockId(30))],
                }],
                Terminator::Switch {
                    ty: ty.clone(),
                    value: ValueId(42),
                    default_bb: BlockId(60),
                    cases: vec![(ConstValue::I(0), BlockId(50))],
                },
            ),
            block(
                50,
                vec![],
                Terminator::Trap {
                    reason: "zero".into(),
                },
            ),
            block(
                60,
                vec![],
                Terminator::Ret {
                    ty: ty.clone(),
                    value: Some(ValueId(42)),
                },
            ),
            block(
                30,
                vec![],
                Terminator::Br {
                    target: BlockId(40),
                },
            ),
            block(
                20,
                vec![],
                Terminator::Br {
                    target: BlockId(40),
                },
            ),
            block(
                10,
                vec![],
                Terminator::CBr {
                    cond: ValueId(41),
                    then_bb: BlockId(20),
                    else_bb: BlockId(30),
                },
            ),
        ],
    };
    (
        FunctionDecl {
            id: f.id,
            name: f.name.clone(),
            signature: FunctionSignature::whale(vec![ty.clone(), Type::Bool], ty),
            linkage: Linkage::Internal,
            link_name: None,
        },
        f,
    )
}
fn module() -> Module {
    let mut m = Module::new("x86_64-whale-linux", DataLayout::default_64bit_le());
    for (id, name) in [(21, "flow"), (22, "other")] {
        let (d, f) = flow(id, name);
        m.declarations.push(d);
        m.functions.push(f);
    }
    m.globals.push(Global {
        id: GlobalId(7),
        name: "flow".into(),
        ty: Type::I32,
        init: ConstValue::I(3),
        init_expr: ConstExpr::literal(Type::I32, ConstValue::I(3)),
        align: 4,
    });
    m
}

#[test]
fn identities_are_explicit_scoped_and_independent_of_names_or_storage_order() {
    let m = module();
    verify_module(&m).unwrap();
    let text = print_module(&m);
    assert_eq!(
        text,
        include_str!("fixtures/text-identities-v3.wir")
            .replace("format_version 3", "format_version 4")
    );
    assert_eq!(text, print_module(&m));
    // IDs are preserved, rather than renumbered by physical block order.
    assert!(text.find("%b40 \"repeat\":").unwrap() < text.find("%b10 \"repeat\":").unwrap());
    assert_eq!(text.matches("entry %b10").count(), 2);
    assert_eq!(text.matches("%v40 \"x\": i32").count(), 2);
}

#[test]
fn all_string_fields_share_escaping_without_changing_printable_unicode() {
    let raw = "한글 λ\"\\\n\r\t\0\u{1}\u{7f}\u{85}\u{2028}\u{2029}";
    let escaped = r#"한글 λ\"\\\n\r\t\0\u{1}\u{7f}\u{85}\u{2028}\u{2029}"#;
    let mut m = module();
    m.declarations[0].name = raw.into();
    m.functions[0].name = raw.into();
    m.functions[0].params[0].name = raw.into();
    for b in &mut m.functions[0].blocks {
        b.name = raw.into();
    }
    m.globals[0].name = raw.into();
    m.functions[0].blocks[1].terminator = Some(Terminator::Trap { reason: raw.into() });
    m.functions[0].blocks[0]
        .instructions
        .push(Instruction::ConstDecl {
            dst: ValueId(43),
            name: raw.into(),
            expression: ConstExpr::literal(Type::I32, ConstValue::I(1)),
            value: ConstValue::I(1),
        });
    m.functions[0].value_types.push((ValueId(43), Type::I32));
    m.functions[0].blocks[0]
        .instructions
        .push(Instruction::TrapIf {
            cond: ValueId(41),
            reason: raw.into(),
        });
    m.declarations.push(FunctionDecl {
        id: FunctionId(23),
        name: "external".into(),
        signature: FunctionSignature::whale(vec![], Type::Void),
        linkage: Linkage::External,
        link_name: Some(raw.replace('\0', "")),
    });
    verify_module(&m).unwrap();
    let before = print_module(&m);
    // Target validation is independent of printing an arbitrary input string.
    m.target = raw.into();
    let text = print_module(&m);
    assert_eq!(text.lines().count(), before.lines().count());
    for prefix in [
        "target ",
        "global @g7 ",
        "declare @f21 ",
        "fn @f21 ",
        "%v40 ",
        "%b40 ",
        "const_decl ",
        "reason=",
    ] {
        assert!(
            text.contains(&format!("{prefix}\"{escaped}\"")),
            "{prefix}: {text}"
        );
    }
    assert!(text.contains(&format!("link_name \"{}\"", escaped.replace(r"\0", ""))));
    for ch in [
        '\0', '\r', '\t', '\u{1}', '\u{7f}', '\u{85}', '\u{2028}', '\u{2029}',
    ] {
        assert!(!text.contains(ch));
    }
}
