use std::process;

use std::time::Instant;

use super::debug::dump_bytes;

use amd64::parser::parse;

use assembler::tokens::tokenize;

pub fn run(args: Vec<String>) {
    if args.is_empty() {
        print_help();
        return;
    }

    let mut arch: Option<String> = None;
    let mut input = None;
    let mut output = None;
    let mut max_input = None;

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
            },

            flag @ ("--amd64" | "--aarch64" | "--arm64" | "--riscv64" | "--loongarch64") => {
                if arch.is_some() {
                    eprintln!("Error: architecture must be specified only once");
                    process::exit(1);
                }
                arch = Some(
                    match flag {
                        "--amd64" => "amd64",
                        "--aarch64" | "--arm64" => "aarch64",
                        "--riscv64" => "riscv64",
                        _ => "loongarch64",
                    }
                    .to_owned(),
                );
            },

            "--arch" => {
                if arch.is_some() {
                    eprintln!("Error: architecture must be specified only once");
                    process::exit(1);
                }
                arch = Some(
                    args.get(i + 1)
                        .filter(|s| !s.starts_with('-'))
                        .unwrap_or_else(|| {
                            eprintln!("Error: --arch requires a registered assembler identity");
                            process::exit(1);
                        })
                        .clone(),
                );
                i += 1;
            },

            "-o" => {
                if output.is_some() {
                    eprintln!("Error: duplicate -o option");
                    process::exit(1);
                }
                let path = args.get(i + 1).filter(|s| !s.starts_with('-')).unwrap_or_else(|| {
                    eprintln!("Error: -o requires a path (use ./ for paths starting with '-')");
                    process::exit(1);
                });
                output = Some(path.clone());
                i += 1;
            },
            "--max-input" => {
                if max_input.is_some() {
                    eprintln!("Error: duplicate --max-input option");
                    process::exit(1);
                }
                let limit = args.get(i + 1).unwrap_or_else(|| {
                    eprintln!("Error: --max-input requires a decimal byte count");
                    process::exit(1);
                });
                max_input = Some(super::input::byte_limit(limit).unwrap_or_else(|e| {
                    eprintln!("Error: {e}");
                    process::exit(1);
                }));
                i += 1;
            },

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
            s => {
                let kind =
                    if s.starts_with('-') { "unknown option" } else { "unexpected argument" };
                eprintln!("Error: {kind}: {s}");
                process::exit(1);
            },
        }
        i += 1;
    }

    let arch = arch.unwrap_or_else(|| {
        eprintln!(
            "Error: architecture must be specified (--amd64, --aarch64, --riscv64, --loongarch64)"
        );
        process::exit(1);
    });

    let registry = crate::builtins::registry();
    let implementation = registry.assembler(&arch).unwrap_or_else(|| {
        eprintln!(
            "Error: unregistered assembler {arch:?}; available: {}",
            registry.assembler_ids().collect::<Vec<_>>().join(", ")
        );
        process::exit(1);
    });
    let target = implementation.object_target().unwrap_or_else(|| {
        eprintln!("Error: assembler {arch:?} does not expose a native object target");
        process::exit(1);
    });
    target.validate().unwrap_or_else(|error| {
        eprintln!("Error: {error}");
        process::exit(1);
    });

    if arch != "amd64" && debug_mode && (show_ast || show_token) {
        eprintln!("Error: --ast and --token expose the AMD64 parser only");
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
    let bytes =
        super::input::read(input.as_ref(), max_input.unwrap_or(super::input::ASM_INPUT_BYTES))
            .unwrap_or_else(|e| {
                eprintln!("Failed to read {}: {}", input, e);
                process::exit(1);
            });
    let src = String::from_utf8(bytes).unwrap_or_else(|e| {
        eprintln!("Failed to read {input}: assembly must be UTF-8: {e}");
        process::exit(1);
    });

    let mut token_len: Option<usize> = None;
    let mut ast_items_len: Option<usize> = None;

    let need_tokens = arch == "amd64" && debug_mode && (show_token || show_ast || show_stats);
    if need_tokens {
        if trace_enable {
            println!("[trace] tokenize start");
        }
        let tokens = tokenize(&src).unwrap_or_else(|e| {
            eprintln!("Tokenize error: {}", e.with_source_name(&input));
            process::exit(1);
        });
        token_len = Some(tokens.len());

        if show_token {
            dbg!(&tokens);
        }

        if show_ast || show_stats {
            if trace_enable {
                println!("[trace] parse start");
            }
            let ast = parse(&tokens).unwrap_or_else(|e| {
                eprintln!("Parse error: {}", e.with_source_name(&input));
                process::exit(1);
            });
            ast_items_len = Some(ast.items.len());

            if show_ast {
                println!("{:#?}", ast);
            }
        }
    }

    if trace_enable {
        println!("[trace] assemble start");
    }
    let start_time = Instant::now();
    let out = registry
        .assemble(&arch, &input, &src, &assembler::AssemblyLimits::default())
        .unwrap_or_else(|e| {
            eprintln!("Assemble error: {}", e);
            process::exit(1);
        });
    let elapsed = start_time.elapsed();

    if trace_enable {
        println!("[trace] creating object file");
    }
    let final_bytes = assembler_object::write(&out, target).unwrap_or_else(|e| {
        eprintln!("ELF build error: {}", e);
        process::exit(1);
    });

    super::output::publish(input.as_ref(), output.as_ref(), &final_bytes).unwrap_or_else(|e| {
        eprintln!("Failed to write {}: {}", output, e);
        process::exit(1);
    });

    if debug_mode && (show_bytes || dump_hex || dump_bin || dump_json) {
        dump_bytes("object", &final_bytes, show_bytes, dump_hex, dump_bin, dump_json);
    }

    if debug_mode && show_stats {
        println!(
            "== STATS ==\nTokens: {}\nAST nodes: {}\nObject bytes: {}\nTime: {} ms",
            token_len.map_or_else(|| "n/a".into(), |n| n.to_string()),
            ast_items_len.map_or_else(|| "n/a".into(), |n| n.to_string()),
            final_bytes.len(),
            elapsed.as_millis()
        );
    }

    println!("Wrote {} bytes to {}", final_bytes.len(), output);
}

fn print_help() {
    println!("Usage:");
    println!("  whale asm --amd64|--aarch64|--arm64|--riscv64|--loongarch64 <input> -o <output.o>");
    println!();
    println!("  whale asm --arch ID <input> -o <output.o>");
    println!("Options:");
    println!("  --max-input N   input byte limit (default 8388608 bytes)");
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
