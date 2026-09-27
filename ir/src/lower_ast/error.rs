// SPDX-License-Identifier: MPL-2.0

use crate::{ConstValue, Type};

#[derive(Debug)]
pub enum LowerError {
    Call(crate::CallError),
    VoidCallUsedAsValue,
    NumericLiteral(String),
    Target(crate::TargetError),
    Layout(crate::LayoutError),
    UnsupportedType(String),
    UnknownVariable(String),
    TypeMismatch { expected: Type, got: Type },
    MissingInit(String),
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
