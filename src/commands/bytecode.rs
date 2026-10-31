//! File and CLI boundaries for the Wave bytecode compiler and verified VM.
use std::{fs, io::Read, process};

use ir::bytecode::{BytecodeLimits, BytecodeProgram};

use super::report::println;

fn fail(message: impl std::fmt::Display) -> ! {
    eprintln!("Error: {message}");
    process::exit(1);
}

pub(super) fn emit(module: &ir::Module, input: &str, output: &str, maximum: Option<usize>) {
    let mut limits = BytecodeLimits::default();
    if let Some(maximum) = maximum {
        limits.max_input_bytes = limits.max_input_bytes.min(maximum as u64);
    }
    let artifacts = crate::builtins::registry_with_bytecode_limits(limits)
        .compile(module, ir::IrLimits::default())
        .unwrap_or_else(|errors| {
            fail(errors.iter().map(|error| error.message.as_str()).collect::<Vec<_>>().join("; "))
        });
    let [artifact]: [_; 1] =
        artifacts.try_into().unwrap_or_else(|_| fail("invalid bytecode artifact count"));
    let limit = maximum.unwrap_or(64 * 1024 * 1024);
    if artifact.bytes.len() > limit {
        fail(format!("bytecode output exceeds {limit} bytes"));
    }
    super::output::publish(input.as_ref(), output.as_ref(), &artifact.bytes)
        .unwrap_or_else(|error| fail(format!("failed to write {output}: {error}")));
    println!("Wrote bytecode to {output}");
}

pub fn run(arguments: Vec<String>) {
    if arguments.is_empty() || arguments.iter().any(|argument| argument == "--help") {
        println!("Usage: whale bytecode verify <input.wbc> [limits]");
        println!(
            "       whale bytecode run <input.wbc> --function @fN [--arg <literal> ...] [limits]"
        );
        println!("Limits: --max-input <bytes>, --max-decoded <bytes>, --max-work <count>, --max-workspace <bytes>");
        println!("Run limits: --max-steps <count>, --max-memory <bytes>");
        println!("Only verified v1 images execute. External runtime imports are unsupported.");
        return;
    }
    let command = arguments[0].as_str();
    if !matches!(command, "verify" | "run") {
        fail(format!("unsupported bytecode command: {command}"));
    }
    let mut limits = BytecodeLimits::default();
    let mut options = ir::InterpreterOptions::default();
    let mut input = None;
    let mut function = None;
    let mut literals = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut arguments = arguments.iter().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--arg" if command == "run" => {
                literals.push(arguments.next().unwrap_or_else(|| fail("--arg requires a literal")));
            },
            "--function" if command == "run" => {
                if function.is_some() {
                    fail("duplicate --function option");
                }
                let value = arguments.next().unwrap_or_else(|| fail("--function requires @fN"));
                let id = value
                    .strip_prefix("@f")
                    .filter(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()))
                    .and_then(|id| id.parse::<u32>().ok())
                    .unwrap_or_else(|| fail("--function requires @fN with a u32 ID"));
                function = Some(ir::FunctionId(id));
            },
            "--max-input" | "--max-decoded" | "--max-work" | "--max-workspace" | "--max-steps"
            | "--max-memory" => {
                if command != "run" && matches!(argument.as_str(), "--max-steps" | "--max-memory") {
                    fail(format!("{argument} is only valid for run"));
                }
                if !seen.insert(argument) {
                    fail(format!("duplicate {argument} option"));
                }
                let value = arguments
                    .next()
                    .unwrap_or_else(|| fail(format!("{argument} requires a count")));
                let count = value
                    .parse::<u64>()
                    .ok()
                    .filter(|_| {
                        !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
                    })
                    .unwrap_or_else(|| fail(format!("{argument} requires a decimal u64 count")));
                match argument.as_str() {
                    "--max-input" => limits.max_input_bytes = count,
                    "--max-decoded" => limits.max_decoded_bytes = count,
                    "--max-work" => {
                        limits.max_work = count;
                        options.ir_limits.max_analysis_work = usize::try_from(count)
                            .unwrap_or_else(|_| fail("--max-work exceeds host count range"));
                    },
                    "--max-workspace" => limits.max_workspace_bytes = count,
                    "--max-steps" => options.max_steps = count,
                    "--max-memory" => options.memory_limits.max_bytes = count,
                    _ => unreachable!(),
                }
            },
            value if !value.starts_with('-') && input.is_none() => input = Some(argument),
            _ => fail(format!("unexpected argument: {argument}")),
        }
    }
    if command == "run" && function.is_none() {
        fail("run requires --function @fN");
    }
    let input = input.unwrap_or_else(|| fail("missing input file"));
    let mut bytes = Vec::new();
    fs::File::open(input)
        .and_then(|file| {
            file.take(limits.max_input_bytes.saturating_add(1)).read_to_end(&mut bytes)
        })
        .unwrap_or_else(|error| fail(format!("failed to read {input}: {error}")));
    if bytes.len() as u64 > limits.max_input_bytes {
        fail("bytecode input byte limit exceeded");
    }
    let mut program = BytecodeProgram::load(&bytes, limits)
        .unwrap_or_else(|error| fail(format!("{input}: {error}")));
    if command == "verify" {
        println!("Verified bytecode {input}");
        return;
    }
    execute(&mut program, function.expect("required function"), &literals, options);
}

fn execute(
    program: &mut BytecodeProgram,
    function: ir::FunctionId,
    literals: &[&String],
    options: ir::InterpreterOptions,
) {
    let info = program
        .function(function)
        .unwrap_or_else(|error| fail(error))
        .unwrap_or_else(|| fail(format!("no body for @f{}", function.0)));
    if !info.has_body {
        fail(format!("no body for @f{}", function.0));
    }
    if info.params.len() != literals.len() {
        fail(format!("expected {} arguments, got {}", info.params.len(), literals.len()));
    }
    let arguments = info
        .params
        .iter()
        .zip(literals)
        .enumerate()
        .map(|(index, (ty, literal))| {
            super::ir::scalar_argument(ty, literal).unwrap_or_else(|| {
                fail(format!("argument {index} requires an exact {ty} scalar literal"))
            })
        })
        .collect::<Vec<_>>();
    let result = program.run(function, &arguments, options).unwrap_or_else(|error| fail(error));
    match result.value.as_ref().map(|value| value.scalar_value()) {
        Some(Some(ir::ConstValue::I(value))) => println!("{} {value}", info.ret),
        Some(Some(ir::ConstValue::U(value))) => println!("{} {value}", info.ret),
        Some(Some(ir::ConstValue::Bool(value))) => println!("bool {value}"),
        Some(Some(ir::ConstValue::F(value))) => println!("{} {value}", info.ret),
        None => println!("void"),
        Some(None) => fail("CLI output requires a scalar or void result"),
    }
}
