// SPDX-License-Identifier: MPL-2.0
//! Strict JSON boundary. Decode raw JSON here before constructing frontend ASTs.
use super::{frontend::Program, AST_FORMAT_VERSION};

use serde::{
    de::{self, MapAccess, SeqAccess, Visitor},
    Deserialize, Deserializer, Serialize,
};

use serde_json::Value;

use std::fmt;

pub const DEFAULT_MAX_INPUT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    format_version: u32,
    semantics_version: u32,
    features: Vec<String>,
    program: Program,
}

/// Reject missing/unknown versions, unknown fields/features and duplicate keys.
/// JSON nesting is also bounded by serde_json's recursion limit.
pub fn decode(source: &str) -> Result<Program, String> {
    decode_with_limit(source, DEFAULT_MAX_INPUT_BYTES)
}

pub fn decode_with_limit(source: &str, max_bytes: usize) -> Result<Program, String> {
    if source.len() > max_bytes {
        return Err("AST JSON exceeds input byte limit".into());
    }
    let value: UniqueValue = serde_json::from_str(source).map_err(|e| e.to_string())?;
    let envelope: Envelope = serde_json::from_value(value.0)
        .map_err(|e| format!("AST envelope/schema error: {e}; expected format_version {AST_FORMAT_VERSION}, semantics_version {}, features [], program", crate::SEMANTICS_VERSION))?;
    if !(2..=AST_FORMAT_VERSION).contains(&envelope.format_version) {
        return Err(format!(
            "unsupported AST format_version {}; expected {AST_FORMAT_VERSION}",
            envelope.format_version
        ));
    }
    if envelope.semantics_version != crate::SEMANTICS_VERSION {
        return Err(format!(
            "unsupported semantics_version {}; expected {}",
            envelope.semantics_version,
            crate::SEMANTICS_VERSION
        ));
    }
    if !envelope.features.is_empty() {
        return Err(format!(
            "unsupported AST features: {:?}; supported feature list is empty",
            envelope.features
        ));
    }
    let extended =
        super::limits::inspect_program_wire(&envelope.program, super::AstLimits::default())?;
    if envelope.format_version < 3 && extended & 1 != 0 {
        return Err("extended frontend operators require AST format 3".into());
    }
    if envelope.format_version < 4 && extended & 2 != 0 {
        return Err("source annotations require AST format 4".into());
    }
    if envelope.format_version < 5 && extended & 4 != 0 {
        return Err("new frontend expressions require AST format 5".into());
    }
    Ok(envelope.program)
}

pub fn encode(program: Program) -> Result<String, String> {
    encode_with_limits(program, super::AstLimits::default())
}

/// Guard direct Rust inputs before recursive pretty serialization. Rejected owned
/// trees are disposed iteratively; output storage never grows past its budget.
pub fn encode_with_limits(program: Program, limits: super::AstLimits) -> Result<String, String> {
    if let Err(error) = super::limits::inspect_program_wire(&program, limits) {
        super::limits::discard_program(program);
        return Err(error.to_string());
    }
    let envelope = Envelope {
        format_version: AST_FORMAT_VERSION,
        semantics_version: crate::SEMANTICS_VERSION,
        features: Vec::new(),
        program,
    };
    struct BoundedOutput {
        bytes: Vec<u8>,
        limit: usize,
    }

    impl std::io::Write for BoundedOutput {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let end = self
                .bytes
                .len()
                .checked_add(bytes.len())
                .filter(|end| *end <= self.limit)
                .ok_or_else(|| std::io::Error::other(super::AstLimitError::Output))?;
            if end > self.bytes.capacity() {
                let capacity =
                    self.bytes.capacity().max(256).saturating_mul(2).max(end).min(self.limit);
                self.bytes
                    .try_reserve_exact(capacity - self.bytes.len())
                    .map_err(|_| std::io::Error::other("AST JSON output allocation failed"))?;
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut output = BoundedOutput { bytes: Vec::new(), limit: limits.max_output_bytes };
    serde_json::to_writer_pretty(&mut output, &envelope).map_err(|e| e.to_string())?;
    String::from_utf8(output.bytes).map_err(|e| e.to_string())
}

// Parsing into ordinary Value first would discard duplicate keys. Preserve and
// check every map entry while consuming the original stream, including enum tags.
struct UniqueValue(Value);

impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct UniqueVisitor;

        impl<'de> Visitor<'de> for UniqueVisitor {
            type Value = UniqueValue;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("JSON without duplicate keys")
            }

            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(UniqueValue(Value::Bool(v)))
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }

            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(v)
                    .map(|v| UniqueValue(Value::Number(v)))
                    .ok_or_else(|| E::custom("non-finite JSON number"))
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }

            fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }

            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(UniqueValue(Value::Null))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(UniqueValue(value)) = seq.next_element()? {
                    values.push(value);
                }
                Ok(UniqueValue(Value::Array(values)))
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut values = serde_json::Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(de::Error::custom(format!("duplicate JSON key {key:?}")));
                    }
                    let UniqueValue(value) = map.next_value()?;
                    values.insert(key, value);
                }
                Ok(UniqueValue(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(UniqueVisitor)
    }
}
