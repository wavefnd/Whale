//! Embedded build provenance and stamps returned by the linked source modules.
use serde_json::{json, Value};

pub fn value() -> Result<Value, &'static str> {
    let mut value: Value =
        serde_json::from_str(include_str!(concat!(env!("OUT_DIR"), "/build-info.json")))
            .map_err(|_| "invalid embedded build provenance")?;
    let mut modules = serde_json::Map::new();
    modules.insert("ir".into(), json!(ir::module_identity()));
    modules.insert("assembler".into(), json!(assembler::module_identity()));
    modules.insert("asmobj".into(), json!(toolchain::assembly_object::module_identity()));
    #[cfg(feature = "asm-amd64")]
    modules.insert("amd64".into(), json!(amd64::module_identity()));
    #[cfg(feature = "asm-aarch64")]
    modules.insert("aarch64".into(), json!(aarch64::module_identity()));
    #[cfg(feature = "asm-riscv64")]
    modules.insert("riscv64".into(), json!(riscv64::module_identity()));
    #[cfg(feature = "asm-loongarch64")]
    modules.insert("loongarch64".into(), json!(loongarch64::module_identity()));
    if modules.values().any(|module| module == "invalid-wave") {
        return Err("linked Wave module identity mismatch");
    }
    let mode = if modules.values().all(|module| module == "wave") {
        "wave"
    } else if modules.get("ir").is_some_and(|module| module == "rust-reference") {
        "rust-reference"
    } else {
        "mixed"
    };
    value["product_mode"] = json!(mode);
    value["implementations"] = Value::Object(modules);
    value["version"] = json!(env!("CARGO_PKG_VERSION"));
    value["ir_format_version"] = json!(ir::IR_FORMAT_VERSION);
    value["semantics_version"] = json!(ir::SEMANTICS_VERSION);
    value["bytecode_format_version"] = json!(1);
    value["assemblers"] = json!(crate::builtins::registry().assembler_ids().collect::<Vec<_>>());
    value["native_linker"] = json!(cfg!(feature = "native-linker"));
    value["socket_cli"] = json!(cfg!(feature = "socket-cli"));
    Ok(value)
}

pub(crate) fn run(arguments: Vec<String>) {
    if !arguments.is_empty() && arguments != ["--json"] {
        eprintln!("Usage: whale build-info [--json]");
        std::process::exit(2);
    }
    let value = value().unwrap_or_else(|error| {
        eprintln!("Error: {error}");
        std::process::exit(1);
    });
    let output = if arguments.is_empty() {
        serde_json::to_string_pretty(&value).expect("serializable provenance")
    } else {
        value.to_string()
    };
    crate::commands::report::println!("{output}");
}
