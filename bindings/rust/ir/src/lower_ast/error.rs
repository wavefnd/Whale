// SPDX-License-Identifier: MPL-2.0

use crate::{ConstValue, Type};

use diagnostic::EscapedName;

#[derive(Debug)]
pub enum LowerError {
    Located { annotation: crate::SourceAnnotation, error: Box<LowerError> },
    ResourceLimit(super::AstLimitError),
    Backend(&'static str),
    Call(crate::CallError),
    VoidCallUsedAsValue,
    NumericLiteral(String),
    ConstantEvaluation(crate::ConstEvalError),
    Target(crate::TargetError),
    Layout(crate::LayoutError),
    UnsupportedType(String),
    UnknownVariable(String),
    TypeMismatch { expected: Type, got: Type },
    MissingInit(String),
    Aggregate(String),
    UnsupportedStmt,
    UnsupportedExpr,
    BreakOutsideLoop,
    ContinueOutsideLoop,
    DuplicateGlobal(String),
    DuplicateFunction(String),
    DuplicateParameter { func: String, param: String },
    ValueReturnedFromVoid,
    InvalidLiteral { ty: Type, value: ConstValue },
    AssignToConst(String),
    NonConstExpr,
}

impl LowerError {
    pub(crate) fn at_expression(self, expression: &super::frontend::Expr) -> Self {
        if let super::frontend::Expr::Located { annotation, .. } = expression {
            self.at(annotation)
        } else {
            self
        }
    }

    pub fn at(self, annotation: &crate::SourceAnnotation) -> Self {
        if matches!(self, Self::Located { .. }) {
            self
        } else {
            Self::Located { annotation: annotation.clone(), error: Box::new(self) }
        }
    }

    pub fn root_cause(&self) -> &Self {
        match self {
            Self::Located { error, .. } => error.root_cause(),
            _ => self,
        }
    }

    pub fn annotation(&self) -> Option<&crate::SourceAnnotation> {
        match self {
            Self::Located { annotation, .. } => Some(annotation),
            _ => None,
        }
    }
}

impl std::fmt::Display for LowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Self::Located { annotation, error } = self {
            if let Some(source) = &annotation.source {
                write!(f, "{}:{}:{}: ", EscapedName(&source.file), source.line, source.column)?;
            }
            write!(f, "original @n{}: {error}", annotation.id)
        } else {
            write!(f, "{self:?}")
        }
    }
}

impl std::error::Error for LowerError {}
