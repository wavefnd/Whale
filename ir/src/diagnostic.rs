use crate::{interpreter::InterpreterError, verifier::VerifyError};

use diagnostic::{Context, Diagnostic, SourceLocation, ToDiagnostic};

impl ToDiagnostic for VerifyError {
    fn diagnostic(&self) -> Diagnostic {
        let code = match self {
            Self::GlobalStorage { .. } => "ir.verify.global-storage",
            Self::SourceMetadata(..) => "ir.verify.source-metadata",
            Self::ResourceLimit(..) => "ir.verify.resource-limit",
            Self::ForbiddenUndef { .. } => "ir.verify.forbidden-undef",
            Self::InvalidMemoryLayout { .. } => "ir.verify.invalid-memory-layout",
            Self::InvalidCast { .. } => "ir.verify.invalid-cast",
            Self::Call { .. } => "ir.verify.call",
            Self::Target(..) => "ir.verify.target",
            Self::InvalidConstExpression { .. } => "ir.verify.invalid-const-expression",
            Self::NonDominatingValue { .. } => "ir.verify.non-dominating-value",
            Self::InvalidPhi { .. } => "ir.verify.invalid-phi",
            Self::InvalidGep { .. } => "ir.verify.invalid-gep",
            Self::InvalidInstructionType { .. } => "ir.verify.invalid-instruction-type",
            Self::OperandTypeMismatch { .. } => "ir.verify.operand-type-mismatch",
            Self::InvalidMemoryAlignment { .. } => "ir.verify.invalid-memory-alignment",
            Self::InvalidSwitchCase { .. } => "ir.verify.invalid-switch-case",
            Self::DuplicateSwitchCase { .. } => "ir.verify.duplicate-switch-case",
            Self::DuplicateFunction { .. } => "ir.verify.duplicate-function",
            Self::DuplicateGlobal { .. } => "ir.verify.duplicate-global",
            Self::InvalidConstant { .. } => "ir.verify.invalid-constant",
            Self::InvalidGlobalInitializer { .. } => "ir.verify.invalid-global-initializer",
            Self::InvalidGlobalAlignment { .. } => "ir.verify.invalid-global-alignment",
            Self::ConditionTypeMismatch { .. } => "ir.verify.condition-type-mismatch",
            Self::DuplicateBlock { .. } => "ir.verify.duplicate-block",
            Self::DuplicateValue { .. } => "ir.verify.duplicate-value",
            Self::DuplicateValueType { .. } => "ir.verify.duplicate-value-type",
            Self::UnexpectedValueType { .. } => "ir.verify.unexpected-value-type",
            Self::ValueTypeMismatch { .. } => "ir.verify.value-type-mismatch",
            Self::InvalidEntryBlock { .. } => "ir.verify.invalid-entry-block",
            Self::EntryHasPredecessor { .. } => "ir.verify.entry-has-predecessor",
            Self::InvalidBranchTarget { .. } => "ir.verify.invalid-branch-target",
            Self::MissingValueType { .. } => "ir.verify.missing-value-type",
            Self::VoidParameter { .. } => "ir.verify.void-parameter",
            Self::UnterminatedBlock { .. } => "ir.verify.unterminated-block",
            Self::RetTypeMismatch { .. } => "ir.verify.ret-type-mismatch",
            Self::UseOfUndefinedValue { .. } => "ir.verify.use-of-undefined-value",
        };
        let mut d = Diagnostic::new("ir", "verify", code, format!("{self:?}"));
        match self {
            Self::DuplicateFunction { name }
            | Self::DuplicateGlobal { name }
            | Self::InvalidGlobalInitializer { name, .. }
            | Self::InvalidGlobalAlignment { name, .. }
            | Self::GlobalStorage { name, .. } => d.contexts.push(Context::Symbol(name.clone())),
            _ => {},
        }
        match self {
            Self::ForbiddenUndef { func, .. }
            | Self::InvalidMemoryLayout { func, .. }
            | Self::InvalidCast { func, .. }
            | Self::Call { func, .. }
            | Self::NonDominatingValue { func, .. }
            | Self::InvalidPhi { func, .. }
            | Self::InvalidGep { func, .. }
            | Self::InvalidInstructionType { func, .. }
            | Self::OperandTypeMismatch { func, .. }
            | Self::InvalidMemoryAlignment { func, .. }
            | Self::InvalidSwitchCase { func, .. }
            | Self::DuplicateSwitchCase { func, .. }
            | Self::InvalidConstant { func, .. }
            | Self::ConditionTypeMismatch { func, .. }
            | Self::DuplicateBlock { func, .. }
            | Self::DuplicateValue { func, .. }
            | Self::DuplicateValueType { func, .. }
            | Self::UnexpectedValueType { func, .. }
            | Self::ValueTypeMismatch { func, .. }
            | Self::InvalidEntryBlock { func, .. }
            | Self::EntryHasPredecessor { func, .. }
            | Self::InvalidBranchTarget { func, .. }
            | Self::MissingValueType { func, .. }
            | Self::VoidParameter { func, .. }
            | Self::UnterminatedBlock { func, .. }
            | Self::RetTypeMismatch { func, .. }
            | Self::UseOfUndefinedValue { func, .. } => {
                d.contexts.push(Context::Symbol(func.clone()))
            },
            _ => {},
        }
        match self {
            Self::NonDominatingValue { block, .. }
            | Self::InvalidPhi { block, .. }
            | Self::InvalidSwitchCase { block, .. }
            | Self::DuplicateSwitchCase { block, .. }
            | Self::DuplicateBlock { block, .. }
            | Self::InvalidBranchTarget { block, .. } => d.contexts.push(Context::Block(block.0)),
            Self::InvalidEntryBlock { entry, .. } | Self::EntryHasPredecessor { entry, .. } => {
                d.contexts.push(Context::Block(entry.0))
            },
            _ => {},
        }
        d
    }
}

impl ToDiagnostic for InterpreterError {
    fn diagnostic(&self) -> Diagnostic {
        let code = match self.root_cause() {
            Self::Diagnostic { .. } => "ir.run.diagnostic",
            Self::Verification(..) => "ir.run.verification",
            Self::UnknownFunction(..) => "ir.run.unknown-function",
            Self::UnsupportedGlobal(..) => "ir.run.unsupported-global",
            Self::GlobalMemory { .. } => "ir.run.global-memory",
            Self::GlobalMemoryLimit { .. } => "ir.run.global-memory-limit",
            Self::UnsupportedSignature(..) => "ir.run.unsupported-signature",
            Self::ArgumentCount { .. } => "ir.run.argument-count",
            Self::ArgumentType { .. } => "ir.run.argument-type",
            Self::UnsupportedInstruction { .. } => "ir.run.unsupported-instruction",
            Self::StepLimit { .. } => "ir.run.step-limit",
            Self::MemoryLimit { .. } => "ir.run.memory-limit",
            Self::CallLimit { .. } => "ir.run.call-limit",
            Self::Trap(..) => "ir.run.trap",
        };
        let mut d = Diagnostic::new("ir", "run", code, self.to_string());
        let site = match self.root_cause() {
            Self::UnsupportedInstruction { site, .. }
            | Self::StepLimit { site, .. }
            | Self::MemoryLimit { site, .. }
            | Self::CallLimit { site, .. } => Some(*site),
            Self::Trap(t) => Some(t.site),
            _ => None,
        };
        if let Some(s) = site {
            d.contexts.extend([
                Context::Function(s.function.0),
                Context::Block(s.block.0),
                Context::Instruction(s.instruction),
            ]);
        }
        if let Some(metadata) = self.diagnostic() {
            if let Some(original) = &metadata.original {
                d.contexts.push(Context::OriginalNode(original.original));
                if let Some((file, span)) = &original.source {
                    d.contexts.push(Context::Source(SourceLocation {
                        source: file.clone(),
                        line: span.line.into(),
                        column: span.column.into(),
                        end_line: Some(span.end_line.into()),
                        end_column: Some(span.end_column.into()),
                        ..Default::default()
                    }));
                }
            }
        }
        d
    }
}

#[cfg(feature = "socket")]
impl ToDiagnostic for crate::lower_ast::LowerError {
    fn diagnostic(&self) -> Diagnostic {
        let code = match self.root_cause() {
            Self::Located { .. } => "ir.lower.located",
            Self::ResourceLimit(..) => "ir.lower.resource-limit",
            Self::Call(..) => "ir.lower.call",
            Self::VoidCallUsedAsValue => "ir.lower.void-call-used-as-value",
            Self::NumericLiteral(..) => "ir.lower.numeric-literal",
            Self::ConstantEvaluation(..) => "ir.lower.constant-evaluation",
            Self::Target(..) => "ir.lower.target",
            Self::Layout(..) => "ir.lower.layout",
            Self::UnsupportedType(..) => "ir.lower.unsupported-type",
            Self::UnknownVariable(..) => "ir.lower.unknown-variable",
            Self::TypeMismatch { .. } => "ir.lower.type-mismatch",
            Self::MissingInit(..) => "ir.lower.missing-init",
            Self::Aggregate(_) => "ir.lower.aggregate",
            Self::UnsupportedStmt => "ir.lower.unsupported-stmt",
            Self::UnsupportedExpr => "ir.lower.unsupported-expr",
            Self::BreakOutsideLoop => "ir.lower.break-outside-loop",
            Self::ContinueOutsideLoop => "ir.lower.continue-outside-loop",
            Self::DuplicateGlobal(..) => "ir.lower.duplicate-global",
            Self::DuplicateFunction(..) => "ir.lower.duplicate-function",
            Self::DuplicateParameter { .. } => "ir.lower.duplicate-parameter",
            Self::ValueReturnedFromVoid => "ir.lower.value-returned-from-void",
            Self::InvalidLiteral { .. } => "ir.lower.invalid-literal",
            Self::AssignToConst(..) => "ir.lower.assign-to-const",
            Self::NonConstExpr => "ir.lower.non-const-expr",
            Self::Backend(..) => "ir.lower.backend",
        };
        let mut d = Diagnostic::new("ir", "lower", code, self.to_string());
        if let Some(a) = self.annotation() {
            d.contexts.push(Context::OriginalNode(a.id));
            if let Some(s) = &a.source {
                d.contexts.push(Context::Source(SourceLocation {
                    source: s.file.clone(),
                    line: s.line.into(),
                    column: s.column.into(),
                    end_line: Some(s.end_line.into()),
                    end_column: Some(s.end_column.into()),
                    ..Default::default()
                }));
            }
        }
        d
    }
}

impl ToDiagnostic for crate::ParseError {
    fn diagnostic(&self) -> Diagnostic {
        let mut d = match &self.kind {
            crate::ParseErrorKind::Syntax(message) => {
                Diagnostic::new("ir", "parse", "ir.parse.syntax", message.clone())
            },
            crate::ParseErrorKind::ResourceLimit(error) => {
                Diagnostic::new("ir", "parse", "ir.parse.resource-limit", error.to_string())
            },
            crate::ParseErrorKind::Verification(error) => error.diagnostic(),
            crate::ParseErrorKind::Backend(component) => Diagnostic::new(
                "ir",
                "parse",
                "ir.parse.backend",
                format!("IR Wave backend error: {component}"),
            ),
        };
        d.contexts.push(Context::Source(SourceLocation {
            source: "<input>".into(),
            line: self.location.line as u64,
            column: self.location.column as u64,
            start_byte: Some(self.location.byte_offset as u64),
            ..Default::default()
        }));
        d
    }
}
