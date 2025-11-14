#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Identifier(String),
    Number(i64),
    StringLiteral(String),
    Comma,
    Colon,
    LBracket,
    RBracket,
    Plus,
    Minus,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub position: usize,
}

pub fn tokenize(src: &str) -> Result<Vec<Token>, String> {
    let mut chars = src.chars().peekable();
    let mut tokens = Vec::new();
    let mut pos = 0;

    while let Some(&ch) = chars.peek() {
        match ch {
            ' ' | '\t' | '\n' | '\r' => {
                chars.next();
                pos += 1;
            }

            ',' => {
                tokens.push(Token { kind: TokenKind::Comma, position: pos });
                chars.next(); pos += 1;
            }

            ':' => {
                tokens.push(Token { kind: TokenKind::Colon, position: pos });
                chars.next(); pos += 1;
            }

            '[' => {
                tokens.push(Token { kind: TokenKind::LBracket, position: pos });
                chars.next(); pos += 1;
            }

            ']' => {
                tokens.push(Token { kind: TokenKind::RBracket, position: pos });
                chars.next(); pos += 1;
            }

            '+' => {
                tokens.push(Token { kind: TokenKind::Plus, position: pos });
                chars.next(); pos += 1;
            }

            '-' => {
                tokens.push(Token { kind: TokenKind::Minus, position: pos });
                chars.next(); pos += 1;
            }

            '"' => {
                chars.next(); pos += 1;
                let mut s = String::new();
                while let Some(c) = chars.next() {
                    pos += 1;
                    if c == '"' { break; }
                    s.push(c);
                }
                tokens.push(Token { kind: TokenKind::StringLiteral(s), position: pos });
            }

            c if c.is_numeric() => {
                let mut n = String::new();
                while let Some(&d) = chars.peek() {
                    if d.is_numeric() {
                        n.push(d);
                        chars.next();
                        pos += 1;
                    } else { break; }
                }
                let parsed = n.parse::<i64>().unwrap();
                tokens.push(Token { kind: TokenKind::Number(parsed), position: pos });
            }

            c if c.is_alphanumeric() || c == '_' => {
                let mut id = String::new();
                while let Some(&d) = chars.peek() {
                    if d.is_alphanumeric() || d == '_' {
                        id.push(d);
                        chars.next();
                        pos += 1;
                    } else { break; }
                }
                tokens.push(Token { kind: TokenKind::Identifier(id), position: pos });
            }

            _ => return Err(format!("Unexpected char '{}' at {}", ch, pos))
        }
    }

    Ok(tokens)
}