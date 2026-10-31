// SPDX-License-Identifier: MPL-2.0
//! Explicit integer constant propagation; the O0 pipeline never enables it.
use super::{Pass, PassChange};

use crate::{IrLimits, Module};

#[cfg(whale_wave_ir)]
#[path = "fold_integer_wave.rs"]
mod wave;

#[derive(Clone, Copy, Debug, Default)]
pub struct IntegerConstantFold {
    pub limits: IrLimits,
}

impl Pass for IntegerConstantFold {
    fn name(&self) -> &str {
        "integer-constant-fold"
    }

    fn run(&mut self, module: &mut Module) -> Result<PassChange, String> {
        #[cfg(whale_wave_ir)]
        return wave::fold(module, self.limits);
        #[cfg(not(whale_wave_ir))]
        {
            let _ = module;
            Err("integer constant folding requires the Wave IR module".into())
        }
    }
}
