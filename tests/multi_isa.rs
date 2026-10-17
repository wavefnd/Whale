use assembler::{AssemblerOutput, AssemblyLimits};

use object::{Machine, ObjectTarget, RelocKind};

use std::{fs, process::Command};

fn assemble(arch: &str, source: &str) -> AssemblerOutput {
    whale::builtins::registry()
        .assemble(arch, "test.s", source, &AssemblyLimits::default())
        .unwrap_or_else(|e| panic!("{arch}: {source:?}: {e}"))
}

fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

fn section<'a>(bytes: &'a [u8], name: &str) -> (usize, &'a [u8]) {
    let table = u64_at(bytes, 40) as usize;
    let count = u16_at(bytes, 60) as usize;
    let shstr = table + u16_at(bytes, 62) as usize * 64;
    let names = &bytes[u64_at(bytes, shstr + 24) as usize..][..u64_at(bytes, shstr + 32) as usize];
    for index in 1..count {
        let header = table + index * 64;
        let offset = u32_at(bytes, header) as usize;
        let raw = &names[offset..];
        let end = raw.iter().position(|b| *b == 0).unwrap();
        if &raw[..end] == name.as_bytes() {
            if u32_at(bytes, header + 4) == 8 {
                return (header, &[]);
            }
            return (
                header,
                &bytes[u64_at(bytes, header + 24) as usize..]
                    [..u64_at(bytes, header + 32) as usize],
            );
        }
    }
    panic!("missing section {name}")
}

#[test]
fn forward_backward_labels_and_numeric_branch_limits_are_checked() {
    for (arch, branch) in [("aarch64", "b"), ("riscv64", "j"), ("loongarch64", "b")] {
        let out =
            assemble(arch, &format!(".text\n_42: {branch} end\nnop\nend: {branch} _42\nret\n"));
        let forward = assemble(arch, &format!("{branch} 8"));
        let backward = assemble(arch, &format!("{branch} -8"));
        assert_eq!(&out.sections[0].data[..4], forward.sections[0].data);
        assert_eq!(&out.sections[0].data[8..12], backward.sections[0].data);
        assert!(out.sections[0].relocs.is_empty());
        assert!(whale::builtins::registry()
            .assemble(arch, "bad.s", &format!("{branch} missing"), &AssemblyLimits::default())
            .is_err());
    }
}

#[test]
fn elf_identity_symbol_relocations_and_sparse_bss_are_preserved() {
    for (arch, machine, elf, call, code) in [
        ("aarch64", Machine::AArch64, 183, "bl", 283),
        ("riscv64", Machine::RiscV64, 243, "jal ra,", 17),
        ("loongarch64", Machine::LoongArch64, 258, "bl", 66),
    ] {
        let source=format!(".text\n.global entry\n.extern external\nentry: {call} external+4\nret\n.data\npayload: .quad entry+8\n.word external-4\n.bss\n.balign 4096\nstorage: .zero 1099511627776\n");
        let output = assemble(arch, &source);
        let target = ObjectTarget::elf64(machine);
        let object = assembler_object::to_object(&output, target).unwrap();
        assert_eq!(object.relocations[0].kind, RelocKind::TargetSpecific { machine, code });
        let bytes = object.write().unwrap();
        assert_eq!(&bytes[..7], b"\x7fELF\x02\x01\x01");
        assert_eq!(u16_at(&bytes, 18), elf);
        assert_eq!(u32_at(&bytes, 48), if machine == Machine::LoongArch64 { 0x41 } else { 0 });
        let (_, text_rela) = section(&bytes, ".rela.text");
        assert_eq!(text_rela.len(), 24);
        assert_eq!(u64_at(text_rela, 0), 0);
        assert_eq!(u64_at(text_rela, 8) as u32, code);
        assert_eq!(u64_at(text_rela, 16), 4);
        let (_, data_rela) = section(&bytes, ".rela.data");
        assert_eq!(data_rela.len(), 48);
        assert_eq!(u64_at(data_rela, 8) as u32, if machine == Machine::AArch64 { 257 } else { 2 });
        assert_eq!(u64_at(data_rela, 16), 8);
        assert_eq!(u64_at(data_rela, 32) as u32, if machine == Machine::AArch64 { 258 } else { 1 });
        assert_eq!(u64_at(data_rela, 40) as i64, -4);
        let (bss, _) = section(&bytes, ".bss");
        assert_eq!(u64_at(&bytes, bss + 32), 1 << 40);
        assert!(bytes.len() < 4096);
        let (_, symbols) = section(&bytes, ".symtab");
        let (_, names) = section(&bytes, ".strtab");
        let ext = symbols
            .chunks_exact(24)
            .find(|s| {
                let tail = &names[u32_at(s, 0) as usize..];
                tail.starts_with(b"external\0")
            })
            .unwrap();
        assert_eq!(ext[4] >> 4, 1);
        assert_eq!(u16_at(ext, 6), 0);
        let mut linker = linker::Linker::new(linker::core::linker::OutputFormat::ELF64Executable);
        linker.add_object(object);
        assert!(linker.link().unwrap_err().contains("unsupported object target"));
    }
}

#[test]
fn cross_section_branches_keep_target_specific_relocations() {
    for (arch, instruction) in [
        ("aarch64", "b.eq target+4"),
        ("riscv64", "beq a0,a1,target+4"),
        ("loongarch64", "beqz $a0,target+4"),
    ] {
        let out = assemble(arch, &format!("{instruction}\n.data\ntarget: .quad 0"));
        assert_eq!(out.sections[0].relocs.len(), 1);
        assert_eq!(out.sections[0].relocs[0].addend, 4);
    }
}

#[test]
fn invalid_operands_ranges_and_unimplemented_extensions_fail_explicitly() {
    let registry = whale::builtins::registry();
    let cases = [
        (
            "aarch64",
            vec![
                "mov x32,x0",
                "mov x0,w1",
                "add x0,xzr,#1",
                "add xzr,x0,#1",
                "adds sp,sp,#1",
                "add x0,sp,x1",
                "mov sp,xzr",
                "add x0,x1,#4096",
                "add x0,x1,x2,lsl #64",
                "movk w0,#1,lsl #32",
                "ldr x0,[x1,#1]",
                "ldr w0,[xzr]",
                "strb x0,[x1]",
                "ldr x0,[x1],#8",
                "ldur x0,[x1,#256]",
                "b 2",
                "b 134217728",
                "b.eq 1048576",
                "adr x0,1048576",
                "ret xzr",
                "fadd d0,d1,d2",
                "and x0,x1,#1",
            ],
        ),
        (
            "riscv64",
            vec![
                "add x32,x0,x1",
                "addi x0,x1,2048",
                "slli x0,x1,64",
                "slliw x0,x1,32",
                "ld x0,2048(x1)",
                "sd x0,-2049(x1)",
                "lui x0,1048576",
                "beq x0,x1,4096",
                "jal x0,1048576",
                "jal x0,3",
                "add a0,a1",
                "fence rr,w",
                "mul a0,a1,a2",
                "c.nop",
                "li a0,4096",
                "call external",
            ],
        ),
        (
            "loongarch64",
            vec![
                "add.d $r32,$r0,$r1",
                "addi.d $a0,$a1,2048",
                "andi $a0,$a1,-1",
                "slli.d $a0,$a1,64",
                "slli.w $a0,$a1,32",
                "ld.d $a0,$a1,-2049",
                "lu12i.w $a0,524288",
                "b 134217728",
                "bl 2",
                "beq $a0,$a1,131072",
                "beqz $a0,4194304",
                "jirl $ra,$a0,2",
                "syscall 32768",
                "fadd.d $f0,$f1,$f2",
            ],
        ),
    ];
    for (arch, inputs) in cases {
        for input in inputs {
            let err = registry
                .assemble(arch, "bad.s", input, &AssemblyLimits::default())
                .err()
                .unwrap_or_else(|| panic!("accepted {arch} {input}"));
            let message = err.to_string();
            assert!(message.contains("bad.s"), "{message}");
            assert!(message.contains("line 1"), "{message}");
        }
    }
}

#[test]
fn shared_directives_limits_diagnostics_and_symbol_validation() {
    let registry = whale::builtins::registry();
    for arch in ["aarch64", "riscv64", "loongarch64"] {
        let out=assemble(arch,".text\r\n.global start\r\nstart: nop // Unicode 주석\r\n.data\n.byte -1,255\n.hword 0x1234\n.word -1\n.quad 0xffffffffffffffff\n.bss\n.zero 8192\n");
        assert_eq!(out.sections[1].data, [vec![255, 255, 0x34, 0x12], vec![255; 12]].concat());
        assert_eq!(out.sections[2].zero_fill, 8192);
        for source in ["nop\nret", ".data\n.zero 9", ".data\n.byte 1\n.balign 16"] {
            assert!(registry
                .assemble(
                    arch,
                    "limit.s",
                    source,
                    &AssemblyLimits { max_output_bytes: 4, ..Default::default() }
                )
                .is_err());
        }
        assert!(registry
            .assemble(
                arch,
                "limit.s",
                "nop",
                &AssemblyLimits { max_output_bytes: 4, ..Default::default() }
            )
            .is_ok());
        assert!(registry
            .assemble(
                arch,
                "limit.s",
                "nop",
                &AssemblyLimits { max_relax_iterations: 0, ..Default::default() }
            )
            .is_err());
        for source in [
            ".data\n.byte 256",
            ".data\n.word missing",
            ".global missing",
            "x:nop\nx:ret",
            ".extern x\nx:nop",
            ".data\n.byte 1\n.text\n.byte 1\nnop",
            ".bss\n.word 0",
            ".balign 3",
            ".p2align 64",
            ".bad",
            ".global 1bad",
        ] {
            assert!(
                registry.assemble(arch, "bad.s", source, &AssemblyLimits::default()).is_err(),
                "{arch} {source}"
            );
        }
        let missing = registry
            .assemble(arch, "missing.s", "\n.global undefined", &AssemblyLimits::default())
            .err()
            .unwrap()
            .to_string();
        assert!(missing.contains("missing.s") && missing.contains("line 2"), "{missing}");
        let duplicate = registry
            .assemble(arch, "duplicate.s", "same: nop\nsame: ret", &AssemblyLimits::default())
            .err()
            .unwrap()
            .to_string();
        assert!(duplicate.contains("previous declaration"));
        assert!(duplicate.contains("line 2"));
    }
}

#[test]
fn mismatched_relocation_namespaces_and_symbol_sections_never_serialize() {
    let mut out = assemble("aarch64", ".extern x\nbl x");
    assert!(assembler_object::write(&out, ObjectTarget::elf64(Machine::RiscV64)).is_err());
    out.sections[0].relocs[0].kind =
        assembler::RelocKind::TargetSpecific { namespace: "unknown".into(), code: 283 };
    assert!(assembler_object::write(&out, ObjectTarget::elf64(Machine::AArch64)).is_err());
    out.sections[0].relocs.clear();
    out.symbols[0].section_index = Some(999);
    assert!(assembler_object::write(&out, ObjectTarget::elf64(Machine::AArch64)).is_err());
}

#[test]
fn cli_assembles_three_isas_and_keeps_existing_output_on_failure() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input.s");
    let output = directory.path().join("output.o");
    for (arch, elf) in [("aarch64", 183), ("arm64", 183), ("riscv64", 243), ("loongarch64", 258)] {
        fs::write(&input, ".global entry\nentry: nop\nret\n").unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_whale"))
            .args(["asm", &format!("--{arch}")])
            .arg(&input)
            .arg("-o")
            .arg(&output)
            .output()
            .unwrap();
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
        let previous = fs::read(&output).unwrap();
        assert_eq!(u16_at(&previous, 18), elf);
        fs::write(&input, "unknown_opcode\n").unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_whale"))
            .args(["asm", &format!("--{arch}")])
            .arg(&input)
            .arg("-o")
            .arg(&output)
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert_eq!(fs::read(&output).unwrap(), previous);
        assert!(String::from_utf8_lossy(&result.stderr).contains("unsupported"));
    }
}

#[test]
fn parser_mutations_and_public_field_guards_never_panic() {
    let registry = whale::builtins::registry();
    let seeds = [
        b".text\n.global entry\nentry: nop\nret\n".as_slice(),
        b".data\n.quad 0xffffffffffffffff\n",
        b".extern ext\nb ext\n",
        b"[[((\0#\xff;:\r\n",
    ];
    let mut state = 0x5748414c45u64;
    for _ in 0..1024 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let mut bytes = seeds[state as usize % seeds.len()].to_vec();
        if !bytes.is_empty() {
            let index = (state >> 8) as usize % bytes.len();
            bytes[index] = (state >> 24) as u8;
        }
        let text = String::from_utf8_lossy(&bytes);
        for arch in ["aarch64", "riscv64", "loongarch64"] {
            let _ = registry.assemble(
                arch,
                "mutation.s",
                &text,
                &AssemblyLimits { max_output_bytes: 4096, ..Default::default() },
            );
        }
    }
    for bits in [0, 33, u32::MAX] {
        assert!(assembler::fixed::signed("1", bits).is_err());
        assert!(assembler::fixed::unsigned("1", bits).is_err());
        assert!(assembler::fixed::displacement(0, bits, 4).is_err());
    }
    assert!(assembler::fixed::displacement(0, 26, 0).is_err());
    assert_eq!(assembler::fixed::signed("-1", 32).unwrap(), u32::MAX);
    assert_eq!(assembler::fixed::unsigned("0xffffffff", 32).unwrap(), u32::MAX);
    assert_eq!(
        assemble("aarch64", "add x0,x1,# 1").sections[0].data,
        assemble("aarch64", "add x0,x1,#1").sections[0].data
    );
}

#[test]
fn fixed_source_metadata_and_input_ceiling_precede_allocation() {
    let registry = whale::builtins::registry();
    let huge = " ".repeat(8 * 1024 * 1024 + 1);
    assert!(registry
        .assemble("aarch64", "large.s", &huge, &AssemblyLimits::default())
        .err()
        .unwrap()
        .to_string()
        .contains("input exceeds"));
    let sections: String = (0..4096).map(|i| format!(".section s{i}\n")).collect();
    assert!(registry
        .assemble("riscv64", "many.s", &sections, &AssemblyLimits::default())
        .err()
        .unwrap()
        .to_string()
        .contains("section count"));
}

#[test]
fn registry_driven_cli_selection_checks_identity_before_reading_input() {
    let registry = whale::builtins::registry();
    assert_eq!(
        registry.assembler_ids().collect::<Vec<_>>(),
        ["aarch64", "amd64", "loongarch64", "riscv64"]
    );
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("source.s");
    let output = directory.path().join("out.o");
    for arch in ["aarch64", "riscv64", "loongarch64"] {
        fs::write(&input, "nop\nret").unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_whale"))
            .args(["asm", "--arch", arch])
            .arg(&input)
            .arg("-o")
            .arg(&output)
            .output()
            .unwrap();
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    }
    fs::remove_file(&input).unwrap();
    fs::write(&output, b"previous").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_whale"))
        .args(["asm", "--arch", "unknown"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("unregistered assembler"));
    assert_eq!(fs::read(&output).unwrap(), b"previous");
    for args in [
        vec!["asm", "--arch"],
        vec!["asm", "--arch", "riscv64", "--amd64"],
        vec!["asm", "--aarch64", "--arch", "aarch64"],
    ] {
        assert!(!Command::new(env!("CARGO_BIN_EXE_whale")).args(args).status().unwrap().success());
    }
}
