// SPDX-License-Identifier: MPL-2.0
//! Canonical, versioned IR text. The product path prints in Wave and drains
//! bounded chunks through a Rust writer; ordinary Cargo keeps a reference path.
use crate::{ConstExpr, Module};

use std::{fmt, io};

#[cfg(any(test, not(whale_wave_ir)))]
#[path = "printer_reference.rs"]
mod reference;

#[cfg(whale_wave_ir)]
#[path = "printer_wave.rs"]
mod wave;

/// Bounds both the caller-owned transport and the Wave output traversal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrintLimits {
    pub max_output_bytes: usize,
    pub max_nodes: usize,
    pub max_work: usize,
    pub max_depth: usize,
    pub max_stack: usize,
}

impl Default for PrintLimits {
    fn default() -> Self {
        Self {
            max_output_bytes: 64 * 1024 * 1024,
            max_nodes: 1_000_000,
            max_work: 512 * 1024 * 1024,
            max_depth: crate::MAX_IR_NESTING,
            max_stack: 16_384,
        }
    }
}

impl PrintLimits {
    fn validate(self) -> Result<(), PrintError> {
        if self.max_depth > crate::MAX_IR_NESTING {
            return Err(PrintError::Depth);
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum PrintError {
    Output,
    Nodes,
    Work,
    Depth,
    Stack,
    Allocation,
    Backend,
    Io(io::Error),
}

impl fmt::Display for PrintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Output => f.write_str("IR printer output limit exceeded"),
            Self::Nodes => f.write_str("IR printer model limit exceeded"),
            Self::Work => f.write_str("IR printer work limit exceeded"),
            Self::Depth => f.write_str("IR printer depth limit exceeded"),
            Self::Stack => f.write_str("IR printer workspace limit exceeded"),
            Self::Allocation => f.write_str("IR printer allocation failed"),
            Self::Backend => f.write_str("IR printer rejected its model transport"),
            Self::Io(error) => write!(f, "IR printer write failed: {error}"),
        }
    }
}

impl std::error::Error for PrintError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

/// Stream canonical bytes without accumulating the rendered module. A writer
/// failure or resource limit can leave a prefix; it never exceeds the ceiling.
pub fn write_module(
    module: &Module,
    writer: &mut impl io::Write,
    limits: PrintLimits,
) -> Result<(), PrintError> {
    #[cfg(whale_wave_ir)]
    {
        wave::write_module(module, writer, limits)
    }
    #[cfg(not(whale_wave_ir))]
    {
        limits.validate()?;
        // Reference builds retain the existing printer for compatibility.
        crate::limits::check_module(
            module,
            crate::IrLimits {
                max_nodes: limits.max_nodes,
                max_type_depth: limits.max_depth,
                max_const_depth: limits.max_depth,
                ..crate::IrLimits::default()
            },
        )
        .map_err(|_| PrintError::Nodes)?;
        let output = reference::print_module(module);
        if output.len() > limits.max_output_bytes {
            return Err(PrintError::Output);
        }
        writer.write_all(output.as_bytes()).map_err(PrintError::Io)
    }
}

/// Collect canonical bytes within the chosen ceiling and return no partial result.
pub fn try_print_module(module: &Module, limits: PrintLimits) -> Result<String, PrintError> {
    let mut output = CappedBuffer::new(limits.max_output_bytes);
    let result = write_module(module, &mut output, limits);
    if output.allocation_failed {
        return Err(PrintError::Allocation);
    }
    result?;
    String::from_utf8(output.bytes).map_err(|_| PrintError::Backend)
}

struct CappedBuffer {
    bytes: Vec<u8>,
    limit: usize,
    allocation_failed: bool,
}

impl CappedBuffer {
    fn new(limit: usize) -> Self {
        Self { bytes: Vec::new(), limit, allocation_failed: false }
    }
}

impl io::Write for CappedBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("IR printer output limit exceeded"));
        }
        let required = self.bytes.len() + bytes.len();
        if required > self.bytes.capacity() {
            let capacity = self.limit.min(required.max(self.bytes.capacity().saturating_mul(2)));
            if self.bytes.try_reserve_exact(capacity - self.bytes.len()).is_err() {
                self.allocation_failed = true;
                return Err(io::Error::other("IR printer allocation failed"));
            }
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub fn print_module(module: &Module) -> String {
    #[cfg(whale_wave_ir)]
    {
        try_print_module(module, PrintLimits::default()).expect("printing canonical IR")
    }
    #[cfg(not(whale_wave_ir))]
    {
        reference::print_module(module)
    }
}

pub fn print_const_expr(expression: &ConstExpr) -> String {
    #[cfg(whale_wave_ir)]
    {
        let limits = PrintLimits::default();
        let mut output = CappedBuffer::new(limits.max_output_bytes);
        wave::write_expression(expression, &mut output, limits)
            .expect("printing canonical constant expression");
        String::from_utf8(output.bytes).expect("canonical IR is UTF-8")
    }
    #[cfg(not(whale_wave_ir))]
    {
        reference::print_const_expr(expression)
    }
}

#[cfg(test)]
#[path = "printer_tests.rs"]
mod tests;
