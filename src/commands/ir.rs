use std::{env, fs, process};

use ir::lower_ast::{frontend, lower_o0};
use ir::{DataLayout, printer, verifier};

pub fn run(args: Vec<String>) {
    if args.is_empty() {
        print_help();
        return;
    }

    let mut sub: Option<String> = None;
    let mut input: Option<String> = None;
    let mut output: Option<String> = None;

    let mut target: String = "x86_64-whale-linux".to_string();
    let mut do_verify = true;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" => { print_help(); return; }

            "lower" => {
                sub = Some("lower".to_string());
            }

            "--target" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: --target requires a value");
                    process::exit(1);
                }
                target = args[i + 1].clone();
                i += 1;
            }

            "-o" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: -o requires a value");
                    process::exit(1);
                }
                output = Some(args[i + 1].clone());
                i += 1;
            }

            "--no-verify" => do_verify = false,

            s if !s.starts_with('-') && input.is_none() => {
                input = Some(s.to_string());
            }

            _ => {}
        }
        i += 1;
    }

    let sub = sub.unwrap_or_else(|| {
        eprintln!("Error: missing subcommand");
        print_help();
        process::exit(1);
    });

    if sub != "lower" {
        eprintln!("Error: unsupported subcommand: {}", sub);
        print_help();
        process::exit(1);
    }

    let input = input.unwrap_or_else(|| {
        eprintln!("Error: missing input file");
        print_help();
        process::exit(1);
    });

    // 1) Read socket json
    let src = fs::read_to_string(&input).unwrap_or_else(|e| {
        eprintln!("Failed to read {}: {}", input, e);
        process::exit(1);
    });

    // 2) JSON -> frontend::Program
    let program: frontend::Program = serde_json::from_str(&src).unwrap_or_else(|e| {
        eprintln!("Failed to parse socket JSON: {}", e);
        process::exit(1);
    });

    // 3) lower
    let module = lower_o0(&program, &target, DataLayout::default_64bit_le()).unwrap_or_else(|e| {
        eprintln!("lower_o0 failed: {:?}", e);
        process::exit(1);
    });

    // 4) verify(optional)
    if do_verify {
        verifier::verify_module(&module).unwrap_or_else(|e| {
            eprintln!("verify failed: {:?}", e);
            process::exit(1);
        });
    }

    // 5) print
    let txt = printer::print_module(&module);

    // 6) output
    if let Some(out) = output {
        fs::write(&out, txt.as_bytes()).unwrap_or_else(|e| {
            eprintln!("Failed to write {}: {}", out, e);
            process::exit(1);
        });
        println!("Wrote IR to {}", out);
    } else {
        print!("{}", txt);
    }
}

fn print_help() {
    println!("Usage:");
    println!("  whale ir <command> [options]");
    println!();
    println!("Commands:");
    println!("  lower <socket.json>   Lower socket AST JSON into Whale IR text");
    println!();
    println!("Options (ir lower):");
    println!("  -o <path>        Write printed IR text to file (default: stdout)");
    println!("  --target <t>     Target triple string (default: x86_64-whale-linux)");
    println!("  --no-verify      Skip verifier");
    println!("  --no-print       Do not print IR text to stdout (useful with -o)");
    println!("  --trace          Print trace logs (parse/lower/verify/write steps)");
    println!("  --help           Show this help");
    println!();
    println!("Input format:");
    println!("  <socket.json> must be a JSON that matches ir::lower_ast::frontend::Program");
    println!("  (SOCKET_VERSION must match the current socket version in ir::lower_ast)");
    println!();
    println!("Examples:");
    println!("  whale ir lower program.json");
    println!("  whale ir lower program.json -o out.wir");
    println!("  whale ir lower program.json --target x86_64-whale-linux");
    println!("  whale ir lower program.json --no-verify");
    println!("  whale ir lower program.json -o out.wir --no-print");
    println!("  whale ir lower program.json --trace");
}
