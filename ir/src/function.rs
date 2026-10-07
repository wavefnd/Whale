// SPDX-License-Identifier: MPL-2.0

use crate::{BasicBlock, BlockId, Type, ValueId};

#[derive(Clone, Debug)]
pub struct Param {
    pub name: String,
    pub id: ValueId,
    pub ty: Type,
}

#[derive(Clone, Debug)]
pub struct Function {
    pub id: crate::FunctionId,
    pub name: String,
    pub params: Vec<Param>,
    pub ret_ty: Type,
    pub blocks: Vec<BasicBlock>,
    pub entry: BlockId,

    pub value_types: Vec<(ValueId, Type)>,
}

impl Function {
    pub fn value_type(&self, id: ValueId) -> Option<&Type> {
        self.value_types
            .iter()
            .find(|(vid, _)| *vid == id)
            .map(|(_, t)| t)
    }
}

/// The convention is part of a function pointer's type, not an ABI inference.
#[cfg_attr(feature = "socket", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CallingConvention {
    Whale,
    SysV64,
}

#[cfg_attr(feature = "socket", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Linkage {
    Internal,
    External,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FunctionSignature {
    pub params: Vec<Type>,
    pub ret: Type,
    pub convention: CallingConvention,
    pub variadic: bool,
}
impl FunctionSignature {
    pub fn whale(params: Vec<Type>, ret: Type) -> Self {
        Self {
            params,
            ret,
            convention: CallingConvention::Whale,
            variadic: false,
        }
    }
}
impl core::fmt::Display for FunctionSignature {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{} (",
            match self.convention {
                CallingConvention::Whale => "whale",
                CallingConvention::SysV64 => "sysv64",
            }
        )?;
        for (i, ty) in self.params.iter().enumerate() {
            if i != 0 {
                write!(f, ", ")?;
            }
            write!(f, "{ty}")?;
        }
        if self.variadic {
            write!(f, ", ...")?;
        }
        write!(f, ") -> {}", self.ret)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionDecl {
    pub id: crate::FunctionId,
    pub name: String,
    pub signature: FunctionSignature,
    pub linkage: Linkage,
    pub link_name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallError {
    ResourceLimit(crate::LimitError),
    UnknownFunction(crate::FunctionId),
    UnknownFunctionName(String),
    ConflictingDeclaration(String),
    DuplicateDefinition(crate::FunctionId),
    InvalidLinkName(String),
    LinkNameCollision(String),
    InvalidSignature(String),
    DefinitionMismatch(crate::FunctionId),
    MissingDefinition(crate::FunctionId),
    UnknownValue(ValueId),
    NotFunctionPointer(ValueId),
    ArgumentCount {
        expected: usize,
        got: usize,
    },
    ArgumentType {
        index: usize,
        expected: Type,
        got: Type,
    },
    ReturnType {
        expected: Type,
        got: Type,
    },
    ResultPresence,
    ConventionMismatch,
    AddressTypeMismatch,
    ForbiddenFunctionPointerCast,
}

pub(crate) fn validate_signature(sig: &FunctionSignature) -> Result<(), CallError> {
    validate_signature_with_limits(sig, crate::IrLimits::default())
}

pub fn validate_signature_with_limits(
    sig: &FunctionSignature,
    limits: crate::IrLimits,
) -> Result<(), CallError> {
    crate::limits::Budget::new(limits)
        .and_then(|mut b| b.signature(sig))
        .map_err(CallError::ResourceLimit)?;
    let invalid = || {
        CallError::InvalidSignature(
            "void parameters, malformed types and variadic calls are unsupported".into(),
        )
    };
    let mut signatures = vec![sig];
    while let Some(sig) = signatures.pop() {
        if sig.variadic {
            return Err(invalid());
        }
        if sig.convention == CallingConvention::SysV64
            && sig
                .params
                .iter()
                .chain(std::iter::once(&sig.ret))
                .any(|t| matches!(t, Type::Array(..) | Type::Struct(..) | Type::Tuple(..)))
        {
            return Err(CallError::InvalidSignature(
                "SysV64 aggregate signatures are unsupported".into(),
            ));
        }
        let mut types: Vec<_> = sig.params.iter().collect();
        if sig.ret != Type::Void {
            types.push(&sig.ret);
        }
        while let Some(ty) = types.pop() {
            match ty {
                Type::Void => return Err(invalid()),
                Type::FnPtr(sig) => signatures.push(sig),
                Type::Ptr(t) if **t == Type::Void => {}
                Type::Ptr(t) | Type::Array(t, _) => types.push(t),
                Type::Struct(ts) | Type::Tuple(ts) => types.extend(ts),
                _ => {}
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_decl_with_limits(
    decl: &FunctionDecl,
    limits: crate::IrLimits,
) -> Result<(), CallError> {
    validate_signature_with_limits(&decl.signature, limits)?;
    match (&decl.linkage, &decl.link_name) {
        (Linkage::Internal, None) => Ok(()),
        (Linkage::External, Some(name)) if !name.is_empty() && !name.contains('\0') => Ok(()),
        _ => Err(CallError::InvalidLinkName(decl.name.clone())),
    }
}
