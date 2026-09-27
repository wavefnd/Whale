use ir::{verify_module, DataLayout, Endian, Module, Target, TargetError, VerifyError};

#[test]
fn verifier_requires_a_supported_target_and_its_exact_layout() {
    let target = Target::lookup("x86_64-whale-linux").unwrap();
    let mut module = Module::new(target.name(), target.data_layout());
    verify_module(&module).unwrap();
    module.target = "banana".into();
    assert!(matches!(
        verify_module(&module),
        Err(VerifyError::Target(TargetError::UnsupportedTarget(_)))
    ));
    module.target = target.name().into();
    for layout in [
        DataLayout {
            ptr_bits: 32,
            endian: Endian::Little,
        },
        DataLayout {
            ptr_bits: 64,
            endian: Endian::Big,
        },
        DataLayout {
            ptr_bits: 0,
            endian: Endian::Little,
        },
    ] {
        module.datalayout = layout;
        assert!(matches!(
            verify_module(&module),
            Err(VerifyError::Target(TargetError::LayoutMismatch { .. }))
        ));
    }
}

#[cfg(feature = "socket")]
#[test]
fn lowering_rejects_target_layout_mismatches_without_relying_on_verification() {
    use ir::lower_ast::{frontend::Program, lower_o0, LowerError};
    let program = Program {
        declarations: vec![],
        globals: vec![],
        functions: vec![],
    };
    let target = Target::X86_64WhaleLinux;
    assert!(matches!(
        lower_o0(&program, "banana", target.data_layout()),
        Err(LowerError::Target(TargetError::UnsupportedTarget(_)))
    ));
    for layout in [
        DataLayout {
            ptr_bits: 32,
            endian: Endian::Little,
        },
        DataLayout {
            ptr_bits: 64,
            endian: Endian::Big,
        },
    ] {
        assert!(matches!(
            lower_o0(&program, target.name(), layout),
            Err(LowerError::Target(TargetError::LayoutMismatch { .. }))
        ));
    }
    let module = lower_o0(&program, target.name(), target.data_layout()).unwrap();
    assert_eq!(
        module.datalayout,
        DataLayout {
            ptr_bits: 64,
            endian: Endian::Little
        }
    );
}
