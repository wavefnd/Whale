// SPDX-License-Identifier: MPL-2.0
use std::{collections::BTreeMap, env, fs, io::Read, path::PathBuf};

const PIN: &str = "e0952ed65d07e14c6ed0aa64f3c9a5532dd7533b";

const SOURCES: &[&str] = &[
    "model.wave",
    "layout.wave",
    "lexer.wave",
    "header.wave",
    "type_parser.wave",
    "literal.wave",
    "expression_parser.wave",
    "instruction_parser.wave",
    "module_parser.wave",
    "lower_hir.wave",
    "lower_model.wave",
    "lower_output.wave",
    "lower_expression.wave",
    "lower_storage.wave",
    "lower_statement.wave",
    "lower_driver.wave",
    "builder.wave",
    "builder_namespace.wave",
    "builder_body.wave",
    "verify.wave",
    "verifier_index.wave",
    "verifier_ssa.wave",
    "printer.wave",
    "cfg.wave",
    "shape.wave",
    "source.wave",
    "memory.wave",
    "lower_limits.wave",
    "wide.wave",
    "integer.wave",
    "float_model.wave",
    "float_arithmetic.wave",
    "float_convert.wave",
    "effects.wave",
    "passes.wave",
    "fold_integer.wave",
    "constant.wave",
    "constant_dependencies.wave",
    "runtime_model.wave",
    "runtime_prepare.wave",
    "runtime_execute.wave",
    "runtime_driver.wave",
    "bytecode_model.wave",
    "bytecode_inventory.wave",
    "bytecode_loader.wave",
    "bytecode_verify_types.wave",
    "bytecode_verify_instructions.wave",
    "bytecode_verify_flow.wave",
    "bytecode_verify.wave",
    "bytecode_writer.wave",
    "bytecode_runtime.wave",
    "entry.wave",
];

const ABI_SOURCES: &[&str] = &[
    "src/wave_layout.rs",
    "src/interpreter/integer.rs",
    "src/float/wave.rs",
    "src/effects_wave.rs",
    "src/pass_wave.rs",
    "src/fold_integer_wave.rs",
    "src/constant_wave.rs",
    "src/interpreter/runtime_wave.rs",
    "src/bytecode.rs",
    "src/verifier/wave_constants.rs",
    "src/source_wave.rs",
    "src/interpreter/memory_wave.rs",
    "src/lower_ast/wave_limits.rs",
    "src/parser/wave_lexer.rs",
    "src/parser/wave_header.rs",
    "src/parser/wave_text.rs",
    "src/parser/wave_type.rs",
    "src/parser/wave_literal.rs",
    "src/parser/wave_expression.rs",
    "src/parser/wave_instruction.rs",
    "src/parser/wave_module.rs",
    "src/lower_ast/wave_hir.rs",
    "src/lower_ast/wave_lower.rs",
    "src/builder/wave.rs",
    "src/builder/namespace.rs",
    "src/builder/body.rs",
    "src/verifier/wave_semantics.rs",
    "src/verifier/wave_index.rs",
    "src/verifier/wave_ssa.rs",
    "src/printer_wave.rs",
    "src/verifier/wave_cfg.rs",
    "src/verifier/wave_shape.rs",
    "src/lib.rs",
    "src/function.rs",
    "src/global_storage.rs",
];

fn update(hash: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(hash, |h, byte| (h ^ u64::from(*byte)).wrapping_mul(0x100000001b3))
}

fn hash(bytes: &[u8]) -> String {
    format!("{:016x}", update(0xcbf29ce484222325, bytes))
}

fn hash_file(path: &std::path::Path, maximum: Option<u64>) -> String {
    let mut file = fs::File::open(path)
        .unwrap_or_else(|_| panic!("missing Wave IR artifact {}", path.display()));
    let mut value = 0xcbf29ce484222325;
    let mut seen = 0;
    let mut buffer = [0; 16384];
    loop {
        let count = file.read(&mut buffer).expect("cannot read Wave IR artifact");
        if count == 0 {
            return format!("{value:016x}");
        }
        seen += count as u64;
        if maximum.is_some_and(|limit| seen > limit) {
            panic!("oversized Wave IR artifact {}", path.display());
        }
        value = update(value, &buffer[..count]);
    }
}

fn main() {
    println!("cargo:rustc-check-cfg=cfg(whale_wave_ir)");
    println!("cargo:rerun-if-env-changed=WHALE_WAVE_IR_DIR");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_RUST_REFERENCE");
    let Some(directory) = env::var_os("WHALE_WAVE_IR_DIR") else {
        assert!(env::var_os("CARGO_FEATURE_RUST_REFERENCE").is_some(),
            "IR requires its Wave module; build with Meson or select rust-reference explicitly for comparison");
        return;
    };
    let directory = PathBuf::from(directory).canonicalize().expect("WHALE_WAVE_IR_DIR must exist");
    let target = env::var("TARGET").expect("Cargo target required");
    let archive = directory.join(if target.contains("msvc") {
        "whale_wave_ir.lib"
    } else {
        "libwhale_wave_ir.a"
    });
    let metadata = directory.join("wave-ir.meta");
    let provenance = directory.join("provenance.json");
    for path in [&archive, &metadata, &provenance] {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    let mut text = String::new();
    fs::File::open(&metadata)
        .expect("missing wave-ir.meta; build with tools/build_wave_ir.py")
        .take(4097)
        .read_to_string(&mut text)
        .expect("invalid Wave IR metadata");
    assert!(text.len() <= 4096, "oversized Wave IR metadata");
    let mut fields = BTreeMap::new();
    for line in text.lines() {
        let (key, value) = line.split_once('=').expect("malformed Wave IR metadata");
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
            "invalid Wave IR metadata fields"
        );
    }
    assert_eq!(fields.len(), 8, "incomplete Wave IR metadata");
    assert_eq!(fields.get("schema"), Some(&"1"), "unsupported Wave IR metadata");
    assert_eq!(fields.get("target"), Some(&target.as_str()), "Wave IR target mismatch");
    assert_eq!(fields.get("wave_revision"), Some(&PIN), "Wave IR compiler revision mismatch");
    assert!(
        [Some(&"0"), Some(&"2")].contains(&fields.get("opt_level")),
        "unsupported Wave IR optimization level"
    );
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let mut source = Vec::new();
    for (index, name) in SOURCES.iter().enumerate() {
        let path = root.join(name);
        println!("cargo:rerun-if-changed={}", path.display());
        if index != 0 {
            source.push(b'\n');
        }
        source.extend(fs::read(path).expect("missing Wave IR source"));
    }
    assert_eq!(fields.get("source_fnv64"), Some(&hash(&source).as_str()), "stale Wave IR archive");
    let mut abi = Vec::new();
    for (index, name) in ABI_SOURCES.iter().enumerate() {
        let path = root.join(name);
        println!("cargo:rerun-if-changed={}", path.display());
        if index != 0 {
            abi.push(b'\n');
        }
        abi.extend(fs::read(path).expect("missing Wave IR ABI source"));
    }
    assert_eq!(fields.get("abi_fnv64"), Some(&hash(&abi).as_str()), "stale Wave IR ABI");
    assert_eq!(
        fields.get("archive_fnv64"),
        Some(&hash_file(&archive, None).as_str()),
        "corrupt Wave IR archive"
    );
    assert_eq!(
        fields.get("provenance_fnv64"),
        Some(&hash_file(&provenance, Some(16 * 1024)).as_str()),
        "corrupt Wave IR provenance"
    );
    println!("cargo:rustc-link-search=native={}", directory.display());
    println!("cargo:rustc-link-lib=static=whale_wave_ir");
    println!("cargo:rustc-cfg=whale_wave_ir");
}
