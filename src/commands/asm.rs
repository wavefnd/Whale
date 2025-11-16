use std::fs;
use std::process;
use std::time::Instant;

use assembler::{assemble, isa::AMD64};
use assembler::isa::amd64::parser::parse;
use assembler::tokens::tokenize;

pub fn run(args: Vec<String>) {
    if args.is_empty() {
        print_help();
        return;
    }

    let mut arch = None;
    let mut input = None;
    let mut output = None;

    // === Developer Mode Flags ===
    let mut debug_mode = false;
    let mut show_ast = false;
    let mut show_token = false;
    let mut show_bytes = false;
    let mut dump_hex = false;
    let mut dump_bin = false;
    let mut dump_json = false;
    let mut no_color = false;
    let mut no_warn_ext = false;
    let mut show_stats = false;
    let mut trace_enable = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" => print_help(),

            "--amd64" => arch = Some("amd64"),
            "--aarch64" => arch = Some("aarch64"),

            "-o" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: -o requires a file name.");
                    process::exit(1);
                }
                output = Some(args[i + 1].clone());
                i += 1;
            }

            "--debug-whale" => debug_mode = true,
            "--ast" => show_ast = true,
            "--token" => show_token = true,
            "--bytes" => show_bytes = true,
            "--dump-hex" => dump_hex = true,
            "--dump-bin" => dump_bin = true,
            "--dump-json" => dump_json = true,
            "--no-color" => no_color = true,
            "--no-warn-extension" => no_warn_ext = true,
            "--stats" => show_stats = true,
            "--trace" => trace_enable = true,

            s if input.is_none() => input = Some(s.to_string()),

            other => {
                eprintln!("Unknown option: {}", other);
                process::exit(1);
            }
        }
        i += 1;
    }

    let arch = arch.unwrap_or_else(|| {
        eprintln!("Error: architecture must be specified (--amd64 / --aarch64)");
        process::exit(1);
    });

    let input = input.unwrap_or_else(|| {
        eprintln!("Error: missing input file.");
        process::exit(1);
    });

    let output = output.unwrap_or_else(|| {
        eprintln!("Error: missing output (-o <file>)");
        process::exit(1);
    });

    if !output.ends_with(".bin") && !no_warn_ext {
        eprintln!("Warning: WhaleASM produces raw binary. '.bin' extension recommended.");
    }

    if trace_enable { println!("[trace] reading input file"); }

    let src = match fs::read_to_string(&input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to read {}: {}", input, e);
            process::exit(1);
        }
    };

    if trace_enable { println!("[trace] tokenize start"); }

    let tokens = match tokenize(&src) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Tokenize error: {}", e);
            process::exit(1);
        }
    };

    if trace_enable { println!("[trace] tokenize end"); }

    if debug_mode && show_token {
        println!("== TOKENS ==");
        dbg!(&tokens);
    }

    if trace_enable { println!("[trace] parse start"); }

    let ast = match parse(&tokens) {
        Ok(ast) => ast,
        Err(e) => {
            eprintln!("Parse error: {:?}", e);
            process::exit(1);
        }
    };

    if trace_enable { println!("[trace] parse end"); }

    if debug_mode && show_ast {
        println!("== AST ==");
        println!("{:#?}", ast);
    }

    if trace_enable { println!("[trace] assemble start"); }
    let start_time = Instant::now();

    let out = match arch {
        "amd64" => assemble(&src, &AMD64),
        "aarch64" => {
            eprintln!("AArch64 assembler is not implemented yet.");
            process::exit(1);
        }
        _ => {
            eprintln!("Unknown architecture: {}", arch);
            process::exit(1);
        }
    };

    let bytes = match out {
        Ok(b) => b.bytes,
        Err(e) => {
            eprintln!("Assemble error: {:?}", e);
            process::exit(1);
        }
    };

    let elapsed = start_time.elapsed();

    if trace_enable { println!("[trace] assemble end"); }

    if debug_mode && show_bytes {
        println!("== BYTES ==");
        println!("{:02X?}", bytes);
    }

    if debug_mode && dump_hex {
        println!("== HEX DUMP ==");
        for (i, chunk) in bytes.chunks(16).enumerate() {
            print!("{:04X}: ", i * 16);
            for b in chunk {
                print!("{:02X} ", b);
            }
            println!();
        }
    }

    if debug_mode && dump_bin {
        println!("== BINARY DUMP ==");
        for b in &bytes {
            println!("{:08b}", b);
        }
    }

    if debug_mode && dump_json {
        println!("== JSON OUTPUT ==");
        println!("{{");
        println!("  \"bytes\": \"{:02X?}\",", bytes);
        println!("  \"input\": \"{}\"", input);
        println!("}}");
    }

    if let Err(e) = fs::write(&output, &bytes) {
        eprintln!("Failed to write {}: {}", output, e);
        process::exit(1);
    }

    if debug_mode && show_stats {
        println!("== STATS ==");
        println!("Tokens: {}", tokens.len());
        println!("AST nodes: {}", ast.items.len());
        println!("Output bytes: {}", bytes.len());
        println!("Time: {} ms", elapsed.as_millis());
    }

    println!("Wrote {} bytes to {}", bytes.len(), output);
}

fn print_help() {
    println!("Usage:");
    println!("  whale asm --amd64 <input.asm> -o <output.bin>");
    println!("  whale asm --aarch64 <input.asm> -o <output.bin>");
    println!();
    println!("Developer Options:");
    println!("  --debug-whale       Enable dev mode");
    println!("  --token             Show tokens");
    println!("  --ast               Show AST");
    println!("  --bytes             Show raw bytes");
    println!("  --dump-hex          Hex dump");
    println!("  --dump-bin          Bit dump");
    println!("  --dump-json         JSON debug output");
    println!("  --stats             Print statistics");
    println!("  --trace             Trace pipeline");
    println!("  --no-color          Disable ANSI colors");
    println!("  --no-warn-extension Suppress .bin warning");
}
