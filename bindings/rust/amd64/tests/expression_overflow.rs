use amd64::AMD64;

use assembler::assemble;

#[test]
fn expression_overflow_is_an_error_in_every_resolution_path() {
    for source in [
        "N equ 18446744073709551615 + 1\ndq N",
        "dq -9223372036854775807 - 2",
        "extern target\ndq target + 18446744073709551615 + 1",
        "extern target\ndq 9223372036854775807 + target + 1",
        "N equ 18446744073709551615\nM equ N + 1\ndq M",
        "N equ 18446744073709551615\nmov rax, N + 1",
        "N equ 18446744073709551615\ndq N + 1",
        "N equ 18446744073709551615\nresb N + 1",
        "N equ 18446744073709551615\nmov rax, [N + 1]",
        "N equ -9223372036854775807\nmov [N - 2], rax",
        "extern target\nmov rax, [target - 9223372036854775807 - 1]",
    ] {
        let error = assemble(source, &AMD64).err().expect(source).to_string();
        assert!(error.contains("overflow"), "{source}: {error}");
    }
}

#[test]
fn signed_expression_boundaries_keep_their_exact_bytes() {
    let output = assemble("dq 9223372036854775806 + 1, -9223372036854775807 - 1", &AMD64).unwrap();
    let expected = [i64::MAX.to_le_bytes(), i64::MIN.to_le_bytes()].concat();
    assert_eq!(output.sections[0].data, expected);
    assert!(output.sections[0].relocs.is_empty());
}
