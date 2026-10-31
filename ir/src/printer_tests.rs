// SPDX-License-Identifier: MPL-2.0
use super::*;

use crate::*;

#[test]
fn canonical_empty_module_is_versioned_and_exact() {
    let module = Module::new(
        "x86_64-unknown-linux-gnu",
        DataLayout { ptr_bits: 64, endian: Endian::Little },
    );
    let expected = format!(
        "module {{\n  format_version {}\n  semantics_version {}\n  target \"x86_64-unknown-linux-gnu\"\n  datalayout {{ ptr=64, endian=little }}\n\n}}\n",
        IR_FORMAT_VERSION, SEMANTICS_VERSION
    );
    assert_eq!(print_module(&module), expected);
}

fn exhaustive_module() -> Module {
    let mut module = Module::new(
        "name\"\\\n\r\t\0\u{7f}\u{85}\u{2028}😀",
        DataLayout { ptr_bits: 32, endian: Endian::Big },
    );
    let names = "mixed\"\\\n\0\u{1f}\u{9f}\u{2029}한글";
    module.sources.files.push(SourceFile { id: 9, name: names.into() });
    module.sources.originals.push(OriginalNode {
        id: u64::MAX,
        span: Some(SourceSpan { file: 9, line: 1, column: 2, end_line: 3, end_column: 4 }),
    });
    module.sources.originals.push(OriginalNode { id: 0, span: None });
    module.sources.mappings.extend([
        SourceMapping {
            target: SourceTarget::Instruction {
                function: FunctionId(8),
                block: BlockId(9),
                index: usize::MAX,
            },
            original: u64::MAX,
        },
        SourceMapping {
            target: SourceTarget::Terminator { function: FunctionId(8), block: BlockId(9) },
            original: 1,
        },
        SourceMapping { target: SourceTarget::Global(GlobalId(5)), original: 2 },
    ]);
    let literal = |x| ConstExpr::literal(Type::I128, ConstValue::I(x));
    let mut expressions = vec![
        literal(i128::MIN),
        ConstExpr {
            ty: Type::I128,
            kind: ConstExprKind::Reference(ConstRef::Global(GlobalId(u32::MAX))),
        },
        ConstExpr {
            ty: Type::I128,
            kind: ConstExprKind::Reference(ConstRef::Local(ValueId(u32::MAX))),
        },
        ConstExpr { ty: Type::I128, kind: ConstExprKind::Neg(Box::new(literal(12))) },
        ConstExpr { ty: Type::I128, kind: ConstExprKind::Not(Box::new(literal(12))) },
    ];
    for op in [
        ConstBinaryOp::Add,
        ConstBinaryOp::Sub,
        ConstBinaryOp::Mul,
        ConstBinaryOp::BitAnd,
        ConstBinaryOp::BitOr,
        ConstBinaryOp::BitXor,
        ConstBinaryOp::Div,
        ConstBinaryOp::Rem,
        ConstBinaryOp::ShiftLeft,
        ConstBinaryOp::ShiftRight,
        ConstBinaryOp::LogicalAnd,
        ConstBinaryOp::LogicalOr,
    ] {
        expressions.push(ConstExpr {
            ty: Type::I128,
            kind: ConstExprKind::Binary {
                op,
                left: Box::new(literal(2)),
                right: Box::new(literal(3)),
            },
        });
    }
    for op in [
        ConstCompareOp::Eq,
        ConstCompareOp::Ne,
        ConstCompareOp::Lt,
        ConstCompareOp::Le,
        ConstCompareOp::Gt,
        ConstCompareOp::Ge,
    ] {
        expressions.push(ConstExpr {
            ty: Type::Bool,
            kind: ConstExprKind::Compare {
                op,
                left: Box::new(literal(2)),
                right: Box::new(literal(3)),
            },
        });
    }
    for (i, expression) in expressions.iter().enumerate() {
        module.globals.push(Global {
            id: GlobalId(i as u32),
            name: names.into(),
            ty: expression.ty.clone(),
            init: ConstValue::I(0),
            init_expr: expression.clone(),
            align: 16,
        });
    }
    for (i, init) in [
        GlobalStorageInit::Value(ConstValue::Bool(true)),
        GlobalStorageInit::Value(ConstValue::Bool(false)),
        GlobalStorageInit::Uninitialized,
    ]
    .into_iter()
    .enumerate()
    {
        module.global_storage.push(GlobalStorage {
            id: GlobalId(i as u32),
            name: names.into(),
            ty: Type::Bool,
            init,
            readonly: i % 2 == 0,
            align: 8,
            linkage: if i % 2 == 0 { Linkage::Internal } else { Linkage::External },
            link_name: if i % 2 == 0 { None } else { Some(names.into()) },
        });
    }
    let mut types = vec![
        Type::Void,
        Type::Bool,
        Type::I1,
        Type::U1,
        Type::I8,
        Type::I16,
        Type::I32,
        Type::I64,
        Type::I128,
        Type::U8,
        Type::U16,
        Type::U32,
        Type::U64,
        Type::U128,
        Type::F16,
        Type::F32,
        Type::F64,
        Type::Ptr(Box::new(Type::I8)),
        Type::Array(Box::new(Type::U16), u64::MAX),
        Type::Struct(vec![Type::Bool, Type::I32]),
        Type::Tuple(vec![Type::Void, Type::U128]),
        Type::Struct(vec![]),
        Type::Tuple(vec![]),
    ];
    let sig = FunctionSignature {
        params: types.clone(),
        ret: Type::Void,
        convention: CallingConvention::Whale,
        variadic: false,
    };
    types.push(Type::FnPtr(Box::new(sig.clone())));
    module.declarations.push(FunctionDecl {
        id: FunctionId(99),
        name: names.into(),
        signature: sig.clone(),
        linkage: Linkage::Internal,
        link_name: None,
    });
    module.declarations.push(FunctionDecl {
        id: FunctionId(100),
        name: names.into(),
        signature: FunctionSignature {
            params: vec![],
            ret: Type::I8,
            convention: CallingConvention::SysV64,
            variadic: true,
        },
        linkage: Linkage::External,
        link_name: Some(names.into()),
    });
    let a = ValueId(1);
    let b = ValueId(u32::MAX);
    let c = ValueId(3);
    let mut instructions = vec![
        Instruction::GlobalAddr { dst: a, global: GlobalId(5), ty: Type::I32 },
        Instruction::LifetimeEnd { ptr: b },
        Instruction::PtrDiff { dst: a, lhs: b, rhs: c },
        Instruction::ConstDecl {
            dst: a,
            name: names.into(),
            expression: expressions[0].clone(),
            value: ConstValue::I(i128::MIN),
        },
    ];
    for ty in &types {
        instructions.push(Instruction::Undef { dst: a, ty: ty.clone() });
    }
    for value in [
        ConstValue::Bool(true),
        ConstValue::Bool(false),
        ConstValue::I(i128::MIN),
        ConstValue::I(i128::MAX),
        ConstValue::U(u128::MAX),
        ConstValue::U(0),
        ConstValue::F(FloatBits::F16(0x8000)),
        ConstValue::F(FloatBits::F32(0x7fa01234)),
        ConstValue::F(FloatBits::F64(0xffff123456789abc)),
    ] {
        instructions.push(Instruction::Const { dst: a, ty: Type::I128, value });
    }
    instructions.push(Instruction::Mov { dst: a, ty: Type::I32, src: b });
    for op in [
        BinOp::Add,
        BinOp::Sub,
        BinOp::Mul,
        BinOp::UDiv,
        BinOp::SDiv,
        BinOp::URem,
        BinOp::SRem,
        BinOp::FAdd,
        BinOp::FSub,
        BinOp::FMul,
        BinOp::FDiv,
        BinOp::FRem,
        BinOp::And,
        BinOp::Or,
        BinOp::Xor,
        BinOp::Shl,
        BinOp::LShr,
        BinOp::AShr,
    ] {
        instructions.push(Instruction::Bin { dst: a, op, ty: Type::I64, lhs: b, rhs: c });
    }
    instructions.push(Instruction::Neg { dst: a, ty: Type::I32, src: b });
    instructions.push(Instruction::Not { dst: a, ty: Type::I32, src: b });
    for op in [
        CmpOp::Eq,
        CmpOp::Ne,
        CmpOp::SLt,
        CmpOp::SLe,
        CmpOp::SGt,
        CmpOp::SGe,
        CmpOp::ULt,
        CmpOp::ULe,
        CmpOp::UGt,
        CmpOp::UGe,
        CmpOp::FEq,
        CmpOp::FNe,
        CmpOp::FLt,
        CmpOp::FLe,
        CmpOp::FGt,
        CmpOp::FGe,
    ] {
        instructions.push(Instruction::Cmp { dst: a, op, ty: Type::I32, lhs: b, rhs: c });
    }
    for pred in [
        ICmpPred::Eq,
        ICmpPred::Ne,
        ICmpPred::Ult,
        ICmpPred::Ule,
        ICmpPred::Ugt,
        ICmpPred::Uge,
        ICmpPred::Slt,
        ICmpPred::Sle,
        ICmpPred::Sgt,
        ICmpPred::Sge,
    ] {
        instructions.push(Instruction::ICmp { dst: a, pred, ty: Type::I32, lhs: b, rhs: c });
    }
    for pred in [
        FCmpPred::Oeq,
        FCmpPred::One,
        FCmpPred::Olt,
        FCmpPred::Ole,
        FCmpPred::Ogt,
        FCmpPred::Oge,
        FCmpPred::Ord,
        FCmpPred::Uno,
        FCmpPred::Ueq,
        FCmpPred::Une,
        FCmpPred::Ult,
        FCmpPred::Ule,
        FCmpPred::Ugt,
        FCmpPred::Uge,
    ] {
        instructions.push(Instruction::FCmp { dst: a, pred, ty: Type::F64, lhs: b, rhs: c });
    }
    instructions.push(Instruction::Select {
        dst: a,
        ty: Type::I32,
        cond: b,
        on_true: c,
        on_false: a,
    });
    for op in [
        CastOp::CheckedInt,
        CastOp::ZExt,
        CastOp::SExt,
        CastOp::Trunc,
        CastOp::FExt,
        CastOp::FTrunc,
        CastOp::IToF_S,
        CastOp::IToF_U,
        CastOp::FToI_S,
        CastOp::FToI_U,
        CastOp::Bitcast,
        CastOp::PtrToInt,
        CastOp::IntToPtr,
    ] {
        instructions.push(Instruction::Cast {
            dst: a,
            op,
            dst_ty: Type::U128,
            src_ty: Type::I32,
            src: b,
        });
    }
    instructions.extend([
        Instruction::Phi {
            dst: a,
            ty: Type::I32,
            incomings: vec![(b, BlockId(1)), (c, BlockId(2))],
        },
        Instruction::Phi { dst: a, ty: Type::I32, incomings: vec![] },
        Instruction::Extract { dst: a, dst_ty: Type::I32, tuple: b, index: 2 },
    ]);
    for op in [
        CheckedOp::UAdd,
        CheckedOp::USub,
        CheckedOp::UMul,
        CheckedOp::SAdd,
        CheckedOp::SSub,
        CheckedOp::SMul,
    ] {
        instructions.push(Instruction::Checked { dst: a, op, ty: Type::I32, lhs: b, rhs: c });
    }
    instructions.extend([
        Instruction::Alloca { dst: a, ty: Type::I32, align: u32::MAX },
        Instruction::VolatileLoad { dst: a, ty: Type::I32, ptr: b, align: 4 },
        Instruction::VolatileStore { ty: Type::I32, value: a, ptr: b, align: 4 },
        Instruction::Load { dst: a, ty: Type::I32, ptr: b, align: 4 },
        Instruction::Uninit { ty: Type::I32, ptr: b, align: 4 },
        Instruction::Store { ty: Type::I32, value: a, ptr: b, align: 4 },
        Instruction::Gep {
            dst: a,
            dst_ty: Type::Ptr(Box::new(Type::I32)),
            base_ptr: b,
            indices: vec![a, b, c],
        },
        Instruction::Gep {
            dst: a,
            dst_ty: Type::Ptr(Box::new(Type::I32)),
            base_ptr: b,
            indices: vec![],
        },
        Instruction::Memcpy { dst: a, src: b, n: c, align: 1 },
        Instruction::Memset { dst: a, val: b, n: c, align: 1 },
        Instruction::NullFunction { dst: a, signature: sig.clone() },
        Instruction::FunctionAddr { dst: a, function: FunctionId(100), signature: sig.clone() },
        Instruction::Call {
            convention: CallingConvention::Whale,
            dst: Some(a),
            ret_ty: Type::I32,
            callee: Callee::Direct(FunctionId(99)),
            args: vec![b, c],
        },
        Instruction::Call {
            convention: CallingConvention::SysV64,
            dst: None,
            ret_ty: Type::Void,
            callee: Callee::Indirect(b),
            args: vec![],
        },
        Instruction::TrapIf { cond: a, reason: names.into() },
    ]);
    let terms = vec![
        Some(Terminator::Br { target: BlockId(2) }),
        Some(Terminator::CBr { cond: a, then_bb: BlockId(3), else_bb: BlockId(4) }),
        Some(Terminator::Switch {
            ty: Type::I128,
            value: a,
            default_bb: BlockId(1),
            cases: vec![
                (ConstValue::I(i128::MIN), BlockId(2)),
                (ConstValue::U(u128::MAX), BlockId(3)),
            ],
        }),
        Some(Terminator::Switch {
            ty: Type::Bool,
            value: a,
            default_bb: BlockId(1),
            cases: vec![],
        }),
        Some(Terminator::Ret { ty: Type::I32, value: Some(a) }),
        Some(Terminator::Ret { ty: Type::I32, value: None }),
        Some(Terminator::Ret { ty: Type::Void, value: Some(a) }),
        Some(Terminator::Trap { reason: names.into() }),
        None,
    ];
    module.functions.push(Function {
        id: FunctionId(8),
        name: names.into(),
        params: types
            .iter()
            .enumerate()
            .map(|(i, ty)| Param { id: ValueId(i as u32), name: names.into(), ty: ty.clone() })
            .collect(),
        ret_ty: Type::Tuple(types),
        blocks: terms
            .into_iter()
            .enumerate()
            .map(|(i, terminator)| BasicBlock {
                id: BlockId(i as u32),
                name: names.into(),
                instructions: if i == 0 { instructions.clone() } else { vec![] },
                terminator,
            })
            .collect(),
        entry: BlockId(0),
        value_types: vec![],
    });
    module
}

#[test]
fn all_current_model_variants_match_canonical_reference() {
    let module = exhaustive_module();
    assert_eq!(print_module(&module), reference::print_module(&module));
    for global in &module.globals {
        assert_eq!(
            print_const_expr(&global.init_expr),
            reference::print_const_expr(&global.init_expr)
        );
    }
}

#[cfg(whale_wave_ir)]
#[test]
fn exact_byte_ceiling_and_streaming_writer_failure() {
    let module = exhaustive_module();
    let expected = reference::print_module(&module);
    let limits = PrintLimits { max_output_bytes: expected.len(), ..PrintLimits::default() };
    assert_eq!(try_print_module(&module, limits).unwrap(), expected);
    assert!(matches!(
        try_print_module(&module, PrintLimits { max_output_bytes: expected.len() - 1, ..limits }),
        Err(PrintError::Output)
    ));
    struct Sink {
        bytes: usize,
        calls: usize,
    }

    impl std::io::Write for Sink {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            assert!(bytes.len() <= 4096);
            self.bytes += bytes.len();
            self.calls += 1;
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let mut sink = Sink { bytes: 0, calls: 0 };
    write_module(&module, &mut sink, limits).unwrap();
    assert_eq!(sink.bytes, expected.len());
    assert!(sink.calls > 1);
    struct Failing;

    impl std::io::Write for Failing {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("injected"))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    assert!(matches!(write_module(&module, &mut Failing, limits), Err(PrintError::Io(_))));
    for (limits, expected) in [
        (PrintLimits { max_output_bytes: 0, ..limits }, "output"),
        (PrintLimits { max_nodes: 0, ..limits }, "model"),
        (PrintLimits { max_work: 0, ..limits }, "work"),
        (PrintLimits { max_stack: 0, ..limits }, "workspace"),
        (PrintLimits { max_depth: 0, ..limits }, "depth"),
    ] {
        assert!(try_print_module(&module, limits).unwrap_err().to_string().contains(expected));
    }
}

#[cfg(whale_wave_ir)]
#[test]
fn generated_128_bit_values_exact_float_bits_and_unicode() {
    let mut seed = 0x123456789abcdef0u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for _ in 0..4096 {
        let low = next();
        let high = next();
        let bits = (u128::from(high) << 64) | u128::from(low);
        for value in [
            ConstValue::I(bits as i128),
            ConstValue::U(bits),
            ConstValue::F(FloatBits::F16(low as u16)),
            ConstValue::F(FloatBits::F32(low as u32)),
            ConstValue::F(FloatBits::F64(high)),
        ] {
            let expr = ConstExpr::literal(Type::U128, value);
            assert_eq!(print_const_expr(&expr), reference::print_const_expr(&expr));
        }
    }
    let mut module = Module::new("", DataLayout { ptr_bits: 64, endian: Endian::Little });
    module.target = (0..=159)
        .filter_map(char::from_u32)
        .chain(['\u{2028}', '\u{2029}', 'é', '🦀', '\u{10ffff}'])
        .collect();
    assert_eq!(print_module(&module), reference::print_module(&module));
}

#[cfg(whale_wave_ir)]
#[test]
fn long_instruction_lists_use_bounded_resumable_workspace() {
    let mut module = Module::new(
        "x86_64-unknown-linux-gnu",
        DataLayout { ptr_bits: 64, endian: Endian::Little },
    );
    module.functions.push(Function {
        id: FunctionId(0),
        name: "long".into(),
        params: vec![],
        ret_ty: Type::Void,
        blocks: vec![BasicBlock {
            id: BlockId(0),
            name: "entry".into(),
            instructions: (0..60_000)
                .map(|i| Instruction::Const {
                    dst: ValueId(i),
                    ty: Type::I32,
                    value: ConstValue::I(i128::from(i)),
                })
                .collect(),
            terminator: Some(Terminator::Ret { ty: Type::Void, value: None }),
        }],
        entry: BlockId(0),
        value_types: vec![],
    });
    let limits = PrintLimits { max_stack: 128, ..PrintLimits::default() };
    assert_eq!(try_print_module(&module, limits).unwrap(), reference::print_module(&module));
}

#[cfg(whale_wave_ir)]
#[test]
fn nested_function_signatures_do_not_confuse_model_and_vm_depth() {
    let mut ty = Type::I32;
    for _ in 0..180 {
        ty = Type::FnPtr(Box::new(FunctionSignature::whale(vec![], ty)));
    }
    let expression = ConstExpr::literal(ty, ConstValue::U(0));
    let limits = PrintLimits { max_depth: 256, ..PrintLimits::default() };
    let mut output = Vec::new();
    wave::write_expression(&expression, &mut output, limits).unwrap();
    assert_eq!(String::from_utf8(output).unwrap(), reference::print_const_expr(&expression));
}

#[cfg(whale_wave_ir)]
#[test]
fn printer_preserves_the_supported_256_level_type_and_constant_ceiling() {
    let mut ty = Type::I32;
    for _ in 0..MAX_IR_NESTING {
        ty = Type::FnPtr(Box::new(FunctionSignature::whale(vec![ty], Type::Void)));
    }
    let mut module = Module::new("x86_64-whale-linux", DataLayout::default_64bit_le());
    module.declarations.push(FunctionDecl {
        id: FunctionId(0),
        name: "deep".into(),
        signature: FunctionSignature::whale(vec![ty], Type::Void),
        linkage: Linkage::External,
        link_name: Some("deep".into()),
    });
    assert_eq!(print_module(&module), reference::print_module(&module));
    assert!(matches!(
        try_print_module(
            &module,
            PrintLimits { max_depth: MAX_IR_NESTING - 1, ..PrintLimits::default() }
        ),
        Err(PrintError::Depth)
    ));
    let mut expression = ConstExpr::literal(Type::I32, ConstValue::I(0));
    for _ in 0..MAX_IR_NESTING {
        expression = ConstExpr { ty: Type::I32, kind: ConstExprKind::Neg(Box::new(expression)) };
    }
    assert_eq!(print_const_expr(&expression), reference::print_const_expr(&expression));
}
