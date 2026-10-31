//! Reduced products must not acquire all ISAs through the composition root.
#[test]
fn registry_contains_exactly_the_selected_assemblers() {
    let registry = whale::builtins::registry();
    let mut expected = Vec::new();
    if cfg!(feature = "asm-amd64") {
        expected.push("amd64");
    }
    if cfg!(feature = "asm-aarch64") {
        expected.push("aarch64");
    }
    if cfg!(feature = "asm-riscv64") {
        expected.push("riscv64");
    }
    if cfg!(feature = "asm-loongarch64") {
        expected.push("loongarch64");
    }
    expected.sort_unstable();
    assert_eq!(registry.assembler_ids().collect::<Vec<_>>(), expected);
    assert!(registry.target("x86_64-whale-linux").is_some());
    let bytecode = registry.target("whale-bytecode-v1").expect("built-in bytecode target");
    assert_eq!(bytecode.memory, target_spec::MemoryModel::Tracked);
    assert!(matches!(bytecode.output, target_spec::OutputKind::Bytecode(_)));
    assert!(registry.backend("whale-bytecode-v1").is_some());
}

#[test]
fn help_reports_selected_components_and_disabled_linking() {
    let help =
        std::process::Command::new(env!("CARGO_BIN_EXE_whale")).arg("--help").output().unwrap();
    assert!(help.status.success());
    let text = String::from_utf8(help.stdout).unwrap();
    let expected = whale::builtins::registry().assembler_ids().collect::<Vec<_>>().join(", ");
    assert!(text.contains(&format!("Selected assemblers: {expected}")));
    assert_eq!(text.contains("whale link "), cfg!(feature = "native-linker"));
    assert!(text.contains("bytecode verify|run"));
    if !cfg!(feature = "native-linker") {
        let result = std::process::Command::new(env!("CARGO_BIN_EXE_whale"))
            .args(["link", "missing.o", "-o", "keep"])
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&result.stderr).contains("not included"));
    }
}
