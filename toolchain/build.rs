// SPDX-License-Identifier: MPL-2.0
use std::{env, fs, io::Read, path::PathBuf};

fn update(hash: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(hash, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x100000001b3))
}

fn hash(bytes: &[u8]) -> String {
    format!("{:016x}", update(0xcbf29ce484222325, bytes))
}

fn archive_hash(path: &std::path::Path) -> String {
    let mut file = fs::File::open(path).expect("missing asmobj archive");
    let mut hash = 0xcbf29ce484222325;
    let mut buffer = [0; 16384];
    loop {
        let count = file.read(&mut buffer).expect("cannot read asmobj archive");
        if count == 0 {
            return format!("{hash:016x}");
        }
        hash = update(hash, &buffer[..count]);
    }
}

fn main() {
    println!("cargo:rustc-check-cfg=cfg(whale_asmobj)");
    println!("cargo:rerun-if-env-changed=WHALE_ASMOBJ_DIR");
    let Some(directory) = env::var_os("WHALE_ASMOBJ_DIR") else { return };
    let directory = PathBuf::from(directory).canonicalize().expect("WHALE_ASMOBJ_DIR must exist");
    let target = env::var("TARGET").unwrap();
    let archive = directory.join(if target.contains("msvc") {
        "whale_asmobj.lib"
    } else {
        "libwhale_asmobj.a"
    });
    let metadata = directory.join("asmobj.meta");
    println!("cargo:rerun-if-changed={}", archive.display());
    println!("cargo:rerun-if-changed={}", metadata.display());
    let mut text = String::new();
    fs::File::open(&metadata)
        .expect("missing asmobj.meta; build with tools/build_asmobj.py")
        .take(4097)
        .read_to_string(&mut text)
        .expect("invalid asmobj metadata");
    assert!(text.len() <= 4096, "oversized asmobj metadata");
    let mut fields = std::collections::BTreeMap::new();
    for line in text.lines() {
        let (key, value) = line.split_once('=').expect("malformed asmobj metadata");
        assert!(
            ["schema", "target", "source_fnv64", "archive_fnv64"].contains(&key)
                && fields.insert(key, value).is_none(),
            "invalid asmobj metadata fields"
        );
    }
    assert_eq!(fields.len(), 4, "incomplete asmobj metadata");
    assert_eq!(fields.get("schema"), Some(&"1"), "unsupported asmobj metadata");
    assert_eq!(fields.get("target"), Some(&target.as_str()), "asmobj archive target mismatch");
    let root =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../assembler/asmobj");
    let mut source = Vec::new();
    for (index, name) in
        ["model.wave", "sections.wave", "symbols.wave", "relocations.wave", "entry.wave"]
            .iter()
            .enumerate()
    {
        let path = root.join(name);
        println!("cargo:rerun-if-changed={}", path.display());
        if index != 0 {
            source.push(b'\n');
        }
        source.extend(fs::read(path).expect("missing asmobj Wave source"));
    }
    assert_eq!(
        fields.get("source_fnv64"),
        Some(&hash(&source).as_str()),
        "stale asmobj archive; rebuild with tools/build_asmobj.py"
    );
    assert_eq!(
        fields.get("archive_fnv64"),
        Some(&archive_hash(&archive).as_str()),
        "corrupt asmobj archive"
    );
    println!("cargo:rustc-link-search=native={}", directory.display());
    println!("cargo:rustc-link-lib=static=whale_asmobj");
    println!("cargo:rustc-cfg=whale_asmobj");
}
