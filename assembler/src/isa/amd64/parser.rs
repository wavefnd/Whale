use crate::ast::*;
use crate::error::AsmError;
use crate::tokens::{Token, TokenKind};

fn parser_err(tokens: &[Token], pos: usize, msg: &str) -> AsmError {
    if let Some(tok) = tokens.get(pos) {
        AsmError::ParserError(format!(
            "{} at line {}, column {}",
            msg, tok.line, tok.column
        ))
    } else if let Some(tok) = tokens.last() {
        AsmError::ParserError(format!(
            "{} near end of input (line {}, column {})",
            msg, tok.line, tok.column
        ))
    } else {
        AsmError::ParserError(format!("{} at line 1, column 1", msg))
    }
}

pub fn parse(tokens: &[Token]) -> Result<AST, AsmError> {
    let mut pos = 0;
    let mut items = Vec::new();

    while pos < tokens.len() {
        match &tokens[pos].kind {
            TokenKind::Identifier(name) => {
                if pos + 1 < tokens.len() && matches!(tokens[pos + 1].kind, TokenKind::Colon) {
                    items.push(ASTNode::Label(name.clone()));
                    pos += 2;
                    continue;
                }

                if pos + 1 < tokens.len() {
                    if let TokenKind::Identifier(eq_kw) = &tokens[pos + 1].kind {
                        if eq_kw.eq_ignore_ascii_case("equ") {
                            items.push(parse_const_definition(tokens, &mut pos)?);
                            continue;
                        }
                    }
                }

                if is_directive(name) {
                    items.push(parse_directive(tokens, &mut pos)?);
                    continue;
                }

                items.push(parse_instruction(tokens, &mut pos)?);
            }
            TokenKind::Newline => pos += 1,
            _ => return Err(parser_err(tokens, pos, "Unexpected token at top level")),
        }
    }
    Ok(AST { items })
}

fn parse_const_definition(tokens: &[Token], pos: &mut usize) -> Result<ASTNode, AsmError> {
    let name = match &tokens[*pos].kind {
        TokenKind::Identifier(s) => s.clone(),
        _ => return Err(parser_err(tokens, *pos, "Expected constant name")),
    };
    *pos += 1;

    match tokens.get(*pos).map(|t| &t.kind) {
        Some(TokenKind::Identifier(s)) if s.eq_ignore_ascii_case("equ") => {
            *pos += 1;
        }
        _ => return Err(parser_err(tokens, *pos, "Expected 'equ'")),
    }

    let expr = parse_expr(tokens, pos, |k| matches!(k, TokenKind::Newline))?;
    while *pos < tokens.len() && matches!(tokens[*pos].kind, TokenKind::Newline) {
        *pos += 1;
    }

    Ok(ASTNode::Const { name, expr })
}

fn parse_instruction(tokens: &[Token], pos: &mut usize) -> Result<ASTNode, AsmError> {
    let mnemonic = parse_mnemonic(tokens, pos)?;
    let operands = parse_operand_list(tokens, pos)?;
    Ok(ASTNode::Instruction(Instruction { mnemonic, operands }))
}

fn parse_mnemonic(tokens: &[Token], pos: &mut usize) -> Result<String, AsmError> {
    match &tokens[*pos].kind {
        TokenKind::Identifier(s) => {
            *pos += 1;
            Ok(s.to_ascii_lowercase())
        }
        _ => Err(parser_err(tokens, *pos, "Expected mnemonic")),
    }
}

fn parse_operand_list(tokens: &[Token], pos: &mut usize) -> Result<Vec<Operand>, AsmError> {
    let mut ops = Vec::new();
    loop {
        if *pos >= tokens.len() {
            break;
        }
        match &tokens[*pos].kind {
            TokenKind::Newline => {
                *pos += 1;
                break;
            }
            _ => {
                let operand = parse_operand(tokens, pos)?;
                ops.push(operand);
            }
        }
        if *pos < tokens.len() && matches!(tokens[*pos].kind, TokenKind::Comma) {
            *pos += 1;
            continue;
        } else {
            break;
        }
    }
    while *pos < tokens.len() && matches!(tokens[*pos].kind, TokenKind::Newline) {
        *pos += 1;
    }
    Ok(ops)
}

fn parse_operand(tokens: &[Token], pos: &mut usize) -> Result<Operand, AsmError> {
    if matches!(tokens[*pos].kind, TokenKind::LBracket) {
        return parse_memory_operand(tokens, pos);
    }

    if let TokenKind::Identifier(name) = &tokens[*pos].kind {
        let reg = name.to_ascii_lowercase();
        let next_is_expr_op = *pos + 1 < tokens.len()
            && matches!(tokens[*pos + 1].kind, TokenKind::Plus | TokenKind::Minus);
        if is_register(&reg) && !next_is_expr_op {
            *pos += 1;
            return Ok(Operand::Register(reg));
        }
    }

    let expr = parse_expr(tokens, pos, |k| {
        matches!(k, TokenKind::Comma | TokenKind::Newline)
    })?;
    match expr {
        ExprValue::Number(n) => Ok(Operand::Immediate(n)),
        ExprValue::Symbol { name, addend } => {
            if addend == 0 {
                Ok(Operand::Label(name))
            } else {
                Ok(Operand::SymbolExpr { name, addend })
            }
        }
    }
}

fn parse_memory_operand(tokens: &[Token], pos: &mut usize) -> Result<Operand, AsmError> {
    *pos += 1; // '['

    let mut base = None;
    let mut index = None;
    let mut symbol = None;
    let mut scale = 1;
    let mut disp = 0i64;
    let mut sign = 1i64;

    // A leading sign is allowed; subsequent signs must separate complete terms.
    match tokens.get(*pos).map(|t| &t.kind) {
        Some(TokenKind::Plus) => *pos += 1,
        Some(TokenKind::Minus) => {
            sign = -1;
            *pos += 1;
        }
        _ => {}
    }

    loop {
        let term_pos = *pos;
        let kind = tokens.get(*pos).map(|t| &t.kind);
        match kind {
            None | Some(TokenKind::Newline) => {
                return Err(parser_err(
                    tokens,
                    *pos,
                    "Missing closing ']' in memory operand",
                ));
            }
            Some(TokenKind::Identifier(name)) => {
                if sign < 0 {
                    return Err(parser_err(
                        tokens,
                        *pos,
                        "Negative register or symbol terms are not supported in memory operands",
                    ));
                }
                let lower = name.to_ascii_lowercase();
                if is_register(&lower) {
                    if base.is_none() {
                        base = Some(lower);
                        *pos += 1;
                    } else if index.is_none() {
                        index = Some(lower);
                        *pos += 1;
                        if matches!(tokens.get(*pos).map(|t| &t.kind), Some(TokenKind::Multiply)) {
                            *pos += 1;
                            match tokens.get(*pos).map(|t| &t.kind) {
                                Some(TokenKind::Number(n)) if matches!(*n, 1 | 2 | 4 | 8) => {
                                    scale = *n as u8;
                                    *pos += 1;
                                }
                                _ => return Err(parser_err(tokens, *pos, "Scale must be 1/2/4/8")),
                            }
                        }
                    } else {
                        return Err(parser_err(
                            tokens,
                            term_pos,
                            "Too many registers in memory operand",
                        ));
                    }
                } else if symbol.is_none() {
                    symbol = Some(name.clone());
                    *pos += 1;
                } else {
                    return Err(parser_err(
                        tokens,
                        term_pos,
                        "Multiple symbols in memory operand",
                    ));
                }
            }
            Some(TokenKind::Number(n)) => {
                disp = n
                    .checked_mul(sign)
                    .and_then(|n| disp.checked_add(n))
                    .ok_or_else(|| parser_err(tokens, term_pos, "Memory displacement overflow"))?;
                *pos += 1;
            }
            _ => return Err(parser_err(tokens, *pos, "Expected term in memory operand")),
        }

        match tokens.get(*pos).map(|t| &t.kind) {
            Some(TokenKind::RBracket) => {
                *pos += 1;
                return Ok(Operand::Memory(MemoryOperand {
                    base,
                    index,
                    scale,
                    disp,
                    symbol,
                }));
            }
            Some(TokenKind::Plus) => {
                sign = 1;
                *pos += 1;
            }
            Some(TokenKind::Minus) => {
                sign = -1;
                *pos += 1;
            }
            None | Some(TokenKind::Newline) => {
                return Err(parser_err(
                    tokens,
                    *pos,
                    "Missing closing ']' in memory operand",
                ));
            }
            _ => {
                return Err(parser_err(
                    tokens,
                    *pos,
                    "Expected '+', '-' or ']' in memory operand",
                ))
            }
        }
    }
}

fn parse_directive(tokens: &[Token], pos: &mut usize) -> Result<ASTNode, AsmError> {
    let name = match &tokens[*pos].kind {
        TokenKind::Identifier(s) => s.to_ascii_lowercase(),
        _ => return Err(parser_err(tokens, *pos, "Expected directive name")),
    };
    *pos += 1;

    if *pos >= tokens.len() {
        return Err(parser_err(tokens, *pos, "Unexpected end after directive"));
    }

    match name.as_str() {
        "section" => {
            if let TokenKind::Identifier(sec_name) = &tokens[*pos].kind {
                *pos += 1;
                Ok(ASTNode::Section(sec_name.clone()))
            } else {
                Err(parser_err(tokens, *pos, "Expected section name"))
            }
        }
        "global" => {
            if let TokenKind::Identifier(sym_name) = &tokens[*pos].kind {
                *pos += 1;
                Ok(ASTNode::Global(sym_name.clone()))
            } else {
                Err(parser_err(tokens, *pos, "Expected global symbol name"))
            }
        }
        "extern" => {
            if let TokenKind::Identifier(sym_name) = &tokens[*pos].kind {
                *pos += 1;
                Ok(ASTNode::Extern(sym_name.clone()))
            } else {
                Err(parser_err(tokens, *pos, "Expected extern symbol name"))
            }
        }
        _ => {
            let mut values = Vec::new();
            loop {
                if *pos >= tokens.len() {
                    break;
                }
                match &tokens[*pos].kind {
                    TokenKind::Newline => {
                        *pos += 1;
                        break;
                    }
                    TokenKind::Comma => {
                        *pos += 1;
                    }
                    TokenKind::StringLiteral(s) => {
                        values.push(DirectiveValue::StringLiteral(s.clone()));
                        *pos += 1;
                    }
                    _ => {
                        let expr = parse_expr(tokens, pos, |k| {
                            matches!(k, TokenKind::Comma | TokenKind::Newline)
                        })?;
                        values.push(DirectiveValue::Expr(expr));
                    }
                }
            }
            Ok(ASTNode::Directive(Directive { name, values }))
        }
    }
}

fn parse_expr<F>(tokens: &[Token], pos: &mut usize, stop: F) -> Result<ExprValue, AsmError>
where
    F: Fn(&TokenKind) -> bool,
{
    let mut acc: Option<ExprValue> = None;
    let mut expect_term = true;
    let mut sign: i64 = 1;

    while *pos < tokens.len() {
        let kind = &tokens[*pos].kind;
        if stop(kind) {
            break;
        }

        if expect_term {
            match kind {
                TokenKind::Plus => {
                    *pos += 1;
                }
                TokenKind::Minus => {
                    sign = -sign;
                    *pos += 1;
                }
                TokenKind::Number(n) => {
                    let term = ExprValue::Number(sign * *n);
                    acc = Some(combine_expr(tokens, *pos, acc, term)?);
                    *pos += 1;
                    sign = 1;
                    expect_term = false;
                }
                TokenKind::Identifier(s) => {
                    if sign < 0 {
                        return Err(parser_err(
                            tokens,
                            *pos,
                            "Unary '-' before symbol is not supported",
                        ));
                    }
                    let term = ExprValue::Symbol {
                        name: s.clone(),
                        addend: 0,
                    };
                    acc = Some(combine_expr(tokens, *pos, acc, term)?);
                    *pos += 1;
                    sign = 1;
                    expect_term = false;
                }
                _ => return Err(parser_err(tokens, *pos, "Expected expression term")),
            }
        } else {
            match kind {
                TokenKind::Plus => {
                    sign = 1;
                    expect_term = true;
                    *pos += 1;
                }
                TokenKind::Minus => {
                    sign = -1;
                    expect_term = true;
                    *pos += 1;
                }
                _ => {
                    return Err(parser_err(
                        tokens,
                        *pos,
                        "Expected '+' or '-' in expression",
                    ))
                }
            }
        }
    }

    if expect_term && acc.is_some() {
        return Err(parser_err(
            tokens,
            *pos,
            "Expression cannot end with operator",
        ));
    }

    acc.ok_or_else(|| parser_err(tokens, *pos, "Expected expression"))
}

fn combine_expr(
    tokens: &[Token],
    pos: usize,
    left: Option<ExprValue>,
    right: ExprValue,
) -> Result<ExprValue, AsmError> {
    match (left, right) {
        (None, r) => Ok(r),
        (Some(ExprValue::Number(a)), ExprValue::Number(b)) => Ok(ExprValue::Number(a + b)),
        (Some(ExprValue::Number(a)), ExprValue::Symbol { name, addend }) => Ok(ExprValue::Symbol {
            name,
            addend: addend + a,
        }),
        (Some(ExprValue::Symbol { name, addend }), ExprValue::Number(b)) => Ok(ExprValue::Symbol {
            name,
            addend: addend + b,
        }),
        (Some(ExprValue::Symbol { .. }), ExprValue::Symbol { .. }) => Err(parser_err(
            tokens,
            pos,
            "Expressions with multiple symbols are not supported",
        )),
    }
}

fn is_register(name: &str) -> bool {
    matches!(
        name,
        "rax"
            | "rbx"
            | "rcx"
            | "rdx"
            | "rsi"
            | "rdi"
            | "rbp"
            | "rsp"
            | "r8"
            | "r9"
            | "r10"
            | "r11"
            | "r12"
            | "r13"
            | "r14"
            | "r15"
            | "eax"
            | "ebx"
            | "ecx"
            | "edx"
            | "esi"
            | "edi"
            | "ebp"
            | "esp"
            | "r8d"
            | "r9d"
            | "r10d"
            | "r11d"
            | "r12d"
            | "r13d"
            | "r14d"
            | "r15d"
            | "ax"
            | "bx"
            | "cx"
            | "dx"
            | "si"
            | "di"
            | "bp"
            | "sp"
            | "r8w"
            | "r9w"
            | "r10w"
            | "r11w"
            | "r12w"
            | "r13w"
            | "r14w"
            | "r15w"
            | "al"
            | "bl"
            | "cl"
            | "dl"
            | "ah"
            | "bh"
            | "ch"
            | "dh"
            | "r8b"
            | "r9b"
            | "r10b"
            | "r11b"
            | "r12b"
            | "r13b"
            | "r14b"
            | "r15b"
    )
}

fn is_directive(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "db" | "dw"
            | "dd"
            | "dq"
            | "resb"
            | "resw"
            | "resd"
            | "resq"
            | "section"
            | "global"
            | "extern"
    )
}
