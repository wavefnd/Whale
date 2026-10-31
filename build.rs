// SPDX-License-Identifier: MPL-2.0
//! Embed portable provenance. Running Whale never needs Git, Cargo or Wave.
use std::{env, fs, io::Read, path::Path, process::Command};

use serde_json::{json, Map, Value};

fn git(root: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(arguments)
        .current_dir(root)
        .output()
        .ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn provenance(directory: &Path) -> Value {
    let path = directory.join("provenance.json");
    println!("cargo:rerun-if-changed={}", path.display());
    let mut bytes = Vec::new();
    fs::File::open(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
        .take(16385)
        .read_to_end(&mut bytes)
        .expect("cannot read module provenance");
    assert!(bytes.len() <= 16384, "oversized module provenance");
    let original: Value = serde_json::from_slice(&bytes).expect("invalid module provenance");
    let mut portable = Map::new();
    // Exclude the bootstrap cache path: it is not part of the built product.
    for key in [
        "wave_revision",
        "target",
        "opt_level",
        "source_sha256",
        "abi_sha256",
        "archive_sha256",
        "compiler_sha256",
        "std_sha256",
        "bootstrap_rust_version",
        "required_llvm_major",
    ] {
        let value = original.get(key).unwrap_or_else(|| panic!("missing provenance {key}"));
        portable.insert(key.into(), value.clone());
    }
    Value::Object(portable)
}

fn main() {
    let root = std::path::PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    println!("cargo:rerun-if-env-changed=WHALE_SOURCE_REVISION");
    println!("cargo:rerun-if-env-changed=RUSTC");
    // Follow the actual worktree's refs and index, including linked worktrees.
    let branch = git(&root, &["symbolic-ref", "-q", "HEAD"]);
    for name in
        [Some("HEAD"), Some("index"), Some("packed-refs"), branch.as_deref()].into_iter().flatten()
    {
        if let Some(path) = git(&root, &["rev-parse", "--git-path", name]) {
            let path = root.join(path);
            if path.exists() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
    if let Some(files) = git(&root, &["ls-files", "-z"]) {
        for path in files.split('\0').filter(|path| !path.is_empty()) {
            println!("cargo:rerun-if-changed={}", root.join(path).display());
        }
    }
    let supplied = env::var("WHALE_SOURCE_REVISION").ok();
    if let Some(revision) = &supplied {
        assert!(
            revision.len() == 40
                && revision
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
            "WHALE_SOURCE_REVISION requires a full lowercase Git commit SHA"
        );
    }
    let commit =
        supplied.or_else(|| git(&root, &["rev-parse", "HEAD"])).unwrap_or_else(|| "unknown".into());
    let dirty = git(&root, &["status", "--porcelain", "--untracked-files=normal"])
        .is_some_and(|status| !status.is_empty());
    let compiler = Command::new(env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
        .arg("-Vv")
        .output()
        .expect("cannot query the Rust compiler");
    assert!(compiler.status.success(), "cannot query the Rust compiler");
    let mut modules = Map::new();
    for (id, variable, feature) in [
        ("ir", "WHALE_WAVE_IR_DIR", None),
        ("assembler", "WHALE_WAVE_ASSEMBLER_DIR", None),
        ("asmobj", "WHALE_ASMOBJ_DIR", None),
        ("amd64", "WHALE_WAVE_AMD64_DIR", Some("CARGO_FEATURE_ASM_AMD64")),
        ("aarch64", "WHALE_WAVE_AARCH64_DIR", Some("CARGO_FEATURE_ASM_AARCH64")),
        ("riscv64", "WHALE_WAVE_RISCV64_DIR", Some("CARGO_FEATURE_ASM_RISCV64")),
        ("loongarch64", "WHALE_WAVE_LOONGARCH64_DIR", Some("CARGO_FEATURE_ASM_LOONGARCH64")),
    ] {
        println!("cargo:rerun-if-env-changed={variable}");
        if feature.is_some_and(|feature| env::var_os(feature).is_none()) {
            continue;
        }
        if let Some(directory) = env::var_os(variable) {
            modules.insert(id.into(), provenance(Path::new(&directory)));
        }
    }
    let record = json!({
        "schema_version": 1,
        "source_commit": commit,
        "source_dirty": dirty,
        "host_target": env::var("TARGET").unwrap(),
        "rustc": String::from_utf8(compiler.stdout).expect("invalid rustc version").trim(),
        "reference_feature": env::var_os("CARGO_FEATURE_RUST_REFERENCE").is_some(),
        "wave_modules": modules,
    });
    let path = std::path::PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("build-info.json");
    fs::write(path, serde_json::to_vec(&record).unwrap()).expect("cannot publish build provenance");
}
