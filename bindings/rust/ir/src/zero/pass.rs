// SPDX-License-Identifier: MPL-2.0

use crate::Module;

pub fn run_zero_pass(_m: &mut Module) {
    // no-op
}

/// Explicit pass-runner adapter. The legacy O0 API above remains a no-op.
pub struct ZeroPass;

impl crate::pass::Pass for ZeroPass {
    fn name(&self) -> &str {
        "zero"
    }

    fn run(&mut self, _module: &mut Module) -> Result<crate::pass::PassChange, String> {
        Ok(crate::pass::PassChange::default())
    }
}
