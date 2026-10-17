//! Dependency-free bounded mutation targets; every accepted input exercises real library APIs.
#![allow(dead_code)]
use std::path::{Path, PathBuf};

pub const MAX_INPUT: usize = 8192;

#[derive(Clone, Copy, Debug)]
pub enum Target {
    Assembler,
    Ir,
    Ast,
    Object,
}

impl Target {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "assembler" => Some(Self::Assembler),
            "ir" => Some(Self::Ir),
            "ast" => Some(Self::Ast),
            "object" => Some(Self::Object),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Assembler => "assembler",
            Self::Ir => "ir",
            Self::Ast => "ast",
            Self::Object => "object",
        }
    }
}

pub fn ir_limits() -> ir::IrLimits {
    ir::IrLimits {
        max_input_bytes: MAX_INPUT,
        max_tokens: 4096,
        max_nodes: 8192,
        max_type_depth: 32,
        max_const_depth: 32,
        max_analysis_work: 131072,
    }
}

/// Rejection is normal. A panic means a target violated its bounded-input contract.
pub fn check(target: Target, bytes: &[u8]) -> bool {
    if bytes.len() > MAX_INPUT {
        return false;
    }
    // ELF payloads and model bytes are arbitrary binary, never UTF-8 text.
    if matches!(target, Target::Object) {
        return check_object(bytes);
    }
    let Ok(source) = std::str::from_utf8(bytes) else {
        return false;
    };
    match target {
        Target::Object => unreachable!("binary target handled before UTF-8 conversion"),
        Target::Assembler => {
            if let Ok(tokens) = assembler::tokens::tokenize(source) {
                let _ = amd64::parser::parse(&tokens);
            }
            let limits =
                assembler::AssemblyLimits { max_output_bytes: 65536, max_relax_iterations: 32 };
            let Ok(output) = assembler::assemble_with_limits(source, &amd64::AMD64, &limits) else {
                return false;
            };
            assert!(
                output.sections.iter().map(|s| s.data.len()).sum::<usize>()
                    <= limits.max_output_bytes
            );
            for symbol in &output.symbols {
                if let Some(index) = symbol.section_index {
                    let section =
                        output.sections.get(index).expect("assembled symbol section must exist");
                    assert!(
                        symbol.offset + symbol.size
                            <= section.data.len() as u64 + section.zero_fill
                    );
                }
            }
            true
        },
        Target::Ir => {
            let Ok(module) = ir::parse_module_with_limits(source, ir_limits()) else {
                return false;
            };
            canonical(&module);
            true
        },
        Target::Ast => {
            #[cfg(feature = "socket-cli")]
            {
                let Ok(program) = ir::lower_ast::interchange::decode_with_limit(source, MAX_INPUT)
                else {
                    return false;
                };
                let limits = ir::lower_ast::AstLimits {
                    max_depth: 16,
                    max_nodes: 2048,
                    max_string_bytes: MAX_INPUT,
                    max_output_bytes: 65536,
                    max_source_work: 65_536,
                };
                let target = ir::Target::X86_64WhaleLinux;
                let Ok(module) = ir::lower_ast::lower_o0_with_limits(
                    &program,
                    target.name(),
                    target.data_layout(),
                    limits,
                ) else {
                    return false;
                };
                match ir::verify_module_with_limits(&module, ir_limits()) {
                    Ok(()) => {},
                    Err(ir::VerifyError::ResourceLimit(_)) => return false,
                    Err(error) => panic!("successful O0 lowering produced invalid IR: {error:?}"),
                }
                canonical(&module);
                // Check both the direct owned encoder and strict JSON decoder.
                if let Ok(encoded) = ir::lower_ast::interchange::encode_with_limits(program, limits)
                {
                    ir::lower_ast::interchange::decode_with_limit(&encoded, 65536)
                        .expect("encoded AST must decode");
                }
                true
            }
            #[cfg(not(feature = "socket-cli"))]
            {
                false
            }
        },
    }
}

fn canonical(module: &ir::Module) {
    let text = ir::print_module(module);
    assert!(text.len() <= 262144, "bounded source expanded unexpectedly");
    let mut limits = ir_limits();
    limits.max_input_bytes = 262144;
    limits.max_tokens = 16384;
    let decoded =
        ir::parse_module_with_limits(&text, limits).expect("printed verified module must parse");
    assert_eq!(ir::print_module(&decoded), text, "canonical text/identities changed");
}

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    pub fn index(&mut self, n: usize) -> usize {
        (self.next() as usize) % n.max(1)
    }
}

pub fn mutate(seed: &[u8], rng: &mut Rng) -> Vec<u8> {
    let mut input = seed[..seed.len().min(MAX_INPUT)].to_vec();
    for _ in 0..1 + rng.index(4) {
        let at = rng.index(input.len() + 1);
        match rng.index(5) {
            0 if at < input.len() => {
                input[at] ^= 1 << rng.index(8);
            },
            1 if input.len() < MAX_INPUT => {
                input.insert(at, rng.next() as u8);
            },
            2 if at < input.len() => {
                let end = (at + 1 + rng.index(32)).min(input.len());
                input.drain(at..end);
            },
            3 if !input.is_empty() => {
                let end = (at + 1 + rng.index(64)).min(input.len());
                let copy = input[at..end].to_vec();
                let room = MAX_INPUT - input.len();
                input.splice(at..at, copy.into_iter().take(room));
            },
            _ => {
                let token = [
                    b"9223372036854775808".as_slice(),
                    b"resq 9223372036854775807",
                    b"{{[[",
                    b"\"format_version\":99",
                    b"\r\n",
                    b"\xff",
                ][rng.index(6)];
                let room = MAX_INPUT - input.len();
                input.splice(at..at, token.iter().copied().take(room));
            },
        }
    }
    input
}

pub fn minimize(mut input: Vec<u8>, fails: impl Fn(&[u8]) -> bool) -> Vec<u8> {
    let mut width = input.len() / 2;
    while width > 0 {
        let mut at = 0;
        while at + width <= input.len() {
            let mut candidate = input.clone();
            candidate.drain(at..at + width);
            if fails(&candidate) {
                input = candidate;
            } else {
                at += width;
            }
        }
        width /= 2;
    }
    input
}

pub fn read_bounded(path: &Path) -> Vec<u8> {
    use std::io::Read;
    let file = std::fs::File::open(path).expect("replay input");
    let mut bytes = Vec::new();
    file.take((MAX_INPUT + 1) as u64).read_to_end(&mut bytes).unwrap();
    assert!(bytes.len() <= MAX_INPUT, "seed/replay exceeds input budget");
    bytes
}

pub fn seeds(root: &Path, target: Target) -> Vec<(PathBuf, Vec<u8>)> {
    let mut entries: Vec<_> = std::fs::read_dir(root.join(target.name()))
        .expect("seed directory")
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_file())
        .collect();
    entries.sort();
    assert!(!entries.is_empty(), "empty seed corpus");
    entries
        .into_iter()
        .map(|path| {
            let bytes = read_bounded(&path);
            assert!(bytes.len() <= MAX_INPUT);
            (path, bytes)
        })
        .collect()
}

/// Both disk bytes and directly constructed public models are independently bounded.
/// WOM1 followed by eight bytes selects model flags, section kind, alignment,
/// symbol value/size, relocation kind/offset and signed addend. Remaining bytes
/// supply at most 64 payload bytes. Other inputs are passed unchanged to ELF read.
pub fn object_limits() -> object::ObjectReadLimits {
    object::ObjectReadLimits {
        max_input_bytes: MAX_INPUT,
        max_sections: 128,
        max_program_headers: 0,
        max_symbols: 256,
        max_relocations: 512,
        max_payload_bytes: 16384,
        max_string_bytes: 32768,
        max_work: 131072,
    }
}

fn check_object(bytes: &[u8]) -> bool {
    if let Some(model) = bytes.strip_prefix(b"WOM1") {
        return check_object_model(model);
    }
    let Ok(object) = object::ObjectFile::read_with_limits(bytes, object_limits()) else {
        return false;
    };
    object_canonical(&object)
}

fn check_object_model(bytes: &[u8]) -> bool {
    use object::{
        ObjectFile, ObjectFormat, ObjectRelocation, ObjectSymbol, RelocKind, SectionKind,
        SymbolBinding, SymbolKind, SymbolVisibility,
    };
    let field = |index: usize| bytes.get(index).copied().unwrap_or(0);
    let flags = field(0);
    let kind = match field(1) % 4 {
        0 => SectionKind::Text,
        1 => SectionKind::Data,
        2 => SectionKind::ReadOnlyData,
        _ => SectionKind::Bss,
    };
    let mut object = ObjectFile::new(ObjectFormat::ELF64);
    let name = if flags & 4 != 0 { "model\0section" } else { "model-section" };
    let align = if flags & 2 != 0 { 3 } else { 1u64 << (field(2) % 16) };
    let section = object.add_section(name, kind, align);
    if kind == SectionKind::Bss {
        object.sections[section].zero_fill = 1u64 << (field(2) % 63);
        if flags & 32 != 0 {
            object.sections[section].data.push(1);
        }
    } else {
        object.sections[section].data =
            if bytes.len() > 8 { bytes[8..bytes.len().min(72)].to_vec() } else { vec![0; 32] };
    }
    let symbol = if flags & 8 != 0 { "model\0symbol" } else { "model-symbol" };
    object.symbols.push(ObjectSymbol {
        name: symbol.into(),
        kind: match (field(1) >> 2) & 3 {
            1 => SymbolKind::Function,
            2 => SymbolKind::Object,
            _ => SymbolKind::NoType,
        },
        section_index: if flags & 128 != 0 {
            Some(999)
        } else if flags & 16 != 0 {
            None
        } else {
            Some(section)
        },
        value: if field(3) == 255 { u64::MAX } else { u64::from(field(3)) },
        size: u64::from(field(4)),
        binding: if field(1) & 64 != 0 {
            SymbolBinding::Local
        } else if flags & 1 != 0 {
            SymbolBinding::Weak
        } else {
            SymbolBinding::Global
        },
        visibility: if field(1) & 128 != 0 {
            SymbolVisibility::Hidden
        } else {
            SymbolVisibility::Default
        },
    });
    if flags & 64 != 0 {
        object.symbols.push(ObjectSymbol {
            name: symbol.into(),
            kind: SymbolKind::Object,
            section_index: Some(section),
            value: 0,
            size: 0,
            binding: SymbolBinding::Local,
            visibility: SymbolVisibility::Hidden,
        });
    }
    if field(5) != 255 {
        let kinds = [
            RelocKind::Absolute64,
            RelocKind::Absolute32,
            RelocKind::Relative32,
            RelocKind::Relative8,
            RelocKind::GOTPCREL,
            RelocKind::PLT32,
        ];
        object.relocations.push(ObjectRelocation {
            section_index: section,
            offset: if field(6) == 255 { usize::MAX } else { usize::from(field(6)) },
            symbol: symbol.into(),
            addend: i64::from(field(7) as i8),
            kind: kinds[usize::from(field(5)) % kinds.len()],
        });
    }
    if object.validate_diagnostic().is_err() {
        assert!(
            object.write_diagnostic_with_limit(65536).is_err(),
            "invalid public object model was serialized"
        );
        return false;
    }
    object_canonical(&object)
}

fn object_canonical(object: &object::ObjectFile) -> bool {
    use object::{ObjectErrorKind, ObjectResource};
    object.validate_diagnostic().expect("reader accepted invalid in-memory model");
    let bytes = match object.write_diagnostic_with_limit(65536) {
        Ok(bytes) => bytes,
        Err(error)
            if error.kind == ObjectErrorKind::ResourceLimit
                && error.resource == Some(ObjectResource::OutputBytes) =>
        {
            return false
        },
        Err(error) => panic!("valid bounded object failed serialization: {error:?}"),
    };
    let mut limits = object_limits();
    limits.max_input_bytes = 65536;
    limits.max_sections = 512;
    limits.max_symbols = 1024;
    limits.max_relocations = 1024;
    limits.max_payload_bytes = 32768;
    limits.max_string_bytes = 262144;
    limits.max_work = 1048576;
    let imported = object::ObjectFile::read_with_limits(&bytes, limits)
        .expect("valid writer output must be readable");
    assert_object_semantics(object, &imported);
    true
}

/// Writer table reordering, BSS compaction and zero-alignment normalization are
/// representation details; payloads and all link-relevant identities must remain.
fn assert_object_semantics(a: &object::ObjectFile, b: &object::ObjectFile) {
    assert_eq!(a.target, b.target);
    assert_eq!(a.sections.len(), b.sections.len());
    for (x, y) in a.sections.iter().zip(&b.sections) {
        assert_eq!(
            (&x.name, x.kind, x.memory_size().unwrap(), x.align.max(1)),
            (&y.name, y.kind, y.memory_size().unwrap(), y.align.max(1))
        );
        if x.kind != object::SectionKind::Bss {
            assert_eq!(x.data, y.data);
        }
    }
    let symbols = |o: &object::ObjectFile| {
        let mut result = o
            .symbols
            .iter()
            .map(|s| {
                (s.name.clone(), s.section_index, s.value, s.size, s.kind, s.binding, s.visibility)
            })
            .collect::<Vec<_>>();
        result.sort_by(|a, b| a.0.cmp(&b.0));
        result
    };
    // A relocation may introduce an implicit undefined symbol in legacy models.
    let mut expected = symbols(a);
    for relocation in &a.relocations {
        if !expected.iter().any(|s| s.0 == relocation.symbol) {
            expected.push((
                relocation.symbol.clone(),
                None,
                0,
                0,
                object::SymbolKind::NoType,
                object::SymbolBinding::Global,
                object::SymbolVisibility::Default,
            ));
        }
    }
    expected.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(expected, symbols(b));
    let relocations = |o: &object::ObjectFile| {
        let mut result = o
            .relocations
            .iter()
            .map(|r| {
                (r.section_index, r.offset, r.symbol.clone(), r.addend, format!("{:?}", r.kind))
            })
            .collect::<Vec<_>>();
        result.sort();
        result
    };
    assert_eq!(relocations(a), relocations(b));
}
