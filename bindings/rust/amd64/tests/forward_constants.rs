use amd64::AMD64;

use assembler::assemble;

#[test]
fn forward_constants_match_backward_definitions_without_relocations() {
    for usage in ["mov eax, N", "dd N + 1", "mov eax, [rbx + N]", "resb N"] {
        let forward = assemble(&format!("{usage}\nN equ M + 1\nM equ 2"), &AMD64).unwrap();
        let backward = assemble(&format!("M equ 2\nN equ M + 1\n{usage}"), &AMD64).unwrap();
        assert_eq!(forward.sections[0].data, backward.sections[0].data, "{usage}");
        assert!(forward.sections[0].relocs.is_empty(), "{usage}");
        assert!(forward.symbols.is_empty());
    }
}

#[test]
fn local_constants_keep_their_defining_label_scope() {
    let output = assemble(
        "first:\nmov eax, .N\n.N equ .M + 1\n.M equ 2\nsecond:\nmov eax, .N\n.N equ 7",
        &AMD64,
    )
    .unwrap();
    assert_eq!(output.sections[0].data, [0xb8, 3, 0, 0, 0, 0xb8, 7, 0, 0, 0]);
    assert!(output.sections[0].relocs.is_empty());
}

#[test]
fn unresolved_cyclic_conflicting_and_overflowing_constants_are_errors() {
    for (source, diagnostic) in [
        ("a equ a", "cycle"),
        ("a equ b\nb equ a", "cycle"),
        ("a equ missing", "unresolved"),
        ("a equ 1\na equ 2", "Duplicate constant"),
        ("a equ 1\nextern a", "conflict"),
        ("extern a\na equ 1", "conflict"),
        ("a:\na equ 1", "conflict"),
        ("a equ 1\na:", "conflict"),
        ("a equ b + 1\nb equ 18446744073709551615", "overflow"),
    ] {
        let error = assemble(source, &AMD64).err().expect(source).to_string();
        assert!(error.contains(diagnostic), "{source}: {error}");
    }
}

#[test]
fn repeated_extern_declarations_have_one_symbol() {
    let output = assemble("extern f\nextern f\ncall f", &AMD64).unwrap();
    assert_eq!(output.symbols.len(), 1);
    assert_eq!(output.symbols[0].name, "f");
    assert_eq!(output.sections[0].relocs.len(), 1);
}
