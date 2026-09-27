use ir::{layout_of, LayoutError, Target, Type};

#[test]
fn nested_aggregate_layout_matches_amd64_fields_and_padding() {
    let record = Type::Struct(vec![Type::U8, Type::U64, Type::U16]);
    let layout = layout_of(&record, Target::X86_64WhaleLinux).unwrap();
    assert_eq!((layout.size, layout.align), (24, 8));
    assert_eq!(layout.field_offsets, [0, 8, 16]);
    let nested = Type::Tuple(vec![
        Type::Bool,
        Type::Array(Box::new(record), 3),
        Type::U32,
    ]);
    let layout = layout_of(&nested, Target::X86_64WhaleLinux).unwrap();
    assert_eq!((layout.size, layout.align), (88, 8));
    assert_eq!(layout.field_offsets, [0, 8, 80]);
}

#[test]
fn layout_overflow_is_an_error_without_allocating_elements() {
    let ty = Type::Array(Box::new(Type::U64), u64::MAX);
    assert_eq!(
        layout_of(&ty, Target::X86_64WhaleLinux),
        Err(LayoutError::Overflow)
    );
}

#[test]
fn scalar_storage_is_target_defined_and_pointers_do_not_layout_pointees() {
    use Type::*;
    let target = Target::X86_64WhaleLinux;
    for (types, size) in [
        (vec![Bool, I1, U1, I8, U8], 1),
        (vec![I16, U16, F16], 2),
        (vec![I32, U32, F32], 4),
        (vec![I64, U64, F64, Ptr(Box::new(Void))], 8),
        (vec![I128, U128], 16),
    ] {
        for ty in types {
            let l = layout_of(&ty, target).unwrap();
            assert_eq!((l.size, u64::from(l.align)), (size, size));
            assert!(l.field_offsets.is_empty());
            assert_eq!(l.element_stride, None);
        }
    }
    assert_eq!(layout_of(&Void, target), Err(LayoutError::Void));
    assert_eq!(
        layout_of(&Array(Box::new(Void), 0), target),
        Err(LayoutError::Void)
    );
}

#[test]
fn arrays_use_padded_stride_but_standalone_alignment_does_not_move_fields() {
    let target = Target::X86_64WhaleLinux;
    // Golden values also obtained by GCC/Clang from fixtures/amd64_layout.c.
    let array = Type::Array(
        Box::new(Type::Struct(vec![Type::U8, Type::U64, Type::U16])),
        3,
    );
    let l = layout_of(&array, target).unwrap();
    assert_eq!((l.size, l.align, l.element_stride), (72, 8, Some(24)));
    assert_eq!(ir::allocation_align(&array, target).unwrap(), 16);
    let bytes = Type::Array(Box::new(Type::U8), 16);
    let record = Type::Struct(vec![Type::U8, bytes.clone(), Type::U8]);
    let l = layout_of(&record, target).unwrap();
    assert_eq!((l.size, l.align), (18, 1));
    assert_eq!(l.field_offsets, [0, 1, 17]);
    assert_eq!(ir::allocation_align(&bytes, target).unwrap(), 16);
    assert_eq!(
        ir::allocation_align(&Type::Array(Box::new(Type::U8), 15), target).unwrap(),
        1
    );
    let l = layout_of(&Type::Struct(vec![Type::U8, Type::I128, Type::U8]), target).unwrap();
    assert_eq!((l.size, l.align), (48, 16));
    assert_eq!(l.field_offsets, [0, 16, 32]);
}

#[test]
fn empty_aggregates_have_layout_but_no_native_pointer_stride() {
    let target = Target::X86_64WhaleLinux;
    for ty in [Type::Struct(vec![]), Type::Tuple(vec![])] {
        let l = layout_of(&ty, target).unwrap();
        assert_eq!((l.size, l.align), (0, 1));
        assert_eq!(
            ir::pointer_stride(&ty, target),
            Err(LayoutError::ZeroSizedPointerArithmetic)
        );
        let l = layout_of(&Type::Array(Box::new(ty), u64::MAX), target).unwrap();
        assert_eq!((l.size, l.align, l.element_stride), (0, 1, Some(0)));
    }
    let l = layout_of(&Type::Array(Box::new(Type::U64), 0), target).unwrap();
    assert_eq!((l.size, l.align, l.element_stride), (0, 8, Some(8)));
    assert_eq!(
        ir::pointer_stride(&Type::Array(Box::new(Type::I32), 4), target).unwrap(),
        16
    );
}

#[test]
fn field_addition_padding_and_nesting_fail_without_wrapping() {
    let target = Target::X86_64WhaleLinux;
    let huge = Type::Array(Box::new(Type::U8), u64::MAX);
    assert_eq!(layout_of(&huge, target).unwrap().size, u64::MAX);
    // Field size addition, field alignment, then tail padding overflow.
    for ty in [
        Type::Struct(vec![huge.clone(), Type::U8]),
        Type::Struct(vec![huge, Type::U64]),
        Type::Struct(vec![
            Type::U64,
            Type::Array(Box::new(Type::U8), u64::MAX - 8),
        ]),
    ] {
        assert_eq!(layout_of(&ty, target), Err(LayoutError::Overflow));
    }
    let nested = Type::Array(Box::new(Type::Array(Box::new(Type::U8), 1)), 1);
    assert_eq!(
        ir::layout_of_with_limit(&nested, target, 1),
        Err(LayoutError::DepthLimit)
    );
    assert_eq!(
        ir::layout_of_with_limit(&nested, target, 2).unwrap().size,
        1
    );
}

#[cfg(feature = "socket")]
#[test]
fn lowering_uses_allocation_layout_and_rejects_overflow() {
    use ir::lower_ast::{frontend as ast, lower_o0, LowerError};
    let make = |len| ast::Program {
        declarations: vec![],
        globals: vec![],
        functions: vec![ast::Function {
            convention: ir::CallingConvention::Whale,
            linkage: ir::Linkage::Internal,
            link_name: None,
            name: "array_parameter".into(),
            parameters: vec![ast::Parameter {
                name: "a".into(),
                ty: ast::TypeRef::Array {
                    elem: Box::new(ast::TypeRef::Int {
                        bits: 64,
                        signed: false,
                    }),
                    len,
                },
            }],
            return_type: ast::TypeRef::Void,
            body: vec![ast::Stmt::Return(None)],
        }],
    };
    let target = Target::X86_64WhaleLinux;
    let module = lower_o0(&make(2), target.name(), target.data_layout()).unwrap();
    ir::verify_module(&module).unwrap();
    assert!(ir::print_module(&module).contains("alloca array<u64, 2>, align 16"));
    assert!(matches!(
        lower_o0(&make(u64::MAX), target.name(), target.data_layout()),
        Err(LowerError::Layout(LayoutError::Overflow))
    ));
}
