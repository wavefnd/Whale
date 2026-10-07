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
    if !matches!(command.as_str(), "lower" | "verify" | "print") {
        fail(format!("unsupported IR command: {command}"));
    }
    #[cfg(not(feature = "socket-cli"))]
    if command == "lower" {
        eprintln!("Error: 'whale ir lower' requires feature 'socket-cli'.");
        eprintln!("Build/run with: cargo run -p whale --features socket-cli -- ir lower ...");
        process::exit(2);
    }
    let mut input = None;
    let mut output = None;
    #[cfg(feature = "socket-cli")]
    let mut target = None;
    #[cfg(feature = "socket-cli")]
    let mut do_verify = true;
    let mut args = args.iter().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-o" if command != "verify" => {
                if output.is_some() {
                    fail("duplicate -o option");
                }
                output = Some(args.next().unwrap_or_else(|| fail("-o requires a path")));
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
    println!("  verify <input.wir>             Read and verify format 3 typed IR");
    println!("  print <input.wir> [-o <path>]   Verify and write canonical typed IR");
    println!("  lower <socket.json> [-o <path>] Lower AST JSON (requires socket-cli)");
    println!("  lower options: --target x86_64-whale-linux, --no-verify");
    println!("  AST envelope: format_version: 2, semantics_version: 1, features: [], program: AST");
    println!("  Typed IR requires format_version 3 and semantics_version 1.");
    println!("  Integer literals are decimal; floats use exact-width 0x storage bits.");
    println!("  Unknown fields, versions, instructions and trailing input are rejected.");
}
