// SPDX-License-Identifier: MPL-2.0
//! Addressable scalar globals, separate from compile-time constant declarations.
use crate::{ConstValue, GlobalId, IrLimits, Linkage, Type};

#[derive(Clone, Debug, PartialEq)]
pub enum GlobalStorageInit {
    Value(ConstValue),
    /// Physical zero-fill does not initialize a value.
    Uninitialized,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GlobalStorage {
    pub id: GlobalId,
    pub name: String,
    pub ty: Type,
    pub init: GlobalStorageInit,
    pub readonly: bool,
    pub align: u32,
    pub linkage: Linkage,
    pub link_name: Option<String>,
}

impl GlobalStorage {
    /// A builder assigns a fresh GlobalId when this declaration is added.
    pub fn internal(
        name: impl Into<String>,
        ty: Type,
        init: GlobalStorageInit,
        readonly: bool,
        align: u32,
    ) -> Self {
        Self {
            id: GlobalId(0),
            name: name.into(),
            ty,
            init,
            readonly,
            align,
            linkage: Linkage::Internal,
            link_name: None,
        }
    }

    pub fn validate_with_limits(&self, limits: IrLimits) -> Result<(), GlobalStorageError> {
        #[cfg(whale_wave_ir)]
        {
            crate::verifier::wave_semantics::validate_global_boundary(self, limits)
        }
        #[cfg(not(whale_wave_ir))]
        {
            crate::limits::Budget::new(limits)
                .and_then(|mut b| b.ty(&self.ty))
                .map_err(GlobalStorageError::ResourceLimit)?;
            if !matches!(
                self.ty,
                Type::Bool
                    | Type::I1
                    | Type::U1
                    | Type::I8
                    | Type::U8
                    | Type::I16
                    | Type::U16
                    | Type::I32
                    | Type::U32
                    | Type::I64
                    | Type::U64
                    | Type::I128
                    | Type::U128
                    | Type::F16
                    | Type::F32
                    | Type::F64
                    | Type::Ptr(_)
                    | Type::FnPtr(_)
            ) {
                return Err(GlobalStorageError::InvalidType);
            }
            crate::function::validate_signature_with_limits(
                &crate::FunctionSignature::whale(vec![self.ty.clone()], Type::Void),
                limits,
            )
            .map_err(|error| match error {
                crate::CallError::ResourceLimit(error) => GlobalStorageError::ResourceLimit(error),
                _ => GlobalStorageError::InvalidType,
            })?;
            if !self.align.is_power_of_two() {
                return Err(GlobalStorageError::InvalidAlignment);
            }
            match &self.init {
                GlobalStorageInit::Value(value)
                    if !crate::constant::valid_constant(&self.ty, value) =>
                {
                    return Err(GlobalStorageError::InvalidInitializer)
                },
                GlobalStorageInit::Uninitialized if self.readonly => {
                    return Err(GlobalStorageError::ReadOnlyUninitialized)
                },
                _ => {},
            }
            match (&self.linkage, &self.link_name) {
                (Linkage::Internal, None) => Ok(()),
                (Linkage::External, Some(name)) if !name.is_empty() && !name.contains('\0') => {
                    Ok(())
                },
                _ => Err(GlobalStorageError::InvalidLinkName),
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GlobalStorageError {
    ResourceLimit(crate::LimitError),
    InvalidType,
    InvalidInitializer,
    ReadOnlyUninitialized,
    InvalidAlignment,
    InvalidLinkName,
    DuplicateIdentity,
    DuplicateName,
    LinkNameCollision,
    UnknownGlobal,
    AddressTypeMismatch,
}

impl std::fmt::Display for GlobalStorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "global storage error: {self:?}")
    }
}

impl std::error::Error for GlobalStorageError {}
