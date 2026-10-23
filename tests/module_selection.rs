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
}
