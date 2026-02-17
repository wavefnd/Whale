use std::fs;
use std::process;
use std::time::Instant;

use assembler::{assemble, isa::AMD64, AssemblerOutput};
use assembler::isa::amd64::parser::parse;
use assembler::tokens::tokenize;

use object::{ObjectFile, ObjectFormat, SectionKind};

pub fn run(args: Vec<String>) {
    if args.is_empty() {
        print_help();
        return;
    }

    let mut arch = None;
    let mut input = None;
    let mut output = None;

    let mut debug_mode = false;
    let mut show_ast = false;
    let mut show_token = false;
    let mut show_bytes = false;
    let mut dump_hex = false;
    let mut dump_bin = false;
    let mut dump_json = false;
    let mut _no_color = false;
    let mut _no_warn_ext = false;
    let mut show_stats = false;
    let mut trace_enable = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" => {
                print_help();
                return;
            }

            "--amd64" => arch = Some("amd64"),
            "--aarch64" => arch = Some("aarch64"),

            "-o" => {
                if i + 1 < args.len() {
                    output = Some(args[i + 1].clone());
                    i += 1;
                }
            }

            "--debug-whale" => debug_mode = true,
            "--ast" => show_ast = true,
            "--token" => show_token = true,
            "--bytes" => show_bytes = true,
            "--dump-hex" => dump_hex = true,
            "--dump-bin" => dump_bin = true,
            "--dump-json" => dump_json = true,
            "--no-color" => _no_color = true,
            "--no-warn-extension" => _no_warn_ext = true,
            "--stats" => show_stats = true,
            "--trace" => trace_enable = true,

            s if input.is_none() && !s.starts_with('-') => input = Some(s.to_string()),
            _ => {}
        }
        i += 1;
    }

    let arch = arch.unwrap_or_else(|| {
        eprintln!("Error: architecture must be specified (--amd64)");
        process::exit(1);
    });

    if arch != "amd64" {
        eprintln!("Error: only --amd64 is supported right now");
        process::exit(1);
    }

    let input = input.unwrap_or_else(|| {
        eprintln!("Error: missing input file.");
        process::exit(1);
    });

    let output = output.unwrap_or_else(|| {
        eprintln!("Error: missing output (-o)");
        process::exit(1);
    });

    if !output.ends_with(".o") {
        eprintln!("Error: for now, asm output must be .o (object).");
        process::exit(1);
    }

    if trace_enable {
        println!("[trace] reading input file: {}", input);
    }
    let src = fs::read_to_string(&input).unwrap_or_else(|e| {
        eprintln!("Failed to read {}: {}", input, e);
        process::exit(1);
    });

    let mut token_len: Option<usize> = None;
    let mut ast_items_len: Option<usize> = None;

    let need_tokens = debug_mode && (show_token || show_ast || show_stats);
    if need_tokens {
        if trace_enable { println!("[trace] tokenize start"); }
        let tokens = tokenize(&src).expect("Tokenize error");
        token_len = Some(tokens.len());

        if show_token {
            dbg!(&tokens);
        }

        if show_ast || show_stats {
            if trace_enable { println!("[trace] parse start"); }
            let ast = parse(&tokens).expect("Parse error");
            ast_items_len = Some(ast.items.len());

            if show_ast {
                println!("{:#?}", ast);
            }
        }
    }

    if trace_enable { println!("[trace] assemble start"); }
    let start_time = Instant::now();
    let out = assemble(&src, &AMD64).expect("Assemble error");
    let elapsed = start_time.elapsed();

    if trace_enable { println!("[trace] creating object file"); }
    let final_bytes = build_elf_from_asm_output(&out);

    fs::write(&output, &final_bytes).unwrap_or_else(|e| {
        eprintln!("Failed to write {}: {}", output, e);
        process::exit(1);
    });

    if debug_mode && (show_bytes || dump_hex || dump_bin || dump_json) {
        dump_bytes("object", &final_bytes, show_bytes, dump_hex, dump_bin, dump_json);
    }

    if debug_mode && show_stats {
        println!(
            "== STATS ==\nTokens: {}\nAST nodes: {}\nObject bytes: {}\nTime: {} ms",
            token_len.unwrap_or(0),
            ast_items_len.unwrap_or(0),
            final_bytes.len(),
            elapsed.as_millis()
        );
    }

    println!("Wrote {} bytes to {}", final_bytes.len(), output);
}

fn build_elf_from_asm_output(out: &AssemblerOutput) -> Vec<u8> {
    let mut obj = ObjectFile::new(ObjectFormat::ELF64);

    // AssemblerOutput의 sections를 그대로 ELF 섹션으로 옮김
    // (심볼/리로케이션 매핑은 out.symbols/out.relocs 구조 확정되면 다음 단계에서 붙이는 게 맞음)
    for sec in &out.sections {
        let kind = match sec.name.as_str() {
            ".text" => SectionKind::Text,
            ".data" => SectionKind::Data,
            ".rodata" => SectionKind::ReadOnlyData,
            ".bss" => SectionKind::Bss,
            _ => SectionKind::Data,
        };

        let align = if sec.name == ".text" { 16 } else { 1 };
        let idx = obj.add_section(&sec.name, kind, align);
        obj.sections[idx].data = sec.data.clone();
    }

    obj.write().expect("Failed to create ELF object")
}

fn dump_bytes(label: &str, bytes: &[u8], show_bytes: bool, dump_hex: bool, dump_bin: bool, dump_json: bool) {
    const LIMIT: usize = 256;
    let n = bytes.len().min(LIMIT);
    let head = &bytes[..n];

    if show_bytes {
        println!("== BYTES ({}, {} bytes, head {} bytes) ==", label, bytes.len(), n);
        for (i, b) in head.iter().enumerate() {
            println!("{:04X}: {}", i, b);
        }
    }

    if dump_hex {
        println!("== HEX ({}, head {} bytes) ==", label, n);
        for (i, chunk) in head.chunks(16).enumerate() {
            print!("{:04X}: ", i * 16);
            for b in chunk {
                print!("{:02X} ", b);
            }
            println!();
        }
    }

    if dump_bin {
        println!("== BIN ({}, head {} bytes) ==", label, n);
        for (i, b) in head.iter().enumerate() {
            println!("{:04X}: {:08b}", i, b);
        }
    }

    if dump_json {
        println!("== JSON ({}) ==", label);
        println!("{{");
        println!("  \"len\": {},", bytes.len());
        print!("  \"head\": [");
        for (i, b) in head.iter().enumerate() {
            if i != 0 { print!(", "); }
            print!("{}", b);
        }
        println!("]");
        println!("}}");
    }
}

fn print_help() {
    println!("Usage:");
    println!("  whale asm --amd64 <input> -o <output.o>");
    println!();
    println!("Options:");
    println!("  --debug-whale   enable debug features");
    println!("  --ast           print parser AST (debug)");
    println!("  --token         print tokens (debug)");
    println!("  --bytes         print object bytes (debug)");
    println!("  --dump-hex      print object bytes as hex (debug)");
    println!("  --dump-bin      print object bytes as binary (debug)");
    println!("  --dump-json     print object bytes as json (debug)");
    println!("  --stats         show stats (debug)");
    println!("  --trace         trace logs");
}
