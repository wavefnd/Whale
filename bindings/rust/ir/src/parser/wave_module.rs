// SPDX-License-Identifier: MPL-2.0
//! Materialization of typed events from the Wave module grammar machine.

use super::{
    lexer::{Kind, Token},
    syntax::Parser,
    wave_text::RawToken,
    ParseError, ParseErrorKind,
};

use crate::*;

#[repr(C)]
#[derive(Default)]
struct State {
    phase: u64,
    at: u64,
    format: u64,
    nodes: u64,
    work: u64,
    a: u64,
    b: u64,
    c: u64,
    d: u64,
    e: u64,
    f: u64,
    g: u64,
    name: u64,
    link: u64,
    terminated: u64,
}

#[repr(C)]
#[derive(Default)]
struct Event {
    status: u64,
    reason: u64,
    at: u64,
    expected: u64,
    event: u64,
    role: u64,
    a: u64,
    b: u64,
    c: u64,
    d: u64,
    e: u64,
    f: u64,
    g: u64,
    name: u64,
    link: u64,
    next: u64,
    nodes: u64,
    work: u64,
}

unsafe extern "C" {
    fn whale_ir_module_step(
        tokens: *const RawToken,
        token_count: u64,
        state: *mut State,
        max_nodes: u64,
        max_work: u64,
        result: *mut Event,
    ) -> i32;
}

const WORDS: &[&str] = &[
    "}",
    ":",
    "=",
    "const",
    ",",
    "align",
    "init_expr",
    "value",
    "uninitialized",
    "access",
    "readonly",
    "readwrite",
    "linkage",
    "internal",
    "external",
    "link_name",
    "whale",
    "sysv64",
    "(",
    ")",
    "->",
    "entry",
    "{",
    "source_file",
    "original",
    "none",
    "source_map",
    "instruction",
    "terminator",
    "global",
    "storage",
    "declare",
    "fn",
    "br",
    "cbr",
    "switch",
    "ret",
    "trap",
];

fn error(tokens: &[Token<'_>], result: &Event, limits: IrLimits) -> ParseError {
    let index = (result.at as usize).min(tokens.len() - 1);
    let found = match &tokens[index].kind {
        Kind::Word(word) | Kind::Mark(word) => *word,
        Kind::String(_) => "<string>",
    };
    let kind = match result.status {
        3 => ParseErrorKind::ResourceLimit(LimitError {
            resource: Resource::Nodes,
            limit: limits.max_nodes,
        }),
        4 => ParseErrorKind::ResourceLimit(LimitError {
            resource: Resource::AnalysisWork,
            limit: limits.max_analysis_work,
        }),
        1 if (1..=17).contains(&result.reason)
            && (result.reason != 1 || (result.expected as usize) < WORDS.len())
            && (result.reason != 4 || result.expected < 5) =>
        {
            ParseErrorKind::Syntax(match result.reason {
                1 => format!(
                    "expected '{}', found '{found}'",
                    WORDS.get(result.expected as usize).unwrap_or(&"?")
                ),
                2 => "expected unsigned decimal integer".into(),
                3 => "integer out of range".into(),
                4 => format!(
                    "expected {} ID",
                    ["%v", "%b", "@g", "@f", "@s"].get(result.expected as usize).unwrap_or(&"?")
                ),
                5 => "invalid numeric ID".into(),
                6 => "ID out of range".into(),
                7 => "expected quoted string".into(),
                8 => "expected word".into(),
                9 => "source metadata requires IR format 7".into(),
                10 => "global storage requires typed IR format 6".into(),
                11 => "unknown source mapping target".into(),
                12 => "expected value or uninitialized global storage".into(),
                13 => "expected readonly or readwrite".into(),
                14 => "unknown linkage".into(),
                15 => "unknown calling convention".into(),
                16 => "unknown module field".into(),
                17 => "instruction after terminator".into(),
                _ => "invalid Wave IR module result".into(),
            })
        },
        _ => ParseErrorKind::Backend("module parser transport"),
    };
    ParseError { location: tokens[index].location, kind }
}

impl Parser<'_> {
    fn module_string(&mut self, index: u64) -> Result<String, ParseError> {
        match self.tokens.get_mut(index as usize) {
            Some(Token { kind: Kind::String(string), .. }) => Ok(std::mem::take(string)),
            _ => Err(self.backend("module parser transport")),
        }
    }

    pub(super) fn module_wave(&mut self) -> Result<Module, ParseError> {
        let (target, layout, version, next) = super::wave_header::parse(&self.tokens)?;
        self.format_version = version;
        self.at = next;
        let mut module = Module::new(target, layout);
        let mut state = State { at: next as u64, format: u64::from(version), ..State::default() };
        let mut ty = None;
        let mut literal = None;
        let mut expression = None;
        let mut signature_params = Vec::new();
        let mut signature_return = None;
        let mut parameter_type = None;
        let mut function: Option<Function> = None;
        let mut block: Option<BasicBlock> = None;
        loop {
            state.at = self.at as u64;
            let nodes = self.limits.max_nodes.saturating_sub(self.nodes);
            let work = self.limits.max_analysis_work.saturating_sub(self.grammar_work);
            let mut event = Event::default();
            let status = unsafe {
                whale_ir_module_step(
                    self.raw_tokens.as_ptr(),
                    self.raw_tokens.len() as u64,
                    &mut state,
                    nodes as u64,
                    work as u64,
                    &mut event,
                )
            };
            if status != event.status as i32
                || event.nodes > nodes as u64
                || event.work > work as u64
                || event.next >= self.tokens.len() as u64
            {
                return Err(self.backend("module parser transport"));
            }
            if status != 0 {
                return Err(error(&self.tokens, &event, self.limits));
            }
            self.at = event.next as usize;
            self.nodes += event.nodes as usize;
            self.grammar_work += event.work as usize;
            match event.event {
                100 => {
                    let parsed = self.ty(0)?;
                    match event.role {
                        1 | 3 => ty = Some(parsed),
                        2 | 4 => self.equal_type(
                            ty.as_ref().ok_or_else(|| self.backend("module parser transport"))?,
                            &parsed,
                        )?,
                        5 => signature_params.push(parsed),
                        6 => signature_return = Some(parsed),
                        7 => parameter_type = Some(parsed),
                        8 => {
                            function
                                .as_mut()
                                .ok_or_else(|| self.backend("module parser transport"))?
                                .ret_ty = parsed
                        },
                        _ => return Err(self.backend("module parser transport")),
                    }
                },
                101 => {
                    literal = Some(self.literal(
                        ty.as_ref().ok_or_else(|| self.backend("module parser transport"))?,
                    )?)
                },
                102 => expression = Some(self.expression(0)?),
                103 => {
                    let (instruction, result) = self.instruction()?;
                    if let Some(result) = result {
                        function
                            .as_mut()
                            .ok_or_else(|| self.backend("module parser transport"))?
                            .value_types
                            .push(result);
                    }
                    block
                        .as_mut()
                        .ok_or_else(|| self.backend("module parser transport"))?
                        .instructions
                        .push(instruction);
                },
                104 => {
                    let terminator = self.terminator()?;
                    block
                        .as_mut()
                        .ok_or_else(|| self.backend("module parser transport"))?
                        .terminator = Some(terminator);
                },
                1 => {
                    let name = self.module_string(event.name)?;
                    module.sources.files.push(SourceFile { id: event.a as u32, name });
                },
                2 => module.sources.originals.push(OriginalNode {
                    id: event.a,
                    span: (event.g != 0).then_some(SourceSpan {
                        file: event.b as u32,
                        line: event.c as u32,
                        column: event.d as u32,
                        end_line: event.e as u32,
                        end_column: event.f as u32,
                    }),
                }),
                3 => {
                    let target = match event.a {
                        0 => SourceTarget::Instruction {
                            function: FunctionId(event.b as u32),
                            block: BlockId(event.c as u32),
                            index: event.d as usize,
                        },
                        1 => SourceTarget::Terminator {
                            function: FunctionId(event.b as u32),
                            block: BlockId(event.c as u32),
                        },
                        2 => SourceTarget::Global(GlobalId(event.b as u32)),
                        _ => return Err(self.backend("module parser transport")),
                    };
                    module.sources.mappings.push(SourceMapping { target, original: event.e });
                },
                4 => {
                    let name = self.module_string(event.name)?;
                    let id = GlobalId(event.a as u32);
                    let location = self.tokens[event.name as usize].location;
                    self.global_locations.entry(name.clone()).or_insert(location);
                    self.global_constant_locations.entry(id).or_insert(location);
                    module.globals.push(Global {
                        id,
                        name,
                        ty: ty.take().ok_or_else(|| self.backend("module parser transport"))?,
                        init: literal
                            .take()
                            .ok_or_else(|| self.backend("module parser transport"))?,
                        init_expr: expression
                            .take()
                            .ok_or_else(|| self.backend("module parser transport"))?,
                        align: event.b as u32,
                    });
                },
                5 => {
                    let name = self.module_string(event.name)?;
                    let location = self.tokens[event.name as usize].location;
                    self.global_locations.entry(name.clone()).or_insert(location);
                    let link_name = if event.link == u64::MAX {
                        None
                    } else {
                        Some(self.module_string(event.link)?)
                    };
                    module.global_storage.push(GlobalStorage {
                        id: GlobalId(event.a as u32),
                        name,
                        ty: ty.take().ok_or_else(|| self.backend("module parser transport"))?,
                        init: if event.e == 0 {
                            GlobalStorageInit::Uninitialized
                        } else {
                            GlobalStorageInit::Value(
                                literal
                                    .take()
                                    .ok_or_else(|| self.backend("module parser transport"))?,
                            )
                        },
                        align: event.b as u32,
                        readonly: event.c != 0,
                        linkage: if event.d == 0 { Linkage::Internal } else { Linkage::External },
                        link_name,
                    });
                },
                6 => {
                    let name = self.module_string(event.name)?;
                    let location = self.tokens[event.name as usize].location;
                    self.declaration_locations.entry(name.clone()).or_insert(location);
                    let link_name = if event.link == u64::MAX {
                        None
                    } else {
                        Some(self.module_string(event.link)?)
                    };
                    module.declarations.push(FunctionDecl {
                        id: FunctionId(event.a as u32),
                        name,
                        signature: FunctionSignature {
                            params: std::mem::take(&mut signature_params),
                            ret: signature_return
                                .take()
                                .ok_or_else(|| self.backend("module parser transport"))?,
                            convention: if event.c == 0 {
                                CallingConvention::Whale
                            } else {
                                CallingConvention::SysV64
                            },
                            variadic: false,
                        },
                        linkage: if event.d == 0 { Linkage::Internal } else { Linkage::External },
                        link_name,
                    });
                },
                7 => {
                    let name = self.module_string(event.name)?;
                    let location = self.tokens[event.name as usize].location;
                    self.function_locations.entry(name.clone()).or_insert(location);
                    function = Some(Function {
                        id: FunctionId(event.a as u32),
                        name,
                        params: Vec::new(),
                        ret_ty: Type::Void,
                        blocks: Vec::new(),
                        entry: BlockId(0),
                        value_types: Vec::new(),
                    });
                },
                8 => {
                    let name = self.module_string(event.name)?;
                    let id = ValueId(event.a as u32);
                    let ty = parameter_type
                        .take()
                        .ok_or_else(|| self.backend("module parser transport"))?;
                    let function =
                        function.as_mut().ok_or_else(|| self.backend("module parser transport"))?;
                    function.value_types.push((id, ty.clone()));
                    function.params.push(Param { id, name, ty });
                },
                9 => {
                    function
                        .as_mut()
                        .ok_or_else(|| self.backend("module parser transport"))?
                        .entry = BlockId(event.a as u32)
                },
                10 => {
                    block = Some(BasicBlock::new(
                        BlockId(event.a as u32),
                        self.module_string(event.name)?,
                    ))
                },
                12 => {
                    let completed =
                        block.take().ok_or_else(|| self.backend("module parser transport"))?;
                    function
                        .as_mut()
                        .ok_or_else(|| self.backend("module parser transport"))?
                        .blocks
                        .push(completed);
                },
                13 => module
                    .functions
                    .push(function.take().ok_or_else(|| self.backend("module parser transport"))?),
                14 => return Ok(module),
                _ => return Err(self.backend("module parser transport")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PREFIX: &str = "module { format_version 8 semantics_version 1 target \"x86_64-whale-linux\" datalayout { ptr=64, endian=little } ";

    fn compare(source: &str, limits: IrLimits) {
        let tokens = match super::super::lexer::lex(source, limits) {
            Ok(tokens) => tokens,
            Err(_) => return,
        };
        let mut wave = Parser::new(tokens, limits);
        let tokens = super::super::lexer::lex(source, limits).unwrap();
        let mut reference = Parser::new(tokens, limits);
        reference.reference_types = true;
        match (wave.module(), reference.module()) {
            (Ok(actual), Ok(expected)) => {
                assert_eq!(actual, expected, "{source:?}");
                assert_eq!(wave.at, reference.at, "{source:?}");
                assert_eq!(wave.nodes, reference.nodes, "{source:?}");
            },
            (Err(actual), Err(expected)) => {
                assert_eq!(actual.location, expected.location, "{source:?}\n{actual}\n{expected}");
                assert_eq!(actual.to_string(), expected.to_string(), "{source:?}");
            },
            (actual, expected) => {
                panic!("module grammar disagreement: {source:?}\n{actual:?}\n{expected:?}")
            },
        }
    }

    #[test]
    fn wave_outer_grammar_preserves_modules_versions_and_exact_error_locations() {
        let fields = [
            "",
            "source_file @s4 \"src.whale\" original 7 @s4 1 2 1 3 source_map global @g2 7",
            "global @g2 \"c\": i32 = const i32 7, align 4, init_expr i32 7",
            "storage @g8 \"mutable\": ptr<void> = uninitialized, align 8, access readwrite, linkage internal",
            "storage @g9 \"constant\": u32 = value u32 17, align 4, access readonly, linkage external, link_name \"published\"",
            "declare @f3 \"f\": whale (i32, ptr<void>) -> i32, linkage external, link_name \"f\"",
            "declare @f3 \"f\": whale () -> void, linkage internal fn @f3 \"f\"() -> void, entry %b8 { %b8 \"entry\": ret void }",
            "fn @f3 \"f\"(%v7 \"a\": i32, %v9 \"b\": ptr<void>) -> i32, entry %b8 { %b8 \"entry\": %v12: i32 = mov i32 %v7 ret i32 %v12 %b9 \"dead\": ret i32 %v7 }",
            "unknown", "source_file", "storage @g0 \"x\": i32 = unknown", "fn @f0 \"f\"(,)",
            "original 18446744073709551616 none", "source_file @s4294967296 \"x\"",
            "source_map invalid @g0 1", "declare @f0 \"f\": invalid () -> void, linkage internal",
            "fn @f0 \"f\"() -> void, entry %b0 { %b0 \"b\": ret void ret void }",
            "fn @f0 \"f\"(%v0 \"a\": i32,) -> void, entry %b0 {}",
            "global @g0 \"x\": array<i32,01> = const array<i32,1> 1, align 4, init_expr i32 1",
        ];
        for field in fields {
            let source = format!("{PREFIX}{field} }}");
            compare(&source, IrLimits::default());
            for version in 3..=8 {
                compare(
                    &source.replace("format_version 8", &format!("format_version {version}")),
                    IrLimits::default(),
                );
            }
            for end in source.char_indices().map(|(index, _)| index) {
                compare(&source[..end], IrLimits::default());
            }
            for max_nodes in 0..16 {
                compare(&source, IrLimits { max_nodes, ..IrLimits::default() });
            }
        }
        let mut state = 0x349_501u64;
        for _ in 0..2000 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let count = state as usize % 12;
            let mut source = PREFIX.to_string();
            for index in 0..count {
                source.push_str(fields[(state as usize + index) % fields.len()]);
                source.push(' ');
            }
            source.push('}');
            compare(&source, IrLimits::default());
        }
    }
}
