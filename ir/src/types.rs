// SPDX-License-Identifier: MPL-2.0

use core::fmt;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Endian {
    Little,
    Big,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DataLayout {
    pub ptr_bits: u32,
    pub endian: Endian,
}

impl DataLayout {
    pub fn default_64bit_le() -> Self {
        Self {
            ptr_bits: 64,
            endian: Endian::Little,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    Void,

    I1,
    I8,
    I16,
    I32,
    I64,
    I128,
    U8,
    U16,
    U32,
    U64,
    U128,

    F16,
    F32,
    F64,

    Ptr(Box<Type>),

    Array(Box<Type>, u64),
    Struct(Vec<Type>),
    Tuple(Vec<Type>),
}

impl Type {
    pub fn ptr_to(inner: Type) -> Self {
        Type::Ptr(Box::new(inner))
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Void => write!(f, "void"),

            Type::I1 => write!(f, "i1"),
            Type::I8 => write!(f, "i8"),
            Type::I16 => write!(f, "i16"),
            Type::I32 => write!(f, "i32"),
            Type::I64 => write!(f, "i64"),
            Type::I128 => write!(f, "i128"),

            Type::U8 => write!(f, "u8"),
            Type::U16 => write!(f, "u16"),
            Type::U32 => write!(f, "u32"),
            Type::U64 => write!(f, "u64"),
            Type::U128 => write!(f, "u128"),

            Type::F16 => write!(f, "f16"),
            Type::F32 => write!(f, "f32"),
            Type::F64 => write!(f, "f64"),

            Type::Ptr(inner) => write!(f, "ptr<{}>", inner),

            Type::Array(inner, n) => write!(f, "array<{}, {}>", inner, n),
            Type::Struct(fields) => {
                write!(f, "struct{{")?;
                for (i, t) in fields.iter().enumerate() {
                    if i != 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{t}")?;
                }
                write!(f, "}}")
            }
            Type::Tuple(items) => {
                write!(f, "tuple<")?;
                for (i, t) in items.iter().enumerate() {
                    if i != 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{t}")?;
                }
                write!(f, ">")
            }
        }
    }
}
