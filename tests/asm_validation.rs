#![cfg(feature = "asm-amd64")]

use std::{fs, path::PathBuf, process::Command};

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn rejected_assembly_never_publishes_or_overwrites_an_object() {
    let directory =
        std::env::temp_dir().join(format!("whale-invalid-assembly-{}", std::process::id()));
    fs::create_dir(&directory).expect("unique test directory");
    let scratch = Scratch(directory);
    let source = scratch.0.join("input.asm");
    let output = scratch.0.join("output.o");
    for (input, diagnostic) in [
        ("]\nnop", "Unexpected token"),
        ("push rax,", "operand after comma"),
        ("mov rax, rbx,\n", "operand after comma"),
        ("mov rax, rbx nop", "end of statement"),
        ("db 1,,2", "Expected expression"),
        ("db\n", "Expected directive value"),
        ("mov rax, [rbx", "closing ']'"),
        ("mov rax, [rbx - rcx]", "Negative register"),
        ("mov rax, [rbx + ]", "Expected term"),
        ("mov rax, [rbx + 4294967296]", "displacement"),
        ("nop 1", "expects 0 operands"),
    ] {
        fs::write(&source, input).unwrap();
        for prior_output in [false, true] {
            if prior_output {
                fs::write(&output, b"existing artifact").unwrap();
            }
            let result = Command::new(env!("CARGO_BIN_EXE_whale"))
                .args(["asm", "--amd64"])
                .arg(&source)
                .arg("-o")
                .arg(&output)
                .output()
                .unwrap();
            assert!(!result.status.success(), "accepted {input}");
            assert!(String::from_utf8_lossy(&result.stderr).contains(diagnostic), "{:?}", result);
            if prior_output {
                assert_eq!(fs::read(&output).unwrap(), b"existing artifact");
                fs::remove_file(&output).unwrap();
            } else {
                assert!(!output.exists(), "published output for {input}");
            }
        }
    }
}
