// SPDX-License-Identifier: MPL-2.0
//! Borrowed token packets and typed AST materialization for the native grammar.
use crate::ast_model::{DirectiveRecord, Expr, Item, Name, OperandRecord, MISSING};

use assembler::ast::*;

use assembler::error::SourceSpan;

use assembler::tokens::{Token, TokenKind};

use assembler::{AsmError, SymbolKind, SymbolVisibility};

use std::{cell::Cell, collections::HashMap};

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RawToken {
    kind: u64,
    position: u64,
    end: u64,
    text: Name,
    low: u64,
    high: u64,
}

#[repr(C)]
struct Input {
    tokens: *const RawToken,
    count: u64,
    mode: u64,
    max_work: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Span {
    start: u64,
    end: u64,
}

#[repr(C)]
struct Output {
    items: *mut Item,
    item_capacity: u64,
    spans: *mut Span,
    span_capacity: u64,
    operands: *mut OperandRecord,
    operand_capacity: u64,
    values: *mut DirectiveRecord,
    value_capacity: u64,
    names: *mut Name,
    name_capacity: u64,
    bytes: *mut u8,
    byte_capacity: u64,
}

impl Default for Output {
    fn default() -> Self {
        Self {
            items: std::ptr::null_mut(),
            item_capacity: 0,
            spans: std::ptr::null_mut(),
            span_capacity: 0,
            operands: std::ptr::null_mut(),
            operand_capacity: 0,
            values: std::ptr::null_mut(),
            value_capacity: 0,
            names: std::ptr::null_mut(),
            name_capacity: 0,
            bytes: std::ptr::null_mut(),
            byte_capacity: 0,
        }
    }
}

#[repr(C)]
#[derive(Default)]
struct ResultRecord {
    status: u64,
    error: u64,
    index: u64,
    detail: u64,
    items: u64,
    operands: u64,
    values: u64,
    names: u64,
    bytes: u64,
    work: u64,
}

unsafe extern "C" {
    fn whale_amd64_parse(
        input: *const Input,
        output: *mut Output,
        result: *mut ResultRecord,
    ) -> i32;
}

fn backend() -> AsmError {
    AsmError::Backend("AMD64 parser transport")
}

fn initialized<T: Clone + Default>(count: u64) -> Result<Vec<T>, AsmError> {
    let count = usize::try_from(count).map_err(|_| backend())?;
    let mut values = Vec::new();
    values.try_reserve_exact(count).map_err(|_| backend())?;
    values.resize(count, T::default());
    Ok(values)
}

fn source_span(tokens: &[Token], position: u64) -> Result<SourceSpan, AsmError> {
    let position = usize::try_from(position).map_err(|_| backend())?;
    if let Some(token) = tokens.get(position) {
        return Ok(token.span());
    }
    if position != tokens.len() {
        return Err(backend());
    }
    Ok(match tokens.last() {
        Some(token) => SourceSpan {
            start: token.end_position,
            end: token.end_position,
            line: token.end_line,
            column: token.end_column,
        },
        None => SourceSpan { start: 0, end: 0, line: 1, column: 1 },
    })
}

fn grammar_error(tokens: &[Token], result: &ResultRecord) -> Result<AsmError, AsmError> {
    let message = match result.error {
        2 => "Unexpected token at top level",
        3 => "Expected constant name",
        4 => "Expected 'equ'",
        5 => match result.detail {
            0 => "cwd expects 0 operands",
            1 => "cdq expects 0 operands",
            2 => "cqo expects 0 operands",
            _ => return Err(backend()),
        },
        6 => "Expected mnemonic",
        7 => "Expected end of statement",
        8 => "Expected operand after comma",
        9 => "Width qualifier requires memory operand",
        10 => "Missing closing ']' in memory operand",
        11 => "Negative register or symbol terms are not supported in memory operands",
        12 => "Scale must be 1/2/4/8",
        13 => "Too many registers in memory operand",
        14 => "Multiple symbols in memory operand",
        15 => "Memory displacement overflow",
        16 => "Expected term in memory operand",
        17 => "rel/abs address requires a symbol or numeric displacement without registers",
        18 => "Expected '+', '-' or ']' in memory operand",
        19 => "Expected directive name",
        20 => "Expected symbol name",
        21 => "Expected comma in symbol declaration",
        22 => "Expected symbol kind function, object or notype",
        23 => "Expected symbol visibility default or hidden",
        24 => "Expected directive name argument",
        25 => "Expected section kind text/data/rodata/bss",
        26 => "Expected directive value",
        27 => "Expected directive value after comma",
        28 => "Expression arithmetic overflow",
        29 => "Unary '-' before symbol is not supported",
        30 => "Expected expression term",
        31 => "Expected '+' or '-' in expression",
        32 => "Expression cannot end with operator",
        33 => "Expected expression",
        34 => "64-bit expression overflow",
        35 => "Symbol addend overflow",
        36 => "Expressions with multiple symbols are not supported",
        _ => return Err(backend()),
    };
    Ok(AsmError::ParserError(message.into()).at(
        "<input>",
        source_span(tokens, result.index)?,
        None,
    ))
}

struct Materializer<'a> {
    names: &'a [Name],
    bytes: &'a [u8],
    identifiers: HashMap<(usize, u64), &'a [u8]>,
    strings: HashMap<(usize, u64), &'a [u8]>,
    remaining_bytes: Cell<u64>,
}

impl<'a> Materializer<'a> {
    fn new(tokens: &'a [Token], names: &'a [Name], bytes: &'a [u8]) -> Result<Self, AsmError> {
        let mut identifiers = HashMap::new();
        let mut strings = HashMap::new();
        identifiers.try_reserve(tokens.len()).map_err(|_| backend())?;
        strings.try_reserve(tokens.len()).map_err(|_| backend())?;
        for token in tokens {
            match &token.kind {
                TokenKind::Identifier(text) => {
                    identifiers
                        .insert((text.as_ptr() as usize, text.len() as u64), text.as_bytes());
                },
                TokenKind::StringLiteral(bytes) => {
                    strings.insert((bytes.as_ptr() as usize, bytes.len() as u64), bytes.as_slice());
                },
                _ => {},
            }
        }
        let payload = tokens.iter().try_fold(0u64, |total, token| {
            let length = match &token.kind {
                TokenKind::Identifier(text) => text.len(),
                TokenKind::StringLiteral(bytes) => bytes.len(),
                _ => 0,
            };
            total.checked_add(length as u64).ok_or_else(backend)
        })?;
        Ok(Self { names, bytes, identifiers, strings, remaining_bytes: Cell::new(payload) })
    }

    fn take_bytes(&self, count: u64) -> Result<(), AsmError> {
        self.remaining_bytes
            .set(self.remaining_bytes.get().checked_sub(count).ok_or_else(backend)?);
        Ok(())
    }

    fn name(&self, id: u64) -> Result<String, AsmError> {
        let name =
            self.names.get(usize::try_from(id).map_err(|_| backend())?).ok_or_else(backend)?;
        let address = name.bytes as usize;
        let count = usize::try_from(name.count).map_err(|_| backend())?;
        let start = self.bytes.as_ptr() as usize;
        let arena = address
            .checked_sub(start)
            .and_then(|offset| offset.checked_add(count).map(|end| (offset, end)))
            .and_then(|(offset, end)| self.bytes.get(offset..end));
        let bytes = if count == 0 {
            &[]
        } else {
            arena
                .or_else(|| self.identifiers.get(&(address, name.count)).copied())
                .ok_or_else(backend)?
        };
        let text = std::str::from_utf8(bytes).map_err(|_| backend())?;
        self.take_bytes(name.count)?;
        let mut owned = String::new();
        owned.try_reserve_exact(text.len()).map_err(|_| backend())?;
        owned.push_str(text);
        Ok(owned)
    }

    fn optional_name(&self, id: u64) -> Result<Option<String>, AsmError> {
        if id == MISSING {
            Ok(None)
        } else {
            self.name(id).map(Some)
        }
    }

    fn expression(&self, raw: Expr) -> Result<ExprValue, AsmError> {
        let value = bits(raw.low, raw.high);
        match raw.kind {
            0 if raw.name == MISSING => Ok(ExprValue::Number(value)),
            1 => Ok(ExprValue::Symbol { name: self.name(raw.name)?, addend: value }),
            _ => Err(backend()),
        }
    }

    fn operand(&self, raw: OperandRecord) -> Result<Operand, AsmError> {
        Ok(match raw.kind {
            0 => Operand::Register(self.name(raw.name)?),
            1 => Operand::Immediate(bits(raw.low, raw.high)),
            2 => Operand::Label(self.name(raw.name)?),
            3 => {
                Operand::SymbolExpr { name: self.name(raw.name)?, addend: bits(raw.low, raw.high) }
            },
            4 => Operand::Memory(MemoryOperand {
                address_mode: match raw.mode {
                    0 => MemoryAddressMode::Default,
                    1 => MemoryAddressMode::Relative,
                    2 => MemoryAddressMode::Absolute,
                    _ => return Err(backend()),
                },
                width: if raw.width == MISSING {
                    None
                } else {
                    Some(u8::try_from(raw.width).map_err(|_| backend())?)
                },
                base: self.optional_name(raw.base)?,
                index: self.optional_name(raw.index)?,
                scale: u8::try_from(raw.scale).map_err(|_| backend())?,
                disp: raw.disp as i64,
                symbol: self.optional_name(raw.symbol)?,
            }),
            _ => return Err(backend()),
        })
    }

    fn directive_value(&self, raw: DirectiveRecord) -> Result<DirectiveValue, AsmError> {
        match raw.kind {
            0 => self.expression(raw.expression).map(DirectiveValue::Expr),
            1 => {
                let bytes = self
                    .strings
                    .get(&(raw.bytes as usize, raw.count))
                    .copied()
                    .ok_or_else(backend)?;
                self.take_bytes(raw.count)?;
                let mut owned = Vec::new();
                owned.try_reserve_exact(bytes.len()).map_err(|_| backend())?;
                owned.extend_from_slice(bytes);
                Ok(DirectiveValue::StringLiteral(owned))
            },
            _ => Err(backend()),
        }
    }
}

fn bits(low: u64, high: u64) -> i128 {
    (u128::from(low) | (u128::from(high) << 64)) as i128
}

fn range<T>(values: &[T], first: u64, count: u64) -> Result<&[T], AsmError> {
    let first = usize::try_from(first).map_err(|_| backend())?;
    let end =
        first.checked_add(usize::try_from(count).map_err(|_| backend())?).ok_or_else(backend)?;
    values.get(first..end).ok_or_else(backend)
}

pub(super) fn parse(tokens: &[Token]) -> Result<AST, AsmError> {
    let mut raw = Vec::new();
    raw.try_reserve_exact(tokens.len()).map_err(|_| backend())?;
    let mut payload_bytes = 0u64;
    for token in tokens {
        let mut native = RawToken {
            position: token.position as u64,
            end: token.end_position as u64,
            ..RawToken::default()
        };
        native.kind = match &token.kind {
            TokenKind::Identifier(text) => {
                native.text = Name { bytes: text.as_ptr(), count: text.len() as u64 };
                0
            },
            TokenKind::Number(value) => {
                native.low = *value as u128 as u64;
                native.high = ((*value as u128) >> 64) as u64;
                1
            },
            TokenKind::StringLiteral(bytes) => {
                native.text = Name { bytes: bytes.as_ptr(), count: bytes.len() as u64 };
                2
            },
            TokenKind::Comma => 3,
            TokenKind::Colon => 4,
            TokenKind::LBracket => 5,
            TokenKind::RBracket => 6,
            TokenKind::Plus => 7,
            TokenKind::Minus => 8,
            TokenKind::Multiply => 9,
            TokenKind::Divide => 10,
            TokenKind::Newline => 11,
        };
        payload_bytes = payload_bytes.checked_add(native.text.count).ok_or_else(backend)?;
        raw.push(native);
    }
    let max_work = (tokens.len() as u64)
        .checked_add(payload_bytes)
        .and_then(|n| n.checked_add(1))
        .and_then(|n| n.checked_mul(16))
        .ok_or_else(backend)?;
    let mut input = Input { tokens: raw.as_ptr(), count: raw.len() as u64, mode: 0, max_work };
    let mut probe = ResultRecord::default();
    let status = unsafe { whale_amd64_parse(&input, &mut Output::default(), &mut probe) };
    if status != probe.status as i32 || probe.work > max_work {
        return Err(backend());
    }
    match status {
        0 => {},
        1 => return Err(grammar_error(tokens, &probe)?),
        _ => return Err(backend()),
    }
    if [probe.items, probe.operands, probe.values, probe.names]
        .iter()
        .any(|count| *count > raw.len() as u64)
        || probe.bytes > payload_bytes
    {
        return Err(backend());
    }
    let mut items = initialized::<Item>(probe.items)?;
    let mut spans = initialized::<Span>(probe.items)?;
    let mut operands = initialized::<OperandRecord>(probe.operands)?;
    let mut values = initialized::<DirectiveRecord>(probe.values)?;
    let mut names = initialized::<Name>(probe.names)?;
    let mut bytes = initialized::<u8>(probe.bytes)?;
    let mut output = Output {
        items: items.as_mut_ptr(),
        item_capacity: items.len() as u64,
        spans: spans.as_mut_ptr(),
        span_capacity: spans.len() as u64,
        operands: operands.as_mut_ptr(),
        operand_capacity: operands.len() as u64,
        values: values.as_mut_ptr(),
        value_capacity: values.len() as u64,
        names: names.as_mut_ptr(),
        name_capacity: names.len() as u64,
        bytes: bytes.as_mut_ptr(),
        byte_capacity: bytes.len() as u64,
    };
    input.mode = 1;
    let mut result = ResultRecord::default();
    let status = unsafe { whale_amd64_parse(&input, &mut output, &mut result) };
    if status != 0
        || result.status != 0
        || result.items != probe.items
        || result.operands != probe.operands
        || result.values != probe.values
        || result.names != probe.names
        || result.bytes != probe.bytes
        || result.work != probe.work
    {
        return Err(backend());
    }
    let materializer = Materializer::new(tokens, &names, &bytes)?;
    let mut ast_items = Vec::new();
    let mut ast_spans = Vec::new();
    ast_items.try_reserve_exact(items.len()).map_err(|_| backend())?;
    ast_spans.try_reserve_exact(items.len()).map_err(|_| backend())?;
    for (item, span) in items.into_iter().zip(spans) {
        let node = match item.kind {
            0 => {
                let records = range(&operands, item.first, item.count)?;
                let mut operands = Vec::new();
                operands.try_reserve_exact(records.len()).map_err(|_| backend())?;
                for record in records {
                    operands.push(materializer.operand(*record)?);
                }
                ASTNode::Instruction(Instruction {
                    mnemonic: materializer.name(item.name)?,
                    operands,
                })
            },
            1 => ASTNode::SymbolMetadata {
                name: materializer.name(item.name)?,
                kind: match item.aux {
                    0 => SymbolKind::NoType,
                    1 => SymbolKind::Function,
                    2 => SymbolKind::Object,
                    _ => return Err(backend()),
                },
                size: materializer.expression(item.expression)?,
                visibility: match item.flags {
                    0 => SymbolVisibility::Default,
                    1 => SymbolVisibility::Hidden,
                    _ => return Err(backend()),
                },
            },
            2 => {
                let records = range(&values, item.first, item.count)?;
                let mut values = Vec::new();
                values.try_reserve_exact(records.len()).map_err(|_| backend())?;
                for record in records {
                    values.push(materializer.directive_value(*record)?);
                }
                ASTNode::Directive(Directive { name: materializer.name(item.name)?, values })
            },
            3 => ASTNode::Label(materializer.name(item.name)?),
            4 => ASTNode::Section(materializer.name(item.name)?),
            5 => ASTNode::SectionAttributes {
                name: materializer.name(item.name)?,
                kind: match item.aux {
                    0 => AsmSectionKind::Text,
                    1 => AsmSectionKind::Data,
                    2 => AsmSectionKind::ReadOnlyData,
                    3 => AsmSectionKind::Bss,
                    _ => return Err(backend()),
                },
            },
            6 => ASTNode::Global(materializer.name(item.name)?),
            7 => ASTNode::Extern(materializer.name(item.name)?),
            8 => ASTNode::Const {
                name: materializer.name(item.name)?,
                expr: materializer.expression(item.expression)?,
            },
            _ => return Err(backend()),
        };
        if span.end <= span.start {
            return Err(backend());
        }
        let first =
            tokens.get(usize::try_from(span.start).map_err(|_| backend())?).ok_or_else(backend)?;
        let last = tokens
            .get(usize::try_from(span.end - 1).map_err(|_| backend())?)
            .ok_or_else(backend)?;
        let mut source = first.span();
        source.end = last.end_position;
        ast_items.push(node);
        ast_spans.push(source);
    }
    Ok(AST { items: ast_items, spans: ast_spans, source_name: "<input>".into() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compare(tokens: &[Token]) {
        let actual = parse(tokens);
        let expected = crate::parser::reference::parse(tokens);
        match (actual, expected) {
            (Ok(actual), Ok(expected)) => {
                assert_eq!(format!("{actual:?}"), format!("{expected:?}"), "tokens {tokens:?}")
            },
            (Err(actual), Err(expected)) => {
                assert_eq!(actual.to_string(), expected.to_string(), "tokens {tokens:?}")
            },
            (actual, expected) => {
                panic!("parser divergence for {tokens:?}: {actual:?} / {expected:?}")
            },
        }
    }

    fn source(text: &str) {
        compare(&assembler::tokens::tokenize(text).unwrap());
    }

    #[test]
    fn native_parser_preserves_every_supported_statement_and_source_range() {
        source("\nSECTION .text, TEXT\r\nGLOBAL Entry\nextern external\nEntry: .L: MOV RAX, qword [rel external + 4]\nVALUE EQU - 1 + 5\ndb \"a\\0\\xFF\", VALUE+2\nsymbol Entry, function, 1 + 2, hidden\nsection .buffer, bss\nresq 8\n");
        for register in crate::tables::REGISTERS_64
            .iter()
            .chain(crate::tables::REGISTERS_32)
            .chain(crate::tables::REGISTERS_16)
            .chain(crate::tables::REGISTERS_8)
            .chain(crate::tables::REGISTERS_XMM)
        {
            source(&format!(
                "unknown {}, {}\nmov rax, [{} + 4]\n",
                register.0.to_ascii_uppercase(),
                register.0,
                register.0
            ));
        }
        for memory in [
            "[rel]",
            "[abs+4]",
            "[rel +4]",
            "[abs -4]",
            "[REL external+8]",
            "[rax+r9*8+external-7]",
            "[r9*2+rbx]",
            "[external+rax]",
            "[rax]",
            "[-9223372036854775808]",
            "[2147483648]",
            "[eax+ecx*4]",
        ] {
            source(&format!("mov rax, qword {memory}\n"));
        }
        for directive in [
            "align 16",
            "db 1,2,\"xy\"",
            "dw -1",
            "dd external+4",
            "dq 18446744073709551615",
            "resb 4",
            "resw 4",
            "resd 4",
            "resq 4",
            "section custom",
            "section custom, rodata",
            "global exported",
            "extern imported",
            "symbol exported, notype, 0, default",
            "symbol exported, object, external+4, hidden",
        ] {
            source(directive);
        }
        source(
            "mov eax, rax+0\nmov rax, - - external + 1\nname equ 18446744073709551615 + 2 - 3\n",
        );
        source("cwd\nCDQ\ncqo\nret");
        source("라벨: db \"안녕\", 기호+4\n");
    }

    #[test]
    fn native_parser_matches_reference_errors_and_exact_eof_locations() {
        for text in [
            "1",
            "]",
            "mov ,",
            "mov rax,",
            "mov rax,\n",
            "cwd rax",
            "CDQ ,",
            "cqo 1",
            "mov byte rax",
            "mov []",
            "mov [rax",
            "mov [-rax]",
            "mov [rax*3]",
            "mov [rax+rbx+rcx]",
            "mov [foo+bar]",
            "mov [9223372036854775808]",
            "mov [rax 1]",
            "mov [rel rax]",
            "mov [rax,rbx]",
            "mov 1 2",
            "mov foo+bar",
            "mov -foo",
            "mov +",
            "mov 1+",
            "mov 18446744073709551615+1",
            "section",
            "global 1",
            "section .x, nope",
            "section .x extra",
            "symbol",
            "symbol foo function",
            "symbol foo, nope, 0, hidden",
            "symbol foo, object, 1",
            "symbol foo, object, 1, nope",
            "symbol foo, object, , hidden",
            "db",
            "db 1,",
            "db 1,\r\n",
            "name equ",
            "name equ 1+",
            "mov [rax+--4]",
        ] {
            source(text);
        }
        compare(&[]);
    }

    fn manual(kinds: Vec<TokenKind>) {
        let tokens: Vec<_> = kinds
            .into_iter()
            .enumerate()
            .map(|(index, kind)| Token {
                kind,
                position: 100 + index * 7,
                end_position: 104 + index * 7,
                line: 9,
                column: 2 + index,
                end_line: 9,
                end_column: 3 + index,
            })
            .collect();
        compare(&tokens);
    }

    #[test]
    fn manual_token_full_signed128_values_keep_checked_arithmetic_and_spans() {
        for value in
            [i128::MIN, i128::MIN + 1, i64::MIN as i128, -1, 0, 1, u64::MAX as i128, i128::MAX]
        {
            for sign in [TokenKind::Plus, TokenKind::Minus] {
                manual(vec![TokenKind::Identifier("mov".into()), sign, TokenKind::Number(value)]);
            }
            for right in [i128::MIN, -1, 1, i128::MAX] {
                manual(vec![
                    TokenKind::Identifier("x".into()),
                    TokenKind::Identifier("EQU".into()),
                    TokenKind::Number(value),
                    TokenKind::Plus,
                    TokenKind::Number(right),
                ]);
                manual(vec![
                    TokenKind::Identifier("mov".into()),
                    TokenKind::Identifier("symbol".into()),
                    TokenKind::Plus,
                    TokenKind::Number(value),
                    TokenKind::Plus,
                    TokenKind::Number(right),
                ]);
            }
        }
        manual(vec![TokenKind::Identifier("mov".into()), TokenKind::Divide]);
        manual(vec![TokenKind::Identifier(String::new()), TokenKind::Identifier("RAX".into())]);
    }

    #[test]
    fn deterministic_token_mutations_match_independent_parser() {
        let atoms = [
            "mov",
            "RAX",
            "word",
            "rax",
            "rel",
            "abs",
            "xmm15",
            "name",
            "equ",
            "section",
            "symbol",
            "db",
            "1",
            "18446744073709551615",
            "+",
            "-",
            "*",
            "[",
            "]",
            ",",
            ":",
            "\n",
            "\"hi\"",
        ];
        let mut random = 0x0357_1280_0001_u64;
        for _ in 0..20_000 {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            let length = (random % 24) as usize;
            let mut text = String::new();
            for _ in 0..length {
                random ^= random << 13;
                random ^= random >> 7;
                random ^= random << 17;
                text.push_str(atoms[random as usize % atoms.len()]);
                text.push(' ');
            }
            source(&text);
        }
    }

    #[test]
    fn malformed_native_spans_and_exhausted_work_are_distinct_from_syntax() {
        let mut input = Input { tokens: std::ptr::null(), count: u64::MAX, mode: 0, max_work: 1 };
        let mut output = Output::default();
        let mut result = ResultRecord::default();
        assert_eq!(unsafe { whale_amd64_parse(&input, &mut output, &mut result) }, 2);
        assert_eq!(result.status, 2);
        let name = b"nop";
        let token =
            RawToken { text: Name { bytes: name.as_ptr(), count: 3 }, ..RawToken::default() };
        input.tokens = &token;
        input.count = 1;
        input.max_work = 0;
        assert_eq!(unsafe { whale_amd64_parse(&input, &mut output, &mut result) }, 3);
        assert_eq!(result.work, 0);
        assert_eq!(result.error, 0);
        input.max_work = 100;
        input.mode = 1;
        assert_eq!(unsafe { whale_amd64_parse(&input, &mut output, &mut result) }, 2);
    }
}
