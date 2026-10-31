// SPDX-License-Identifier: MPL-2.0
//! Typed buffers and diagnostic text for the native A64 module.
use assembler::fixed::{native, Context, Instruction};

use assembler::{AsmError, AssemblerOutput, AssemblyLimits};

use native::{FixedContext, FixedInput, FixedInstruction, FixedResult};

unsafe extern "C" {
    fn whale_aarch64_assemble(packet: *const FixedInput, result: *mut FixedResult) -> i32;

    fn whale_aarch64_encode(
        instruction: *const FixedInstruction,
        context: *mut FixedContext,
        result: *mut FixedResult,
    ) -> i32;
}

fn render(code: u64, text: &str, _detail: u64) -> AsmError {
    let message = match code {
        1000 => return assembler::fixed::error(format!("invalid A64 register {text:?}")),
        1001 => "register widths must match",
        1002 => "SP is not valid in this operand",
        1003 => "shift requires kind and amount",
        1004 => "unsupported shift kind",
        1005 => "memory operand requires [base, #offset]; writeback is unsupported",
        1006 => "memory operand accepts a base and immediate offset",
        1007 => "memory base requires X0..X30 or SP",
        1008 => "indirect control flow requires X0..X30",
        1009 => "mov cannot mix SP and ZR",
        1010 => "wide move requires register, imm16 and optional lsl",
        1011 => "wide move shift must be 0/16 for W, or 0/16/32/48 for X",
        1012 => "logical register operation requires three operands and optional shift",
        1013 => "adr requires an X register",
        1014 => "invalid A64 branch condition",
        1015 => return assembler::fixed::error(format!("unsupported A64 instruction {text:?}")),
        1016 => "mov immediate does not fit the destination register width",
        1017 => "mov immediate requires a single 16-bit MOVZ/MOVN; use movz/movk for larger values",
        1018 => "comparison requires operands",
        1019 => "arithmetic requires three operands and optional shift",
        1020 => "immediate arithmetic uses SP for register 31, except the flag-setting destination",
        1021 => "immediate add/sub accepts only lsl #0 or #12",
        1022 => "add/sub does not support ror",
        1023 => "byte/halfword transfer requires a W register",
        1024 => "ldrsw requires an X register",
        1025 => "unsigned load/store offset must be scaled, nonnegative and fit 12 bits",
        1026 => "unscaled load/store offset must fit signed 9 bits",
        _ => return AsmError::Backend("A64 diagnostic transport"),
    };
    assembler::fixed::error(message)
}

pub(super) fn assemble(
    name: &str,
    source: &str,
    limits: &AssemblyLimits,
) -> Result<AssemblerOutput, AsmError> {
    native::assemble(name, source, limits, "aarch64", false, whale_aarch64_assemble, render)
}

pub(super) fn encode(
    instruction: &Instruction,
    context: &mut Context<'_>,
) -> Result<u32, AsmError> {
    native::encode(instruction, context, whale_aarch64_encode, render)
}

#[cfg(all(test, feature = "rust-reference"))]
mod tests {
    use super::*;

    use assembler::fixed::FixedIsa;

    struct ReferenceA64;

    impl FixedIsa for ReferenceA64 {
        fn id(&self) -> &'static str {
            "aarch64"
        }

        fn hash_comments(&self) -> bool {
            false
        }

        fn encode(
            &self,
            instruction: &Instruction,
            context: &mut Context<'_>,
        ) -> Result<u32, AsmError> {
            crate::encoder::encode(instruction, context)
        }
    }

    fn snapshot(output: &AssemblerOutput) -> String {
        let sections: Vec<_> = output
            .sections
            .iter()
            .map(|section| {
                let relocations: Vec<_> = section
                    .relocs
                    .iter()
                    .map(|relocation| {
                        (
                            relocation.offset,
                            &relocation.symbol,
                            format!("{:?}", relocation.kind),
                            relocation.addend,
                        )
                    })
                    .collect();
                (
                    &section.name,
                    section.kind,
                    section.align,
                    &section.data,
                    section.zero_fill,
                    relocations,
                )
            })
            .collect();
        let symbols: Vec<_> = output
            .symbols
            .iter()
            .map(|symbol| {
                (
                    &symbol.name,
                    symbol.kind,
                    symbol.size,
                    symbol.visibility,
                    symbol.section_index,
                    symbol.offset,
                    symbol.is_global,
                )
            })
            .collect();
        format!("{sections:?}/{symbols:?}")
    }

    fn compare_result(
        actual: Result<AssemblerOutput, AsmError>,
        expected: Result<AssemblerOutput, AsmError>,
        source: &str,
    ) {
        match (actual, expected) {
            (Ok(actual), Ok(expected)) => {
                assert_eq!(snapshot(&actual), snapshot(&expected), "{source}")
            },
            (Err(actual), Err(expected)) => {
                assert_eq!(actual.to_string(), expected.to_string(), "{source}")
            },
            (actual, expected) => {
                panic!("A64 divergence {source:?}: {:?}/{:?}", actual.err(), expected.err())
            },
        }
    }

    fn compare(source: &str) {
        compare_result(
            assemble("oracle.s", source, &AssemblyLimits::default()),
            assembler::fixed::reference::assemble(
                &ReferenceA64,
                "oracle.s",
                source,
                &AssemblyLimits::default(),
            ),
            source,
        );
    }

    #[test]
    fn native_a64_matches_retained_independent_instruction_and_error_policy() {
        for source in [
            "nop",
            "ret",
            "RET X9",
            "mov x0,sp",
            "mov sp,x0",
            "mov wzr,#0",
            "mov x0,#-65536",
            "mov w1,#-2147483648",
            "mov x2,#0xffff000000000000",
            "movz w0,#3,LSL\u{2003}#16",
            "b.eq #-4",
            "b.hs #4",
            "b.lo #0",
            "b.nv #8",
            "adr x0,#-1",
            "cbnz xzr,#4",
            "ldr x0,[sp,#32760]",
            "ldur xzr,[x30,#-256]",
            "ldr w0,[x1]",
            "strh wzr,[sp,#2]",
            "mov x31,#1",
            "mov X01,x0",
            "mov x0,w1",
            "mov sp,xzr",
            "mov xzr,sp",
            "add sp,x0,x1",
            "adds sp,sp,#1",
            "add xzr,x0,#1",
            "add x0,xzr,#1",
            "cmp xzr,#1",
            "cmp",
            "cmp BAD",
            "cmp x0",
            "cmp bad,x0,x1,x2",
            "add x0,x1,x2,ror #1",
            "add x0,x1,#1,lsl #1",
            "mov x0,#0x12345678",
            "mov w0,#4294967296",
            "mov x0,#-9223372036854775809",
            "mov x0,#0x+1",
            "mov x0,#0b+1",
            "mov x0,#0x-1",
            "mov x0,#-0x-1",
            "mov x0,#0x_+_1_",
            "mov x0,#0x-80000000000000000000000000000000",
            "mov x0,#-0x-80000000000000000000000000000000",
            "svc #0x+1",
            "mov x0,#-170141183460469231731687303715884105728",
            "mov x0,#170141183460469231731687303715884105728",
            "movk w0,#1,lsl #32",
            "movz x0,#65536",
            "movz sp,#1",
            "movz x0,#1,lsr #16",
            "lsl w0,w1,#32",
            "asr x0,x1,#64",
            "and x0,x1,x2,rotate #1",
            "orr x0,x1,x2,lsl #1 #2",
            "ldr x0,[w1]",
            "ldr x0,[xzr]",
            "ldr x0,[]",
            "ldr x0,[x1,#1,#2]",
            "ldr x0,[x1,,#2]",
            "ldr x0,[x1,#1]",
            "ldr x0,[x1] !",
            "ldur x0,[sp,#256]",
            "ldrsw w0,[x1]",
            "ldrb x0,[x1]",
            "br sp",
            "ret xzr",
            "ret w0",
            "svc #-1",
            "svc #65536",
            "b.eq #2",
            "b #134217728",
            "adr w0,#0",
            "b.what #0",
            "b.eq",
            "unsupported",
            "mov bad,#1\nfoo:\nfoo:\n",
            ".word missing\n.invalid\n",
            "mov bad,#1\n.bss\nnop\n",
        ] {
            compare(source);
        }
    }

    #[test]
    fn native_a64_matches_sections_symbols_relocations_and_legacy_source_order() {
        for source in [
            ".global entry\n.extern external\nentry: bl external+4\nb local\nlocal: adr x0,entry\nret\n",
            ".text\nstart: b datum\n.data\ndatum: .quad start+8\n.word start-4\n.bss\n.zero 4096\n.align 12\n",
            ".section .custom\n.byte 1,2,-1\n.hword 65535\n.short -32768\n.long 4294967295\n.balign 16\n",
            "\u{2003}MAIN:\u{00a0}MOV\u{2003}X0,#1\r\nret // preserved comment\n",
            ".global missing\nnop\n", ".extern x\nx: nop\n", "x: nop\n.extern x\n",
            ".bss\n.word 0\n", ".text\n.byte 1\nnop\n", ".word undefined\n", ".byte undefined\n",
        ] { compare(source); }
    }

    #[test]
    fn generated_native_a64_fields_match_independent_reference_words() {
        let mut random = 0x0358_6400_0001_u64;
        for _ in 0..5000 {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            let wide = random & 1 != 0;
            let prefix = if wide { "x" } else { "w" };
            let rd = random % 31;
            let rn = (random >> 7) % 31;
            let rm = (random >> 13) % 31;
            let shift = (random >> 19) % if wide { 64 } else { 32 };
            let immediate = (random >> 25) & 4095;
            let source = match (random >> 37) % 8 {
                0 => format!("add {prefix}{rd},{prefix}{rn},{prefix}{rm},lsl #{shift}"),
                1 => format!("subs {prefix}{rd},{prefix}{rn},#{immediate},lsl #12"),
                2 => format!("orr {prefix}{rd},{prefix}{rn},{prefix}{rm},ror #{shift}"),
                3 => format!("asr {prefix}{rd},{prefix}{rn},#{shift}"),
                4 => format!(
                    "movk {prefix}{rd},#{},lsl #{}",
                    random & 65535,
                    ((random >> 48) % if wide { 4 } else { 2 }) * 16
                ),
                5 => format!("ldr {prefix}{rd},[x{rn},#{}]", immediate * if wide { 8 } else { 4 }),
                6 => format!("stur {prefix}{rd},[x{rn},#{}]", ((random >> 45) & 511) as i64 - 256),
                _ => format!(
                    "cbnz {prefix}{rd},#{}",
                    (((random >> 21) & 0x7ffff) as i64 - 0x40000) * 4
                ),
            };
            compare(&source);
        }
    }

    struct Manual<'a> {
        instruction: &'a Instruction,
        native: bool,
    }

    impl FixedIsa for Manual<'_> {
        fn id(&self) -> &'static str {
            "aarch64"
        }

        fn encode(
            &self,
            _instruction: &Instruction,
            context: &mut Context<'_>,
        ) -> Result<u32, AsmError> {
            if self.native {
                encode(self.instruction, context)
            } else {
                crate::encoder::encode(self.instruction, context)
            }
        }
    }

    #[test]
    fn direct_leaf_keeps_manual_spelling_and_context_authority() {
        for (mnemonic, operands) in [
            ("MOV", vec!["x0", "#1"]),
            ("mov", vec!["\u{2003}X0\u{00a0}", "#1"]),
            ("ldr", vec!["x0", " [x1]"]),
            ("ldr", vec!["x0", "[x1] "]),
            ("add", vec!["x0", "x1", "x2", "lsl\u{2003}#1"]),
            ("bl", vec!["Target+4"]),
            ("adr", vec!["x0", "Target-8"]),
        ] {
            let instruction = Instruction {
                mnemonic: mnemonic.into(),
                operands: operands.into_iter().map(str::to_owned).collect(),
            };
            let source = ".extern Target\nnop\n";
            compare_result(
                assembler::fixed::reference::assemble(
                    &Manual { instruction: &instruction, native: true },
                    "manual.s",
                    source,
                    &AssemblyLimits::default(),
                ),
                assembler::fixed::reference::assemble(
                    &Manual { instruction: &instruction, native: false },
                    "manual.s",
                    source,
                    &AssemblyLimits::default(),
                ),
                mnemonic,
            );
        }
    }

    #[test]
    fn malformed_native_records_do_not_fabricate_instruction_words() {
        let mut result = FixedResult { word: u64::MAX, ..FixedResult::default() };
        assert_eq!(unsafe { whale_aarch64_assemble(std::ptr::null(), &mut result) }, 2);
        assert_eq!((result.status, result.word), (2, 0));
        result = FixedResult { word: u64::MAX, ..FixedResult::default() };
        assert_eq!(
            unsafe { whale_aarch64_encode(std::ptr::null(), std::ptr::null_mut(), &mut result) },
            2
        );
        assert_eq!((result.status, result.word), (2, 0));
        let instruction = FixedInstruction {
            mnemonic: native::FixedText { data: std::ptr::null(), count: u64::MAX },
            ..FixedInstruction::default()
        };
        let mut context = FixedContext::default();
        result = FixedResult::default();
        assert_eq!(unsafe { whale_aarch64_encode(&instruction, &mut context, &mut result) }, 2);
        assert_eq!((result.status, result.word), (2, 0));
    }
}
