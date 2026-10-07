use std::{fs, io::Read, process};

#[cfg(feature = "socket-cli")]
use ir::lower_ast::{interchange, lower_o0};
use ir::{parse_module, printer, IrLimits};

fn fail(message: impl std::fmt::Display) -> ! {
    eprintln!("Error: {message}");
    process::exit(1);
}
pub fn run(args: Vec<String>) {
    if args.is_empty() || args.iter().any(|s| s == "--help") {
        print_help();
        return;
    }
    let command = &args[0];
    if !matches!(command.as_str(), "lower" | "verify" | "print" | "run") {
        fail(format!("unsupported IR command: {command}"));
    }
    #[cfg(not(feature = "socket-cli"))]
    if command == "lower" {
        eprintln!("Error: 'whale ir lower' requires feature 'socket-cli'.");
        eprintln!("Build/run with: cargo run -p whale --features socket-cli -- ir lower ...");
        process::exit(2);
    }
    let mut function = None;
    let mut arguments = Vec::new();
    let mut max_steps = None;
    let mut max_memory = None;
    let mut input = None;
    let mut output = None;
    #[cfg(feature = "socket-cli")]
    let mut target = None;
    #[cfg(feature = "socket-cli")]
    let mut do_verify = true;
    let mut args = args.iter().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-o" if matches!(command.as_str(), "lower" | "print") => {
                if output.is_some() {
                    fail("duplicate -o option");
                }
                output = Some(args.next().unwrap_or_else(|| fail("-o requires a path")));
            }
            "--function" if command == "run" => {
                if function.is_some() {
                    fail("duplicate --function option");
                }
                let value = args
                    .next()
                    .unwrap_or_else(|| fail("--function requires @fN"));
                function = Some(ir::FunctionId(
                    value
                        .strip_prefix("@f")
                        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
                        .and_then(|s| s.parse().ok())
                        .unwrap_or_else(|| fail("--function requires @fN with a u32 ID")),
                ));
            }
            "--arg" if command == "run" => arguments.push(
                args.next()
                    .unwrap_or_else(|| fail("--arg requires a literal")),
            ),
            "--max-steps" if command == "run" => {
                if max_steps.is_some() {
                    fail("duplicate --max-steps option");
                }
                let value = args
                    .next()
                    .unwrap_or_else(|| fail("--max-steps requires a u64 count"));
                max_steps = Some(
                    value
                        .parse::<u64>()
                        .ok()
                        .filter(|_| !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()))
                        .unwrap_or_else(|| fail("--max-steps requires a decimal u64 count")),
                );
            }
            "--max-memory" if command == "run" => {
                if max_memory.is_some() {
                    fail("duplicate --max-memory option");
                }
                let value = args
                    .next()
                    .unwrap_or_else(|| fail("--max-memory requires a u64 byte count"));
                max_memory = Some(
                    value
                        .parse::<u64>()
                        .ok()
                        .filter(|_| !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()))
                        .unwrap_or_else(|| fail("--max-memory requires a decimal u64 byte count")),
                );
            }
            #[cfg(feature = "socket-cli")]
            "--target" if command == "lower" => {
                if target.is_some() {
                    fail("duplicate --target option");
                }
                target = Some(
                    args.next()
                        .unwrap_or_else(|| fail("--target requires a value")),
                );
            }
            #[cfg(feature = "socket-cli")]
            "--no-verify" if command == "lower" => do_verify = false,
            s if !s.starts_with('-') && input.is_none() => input = Some(arg),
            _ => fail(format!("unexpected argument: {arg}")),
        }
    }
    if command == "run" && function.is_none() {
        fail("run requires --function @fN");
    }
    let input = input.unwrap_or_else(|| fail("missing input file"));
    #[cfg(feature = "socket-cli")]
    let lower_target = if command == "lower" {
        Some(
            ir::Target::lookup(target.map(String::as_str).unwrap_or("x86_64-whale-linux"))
                .unwrap_or_else(|e| fail(e)),
        )
    } else {
        None
    };
    let max_bytes = IrLimits::default().max_input_bytes;
    #[cfg(feature = "socket-cli")]
    let max_bytes = if command == "lower" {
        interchange::DEFAULT_MAX_INPUT_BYTES
    } else {
        max_bytes
    };
    let source = (|| -> std::io::Result<String> {
        let mut source = String::new();
        fs::File::open(input)?
            .take((max_bytes + 1) as u64)
            .read_to_string(&mut source)?;
        Ok(source)
    })()
    .unwrap_or_else(|e| fail(format!("failed to read {input}: {e}")));
    let module = if command == "lower" {
        #[cfg(feature = "socket-cli")]
        {
            let target = lower_target.expect("lower target checked before reading input");
            let program = interchange::decode(&source)
                .unwrap_or_else(|e| fail(format!("Failed to parse socket JSON: {e}")));
            let module = lower_o0(&program, target.name(), target.data_layout())
                .unwrap_or_else(|e| fail(format!("lower_o0 failed: {e:?}")));
            if do_verify {
                ir::verify_module(&module)
                    .unwrap_or_else(|e| fail(format!("verify failed: {e:?}")));
            }
            module
        }
        #[cfg(not(feature = "socket-cli"))]
        unreachable!("lower rejected before reading input")
    } else {
        parse_module(&source).unwrap_or_else(|e| fail(format!("{input}:{e}")))
    };
    if command == "verify" {
        println!("Verified IR {input}");
        return;
    }
    if command == "run" {
        execute(
            &module,
            input,
            function.expect("required function"),
            &arguments,
            max_steps,
            max_memory,
        );
        return;
    }
    let text = printer::print_module(&module);
    if let Some(output) = output {
        super::output::publish(input.as_ref(), output.as_ref(), text.as_bytes())
            .unwrap_or_else(|e| fail(format!("failed to write {output}: {e}")));
        println!("Wrote IR to {output}");
    } else {
        print!("{text}");
    }
}
fn print_help() {
    println!("Usage: whale ir <command> [options]");
    println!("  verify <input.wir>             Read and verify format 3/4 typed IR");
    println!("  print <input.wir> [-o <path>]   Verify and write canonical typed IR");
    println!("  run <input.wir> --function @fN [--arg <literal> ...] [--max-steps <u64>] [--max-memory <u64>]");
    println!("    Execute integer/bool, control flow and tracked stack memory; default 1000000 steps, 64 MiB storage.");
    println!("  lower <socket.json> [-o <path>] Lower AST JSON (requires socket-cli)");
    println!("  lower options: --target x86_64-whale-linux, --no-verify");
    println!("  AST envelope: format_version: 2, semantics_version: 1, features: [], program: AST");
    println!("  Typed IR reads format_version 3/4, writes 4; semantics_version 1. Legacy undef is invalid.");
    println!("  Integer literals are decimal; floats use exact-width 0x storage bits.");
    println!("  Unknown fields, versions, instructions and trailing input are rejected.");
}

fn execute(
    module: &ir::Module,
    input: &str,
    function: ir::FunctionId,
    literals: &[&String],
    max_steps: Option<u64>,
    max_memory: Option<u64>,
) {
    let fun = module
        .functions
        .iter()
        .find(|f| f.id == function)
        .unwrap_or_else(|| fail(format!("{input}: no body for @f{}", function.0)));
    if fun.params.len() != literals.len() {
        fail(format!(
            "{input}: expected {} arguments, got {}",
            fun.params.len(),
            literals.len()
        ));
    }
    let arguments: Vec<_> = fun.params.iter().zip(literals).enumerate().map(|(index, (param, literal))| {
        scalar_argument(&param.ty, literal).unwrap_or_else(|| {
            fail(format!("{input}: argument {index} must be an exact decimal {} literal (bool uses true/false)", param.ty))
        })
    }).collect();
    let mut options = ir::InterpreterOptions::default();
    if let Some(limit) = max_steps {
        options.max_steps = limit;
    }
    if let Some(limit) = max_memory {
        options.memory_limits.max_bytes = limit;
    }
    let result = ir::interpret_with_options(module, function, &arguments, options)
        .unwrap_or_else(|e| fail(format!("{input}: {e}")));
    match result.value {
        Some(ir::ConstValue::I(v)) => println!("{} {v}", fun.ret_ty),
        Some(ir::ConstValue::U(v)) => println!("{} {v}", fun.ret_ty),
        Some(ir::ConstValue::Bool(v)) => println!("bool {v}"),
        None => println!("void"),
        Some(ir::ConstValue::F(_)) => unreachable!("interpreter scalar subset"),
    }
}

fn scalar_argument(ty: &ir::Type, literal: &str) -> Option<ir::ConstValue> {
    let decimal = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    match ty {
        ir::Type::Bool => match literal {
            "true" => Some(ir::ConstValue::Bool(true)),
            "false" => Some(ir::ConstValue::Bool(false)),
            _ => None,
        },
        ir::Type::I1
        | ir::Type::I8
        | ir::Type::I16
        | ir::Type::I32
        | ir::Type::I64
        | ir::Type::I128
            if decimal(literal.strip_prefix('-').unwrap_or(literal)) =>
        {
            literal.parse().ok().map(ir::ConstValue::I)
        }
        ir::Type::U1
        | ir::Type::U8
        | ir::Type::U16
        | ir::Type::U32
        | ir::Type::U64
        | ir::Type::U128
            if decimal(literal) =>
        {
            literal.parse().ok().map(ir::ConstValue::U)
        }
        _ => None,
    }
}
