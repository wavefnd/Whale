use std::env;

use crate::commands;

pub fn run() {
    let mut args = env::args().skip(1);

    let Some(cmd) = args.next() else {
        print_help();
        return;
    };

    match cmd.as_str() {
        "asm" => {
            commands::asm::run(args.collect());
        }
        "object" => {
            commands::object::run(args.collect());
        }
        "link" => {
            commands::linker::run(args.collect());
        }
        _ => {
            eprintln!("Unknown command: {}", cmd);
            print_help();
        }
    }
}

fn print_help() {
    println!("Usage:");
    println!("  whale asm [--amd64 | --aarch64] <input> -o <output>");
    println!();
    println!("Commands:");
    println!("  asm     Assemble source file");
    println!("  object  Generate object file from binary or IR");
    println!("  link    Link object files into an executable");
}
