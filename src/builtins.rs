//! Built-in composition belongs to the application, not the extension APIs.
pub fn registry() -> toolchain::Registry {
    let mut registry = toolchain::Registry::new();
    registry
        .register_target(ir::Target::X86_64WhaleLinux.specification())
        .expect("valid built-in target");
    #[cfg(feature = "asm-amd64")]
    registry.register_assembler(amd64::AMD64).expect("unique built-in assembler");
    #[cfg(feature = "asm-aarch64")]
    registry.register_assembler(aarch64::AArch64).expect("unique built-in assembler");
    #[cfg(feature = "asm-riscv64")]
    registry.register_assembler(riscv64::RiscV64).expect("unique built-in assembler");
    #[cfg(feature = "asm-loongarch64")]
    registry.register_assembler(loongarch64::LoongArch64).expect("unique built-in assembler");
    registry
}
