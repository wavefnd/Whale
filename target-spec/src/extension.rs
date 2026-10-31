// SPDX-License-Identifier: MPL-2.0
//! Open target contracts. Native object identities remain a separate contract.
use crate::{DataLayout, Target};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutputKind {
    NativeObject(crate::ObjectTarget),
    JvmClass { major: u16 },
    CliAssembly,
    Custom(String),
    Bytecode(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemoryModel {
    Tracked,
    Native,
    Custom(String),
}

/// Scalar ABI alignments for byte sizes 1, 2, 4, 8 and 16 respectively.
/// Sizes are defined by IR types; placement is selected by the target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StorageLayout {
    pub data: DataLayout,
    pub scalar_align: [u32; 5],
    pub pointer_align: u32,
    pub large_array: Option<(u64, u32)>,
}

impl StorageLayout {
    pub fn validate(self) -> Result<(), String> {
        if !matches!(self.data.ptr_bits, 32 | 64) {
            return Err("storage layout requires 32-bit or 64-bit pointers".into());
        }
        let valid = |n: u32| n.is_power_of_two();
        if !self.scalar_align.into_iter().all(valid)
            || !valid(self.pointer_align)
            || self.large_array.is_some_and(|(_, a)| !valid(a))
        {
            return Err("storage alignment must be a nonzero power of two".into());
        }
        // Element stride must preserve its natural alignment.
        if self.scalar_align.iter().enumerate().any(|(i, a)| *a > (1 << i))
            || self.pointer_align > self.data.ptr_bits / 8
        {
            return Err("scalar alignment cannot exceed scalar storage size".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetSpec {
    pub id: String,
    pub output: OutputKind,
    pub storage: StorageLayout,
    pub memory: MemoryModel,
}

/// The built-in versioned bytecode target has tracked 64-bit little-endian storage.
/// It does not describe a native machine or promise a native object encoder.
pub fn whale_bytecode_v1() -> TargetSpec {
    TargetSpec {
        id: "whale-bytecode-v1".into(),
        output: OutputKind::Bytecode("whale-bytecode-v1".into()),
        storage: StorageLayout {
            data: DataLayout { ptr_bits: 64, endian: crate::Endian::Little },
            scalar_align: [1, 2, 4, 8, 16],
            pointer_align: 8,
            large_array: Some((16, 16)),
        },
        memory: MemoryModel::Tracked,
    }
}

impl TargetSpec {
    pub fn lookup_builtin(id: &str) -> Result<Self, crate::TargetError> {
        if id == "whale-bytecode-v1" {
            return Ok(whale_bytecode_v1());
        }
        Target::lookup(id).map(Target::specification)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty() || self.id.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err(
                "target identity must be nonempty and contain no whitespace or controls".into()
            );
        }
        self.storage.validate()?;
        if let OutputKind::NativeObject(object) = self.output {
            if object.address_bits != self.storage.data.ptr_bits
                || object.endian != self.storage.data.endian
            {
                return Err("native object and storage layouts disagree".into());
            }
        }
        Ok(())
    }
}

impl Target {
    pub fn specification(self) -> TargetSpec {
        TargetSpec {
            id: self.name().into(),
            output: OutputKind::NativeObject(self.object_target()),
            storage: StorageLayout {
                data: self.data_layout(),
                scalar_align: [1, 2, 4, 8, 16],
                pointer_align: 8,
                large_array: Some((16, 16)),
            },
            memory: MemoryModel::Tracked,
        }
    }
}
