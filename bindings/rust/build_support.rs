// SPDX-License-Identifier: MPL-2.0
// Cargo links previously verified Vex results; it never prepares a compiler.
use std::{
    collections::BTreeMap,
    env, fs,
    io::Read,
    path::{Path, PathBuf},
};

fn update(value: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(value, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3))
}

fn file_hash(path: &Path, maximum: Option<u64>) -> String {
    let mut file = fs::File::open(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    assert!(file.metadata().unwrap().is_file(), "module artifact must be a regular file");
    let mut hash = 0xcbf29ce484222325;
    let mut buffer = [0; 16384];
    let mut seen = 0u64;
    loop {
        let count = file.read(&mut buffer).expect("cannot read module artifact");
        if count == 0 {
            return format!("{hash:016x}");
        }
        seen += count as u64;
        assert!(maximum.is_none_or(|limit| seen <= limit), "oversized module artifact");
        hash = update(hash, &buffer[..count]);
    }
}

fn collect(directory: &Path, extension: &str, files: &mut Vec<PathBuf>) {
    println!("cargo:rerun-if-changed={}", directory.display());
    for entry in fs::read_dir(directory).expect("missing module source directory") {
        let path = entry.expect("cannot inspect module source").path();
        let kind = fs::symlink_metadata(&path).expect("cannot inspect module source").file_type();
        assert!(!kind.is_symlink(), "module sources must not contain links");
        if kind.is_dir() {
            collect(&path, extension, files);
        } else if path.extension().is_some_and(|value| value == extension) {
            files.push(path);
        }
    }
}

fn source_hash(root: &Path, mut paths: Vec<PathBuf>) -> String {
    paths.sort();
    paths.dedup();
    let mut hash = 0xcbf29ce484222325;
    for path in paths {
        println!("cargo:rerun-if-changed={}", path.display());
        let name = path
            .strip_prefix(root)
            .unwrap()
            .to_str()
            .expect("module paths require UTF-8")
            .replace('\\', "/");
        hash = update(hash, name.as_bytes());
        hash = update(hash, &[0]);
        hash = update(
            hash,
            &fs::read(&path).unwrap_or_else(|_| panic!("missing module input {}", path.display())),
        );
        hash = update(hash, &[0]);
    }
    format!("{hash:016x}")
}

fn main() {
    println!("cargo:rustc-check-cfg=cfg({CFG})");
    println!("cargo:rerun-if-env-changed={ENV}");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_RUST_REFERENCE");
    let Some(directory) = env::var_os(ENV) else {
        assert!(MODULE == "asmobj" || env::var_os("CARGO_FEATURE_RUST_REFERENCE").is_some(),
            "{MODULE} requires its Wave module; build with Meson/Vex or explicitly select rust-reference");
        return;
    };
    let directory =
        PathBuf::from(directory).canonicalize().expect("Wave artifact directory must exist");
    if matches!(MODULE, "amd64" | "aarch64" | "riscv64" | "loongarch64") {
        assert!(
            env::var_os("WHALE_WAVE_ASSEMBLER_DIR").is_some(),
            "Wave ISA modules require WHALE_WAVE_ASSEMBLER_DIR for shared utilities"
        );
    }
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest.join("../../..").canonicalize().unwrap();
    let target = env::var("TARGET").unwrap();
    let archive = directory.join(if target.contains("msvc") {
        format!("{LIBRARY}.lib")
    } else {
        format!("lib{LIBRARY}.a")
    });
    let metadata = directory.join("module.meta");
    let provenance = directory.join("provenance.json");
    for path in [&archive, &metadata, &provenance] {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    let mut text = String::new();
    fs::File::open(&metadata)
        .expect("missing Vex module metadata; build with Meson")
        .take(4097)
        .read_to_string(&mut text)
        .expect("invalid Vex module metadata");
    assert!(text.len() <= 4096, "oversized Vex module metadata");
    let mut fields = BTreeMap::new();
    for line in text.lines() {
        let (key, value) = line.split_once('=').expect("malformed Vex module metadata");
        assert!(
            [
                "schema",
                "target",
                "wave_revision",
                "opt_level",
                "source_fnv64",
                "abi_fnv64",
                "archive_fnv64",
                "provenance_fnv64"
            ]
            .contains(&key)
                && fields.insert(key, value).is_none(),
            "invalid Vex module metadata fields"
        );
    }
    assert_eq!(fields.len(), 8, "incomplete Vex module metadata");
    assert_eq!(fields.get("schema"), Some(&"3"), "unsupported Vex module metadata");
    assert_eq!(
        fields.get("target"),
        Some(&target.as_str()),
        "Wave module executable-host target mismatch"
    );
    let revision = fields.get("wave_revision").expect("missing compiler revision");
    assert!(
        revision.len() == 40
            && revision.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "invalid Wave nightly revision"
    );
    assert!(
        [Some(&"0"), Some(&"2")].contains(&fields.get("opt_level")),
        "unsupported Wave optimization level"
    );
    let mut sources = vec![root.join("vex.ws"), root.join("vex.lock")];
    for directory in SOURCE_ROOTS {
        collect(&root.join(directory).join("src"), "wave", &mut sources);
        sources.push(root.join(directory).join("vex.ws"));
    }
    assert_eq!(
        fields.get("source_fnv64"),
        Some(&source_hash(&root, sources).as_str()),
        "stale Wave source or Vex graph"
    );
    let mut abi = vec![manifest.join("Cargo.toml")];
    collect(&manifest.join("src"), "rs", &mut abi);
    assert_eq!(
        fields.get("abi_fnv64"),
        Some(&source_hash(&root, abi).as_str()),
        "stale Rust/Wave ABI"
    );
    assert_eq!(
        fields.get("archive_fnv64"),
        Some(&file_hash(&archive, None).as_str()),
        "corrupt Wave archive"
    );
    assert_eq!(
        fields.get("provenance_fnv64"),
        Some(&file_hash(&provenance, Some(16384)).as_str()),
        "corrupt Wave provenance"
    );
    println!("cargo:rustc-link-search=native={}", directory.display());
    println!("cargo:rustc-link-lib=static={LIBRARY}");
    println!("cargo:rustc-cfg={CFG}");
}
