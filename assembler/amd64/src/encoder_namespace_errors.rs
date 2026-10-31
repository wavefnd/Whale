// SPDX-License-Identifier: MPL-2.0
//! Rendering of native namespace/layout errors; decisions remain in Wave.
use assembler::AsmError;

pub(super) fn message(code: u64, symbol: &str, limit: usize) -> AsmError {
    let text = match code {
        100 => format!("Local label '{symbol}' has no parent non-local label in scope"),
        101 => format!("Duplicate constant definition '{symbol}'"),
        102 => format!("Constant '{symbol}' conflicts with a label or extern symbol"),
        103 => format!("equ requires a constant expression; unresolved symbol '{symbol}'"),
        104 => format!("Signed expression overflow while resolving {symbol:?}"),
        105 => format!("Constant dependency cycle prevents resolving '{symbol}'"),
        106 => "Symbol addend overflow".into(),
        110 => format!("Duplicate label definition '{symbol}'"),
        111 => format!("Extern symbol '{symbol}' was already defined in this file"),
        112 => format!("Label '{symbol}' conflicts with extern declaration"),
        113 => format!("Global symbol '{symbol}' has no definition or extern declaration"),
        114 => format!("Duplicate symbol metadata for '{symbol}'"),
        115 => format!("Metadata symbol '{symbol}' has no label or extern declaration"),
        116 => format!("symbol '{symbol}' size must be a constant"),
        117 => format!("symbol '{symbol}' size must be a nonnegative u64"),
        118 => format!("Symbol '{symbol}' size exceeds its section"),
        119 => format!("Incompatible section redeclaration '{symbol}'"),
        120 => format!("Standard section '{symbol}' has incompatible kind"),
        121 => "instructions are not valid in BSS".into(),
        122 => format!("Branch relaxation did not converge within {limit} passes"),
        123 => format!(
            "Undefined symbol '{symbol}' (define it, declare with extern, or define with equ)"
        ),
        124 => format!("loop target '{symbol}' is out of 8-bit range"),
        151 => "directive requires numeric expression".into(),
        152 => "res* expects exactly one count operand".into(),
        153 => "res* count must be numeric expression".into(),
        154 => format!("res* count must be constant; unresolved symbol '{symbol}'"),
        155 => "res* count must be non-negative".into(),
        156 => "res* size overflow".into(),
        157 => "align expects one power-of-two operand".into(),
        158 => "align requires numeric expression".into(),
        159 => "align requires a resolved constant".into(),
        160 => "align requires a positive power of two".into(),
        161 => "Alignment size overflow".into(),
        162 => "BSS size overflow".into(),
        163 => format!("Unknown directive {symbol}"),
        164 => "dw only supports numeric expressions".into(),
        165 => "Directive output size overflow".into(),
        166 => "BSS has a nonzero initializer or relocation".into(),
        167 => "db value out of range".into(),
        168 => "Only dd/dq support symbolic expressions".into(),
        169 => "dd only supports numeric expressions".into(),
        170 => "dq only supports numeric expressions".into(),
        171 => "dw value out of range".into(),
        172 => "dd value out of range".into(),
        173 => "dq value out of range".into(),
        _ => return AsmError::Backend("AMD64 namespace diagnostic transport"),
    };
    if matches!(code, 100..=103 | 105 | 110..=118 | 123) {
        AsmError::SymbolError(text)
    } else {
        AsmError::EncodeError(text)
    }
}
