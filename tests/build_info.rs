use std::process::Command;

#[test]
fn embedded_identity_matches_the_selected_modules_and_has_no_build_paths() {
    let result =
        Command::new(env!("CARGO_BIN_EXE_whale")).args(["build-info", "--json"]).output().unwrap();
    assert!(result.status.success(), "{result:?}");
    let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(value["semantics_version"], ir::SEMANTICS_VERSION);
    assert_eq!(value["implementations"]["ir"], ir::module_identity());
    assert_eq!(value["implementations"]["assembler"], assembler::module_identity());
    assert_eq!(value["implementations"]["asmobj"], toolchain::assembly_object::module_identity());
    let selected =
        whale::builtins::registry().assembler_ids().map(str::to_owned).collect::<Vec<_>>();
    assert_eq!(value["assemblers"], serde_json::json!(selected));
    assert_eq!(value["native_linker"], cfg!(feature = "native-linker"));
    let serialized = String::from_utf8(result.stdout).unwrap();
    assert!(!serialized.contains(env!("CARGO_MANIFEST_DIR")));
    assert!(!serialized.contains("bootstrap_provenance"));
    for module in value["wave_modules"].as_object().unwrap().values() {
        for key in
            ["source_sha256", "abi_sha256", "archive_sha256", "compiler_sha256", "std_sha256"]
        {
            let digest = module[key].as_str().unwrap();
            assert_eq!(digest.len(), 64);
            assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
        }
    }
}

#[test]
fn build_info_rejects_unknown_options_and_help_names_unavailable_bytecode() {
    let result = Command::new(env!("CARGO_BIN_EXE_whale"))
        .args(["build-info", "--unknown"])
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(2));
    let help = Command::new(env!("CARGO_BIN_EXE_whale")).arg("--help").output().unwrap();
    let help = String::from_utf8(help.stdout).unwrap();
    assert!(help.contains("build-info [--json]"));
    assert_eq!(help.contains("bytecode Unavailable:"), !ir::bytecode::available());
}
