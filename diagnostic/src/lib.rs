// SPDX-License-Identifier: MPL-2.0
//! Shared rendering boundary. Component APIs retain their original typed causes.
use std::fmt;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceLocation {
    pub source: String,
    pub line: u64,
    pub column: u64,
    pub end_line: Option<u64>,
    pub end_column: Option<u64>,
    pub start_byte: Option<u64>,
    pub end_byte: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Context {
    Source(SourceLocation),
    Object { name: String, member: Option<String> },
    Input(usize),
    Section { index: usize, name: Option<String> },
    FileOffset(u64),
    SectionOffset(u64),
    Symbol(String),
    Function(u32),
    Block(u32),
    Instruction(usize),
    OriginalNode(u64),
}

/// `component`, `phase` and `code` are stable machine-readable identifiers.
/// Renderers must use these fields, never classify `message` by English text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub component: &'static str,
    pub phase: &'static str,
    pub code: &'static str,
    pub message: String,
    pub contexts: Vec<Context>,
}

impl Diagnostic {
    pub fn new(
        component: &'static str,
        phase: &'static str,
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self { component, phase, code, message: message.into(), contexts: Vec::new() }
    }

    pub fn with_context(mut self, context: Context) -> Self {
        self.contexts.push(context);
        self
    }
}

/// Convert the same component cause for human or structured rendering.
pub trait ToDiagnostic {
    fn diagnostic(&self) -> Diagnostic;
}

impl ToDiagnostic for Diagnostic {
    fn diagnostic(&self) -> Diagnostic {
        self.clone()
    }
}

// Keep ordinary Unicode paths readable while preventing names from creating
// new terminal lines or control sequences. Structured contexts stay lossless.
/// Render a user-controlled name without terminal control characters.
pub struct EscapedName<'a>(pub &'a str);

impl fmt::Display for EscapedName<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use fmt::Write;
        for character in self.0.chars() {
            if character.is_control() {
                write!(f, "{}", character.escape_debug())?;
            } else {
                f.write_char(character)?;
            }
        }
        Ok(())
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)?;
        for context in &self.contexts {
            match context {
                Context::Source(s) => {
                    write!(f, " at {}:{}:{}", EscapedName(&s.source), s.line, s.column)?
                },
                Context::Object { name, member } => {
                    write!(f, " in {}", EscapedName(name))?;
                    if let Some(m) = member {
                        write!(f, "({})", EscapedName(m))?;
                    }
                },
                Context::Input(i) => write!(f, " input {i}")?,
                Context::Section { index, name } => {
                    write!(f, " section {index}")?;
                    if let Some(n) = name {
                        write!(f, " {n:?}")?;
                    }
                },
                Context::FileOffset(at) => write!(f, " at file offset {at}")?,
                Context::SectionOffset(at) => write!(f, " at section offset {at}")?,
                Context::Symbol(s) => write!(f, " symbol {s:?}")?,
                Context::Function(id) => write!(f, " function @f{id}")?,
                Context::Block(id) => write!(f, " block %b{id}")?,
                Context::Instruction(i) => write!(f, " instruction {i}")?,
                Context::OriginalNode(id) => write!(f, " original @n{id}")?,
            }
        }
        Ok(())
    }
}

impl std::error::Error for Diagnostic {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendering_preserves_machine_fields_and_contexts() {
        let d = Diagnostic::new("object", "read", "invalid-index", "symbol section is absent")
            .with_context(Context::Object { name: "lib.a".into(), member: Some("part.o".into()) })
            .with_context(Context::Section { index: 3, name: Some(".text".into()) })
            .with_context(Context::FileOffset(64));
        let render = d.to_string();
        assert!(
            render.contains("lib.a(part.o)") && render.contains(".text") && render.contains("64")
        );
        assert_eq!(d.diagnostic(), d);
        assert_eq!(d.code, "invalid-index");
    }
}

#[cfg(test)]
mod control_tests {
    use super::*;

    #[test]
    fn context_names_escape_controls_without_changing_structured_fields() {
        for control in ['\n', '\r', '\t', '\x1b'] {
            let name = format!("경로/{control}file");
            let d = Diagnostic::new("audit", "read", "audit.invalid", "invalid")
                .with_context(Context::Source(SourceLocation {
                    source: name.clone(),
                    line: 2,
                    column: 3,
                    ..Default::default()
                }))
                .with_context(Context::Object { name: name.clone(), member: Some(name.clone()) });
            let original = d.clone();
            let rendered = d.to_string();
            assert!(!rendered.contains(control), "{rendered:?}");
            assert!(rendered.contains("경로/"));
            assert!(rendered.contains(":2:3"));
            assert!(rendered.contains(&control.escape_debug().to_string()));
            assert_eq!(d, original);
            assert_eq!(d.diagnostic(), original);
        }
        let d = Diagnostic::new("audit", "read", "audit.invalid", "invalid").with_context(
            Context::Source(SourceLocation {
                source: "경로/café.s".into(),
                line: 4,
                column: 5,
                ..Default::default()
            }),
        );
        assert_eq!(d.to_string(), "audit.invalid: invalid at 경로/café.s:4:5");
    }
}
