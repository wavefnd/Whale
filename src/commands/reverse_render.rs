//! Streaming rendering stops at the byte ceiling before constructing a full report.
use reverse::{
    backend_name,
    image::{Annotation, CodeRegion, Image},
    DecodeError, Instruction, Operand,
};

use serde::{
    ser::{SerializeMap, SerializeSeq},
    Serialize, Serializer,
};

use std::{fmt, io};

pub(super) struct BoundedOutput {
    bytes: Vec<u8>,
    limit: usize,
}

impl BoundedOutput {
    pub(super) fn new(limit: usize) -> Self {
        Self { bytes: Vec::new(), limit }
    }

    pub(super) fn finish(self) -> Result<String, String> {
        String::from_utf8(self.bytes).map_err(|_| "reverse rendering is not UTF-8".into())
    }
}

impl io::Write for BoundedOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("reverse output byte limit exceeded"));
        }
        let needed = self.bytes.len() + bytes.len();
        if needed > self.bytes.capacity() {
            // Grow geometrically without reserving past the caller's byte ceiling.
            let capacity = needed.max(self.bytes.capacity().saturating_mul(2).min(self.limit));
            self.bytes
                .try_reserve_exact(capacity - self.bytes.len())
                .map_err(|_| io::Error::other("reverse output allocation failed"))?;
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl fmt::Write for BoundedOutput {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        io::Write::write_all(self, s.as_bytes()).map_err(|_| fmt::Error)
    }
}

pub(super) struct Stream<I>(pub(super) I);

impl<I> Serialize for Stream<I>
where
    I: Iterator + Clone,
    I::Item: Serialize,
{
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(None)?;
        for item in self.0.clone() {
            sequence.serialize_element(&item)?;
        }
        sequence.end()
    }
}

#[derive(Serialize)]
struct Region<'a> {
    id: usize,
    section: Option<usize>,
    name: &'a str,
    file_offset: u64,
    address: u64,
    file_size: usize,
}

#[derive(Serialize)]
struct Symbol<'a> {
    name: &'a str,
    region: Option<usize>,
    offset: u64,
    address: Option<u64>,
}

#[derive(Serialize)]
struct Relocation<'a> {
    region: usize,
    offset: usize,
    symbol: &'a str,
    addend: i64,
    kind: &'static str,
}

fn relocation_name(kind: object::RelocKind) -> &'static str {
    use object::RelocKind::*;
    match kind {
        TargetSpecific { .. } => "TargetSpecific",
        Absolute64 => "Absolute64",
        Absolute32 => "Absolute32",
        SignedAbsolute32 => "SignedAbsolute32",
        Relative32 => "Relative32",
        Relative8 => "Relative8",
        GOTPCREL => "GOTPCREL",
        PLT32 => "PLT32",
    }
}

struct Inspection<'a>(&'a Image<'a>);

impl Serialize for Inspection<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let image = self.0;
        let mut map = serializer.serialize_map(Some(7))?;
        map.serialize_entry("schema", &1)?;
        map.serialize_entry("kind", image.kind)?;
        map.serialize_entry("entry", &image.entry)?;
        map.serialize_entry("decoder", backend_name())?;
        map.serialize_entry(
            "regions",
            &Stream(image.regions.iter().map(|r| Region {
                id: r.id,
                section: r.section,
                name: &r.name,
                file_offset: r.file_offset,
                address: r.address,
                file_size: r.bytes.len(),
            })),
        )?;
        map.serialize_entry(
            "symbols",
            &Stream(image.symbols.iter().map(|s| Symbol {
                name: &s.name,
                region: s.region,
                offset: s.offset,
                address: s.address,
            })),
        )?;
        map.serialize_entry(
            "relocations",
            &Stream(image.relocations.iter().map(|r| Relocation {
                region: r.region,
                offset: r.offset,
                symbol: &r.symbol,
                addend: r.addend,
                kind: relocation_name(r.kind),
            })),
        )?;
        map.end()
    }
}

pub(super) fn stream_structured(value: &impl Serialize, limit: usize) -> Result<String, String> {
    let mut output = BoundedOutput::new(limit);
    serde_json::to_writer_pretty(&mut output, value).map_err(|e| e.to_string())?;
    io::Write::write_all(&mut output, b"\n").map_err(|e| e.to_string())?;
    output.finish()
}

pub(super) fn inspect(image: &Image<'_>, structured: bool, limit: usize) -> Result<String, String> {
    if structured {
        return stream_structured(&Inspection(image), limit);
    }
    let mut output = BoundedOutput::new(limit);
    {
        use fmt::Write;
        let render = (|| -> fmt::Result {
            writeln!(output, "{} entry={:?} decoder={}", image.kind, image.entry, backend_name())?;
            for r in &image.regions {
                writeln!(
                    output,
                    "{}: {:?} file=0x{:x} address=0x{:x} bytes={}",
                    r.id,
                    r.name,
                    r.file_offset,
                    r.address,
                    r.bytes.len()
                )?;
            }
            for s in &image.symbols {
                writeln!(
                    output,
                    "symbol {:?} region={:?} offset=0x{:x} address={:?}",
                    s.name, s.region, s.offset, s.address
                )?;
            }
            for r in &image.relocations {
                writeln!(
                    output,
                    "relocation region={} offset={} {:?} {:?}{:+}",
                    r.region, r.offset, r.kind, r.symbol, r.addend
                )?;
            }
            Ok(())
        })();
        render.map_err(|_| "reverse output byte limit exceeded")?;
    }
    output.finish()
}

fn annotations<'a>(
    image: &'a Image<'_>,
    region: usize,
    offset: usize,
    length: usize,
) -> impl Iterator<Item = &'a Annotation> + Clone {
    let start = image
        .relocations
        .partition_point(|r| (r.region, r.offset) < (region, offset.saturating_sub(7)));
    let end = offset.saturating_add(length);
    image.relocations[start..]
        .iter()
        .take_while(move |r| r.region == region && r.offset < end)
        .filter(move |r| r.offset.saturating_add(r.width) > offset)
}

#[derive(Serialize)]
struct InstructionRelocation<'a> {
    offset: usize,
    symbol: &'a str,
    addend: i64,
    kind: &'static str,
    width_bytes: usize,
}

pub(super) struct InstructionView<'a, 'b> {
    pub(super) instruction: &'a Instruction,
    pub(super) image: &'a Image<'b>,
    pub(super) region: usize,
    pub(super) base: u64,
}

impl Serialize for InstructionView<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let instruction = self.instruction;
        let annotations =
            annotations(self.image, self.region, instruction.offset, instruction.length as usize);
        // Decoded instructions have at most three operands and fifteen bytes.
        // Their fixed-size facts can use the existing formatter; potentially
        // large relocation names/counts remain borrowed and stream separately.
        let facts = reverse::format::instruction_json(instruction, self.base, &[]);
        let facts = facts.as_object().expect("instruction formatter returns a map");
        let mut map = serializer.serialize_map(Some(facts.len()))?;
        for (key, value) in facts {
            match key.as_str() {
                "target" if annotations.clone().next().is_some() => {
                    map.serialize_entry(key, &Option::<u64>::None)?;
                },
                "relocations" => {
                    map.serialize_entry(
                        key,
                        &Stream(annotations.clone().map(|r| InstructionRelocation {
                            offset: r.offset,
                            symbol: &r.symbol,
                            addend: r.addend,
                            kind: relocation_name(r.kind),
                            width_bytes: r.width,
                        })),
                    )?;
                },
                _ => map.serialize_entry(key, value)?,
            }
        }
        map.end()
    }
}

#[derive(Serialize)]
struct InstructionRecord<'a, 'b> {
    region: usize,
    file_offset: u64,
    instruction: InstructionView<'a, 'b>,
}

#[derive(Serialize)]
struct DataRecord<'a> {
    region: usize,
    offset: usize,
    file_offset: u64,
    byte: u8,
    status: &'a str,
    code: bool,
}

/// Accumulates only the final bounded bytes, never a collection of JSON values.
pub(super) struct ListingOutput {
    output: BoundedOutput,
    structured: bool,
    first: bool,
}

impl ListingOutput {
    pub(super) fn new(image: &Image<'_>, structured: bool, limit: usize) -> Result<Self, String> {
        let mut result = Self { output: BoundedOutput::new(limit), structured, first: true };
        if structured {
            result.bytes(b"{\n  \"schema\": 1,\n  \"kind\": ")?;
            result.json(&image.kind)?;
            result.bytes(b",\n  \"backend\": ")?;
            result.json(&backend_name())?;
            result.bytes(b",\n  \"records\": [")?;
        }
        Ok(result)
    }

    fn bytes(&mut self, bytes: &[u8]) -> Result<(), String> {
        io::Write::write_all(&mut self.output, bytes).map_err(|e| e.to_string())
    }

    fn json(&mut self, value: &impl Serialize) -> Result<(), String> {
        serde_json::to_writer_pretty(&mut self.output, value).map_err(|e| e.to_string())
    }

    fn record(&mut self, value: &impl Serialize) -> Result<(), String> {
        self.bytes(if self.first { b"\n" } else { b",\n" })?;
        self.first = false;
        self.json(value)
    }

    fn text(&mut self, args: fmt::Arguments<'_>) -> Result<(), String> {
        fmt::Write::write_fmt(&mut self.output, args)
            .map_err(|_| "reverse output byte limit exceeded".into())
    }

    pub(super) fn region(&mut self, region: &CodeRegion<'_>) -> Result<(), String> {
        if !self.structured {
            self.text(format_args!(
                "; region {} {} file=0x{:x} base=0x{:x}\n",
                region.id, region.name, region.file_offset, region.address
            ))?;
        }
        Ok(())
    }

    pub(super) fn instruction(
        &mut self,
        image: &Image<'_>,
        region: &CodeRegion<'_>,
        instruction: &Instruction,
    ) -> Result<(), String> {
        if self.structured {
            return self.record(&InstructionRecord {
                region: region.id,
                file_offset: region.file_offset + instruction.offset as u64,
                instruction: InstructionView {
                    instruction,
                    image,
                    region: region.id,
                    base: region.address,
                },
            });
        }
        let annotations =
            annotations(image, region.id, instruction.offset, instruction.length as usize);
        if let Some(address) = region.address.checked_add(instruction.offset as u64) {
            self.text(format_args!("{address:016x}  "))?;
        } else {
            self.bytes(b"overflow  ")?;
        }
        for (index, byte) in instruction.original_bytes().iter().enumerate() {
            if index != 0 {
                self.bytes(b" ")?;
            }
            self.text(format_args!("{byte:02x}"))?;
        }
        let bytes_width = (instruction.length as usize).saturating_mul(3).saturating_sub(1);
        for _ in bytes_width..44 {
            self.bytes(b" ")?;
        }
        self.text(format_args!("  {}", instruction.mnemonic()))?;
        for (index, operand) in instruction.operands.iter().enumerate() {
            self.bytes(if index == 0 { b" " } else { b", " })?;
            if let (Operand::Relative(_), Some(annotation)) = (operand, annotations.clone().next())
            {
                self.text(format_args!("reloc({}{:+})", annotation.symbol, annotation.addend))?;
            } else {
                self.text(format_args!(
                    "{}",
                    reverse::format::operand_text(operand, instruction, region.address)
                ))?;
            }
        }
        for annotation in annotations {
            self.text(format_args!(
                " ; {:?} {}{:+} @section+{}",
                annotation.kind, annotation.symbol, annotation.addend, annotation.offset
            ))?;
        }
        self.bytes(b"\n")
    }

    pub(super) fn data(
        &mut self,
        region: &CodeRegion<'_>,
        offset: usize,
        error: &DecodeError,
    ) -> Result<(), String> {
        let byte = region.bytes[offset];
        if self.structured {
            self.record(&DataRecord {
                region: region.id,
                offset,
                file_offset: region.file_offset + offset as u64,
                byte,
                status: &error.to_string(),
                code: false,
            })
        } else {
            self.text(format_args!(
                "{:016x}  {byte:02x}  db 0x{byte:02x} ; {error}; following records are a linear listing\n",
                region.address + offset as u64
            ))
        }
    }

    pub(super) fn finish(mut self) -> Result<String, String> {
        if self.structured {
            self.bytes(b"\n  ]\n}\n")?;
        }
        self.output.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call_instruction() -> Instruction {
        Instruction {
            offset: 0,
            length: 5,
            bytes: [0xe8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            opcode: reverse::Opcode::Call,
            condition: None,
            operands: vec![Operand::Relative(reverse::Relative {
                displacement: 0,
                encoded_width: 32,
            })],
            encoding: reverse::Encoding {
                prefixes: Vec::new(),
                rex: None,
                opcode_offset: 0,
                opcode_len: 1,
                modrm: None,
                sib: None,
                displacement: None,
                immediate: Some(reverse::EncodingField { offset: 1, width: 4, bits: 0 }),
            },
            flow: reverse::Flow::Call { relative: Some(0) },
        }
    }

    fn render_listing(
        image: &Image<'_>,
        instruction: &Instruction,
        structured: bool,
        limit: usize,
    ) -> Result<String, String> {
        let mut output = ListingOutput::new(image, structured, limit)?;
        output.region(&image.regions[0])?;
        output.instruction(image, &image.regions[0], instruction)?;
        output.data(&image.regions[0], 5, &DecodeError::Truncated { offset: 5 })?;
        output.finish()
    }

    #[test]
    fn streaming_output_honors_tiny_limits_before_large_name_rendering() {
        let mut image = Image::raw(&[0xc3], 0).unwrap();
        for n in 0..10000 {
            image.symbols.push(reverse::image::ImageSymbol {
                name: format!("symbol{n}"),
                region: Some(0),
                offset: 0,
                address: Some(0),
            });
        }
        for json in [false, true] {
            assert!(inspect(&image, json, 1).unwrap_err().contains("limit"));
        }
        let text = inspect(&image, true, 2_000_000).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["symbols"].as_array().unwrap().len(), 10000);
        assert!(inspect(&image, true, text.len() - 1).is_err());
        assert_eq!(inspect(&image, true, text.len()).unwrap(), text);
    }

    #[test]
    fn listing_streams_identical_instruction_facts_and_honors_exact_byte_boundary() {
        let instruction = call_instruction();
        let mut image = Image::raw(&[0xe8, 0, 0, 0, 0, 0x0f], 0x1000).unwrap();
        image.relocations.push(Annotation {
            region: 0,
            offset: 1,
            symbol: "external\"\nname".into(),
            addend: -4,
            kind: object::RelocKind::PLT32,
            width: 4,
        });
        let annotations = image.annotations(0, 0, 5);
        let json = render_listing(&image, &instruction, true, 10000).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            parsed["records"][0]["instruction"],
            reverse::format::instruction_json(&instruction, 0x1000, &annotations)
        );
        assert_eq!(parsed["records"][1]["byte"], 0x0f);
        assert_eq!(parsed["records"][1]["code"], false);
        let text = render_listing(&image, &instruction, false, 10000).unwrap();
        assert!(text.contains(&reverse::format::instruction_text(
            &instruction,
            0x1000,
            &annotations
        )));
        for (structured, expected) in [(true, json), (false, text)] {
            assert_eq!(
                render_listing(&image, &instruction, structured, expected.len()).unwrap(),
                expected
            );
            assert!(render_listing(&image, &instruction, structured, expected.len() - 1)
                .unwrap_err()
                .contains("limit"));
        }
    }

    #[test]
    fn listing_borrows_large_names_and_streams_large_annotation_counts() {
        let instruction = call_instruction();
        let mut image = Image::raw(&[0xe8, 0, 0, 0, 0, 0x0f], 0).unwrap();
        image.relocations.push(Annotation {
            region: 0,
            offset: 1,
            symbol: "x".repeat(1024 * 1024),
            addend: 0,
            kind: object::RelocKind::Relative32,
            width: 4,
        });
        for structured in [false, true] {
            let mut output = ListingOutput::new(&image, structured, 512).unwrap();
            output.region(&image.regions[0]).unwrap();
            assert!(output
                .instruction(&image, &image.regions[0], &instruction)
                .unwrap_err()
                .contains("limit"));
            assert!(output.output.bytes.len() <= 512);
            assert!(output.output.bytes.capacity() <= 512);
        }
        image.relocations[0].symbol = "x".into();
        for _ in 1..10000 {
            image.relocations.push(image.relocations[0].clone());
        }
        for structured in [false, true] {
            assert!(render_listing(&image, &instruction, structured, 512)
                .unwrap_err()
                .contains("limit"));
        }
    }
}
