use std::{fs, process::Command};

fn whale() -> Command {
    Command::new(env!("CARGO_BIN_EXE_whale"))
}

fn object() -> Vec<u8> {
    use object::*;
    let mut o = ObjectFile::new(ObjectFormat::ELF64);
    o.add_section(".text", SectionKind::Text, 1);
    o.sections[0].data = vec![0xe8, 0, 0, 0, 0, 0xc3];
    o.symbols.push(ObjectSymbol {
        name: "external".into(),
        section_index: None,
        value: 0,
        size: 0,
        binding: SymbolBinding::Global,
        kind: SymbolKind::Function,
        visibility: SymbolVisibility::Default,
    });
    o.relocations.push(ObjectRelocation {
        section_index: 0,
        offset: 1,
        symbol: "external".into(),
        kind: RelocKind::PLT32,
        addend: -4,
    });
    o.write().unwrap()
}

#[test]
fn inspection_is_available_in_ordinary_builds_and_arguments_are_strict() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("sample.o");
    fs::write(&input, object()).unwrap();
    let result = whale().args(["reverse", "inspect"]).arg(&input).arg("--json").output().unwrap();
    assert!(result.status.success());
    let v: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(v["kind"], "elf64-amd64-relocatable");
    assert_eq!(v["relocations"][0]["symbol"], "external");
    for args in [
        vec!["disasm", "--raw", "missing"],
        vec!["reverse", "inspect", "--wat"],
        vec!["reverse", "missing"],
        vec!["disasm", "--amd64", "--amd64", "missing"],
        vec!["disasm", "--max-instructions", "0", "missing"],
    ] {
        assert!(!whale().args(args).output().unwrap().status.success());
    }
    for args in
        [vec!["disasm", "--help"], vec!["reverse", "--help"], vec!["reverse", "cfg", "--help"]]
    {
        assert!(whale().args(args).output().unwrap().status.success());
    }
}

#[test]
fn unavailable_disassembly_preserves_input_and_failed_output() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("sample.o");
    let bytes = object();
    fs::write(&input, &bytes).unwrap();
    let output = dir.path().join("listing");
    fs::write(&output, b"old").unwrap();
    let result = whale().arg("disasm").arg(&input).arg("--json").output().unwrap();
    assert!(!reverse::backend_available());
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("reverse decoder is unavailable"));
    assert!(!whale()
        .arg("disasm")
        .arg(&input)
        .arg("-o")
        .arg(&input)
        .output()
        .unwrap()
        .status
        .success());
    assert_eq!(fs::read(&input).unwrap(), bytes);
    assert!(!whale()
        .arg("disasm")
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .args(["--max-output", "1"])
        .output()
        .unwrap()
        .status
        .success());
    assert_eq!(fs::read(&output).unwrap(), b"old");
}

#[test]
fn ordinary_build_reports_cfg_and_xrefs_core_unavailable_explicitly() {
    assert!(!reverse::backend_available());
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("ret");
    fs::write(&input, [0xc3]).unwrap();
    for mode in ["cfg", "xrefs"] {
        let result = whale()
            .args(["reverse", mode, "--raw", "--amd64", "--base", "0"])
            .arg(&input)
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(
            String::from_utf8_lossy(&result.stderr)
                .contains("control-flow analysis is unavailable"),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(result.stdout.is_empty());
    }
}

#[test]
fn graph_budget_options_are_validated_without_an_analysis_backend() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("raw");
    fs::write(&input, [0x90, 0xc3]).unwrap();
    for mode in ["cfg", "xrefs"] {
        for (flag, value) in [
            ("--max-work", "0"),
            ("--max-work", "1073741825"),
            ("--max-workspace", "0"),
            ("--max-workspace", "536870913"),
        ] {
            let result = whale()
                .args(["reverse", mode, "--raw", "--amd64", "--base", "0"])
                .arg(&input)
                .args([flag, value])
                .output()
                .unwrap();
            assert!(!result.status.success());
            assert!(String::from_utf8_lossy(&result.stderr).contains(flag));
        }
    }
}

#[test]
fn static_inspection_preserves_boundary_markers_and_omits_tls_offsets() {
    let mut bytes = vec![0; 0x294];
    bytes[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    for (at, value) in [(16, 2u16), (18, 62), (52, 64), (54, 56), (56, 1), (58, 64), (60, 5)] {
        bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
    for (at, value) in [(20, 1u32), (64, 1), (68, 5)] {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    for (at, value) in [
        (24, 0x400200u64),
        (32, 64),
        (40, 128),
        (80, 0x400000),
        (96, 0x294),
        (104, 0x294),
        (112, 4096),
    ] {
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    for (index, kind, flags, address, offset, size, link, stride) in [
        (1, 1u32, 6u64, 0x400200u64, 0x200u64, 1u64, 0u32, 0u64),
        (2, 2, 0, 0, 0x210, 96, 3, 24),
        (3, 3, 0, 0, 0x280, 20, 0, 0),
        (4, 1, 0x403, 0x400201, 0x201, 16, 0, 0),
    ] {
        let at = 128 + index * 64;
        bytes[at + 4..at + 8].copy_from_slice(&kind.to_le_bytes());
        bytes[at + 40..at + 44].copy_from_slice(&link.to_le_bytes());
        for (field, value) in
            [(8, flags), (16, address), (24, offset), (32, size), (48, 1), (56, stride)]
        {
            bytes[at + field..at + field + 8].copy_from_slice(&value.to_le_bytes());
        }
    }
    bytes[128 + 2 * 64 + 44..128 + 2 * 64 + 48].copy_from_slice(&1u32.to_le_bytes());
    bytes[0x200] = 0xc3;
    bytes[0x280..].copy_from_slice(b"\0start\0boundary\0tls\0");
    for (index, name, kind, section, value, size) in [
        (1, 1u32, 0x12u8, 1u16, 0x400200u64, 1u64),
        (2, 7, 0x10, 1, 0x500000, 0),
        (3, 16, 0x16, 4, 8, 8),
    ] {
        let at = 0x210 + index * 24;
        bytes[at..at + 4].copy_from_slice(&name.to_le_bytes());
        bytes[at + 4] = kind;
        bytes[at + 6..at + 8].copy_from_slice(&section.to_le_bytes());
        bytes[at + 8..at + 16].copy_from_slice(&value.to_le_bytes());
        bytes[at + 16..at + 24].copy_from_slice(&size.to_le_bytes());
    }
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("metadata-exec");
    fs::write(&input, bytes).unwrap();
    let result = whale().args(["reverse", "inspect"]).arg(&input).arg("--json").output().unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let image: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    let symbols = image["symbols"].as_array().unwrap();
    assert_eq!(symbols.len(), 2);
    assert_eq!(symbols[0]["name"], "start");
    assert_eq!(symbols[0]["region"], 0);
    assert_eq!(symbols[1]["name"], "boundary");
    assert!(symbols[1]["region"].is_null());
    assert_eq!(symbols[1]["address"], 0x500000);
}
