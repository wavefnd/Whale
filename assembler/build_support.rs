// SPDX-License-Identifier: MPL-2.0
// Internal Cargo artifact guard, shared only by the explicit assembler crates.
use std::{collections::BTreeMap, env, fs, io::Read, path::PathBuf};

const PIN: &str = "e0952ed65d07e14c6ed0aa64f3c9a5532dd7533b";

fn update(hash: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(hash, |h, byte| (h ^ u64::from(*byte)).wrapping_mul(0x100000001b3))
}

fn hash(bytes: &[u8]) -> String {
    format!("{:016x}", update(0xcbf29ce484222325, bytes))
}

fn hash_file(path: &std::path::Path, maximum: Option<u64>) -> String {
    let mut file = fs::File::open(path)
        .unwrap_or_else(|_| panic!("missing Wave assembler artifact {}", path.display()));
    let mut value = 0xcbf29ce484222325;
    let mut seen = 0;
    let mut buffer = [0; 16384];
    loop {
        let count = file.read(&mut buffer).expect("cannot read Wave assembler artifact");
        if count == 0 {
            return format!("{value:016x}");
        }
        seen += count as u64;
        if maximum.is_some_and(|limit| seen > limit) {
            panic!("oversized Wave assembler artifact {}", path.display());
        }
        value = update(value, &buffer[..count]);
    }
}

fn main() {
    println!("cargo:rustc-check-cfg=cfg({CFG})");
    println!("cargo:rerun-if-env-changed={ENV}");
    let Some(directory) = env::var_os(ENV) else { return };
    if CFG == "whale_wave_amd64" {
        assert!(
            env::var_os("WHALE_WAVE_ASSEMBLER_DIR").is_some(),
            "Wave AMD64 requires WHALE_WAVE_ASSEMBLER_DIR for shared assembler utilities"
        );
    }
    let directory = PathBuf::from(directory)
        .canonicalize()
        .expect("Wave assembler artifact directory must exist");
    let target = env::var("TARGET").expect("Cargo target required");
    let archive = directory.join(if target.contains("msvc") {
        format!("{LIBRARY}.lib")
    } else {
        format!("lib{LIBRARY}.a")
    });
    let metadata = directory.join(METADATA);
    let provenance = directory.join("provenance.json");
    for path in [&archive, &metadata, &provenance] {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    let mut text = String::new();
    fs::File::open(&metadata)
        .expect("missing assembler metadata; build with tools/build_wave_assembler.py")
        .take(4097)
        .read_to_string(&mut text)
        .expect("invalid Wave assembler metadata");
    assert!(text.len() <= 4096, "oversized Wave assembler metadata");
    let mut fields = BTreeMap::new();
    for line in text.lines() {
        let (key, value) = line.split_once('=').expect("malformed Wave assembler metadata");
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
            "invalid Wave assembler metadata fields"
        );
    }
    assert_eq!(fields.len(), 8, "incomplete Wave assembler metadata");
    assert_eq!(fields.get("schema"), Some(&"1"), "unsupported Wave assembler metadata");
    assert_eq!(fields.get("target"), Some(&target.as_str()), "Wave assembler target mismatch");
    assert_eq!(
        fields.get("wave_revision"),
        Some(&PIN),
        "Wave assembler compiler revision mismatch"
    );
    assert!(
        [Some(&"0"), Some(&"2")].contains(&fields.get("opt_level")),
        "unsupported Wave assembler optimization level"
    );
    let mut root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    for _ in 0..ROOT_UP {
        assert!(root.pop(), "invalid assembler source root");
    }
    let mut source = Vec::new();
    for (index, name) in SOURCES.iter().enumerate() {
        let path = root.join(name);
        println!("cargo:rerun-if-changed={}", path.display());
        if index != 0 {
            source.push(b'\n');
        }
        source.extend(fs::read(path).expect("missing Wave assembler source"));
    }
    assert_eq!(
        fields.get("source_fnv64"),
        Some(&hash(&source).as_str()),
        "stale Wave assembler archive"
    );
    let mut abi = Vec::new();
    for (index, name) in ABI_SOURCES.iter().enumerate() {
        let path = root.join(name);
        println!("cargo:rerun-if-changed={}", path.display());
        if index != 0 {
            abi.push(b'\n');
        }
        abi.extend(fs::read(path).expect("missing Wave assembler ABI source"));
    }
    assert_eq!(fields.get("abi_fnv64"), Some(&hash(&abi).as_str()), "stale Wave assembler ABI");
    assert_eq!(
        fields.get("archive_fnv64"),
        Some(&hash_file(&archive, None).as_str()),
        "corrupt Wave assembler archive"
    );
    assert_eq!(
        fields.get("provenance_fnv64"),
        Some(&hash_file(&provenance, Some(16 * 1024)).as_str()),
        "corrupt Wave assembler provenance"
    );
    println!("cargo:rustc-link-search=native={}", directory.display());
    println!("cargo:rustc-link-lib=static={LIBRARY}");
    println!("cargo:rustc-cfg={CFG}");
}
