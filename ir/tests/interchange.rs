#![cfg(feature = "socket")]
use ir::lower_ast::{
    interchange::{decode, decode_with_limit, encode},
    lower_o0,
};
use ir::{DataLayout, FloatBits, Type};

fn lower(source: &str) -> Result<ir::Module, String> {
    let program = decode(source)?;
    lower_o0(
        &program,
        "x86_64-whale-linux",
        DataLayout::default_64bit_le(),
    )
    .map_err(|e| format!("{e:?}"))
}
const INTEGER: &str = include_str!("fixtures/ast-v2-u128.json");
const FLOAT: &str = include_str!("fixtures/ast-v2-float.json");

#[test]
fn exact_numeric_fixtures_round_trip_and_lower() {
    for source in [INTEGER, FLOAT] {
        let once = encode(decode(source).unwrap()).unwrap();
        let twice = encode(decode(&once).unwrap()).unwrap();
        assert_eq!(once, twice);
        let module = lower(&twice).unwrap();
        ir::verify_module(&module).unwrap();
    }
    let printed = ir::print_module(&lower(INTEGER).unwrap());
    assert!(printed.contains(&u128::MAX.to_string()));
    assert!(printed.contains("format_version 2\n  semantics_version 1"));
    assert!(ir::print_module(&lower(FLOAT).unwrap()).contains("0xffc01234"));
    let signed = INTEGER
        .replace("false", "true")
        .replace(&u128::MAX.to_string(), &i128::MIN.to_string());
    assert!(ir::print_module(&lower(&signed).unwrap()).contains(&i128::MIN.to_string()));
}

#[test]
fn raw_json_rejects_duplicate_keys_even_inside_enum_and_escaped_names() {
    for (from, to) in [
        (
            "\"format_version\": 2",
            "\"format_version\": 2, \"format_version\": 2",
        ),
        ("\"globals\": []", "\"globals\": [], \"glob\\u0061ls\": []"),
        ("\"bits\": 128", "\"bits\": 128, \"bits\": 128"),
        (
            "\"name\": \"answer\"",
            "\"name\": \"answer\", \"name\": \"answer\"",
        ),
    ] {
        let error = decode(&INTEGER.replacen(from, to, 1)).unwrap_err();
        assert!(error.contains("duplicate JSON key"), "{error}");
    }
}

#[test]
fn unsupported_schema_versions_and_features_are_errors() {
    for source in [
        "{\"globals\":[],\"functions\":[]}".to_string(),
        INTEGER.replace("\"format_version\": 2", "\"format_version\": 99"),
        INTEGER.replace("\"semantics_version\": 1", "\"semantics_version\": 99"),
        INTEGER.replace("\"features\": []", "\"features\": [\"unknown\"]"),
        INTEGER.replace("\"globals\": []", "\"extra\": 0, \"globals\": []"),
        INTEGER.replace("\"bits\": 128", "\"extra\": 0, \"bits\": 128"),
        INTEGER.replace("\"name\": \"answer\"", "\"extra\": 0, \"name\": \"answer\""),
        INTEGER.replace(
            "\"value\": \"340282366920938463463374607431768211455\"",
            "\"value\": 42",
        ),
        format!("{INTEGER} null"),
    ] {
        assert!(decode(&source).is_err(), "{source}");
    }
    assert!(decode_with_limit(INTEGER, INTEGER.len() - 1).is_err());
    assert!(decode_with_limit(INTEGER, INTEGER.len()).is_ok());
    let deep = format!("{}0{}", "[".repeat(200), "]".repeat(200));
    assert!(decode(&deep).is_err());
}

#[test]
fn numeric_ranges_and_storage_widths_are_checked() {
    for value in [
        "-1",
        "+1",
        "1.0",
        " 1",
        "1e2",
        "",
        "340282366920938463463374607431768211456",
    ] {
        assert!(
            lower(&INTEGER.replace(&u128::MAX.to_string(), value)).is_err(),
            "{value}"
        );
    }
    assert!(lower(&INTEGER.replace("128", "8")).is_err());
    for value in ["nan", "1.5", "0x1234", "0x100000000", "0xzzzzzzzz"] {
        assert!(lower(&FLOAT.replace("0xffc01234", value)).is_err());
    }
}

#[test]
fn every_float_width_preserves_zero_infinity_and_nan_storage() {
    for (ty, values) in [
        (
            Type::F16,
            vec![
                FloatBits::F16(0),
                FloatBits::F16(0x8000),
                FloatBits::F16(0x7c00),
                FloatBits::F16(0x7e01),
                FloatBits::F16(0xfe02),
            ],
        ),
        (
            Type::F32,
            vec![
                FloatBits::F32(0),
                FloatBits::F32(0x80000000),
                FloatBits::F32(0x7f800000),
                FloatBits::F32(0x7fc00001),
                FloatBits::F32(0xffc00002),
            ],
        ),
        (
            Type::F64,
            vec![
                FloatBits::F64(0),
                FloatBits::F64(0x8000000000000000),
                FloatBits::F64(0x7ff0000000000000),
                FloatBits::F64(0x7ff8000000000001),
                FloatBits::F64(0xfff8000000000002),
            ],
        ),
    ] {
        for bits in values {
            assert_eq!(
                FloatBits::parse(bits.width(), &bits.to_string()).unwrap(),
                bits
            );
            let source = FLOAT
                .replace("\"bits\": 32", &format!("\"bits\": {}", bits.width()))
                .replace("0xffc01234", &bits.to_string());
            let module = lower(&encode(decode(&source).unwrap()).unwrap()).unwrap();
            assert_eq!(module.functions[0].ret_ty, ty);
            assert!(ir::print_module(&module).contains(&bits.to_string()));
            ir::verify_module(&module).unwrap();
        }
    }
}

#[test]
fn unit_variant_objects_have_the_same_meaning_and_encode_canonically() {
    let source = r#"{"format_version":2,"semantics_version":1,"features":[],"program":{"declarations":[],"globals":[],"functions":[{"name":"empty","convention":"Whale","linkage":"Internal","link_name":null,"parameters":[],"return_type":{"Void":null},"body":[{"Return":null}]}]}}"#;
    let encoded = encode(decode(source).unwrap()).unwrap();
    assert!(encoded.contains("\"return_type\": \"Void\""));
    ir::verify_module(&lower(source).unwrap()).unwrap();
    assert!(decode(&source.replace("\"Void\":null", "\"Void\":{\"extra\":0}")).is_err());
}
