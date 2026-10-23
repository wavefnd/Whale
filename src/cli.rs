use crate::commands::report::println;

use std::env;

use crate::commands;

pub fn run() {
    // Decode deliberately so unsupported OS paths/options are diagnostics,
    // rather than env::args panics. Validate all arguments before file I/O.
    let arguments: Vec<String> = env::args_os().skip(1).map(|arg| arg.into_string().unwrap_or_else(|_| {
        eprintln!("Error: CLI arguments must be valid UTF-8; non-UTF-8 paths and options are unsupported");
        std::process::exit(2);
    })).collect();
    let mut args = arguments.into_iter();

    let Some(cmd) = args.next() else {
        print_help();
        return;
    };

    match cmd.as_str() {
        "--help" | "-h" => print_help(),
        "--version" | "-V" => println!("whale {}", env!("CARGO_PKG_VERSION")),
        "asm" => commands::asm::run(args.collect()),
        "object" => commands::object::run(args.collect()),
        "link" => commands::linker::run(args.collect()),
        "ir" => commands::ir::run(args.collect()),
        "disasm" => commands::reverse::disasm(args.collect()),
        "reverse" => commands::reverse::reverse(args.collect()),
        _ => {
            eprintln!("Unknown command: {}", cmd);
            print_help();
            std::process::exit(2);
        },
    }
}

fn print_help() {
    println!("Usage:");
    println!("  whale asm --amd64|--aarch64|--riscv64|--loongarch64 <input> -o <output.o>");
    println!("  whale asm --arch ID <input> -o <output.o>");
    println!("  whale object <input> -o <output>");
    println!("  whale link <input.o|input.a>... -o <executable> [--entry NAME]");
    println!("  whale ir <subcommand> [options]");
    println!("  whale disasm [--amd64] <file.o|executable> [--json]");
    println!("  whale reverse inspect|cfg|xrefs <file> [options]");
    println!("  whale --version | -V");
    println!();
    println!("Commands:");
    println!("  asm     Assemble AMD64/AArch64/RV64I/LoongArch64 source into an ELF64 object");
    println!("  object  Wrap raw bytes in an AMD64 ELF64 object");
    println!("  link    Static Linux AMD64 executable linking");
    println!("  disasm  Disassembly entry point (core currently unavailable)");
    println!("  reverse ELF inspection; CFG/xrefs core currently unavailable");
    println!("  ir      IR tools (verify/print/run; socket-cli enables AST lower)");
}
