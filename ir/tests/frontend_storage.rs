#![cfg(feature = "socket")]
use ir::lower_ast::{frontend as a, interchange};

use ir::*;

fn uint(bits: u16) -> a::TypeRef {
    a::TypeRef::Int { bits, signed: false }
}

fn n(bits: u16, value: u128) -> a::Expr {
    a::Expr::Lit(a::Lit::Int { bits, signed: false, value: value.to_string() })
}

fn var(name: &str) -> a::Expr {
    a::Expr::Var(name.into())
}

fn decl(name: &str, ty: a::TypeRef, value: a::Expr) -> a::Stmt {
    a::Stmt::VarDecl { name: name.into(), ty, init: Some(value) }
}

fn function(name: &str, ret: a::TypeRef, body: Vec<a::Stmt>) -> a::Function {
    a::Function {
        name: name.into(),
        return_type: ret,
        parameters: vec![],
        body,
        convention: CallingConvention::Whale,
        linkage: Linkage::Internal,
        link_name: None,
    }
}

fn lower(functions: Vec<a::Function>) -> Result<Module, LowerError> {
    let p = a::Program { declarations: vec![], globals: vec![], functions };
    let json = interchange::encode(p).unwrap();
    let p = interchange::decode(&json).unwrap();
    let m = lower_o0(&p, "x86_64-whale-linux", DataLayout::default_64bit_le())?;
    verify_module(&m).unwrap();
    let text = print_module(&m);
    assert_eq!(print_module(&parse_module(&text).unwrap()), text);
    Ok(m)
}

fn record() -> a::TypeRef {
    a::TypeRef::Struct {
        fields: vec![
            a::StructField { name: "byte".into(), ty: uint(8) },
            a::StructField { name: "wide".into(), ty: uint(64) },
        ],
    }
}

fn field(base: a::Expr, name: &str) -> a::Expr {
    a::Expr::Field { base: Box::new(base), layout: record(), name: name.into() }
}

fn index(base: a::Expr, index: a::Expr) -> a::Expr {
    a::Expr::Index { base: Box::new(base), index: Box::new(index) }
}

fn array() -> a::TypeRef {
    a::TypeRef::Array { elem: Box::new(uint(32)), len: 3 }
}

fn literal() -> a::Expr {
    a::Expr::Array { ty: array(), elements: vec![n(32, 10), n(32, 20), n(32, 30)] }
}

fn address(e: a::Expr) -> a::Expr {
    a::Expr::AddressOf(Box::new(e))
}

fn deref(e: a::Expr) -> a::Expr {
    a::Expr::Deref(Box::new(e))
}

#[test]
fn aggregate_assignment_snapshots_aliasing_rhs_without_changing_explicit_memcpy() {
    let assignments = [
        a::Stmt::Assign { name: "a".into(), value: var("a") },
        a::Stmt::Store { place: var("a"), value: var("a") },
        a::Stmt::Store { place: deref(address(var("a"))), value: var("a") },
    ];
    for assignment in assignments {
        let m = lower(vec![function(
            "array_alias",
            uint(32),
            vec![
                decl("a", array(), literal()),
                assignment,
                a::Stmt::Return(Some(index(var("a"), n(8, 1)))),
            ],
        )])
        .unwrap();
        assert_eq!(interpret(&m, FunctionId(0), &[]).unwrap().value, Some(ConstValue::U(20)));
    }
    let init = a::Expr::Struct {
        ty: record(),
        fields: vec![
            a::FieldInit { name: "byte".into(), value: n(8, 1) },
            a::FieldInit { name: "wide".into(), value: n(64, 42) },
        ],
    };
    for assignment in [
        a::Stmt::Assign { name: "s".into(), value: var("s") },
        a::Stmt::Store { place: deref(address(var("s"))), value: var("s") },
    ] {
        let m = lower(vec![function(
            "struct_alias",
            uint(64),
            vec![
                decl("s", record(), init.clone()),
                assignment,
                a::Stmt::Return(Some(field(var("s"), "wide"))),
            ],
        )])
        .unwrap();
        assert_eq!(interpret(&m, FunctionId(0), &[]).unwrap().value, Some(ConstValue::U(42)));
    }
}

#[test]
fn padded_structs_construct_copy_read_and_update_without_host_layout() {
    let init = a::Expr::Struct {
        ty: record(),
        fields: vec![
            a::FieldInit { name: "wide".into(), value: n(64, 40) },
            a::FieldInit { name: "byte".into(), value: n(8, 1) },
        ],
    };
    let body = vec![
        decl("s", record(), init),
        decl("copy", record(), var("s")),
        a::Stmt::Store { place: field(var("s"), "wide"), value: n(64, 99) },
        a::Stmt::Store { place: deref(address(field(var("copy"), "wide"))), value: n(64, 42) },
        a::Stmt::Return(Some(field(var("copy"), "wide"))),
    ];
    let m = lower(vec![function("test", uint(64), body)]).unwrap();
    assert_eq!(interpret(&m, FunctionId(0), &[]).unwrap().value, Some(ConstValue::U(42)));
    assert_eq!(
        layout_of(&Type::Struct(vec![Type::U8, Type::U64]), Target::X86_64WhaleLinux)
            .unwrap()
            .field_offsets,
        vec![0, 8]
    );
    assert!(m.functions[0].blocks.iter().flat_map(|b| &b.instructions).all(|i| !matches!(
        i,
        Instruction::Load { ty: Type::Struct(_), .. }
            | Instruction::Store { ty: Type::Struct(_), .. }
    )));
}

#[test]
fn arrays_check_endpoints_negative_indices_and_compound_pointer_strides() {
    for which in [0, 2] {
        let ptr = a::Expr::PointerOffset {
            base: Box::new(address(index(var("a"), n(8, 0)))),
            index: Box::new(n(8, which)),
        };
        let body = vec![
            decl("a", array(), literal()),
            a::Stmt::Store { place: deref(ptr), value: n(32, 42) },
            a::Stmt::Return(Some(index(var("a"), n(8, which)))),
        ];
        let m = lower(vec![function("test", uint(32), body)]).unwrap();
        assert_eq!(interpret(&m, FunctionId(0), &[]).unwrap().value, Some(ConstValue::U(42)));
    }
    for idx in [
        n(8, 3),
        n(128, u128::MAX),
        a::Expr::Lit(a::Lit::Int { bits: 8, signed: true, value: "-1".into() }),
    ] {
        let m = lower(vec![function(
            "bad",
            uint(32),
            vec![decl("a", array(), literal()), a::Stmt::Return(Some(index(var("a"), idx)))],
        )])
        .unwrap();
        assert!(
            matches!(interpret(&m,FunctionId(0),&[]),Err(InterpreterError::Trap(t)) if t.reason==TrapReason::Explicit("array index out of bounds".into()))
        );
    }
    let end = a::Expr::PointerOffset {
        base: Box::new(address(index(var("a"), n(8, 0)))),
        index: Box::new(n(8, 3)),
    };
    let m = lower(vec![function(
        "bad",
        uint(32),
        vec![decl("a", array(), literal()), a::Stmt::Return(Some(deref(end)))],
    )])
    .unwrap();
    assert!(
        matches!(interpret(&m,FunctionId(0),&[]),Err(InterpreterError::Trap(t)) if matches!(t.reason,TrapReason::Memory(MemoryTrap::OutOfBounds)))
    );
}

#[test]
fn initialization_calls_bases_and_nested_indices_execute_once_in_source_order() {
    // mark returns old counter and increments it. After array initializers the
    // next two calls select a[0], which itself becomes the outer array index.
    let ptr = a::TypeRef::Ptr(Box::new(uint(32)));
    let mark = a::Function {
        parameters: vec![a::Parameter { name: "p".into(), ty: ptr.clone() }],
        ..function(
            "mark",
            uint(32),
            vec![
                decl("old", uint(32), deref(var("p"))),
                a::Stmt::Store {
                    place: deref(var("p")),
                    value: a::Expr::Binary {
                        left: Box::new(deref(var("p"))),
                        op: a::BinOpRef::Add,
                        right: Box::new(n(32, 1)),
                    },
                },
                a::Stmt::Return(Some(var("old"))),
            ],
        )
    };
    let call = || a::Expr::Call {
        callee: a::CalleeRef::Direct("mark".into()),
        args: vec![address(var("counter"))],
    };
    let init = a::Expr::Array { ty: array(), elements: vec![call(), call(), call()] };
    let nested = index(var("a"), index(var("a"), n(8, 0)));
    let body = vec![
        decl("counter", uint(32), n(32, 0)),
        decl("a", array(), init),
        a::Stmt::Store { place: nested, value: n(32, 42) },
        a::Stmt::Return(Some(a::Expr::Binary {
            left: Box::new(index(var("a"), n(8, 0))),
            op: a::BinOpRef::Add,
            right: Box::new(var("counter")),
        })),
    ];
    let m = lower(vec![function("test", uint(32), body), mark]).unwrap();
    assert_eq!(interpret(&m, FunctionId(0), &[]).unwrap().value, Some(ConstValue::U(45)));
}

#[test]
fn malformed_fields_types_lengths_and_aggregate_abi_fail_lowering() {
    for fields in [
        vec![
            a::FieldInit { name: "byte".into(), value: n(8, 1) },
            a::FieldInit { name: "byte".into(), value: n(8, 1) },
        ],
        vec![
            a::FieldInit { name: "bad".into(), value: n(8, 1) },
            a::FieldInit { name: "wide".into(), value: n(64, 1) },
        ],
        vec![
            a::FieldInit { name: "byte".into(), value: n(64, 1) },
            a::FieldInit { name: "wide".into(), value: n(64, 1) },
        ],
    ] {
        assert!(lower(vec![function(
            "bad",
            uint(8),
            vec![
                decl("s", record(), a::Expr::Struct { ty: record(), fields }),
                a::Stmt::Return(Some(n(8, 0)))
            ]
        )])
        .is_err());
    }
    for values in [vec![], vec![n(8, 1), n(32, 2), n(32, 3)]] {
        assert!(lower(vec![function(
            "bad",
            uint(8),
            vec![
                decl("a", array(), a::Expr::Array { ty: array(), elements: values }),
                a::Stmt::Return(Some(n(8, 0)))
            ]
        )])
        .is_err());
    }
    let mut f = function(
        "bad",
        record(),
        vec![a::Stmt::Return(Some(a::Expr::Struct { ty: record(), fields: vec![] }))],
    );
    assert!(lower(vec![f.clone()]).is_err());
    f.return_type = uint(8);
    f.parameters = vec![a::Parameter { name: "s".into(), ty: record() }];
    assert!(lower(vec![f]).is_err());
    let unknown = field(a::Expr::Struct { ty: record(), fields: vec![] }, "missing");
    assert!(lower(vec![function("bad", uint(64), vec![a::Stmt::Return(Some(unknown))])]).is_err());
}

#[test]
fn null_and_retired_stack_pointer_dereferences_follow_tracked_memory_contract() {
    let f = a::Function {
        parameters: vec![a::Parameter {
            name: "p".into(),
            ty: a::TypeRef::Ptr(Box::new(uint(32))),
        }],
        ..function("read", uint(32), vec![a::Stmt::Return(Some(deref(var("p"))))])
    };
    let m = lower(vec![f]).unwrap();
    let null = RuntimeValue::raw_pointer(Type::ptr_to(Type::U32), 0).unwrap();
    assert!(
        matches!(interpret_values(&m,FunctionId(0),&[null]),Err(InterpreterError::Trap(t)) if matches!(t.reason,TrapReason::Memory(MemoryTrap::NullPointer)))
    );
    let make = function(
        "make",
        a::TypeRef::Ptr(Box::new(uint(32))),
        vec![decl("x", uint(32), n(32, 42)), a::Stmt::Return(Some(address(var("x"))))],
    );
    let read = function(
        "read",
        uint(32),
        vec![a::Stmt::Return(Some(deref(a::Expr::Call {
            callee: a::CalleeRef::Direct("make".into()),
            args: vec![],
        })))],
    );
    let m = lower(vec![read, make]).unwrap();
    assert!(
        matches!(interpret(&m,FunctionId(0),&[]),Err(InterpreterError::Trap(t)) if matches!(t.reason,TrapReason::Memory(MemoryTrap::ExpiredAllocation)))
    );
}

#[test]
fn base_call_precedes_index_call_and_both_execute_once() {
    let pointer = a::TypeRef::Ptr(Box::new(uint(32)));
    let array_pointer = a::TypeRef::Ptr(Box::new(array()));
    let increment = a::Stmt::Store {
        place: deref(var("c")),
        value: a::Expr::Binary {
            left: Box::new(deref(var("c"))),
            op: a::BinOpRef::Add,
            right: Box::new(n(32, 1)),
        },
    };
    let base = a::Function {
        parameters: vec![
            a::Parameter { name: "a".into(), ty: array_pointer.clone() },
            a::Parameter { name: "c".into(), ty: pointer.clone() },
        ],
        ..function("base", array_pointer, vec![increment.clone(), a::Stmt::Return(Some(var("a")))])
    };
    let next = a::Function {
        parameters: vec![a::Parameter { name: "c".into(), ty: pointer }],
        ..function(
            "next",
            uint(32),
            vec![
                decl("old", uint(32), deref(var("c"))),
                increment,
                a::Stmt::Return(Some(var("old"))),
            ],
        )
    };
    let call_base = a::Expr::Call {
        callee: a::CalleeRef::Direct("base".into()),
        args: vec![address(var("a")), address(var("counter"))],
    };
    let call_index = a::Expr::Call {
        callee: a::CalleeRef::Direct("next".into()),
        args: vec![address(var("counter"))],
    };
    let body = vec![
        decl("counter", uint(32), n(32, 0)),
        decl("a", array(), literal()),
        decl("selected", uint(32), index(deref(call_base), call_index)),
        a::Stmt::Return(Some(a::Expr::Binary {
            left: Box::new(var("selected")),
            op: a::BinOpRef::Add,
            right: Box::new(var("counter")),
        })),
    ];
    let m = lower(vec![function("test", uint(32), body), base, next]).unwrap();
    assert_eq!(interpret(&m, FunctionId(0), &[]).unwrap().value, Some(ConstValue::U(22)));
}

#[test]
fn aggregate_storage_expansion_and_new_ast_forms_are_bounded_and_versioned() {
    let fields: Vec<_> =
        (0..256).map(|n| a::StructField { name: format!("f{n}"), ty: uint(8) }).collect();
    let ty = a::TypeRef::Struct { fields };
    let mut body = vec![a::Stmt::VarDecl { name: "s".into(), ty, init: None }];
    for _ in 0..300 {
        body.push(a::Stmt::If {
            cond: a::Expr::Lit(a::Lit::Bool(true)),
            then_body: vec![],
            else_body: vec![],
        });
    }
    body.push(a::Stmt::Return(Some(n(8, 0))));
    let p = a::Program {
        declarations: vec![],
        globals: vec![],
        functions: vec![function("bounded", uint(8), body)],
    };
    let json = interchange::encode(p.clone()).unwrap();
    assert!(interchange::decode(&json.replace("\"format_version\": 5", "\"format_version\": 4"))
        .is_err());
    assert!(matches!(
        lower_o0(&p, "x86_64-whale-linux", DataLayout::default_64bit_le()),
        Err(LowerError::ResourceLimit(AstLimitError::Expansion))
    ));
}
