use crate::tokens::{Token, TokenKind};
use crate::ast::*;
use crate::error::AsmError;

pub fn parse(tokens: &[Token]) -> Result<AST, AsmError> {
    let mut pos = 0;
    let mut items = Vec::new();

    while pos < tokens.len() {
        match &tokens[pos].kind {
            // label: identifier :
            TokenKind::Identifier(name) => {
                if pos + 1 < tokens.len() && matches!(tokens[pos + 1].kind, TokenKind::Colon) {
                    items.push(ASTNode::Label(name.clone()));
                    pos += 2;
                    continue;
                }

                if is_directive(name) {
                    items.push(parse_directive(tokens, &mut pos)?);
                    continue;
                }

                // instruction start
                items.push(parse_instruction(tokens, &mut pos)?);
            }

            TokenKind::Newline => pos += 1,
            // skip newlines or unknown tokens for now
            _ => pos += 1,
        }
    }

    Ok(AST { items })
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
            Ok(s.clone())
        }
        _ => Err(AsmError::ParserError("Expected mnemonic".into()))
    }
}

fn parse_operand_list(tokens: &[Token], pos: &mut usize) -> Result<Vec<Operand>, AsmError> {
    let mut ops = Vec::new();

    loop {
        if *pos >= tokens.len() { break; }

        match &tokens[*pos].kind {
            TokenKind::Newline => { *pos += 1; break; }
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
    match &tokens[*pos].kind {
        TokenKind::Number(n) => {
            *pos += 1;
            Ok(Operand::Immediate(*n))
        }

        TokenKind::Identifier(name) => {
            let s = name.clone();
            *pos += 1;

            if is_register(&s) {
                return Ok(Operand::Register(s));
            }

            return Ok(Operand::Label(s));
        }

        TokenKind::LBracket => {
            parse_memory_operand(tokens, pos)
        }

        _ => Err(AsmError::ParserError("Unexpected token in operand".into())),
    }
}


fn parse_memory_operand(tokens: &[Token], pos: &mut usize) -> Result<Operand, AsmError> {
    *pos += 1;

    let mut base: Option<String> = None;
    let mut index: Option<String> = None;
    let mut scale: u8 = 1;
    let mut disp: i64 = 0;
    let mut negative = false;

    while *pos < tokens.len() {
        match &tokens[*pos].kind {
            TokenKind::Identifier(name) => {
                let reg = name.clone();
                *pos += 1;

                if is_register(&reg) {
                    if base.is_none() {
                        base = Some(reg);
                    } else if index.is_none() {
                        index = Some(reg);

                        if *pos < tokens.len() && matches!(tokens[*pos].kind, TokenKind::Multiply) {
                            *pos += 1; // skip '*'

                            match &tokens[*pos].kind {
                                TokenKind::Number(n) => {
                                    scale = *n as u8;
                                    *pos += 1;
                                }
                                _ => return Err(AsmError::ParserError("Invalid scale".into())),
                            }
                        }
                    } else {
                        return Err(AsmError::ParserError("Too many registers in memory operand".into()));
                    }
                } else {
                    return Ok(Operand::Memory(MemoryOperand {
                        base: None,
                        index: None,
                        scale: 1,
                        disp: 0,
                    }));
                }
            }

            TokenKind::Number(n) => {
                let val = if negative { -*n } else { *n };
                disp += val;
                negative = false;
                *pos += 1;
            }

            TokenKind::Plus => {
                negative = false;
                *pos += 1;
            }

            TokenKind::Minus => {
                negative = true;
                *pos += 1;
            }

            TokenKind::RBracket => {
                *pos += 1;
                break;
            }

            TokenKind::Comma | TokenKind::Newline => break,

            _ => return Err(AsmError::ParserError("Unexpected token in memory operand".into())),
        }
    }

    Ok(Operand::Memory(MemoryOperand {
        base,
        index,
        scale,
        disp,
    }))
}

fn parse_directive(tokens: &[Token], pos: &mut usize) -> Result<ASTNode, AsmError> {
    let name = match &tokens[*pos].kind {
        TokenKind::Identifier(s) => s.clone(),
        _ => return Err(AsmError::ParserError("Expected directive name".into())),
    };
    *pos += 1;

    let mut values = Vec::new();

    loop {
        if *pos >= tokens.len() { break; }

        match &tokens[*pos].kind {
            TokenKind::Newline => {
                *pos += 1;
                break;
            }

            TokenKind::Number(n) => {
                values.push(DirectiveValue::Number(*n));
                *pos += 1;
            }

            TokenKind::StringLiteral(s) => {
                values.push(DirectiveValue::StringLiteral(s.clone()));
                *pos += 1;
            }

            TokenKind::Identifier(s) => {
                values.push(DirectiveValue::StringLiteral(s.clone()));
                *pos += 1;
            }

            TokenKind::Comma => {
                *pos += 1;
                continue;
            }

            _ => break,
        }
    }

    Ok(ASTNode::Directive(Directive {name, values}))
}

fn is_register(name: &str) -> bool {
    matches!(
        name,
        // 64bit
        "rax" | "rbx" | "rcx" | "rdx" |
        "rsi" | "rdi" | "rbp" | "rsp" |
        "r8" | "r9" | "r10" | "r11" |
        "r12" | "r13" | "r14" | "r15" |
        // 32bit
        "eax" | "ebx" | "ecx" | "edx" |
        "esi" | "edi" | "dbp" | "esp" |
        // 16bit
        "ax" | "bx" | "cx" | "dx" |
        "si" | "di" | "bp" | "sp" |
        // 8bit
        "al" | "bl" | "cl" | "dl" |
        "ah" | "bh" | "ch" | "dh"
    )
}

fn is_directive(name: &str) -> bool {
    matches!(name, "db" | "dw" | "dd" | "dq")
}