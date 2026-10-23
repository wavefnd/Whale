// SPDX-License-Identifier: MPL-2.0
//! Independent public-contract cases for the complete Wave asmobj conversion.
use toolchain::assembly_object;

#[cfg(not(whale_asmobj))]
#[test]
fn missing_module_is_an_explicit_error() {
    let input = assembler::AssemblerOutput { sections: vec![], symbols: vec![] };
    assert!(!assembly_object::available());
    assert!(assembly_object::to_object(&input, object::Target::X86_64WhaleLinux.object_target())
        .err()
        .unwrap()
        .contains("unavailable"));
}

#[cfg(whale_asmobj)]
mod native {
    use super::*;

    use assembler::assembler::RelocKind as AsmReloc;

    use assembler::{AsmSection, AsmSectionKind, AsmSymbol, AssemblerOutput, Relocation};

    use object::{Machine, ObjectFormat, ObjectTarget, RelocKind, SectionKind, SymbolBinding};

    fn section(name: &str, kind: AsmSectionKind) -> AsmSection {
        AsmSection {
            name: name.into(),
            kind,
            align: 16,
            data: if matches!(kind, AsmSectionKind::Bss) { vec![] } else { vec![0; 256] },
            zero_fill: if matches!(kind, AsmSectionKind::Bss) { 1 << 40 } else { 0 },
            relocs: vec![],
        }
    }

    fn symbol(name: &str, section_index: Option<usize>) -> AsmSymbol {
        AsmSymbol {
            name: name.into(),
            section_index,
            offset: if section_index.is_some() { 4 } else { 0 },
            size: if section_index.is_some() { 8 } else { 0 },
            is_global: section_index.is_none(),
            kind: assembler::SymbolKind::Function,
            visibility: assembler::SymbolVisibility::Hidden,
        }
    }

    fn fixture(relocations: Vec<(&str, AsmReloc)>) -> AssemblerOutput {
        let mut section = section(".text", AsmSectionKind::Text);
        section.relocs = relocations
            .into_iter()
            .enumerate()
            .map(|(index, (symbol, kind))| Relocation {
                offset: index * 8,
                symbol: symbol.into(),
                kind,
                addend: -(index as i64),
            })
            .collect();
        AssemblerOutput {
            sections: vec![section],
            symbols: vec![symbol("defined", Some(0)), symbol("declared", None)],
        }
    }

    fn target(format: ObjectFormat, machine: Machine) -> ObjectTarget {
        ObjectTarget { format, machine, endian: object::Endian::Little, address_bits: 64 }
    }

    fn specific(namespace: &str, code: u32) -> AsmReloc {
        AsmReloc::TargetSpecific { namespace: namespace.into(), code }
    }

    #[test]
    fn all_generic_relocations_preserve_field_identity_and_order() {
        assert!(assembly_object::available());
        let input = fixture(vec![
            ("defined", AsmReloc::Absolute64),
            ("defined", AsmReloc::Absolute32),
            ("defined", AsmReloc::SignedAbsolute32),
            ("defined", AsmReloc::Relative32),
            ("defined", AsmReloc::Relative8),
            ("defined", AsmReloc::Branch32),
            ("declared", AsmReloc::Branch32),
        ]);
        let output =
            assembly_object::to_object(&input, ObjectTarget::elf64(Machine::X86_64)).unwrap();
        let expected = [
            RelocKind::Absolute64,
            RelocKind::Absolute32,
            RelocKind::SignedAbsolute32,
            RelocKind::Relative32,
            RelocKind::Relative8,
            RelocKind::Relative32,
            RelocKind::PLT32,
        ];
        for (index, (relocation, kind)) in output.relocations.iter().zip(expected).enumerate() {
            assert_eq!(relocation.kind, kind);
            assert_eq!(relocation.section_index, 0);
            assert_eq!(relocation.offset, index * 8);
            assert_eq!(relocation.addend, -(index as i64));
            assert_eq!(relocation.symbol, input.sections[0].relocs[index].symbol);
        }
        assert_eq!(output.relocations.len(), expected.len());
        assert_eq!(output.symbols.len(), 2);
    }

    #[test]
    fn generic_relocation_format_support_matches_independent_contract_table() {
        use RelocKind::{Absolute32, Absolute64, Relative32, Relative8, SignedAbsolute32, PLT32};
        let all = [
            Some(Absolute64),
            Some(Absolute32),
            Some(SignedAbsolute32),
            Some(Relative32),
            Some(PLT32),
            Some(Relative8),
        ];
        let absolute = [Some(Absolute64), Some(Absolute32), None, None, None, None];
        let coff_x64 =
            [Some(Absolute64), Some(Absolute32), None, Some(Relative32), Some(PLT32), None];
        let macho_x64 = [
            Some(Absolute64),
            None,
            None,
            Some(Relative32),
            Some(RelocKind::MachSpecific { machine: Machine::X86_64, code: 2 }),
            None,
        ];
        let macho_arm = [
            Some(Absolute64),
            None,
            None,
            None,
            Some(RelocKind::MachSpecific { machine: Machine::AArch64, code: 2 }),
            None,
        ];
        for (target, expected) in [
            (ObjectTarget::elf64(Machine::X86_64), all),
            (ObjectTarget::elf64(Machine::AArch64), absolute),
            (ObjectTarget::elf64(Machine::RiscV64), absolute),
            (ObjectTarget::elf64(Machine::LoongArch64), absolute),
            (ObjectTarget::coff(Machine::X86_64), coff_x64),
            (ObjectTarget::coff(Machine::AArch64), absolute),
            (ObjectTarget::macho64(Machine::X86_64), macho_x64),
            (ObjectTarget::macho64(Machine::AArch64), macho_arm),
        ] {
            for (kind, expected) in [
                AsmReloc::Absolute64,
                AsmReloc::Absolute32,
                AsmReloc::SignedAbsolute32,
                AsmReloc::Relative32,
                AsmReloc::Branch32,
                AsmReloc::Relative8,
            ]
            .into_iter()
            .zip(expected)
            {
                let input = fixture(vec![("declared", kind)]);
                let actual = assembly_object::to_object(&input, target);
                match expected {
                    Some(kind) => assert_eq!(actual.unwrap().relocations[0].kind, kind),
                    None => assert!(
                        actual.is_err(),
                        "unexpected generic relocation accepted for {target:?}"
                    ),
                }
            }
        }
    }

    #[test]
    fn all_isa_specific_elf_codes_keep_their_machine_namespace() {
        for (machine, namespace, codes) in [
            (Machine::AArch64, "aarch64", &[274, 280, 282, 283][..]),
            (Machine::RiscV64, "riscv64", &[16, 17][..]),
            (Machine::LoongArch64, "loongarch64", &[64, 65, 66][..]),
        ] {
            let input = fixture(
                codes.iter().map(|code| ("declared", specific(namespace, *code))).collect(),
            );
            let output = assembly_object::to_object(&input, ObjectTarget::elf64(machine)).unwrap();
            let actual =
                output.relocations.iter().map(|relocation| relocation.kind).collect::<Vec<_>>();
            let expected = codes
                .iter()
                .map(|code| RelocKind::TargetSpecific { machine, code: *code })
                .collect::<Vec<_>>();
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn arm64_coff_and_macho_relocations_use_format_specific_codes() {
        for (format, code, expected) in [
            (
                ObjectFormat::COFF,
                274,
                RelocKind::CoffSpecific { machine: Machine::AArch64, code: 5 },
            ),
            (
                ObjectFormat::COFF,
                280,
                RelocKind::CoffSpecific { machine: Machine::AArch64, code: 15 },
            ),
            (
                ObjectFormat::COFF,
                282,
                RelocKind::CoffSpecific { machine: Machine::AArch64, code: 3 },
            ),
            (
                ObjectFormat::COFF,
                283,
                RelocKind::CoffSpecific { machine: Machine::AArch64, code: 3 },
            ),
            (
                ObjectFormat::MachO64,
                282,
                RelocKind::MachSpecific { machine: Machine::AArch64, code: 2 },
            ),
            (
                ObjectFormat::MachO64,
                283,
                RelocKind::MachSpecific { machine: Machine::AArch64, code: 2 },
            ),
        ] {
            let input = fixture(vec![("declared", specific("aarch64", code))]);
            let output =
                assembly_object::to_object(&input, target(format, Machine::AArch64)).unwrap();
            assert_eq!(output.relocations[0].kind, expected);
        }
        for machine in [Machine::X86_64, Machine::AArch64] {
            let input =
                fixture(vec![("defined", AsmReloc::Branch32), ("missing", AsmReloc::Branch32)]);
            let output =
                assembly_object::to_object(&input, ObjectTarget::macho64(machine)).unwrap();
            assert!(output
                .relocations
                .iter()
                .all(|relocation| relocation.kind == RelocKind::MachSpecific { machine, code: 2 }));
        }
    }

    #[test]
    fn synthesized_symbols_deduplicate_by_utf8_bytes_and_keep_first_use_order() {
        let input = fixture(vec![
            ("외부_b", AsmReloc::Branch32),
            ("外部_a", AsmReloc::Branch32),
            ("외부_b", AsmReloc::Absolute64),
            ("declared", AsmReloc::Branch32),
        ]);
        let output =
            assembly_object::to_object(&input, ObjectTarget::elf64(Machine::X86_64)).unwrap();
        assert_eq!(
            output.symbols.iter().map(|symbol| symbol.name.as_str()).collect::<Vec<_>>(),
            ["defined", "declared", "외부_b", "外部_a"]
        );
        for symbol in &output.symbols[2..] {
            assert_eq!(symbol.section_index, None);
            assert_eq!(symbol.binding, SymbolBinding::Global);
            assert_eq!(symbol.kind, object::SymbolKind::NoType);
            assert_eq!(symbol.visibility, object::SymbolVisibility::Default);
            assert_eq!((symbol.value, symbol.size), (0, 0));
        }
        assert_eq!(output.symbols[0].kind, object::SymbolKind::Function);
        assert_eq!(output.symbols[0].binding, SymbolBinding::Local);
        assert_eq!(output.symbols[0].visibility, object::SymbolVisibility::Hidden);
        assert_eq!((output.symbols[0].value, output.symbols[0].size), (4, 8));
        assert_eq!(input.symbols.len(), 2);
        assert_eq!(input.sections[0].relocs.len(), 4);
    }

    #[test]
    fn section_roles_and_sparse_bss_remain_exact() {
        let input = AssemblerOutput {
            sections: vec![
                section(".text", AsmSectionKind::Text),
                section(".data", AsmSectionKind::Data),
                section(".rodata", AsmSectionKind::ReadOnlyData),
                section(".bss", AsmSectionKind::Bss),
            ],
            symbols: vec![],
        };
        let output =
            assembly_object::to_object(&input, ObjectTarget::elf64(Machine::X86_64)).unwrap();
        assert_eq!(
            output.sections.iter().map(|section| section.kind).collect::<Vec<_>>(),
            [SectionKind::Text, SectionKind::Data, SectionKind::ReadOnlyData, SectionKind::Bss]
        );
        for (actual, original) in output.sections.iter().zip(&input.sections) {
            assert_eq!(actual.name, original.name);
            assert_eq!(actual.align, original.align);
            assert_eq!(actual.data, original.data);
            assert_eq!(actual.zero_fill, original.zero_fill);
        }
        let bytes = output.write().unwrap();
        assert!(bytes.len() < 4096, "sparse BSS was materialized");
        assert_eq!(input.sections[3].zero_fill, 1 << 40);
    }

    #[test]
    fn target_namespace_and_symbol_errors_are_not_silently_reinterpreted() {
        for (format, machine, namespace, code, message) in [
            (
                ObjectFormat::ELF64,
                Machine::AArch64,
                "unknown",
                282,
                "unsupported relocation namespace",
            ),
            (ObjectFormat::ELF64, Machine::AArch64, "riscv64", 17, "namespace does not match"),
            (ObjectFormat::COFF, Machine::AArch64, "aarch64", 999, "unsupported AArch64 COFF"),
            (
                ObjectFormat::MachO64,
                Machine::AArch64,
                "aarch64",
                274,
                "does not support this format",
            ),
            (ObjectFormat::COFF, Machine::RiscV64, "riscv64", 17, "unsupported object target"),
        ] {
            let input = fixture(vec![("declared", specific(namespace, code))]);
            let error = assembly_object::to_object(&input, target(format, machine)).err().unwrap();
            assert!(error.contains(message), "{error}");
        }
        let mut duplicate = fixture(vec![]);
        duplicate.symbols.push(symbol("defined", None));
        assert!(assembly_object::to_object(&duplicate, ObjectTarget::elf64(Machine::X86_64))
            .err()
            .unwrap()
            .contains("duplicate symbol"));
        let mut invalid = fixture(vec![]);
        invalid.symbols[0].section_index = Some(2);
        assert!(assembly_object::to_object(&invalid, ObjectTarget::elf64(Machine::X86_64))
            .err()
            .unwrap()
            .contains("symbol section index"));
    }
}
