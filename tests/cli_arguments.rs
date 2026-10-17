use std::{fs, process::Command};

#[test]
fn invalid_artifact_arguments_fail_without_publishing_output() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("input");
    let output = directory.path().join("output.o");
    let another_output = directory.path().join("another.o");
    fs::write(&input, "nop\n").unwrap();
    for command in ["asm", "object"] {
        for (extra, diagnostic) in [
            (vec!["--unknown"], "unknown option"),
            (vec!["extra-input"], "unexpected argument"),
            (vec!["-o", another_output.to_str().unwrap()], "duplicate -o"),
            (vec!["-o"], "duplicate -o"),
        ] {
            for existing in [false, true] {
                if existing {
                    fs::write(&output, b"previous artifact").unwrap();
                }
                let mut invocation = Command::new(env!("CARGO_BIN_EXE_whale"));
                invocation.arg(command);
                if command == "asm" {
                    invocation.arg("--amd64");
                }
                let result =
                    invocation.arg(&input).arg("-o").arg(&output).args(&extra).output().unwrap();
                assert!(!result.status.success(), "accepted {command} {extra:?}");
                assert!(String::from_utf8_lossy(&result.stderr).contains(diagnostic), "{result:?}");
                if existing {
                    assert_eq!(fs::read(&output).unwrap(), b"previous artifact");
                    fs::remove_file(&output).unwrap();
                } else {
                    assert!(!output.exists());
                }
            }
        }
        for extra in [vec!["-o"], vec!["-o", "--trace"]] {
            let mut invocation = Command::new(env!("CARGO_BIN_EXE_whale"));
            invocation.arg(command);
            if command == "asm" {
                invocation.arg("--amd64");
            }
            let result = invocation.arg(&input).args(extra).output().unwrap();
            assert!(!result.status.success());
            assert!(String::from_utf8_lossy(&result.stderr).contains("-o requires a path"));
        }
        assert_eq!(fs::read(&input).unwrap(), b"nop\n");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}

#[test]
fn repeated_or_conflicting_architecture_options_fail_before_reading_input() {
    for architectures in
        [["--amd64", "--amd64"], ["--amd64", "--aarch64"], ["--aarch64", "--amd64"]]
    {
        let result = Command::new(env!("CARGO_BIN_EXE_whale"))
            .arg("asm")
            .args(architectures)
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("architecture"));
    }
}

#[test]
fn help_describes_available_commands_and_object_options() {
    for args in [vec![], vec!["--help"], vec!["-h"]] {
        let result = Command::new(env!("CARGO_BIN_EXE_whale")).args(args).output().unwrap();
        assert!(result.status.success());
        let text = String::from_utf8(result.stdout).unwrap();
        assert!(text.contains("whale asm --amd64"));
        assert!(text.contains("raw bytes"));
        assert!(text.contains("Static Linux AMD64"));
        assert!(text.contains("run"));
        for flag in ["--aarch64", "--riscv64", "--loongarch64"] {
            assert!(text.contains(flag));
        }
    }
    for args in [vec!["object"], vec!["object", "--help"]] {
        let result = Command::new(env!("CARGO_BIN_EXE_whale")).args(args).output().unwrap();
        assert!(result.status.success());
        let text = String::from_utf8(result.stdout).unwrap();
        assert!(text.contains("whale object <input> -o <output>"));
        assert!(text.contains("--dump-json"));
        assert!(text.contains("--debug-whale"));
    }
}

#[test]
fn object_debug_formats_dump_the_published_elf_with_a_bounded_head() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("raw.bin");
    let output = directory.path().join("raw.o");
    fs::write(&input, [0xc3]).unwrap();
    for (option, expected) in [
        ("--bytes", "0000: 127"),
        ("--dump-hex", "0000: 7F 45 4C 46"),
        ("--dump-bin", "0000: 01111111"),
        ("--dump-json", "\"head\": [127, 69, 76, 70"),
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_whale"))
            .arg("object")
            .arg(&input)
            .arg("-o")
            .arg(&output)
            .args(["--debug-whale", option])
            .output()
            .unwrap();
        assert!(result.status.success());
        let text = String::from_utf8(result.stdout).unwrap();
        assert!(text.contains(expected), "{text}");
        assert_eq!(&fs::read(&output).unwrap()[..4], b"\x7fELF");
        assert!(!text.contains("0100:"), "dump exceeded 256 bytes");
        if option == "--dump-json" {
            let head = text.split("\"head\": [").nth(1).unwrap().split(']').next().unwrap();
            let dumped: Vec<u8> = head.split(',').map(|s| s.trim().parse().unwrap()).collect();
            assert_eq!(dumped, fs::read(&output).unwrap()[..256]);
            assert!(text.contains(&format!("\"len\": {}", fs::metadata(&output).unwrap().len())));
        }
    }
    let result = Command::new(env!("CARGO_BIN_EXE_whale"))
        .arg("object")
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .arg("--dump-hex")
        .output()
        .unwrap();
    assert!(result.status.success());
    assert!(!String::from_utf8_lossy(&result.stdout).contains("== HEX"));
}
