use ir::{
    allocation_align_for_target, layout_for_target, verify_module_for_target, DataLayout, Endian,
    IrLimits, Module, Type,
};

use target_spec::{MemoryModel, OutputKind, StorageLayout, TargetSpec};

fn target() -> TargetSpec {
    TargetSpec {
        id: "vendor.example32".into(),
        output: OutputKind::Custom("example.image".into()),
        storage: StorageLayout {
            data: DataLayout { ptr_bits: 32, endian: Endian::Big },
            scalar_align: [1, 2, 4, 4, 8],
            pointer_align: 4,
            large_array: None,
        },
        memory: MemoryModel::Tracked,
    }
}

#[test]
fn external_layout_controls_offsets_pointer_width_and_array_placement() {
    let t = target();
    let ty = Type::Struct(vec![Type::U8, Type::I64, Type::ptr_to(Type::I32)]);
    let layout = layout_for_target(&ty, &t, 128).unwrap();
    assert_eq!(layout.field_offsets, [0, 4, 12]);
    assert_eq!((layout.size, layout.align), (16, 4));
    assert_eq!(allocation_align_for_target(&Type::Array(Box::new(Type::U8), 20), &t).unwrap(), 1);
    assert!(matches!(
        layout_for_target(&Type::Array(Box::new(Type::I64), 1 << 30), &t, 128),
        Err(ir::LayoutError::Overflow)
    ));
    let m = Module::new(&t.id, t.storage.data);
    verify_module_for_target(&m, &t, IrLimits::default()).unwrap();
    assert!(
        ir::verify_module(&m).is_err(),
        "built-in lookup must not silently accept unknown targets"
    );
}

#[test]
fn invalid_or_mismatched_target_contracts_fail_before_layout() {
    let mut t = target();
    let m = Module::new(&t.id, t.storage.data);
    t.storage.scalar_align[2] = 3;
    assert!(verify_module_for_target(&m, &t, IrLimits::default()).is_err());
    assert_eq!(layout_for_target(&Type::I32, &t, 128), Err(ir::LayoutError::InvalidTarget));
    let mut t = target();
    t.id = "another-target".into();
    assert!(verify_module_for_target(&m, &t, IrLimits::default()).is_err());
}
