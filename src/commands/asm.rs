use std::fs;
use std::process;
use std::time::Instant;

use assembler::{assemble, isa::AMD64, AssemblerOutput, RelocKind as AsmRelocKind};
use assembler::isa::amd64::parser::parse;
use assembler::tokens::tokenize;
use object::{ObjectFile, ObjectFormat, ObjectSymbol, ObjectRelocation, RelocKind as ObjRelocKind, SectionKind, SymbolBinding, SymbolVisibility};

pub fn run(args: Vec<String>) {
    if args.is_empty() { print_help(); return; }

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
    let mut no_warn_ext = false;
    let mut show_stats = false;
    let mut trace_enable = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" => print_help(),
            "--amd64" => arch = Some("amd64"),
            "--aarch64" => arch = Some("aarch64"),
            "-o" => { if i + 1 < args.len() { output = Some(args[i + 1].clone()); i += 1; } }
            "--debug-whale" => debug_mode = true,
            "--ast" => show_ast = true,
            "--token" => show_token = true,
            "--bytes" => show_bytes = true,
            "--dump-hex" => dump_hex = true,
            "--dump-bin" => dump_bin = true,
            "--dump-json" => dump_json = true,
            "--no-color" => _no_color = true,
            "--no-warn-extension" => no_warn_ext = true,
            "--stats" => show_stats = true,
            "--trace" => trace_enable = true,
            s if input.is_none() => input = Some(s.to_string()),
            _ => {}
        }
        i += 1;
    }

    let arch = arch.unwrap_or_else(|| { eprintln!("Error: architecture must be specified"); process::exit(1); });
    let input = input.unwrap_or_else(|| { eprintln!("Error: missing input file."); process::exit(1); });
    let output = output.unwrap_or_else(|| { eprintln!("Error: missing output (-o)"); process::exit(1); });

    if trace_enable { println!("[trace] reading input file"); }
    let src = fs::read_to_string(&input).expect("Failed to read input");

    if trace_enable { println!("[trace] tokenize start"); }
    let tokens = tokenize(&src).expect("Tokenize error");
    if debug_mode && show_token { dbg!(&tokens); }

    if trace_enable { println!("[trace] parse start"); }
    let ast = parse(&tokens).expect("Parse error");
    if debug_mode && show_ast { println!("{:#?}", ast); }

    if trace_enable { println!("[trace] assemble start"); }
    let start_time = Instant::now();

    let out = match arch {
        "amd64" => assemble(&src, &AMD64).expect("Assemble error"),
        _ => { eprintln!("Unknown architecture"); process::exit(1); }
    };

    let elapsed = start_time.elapsed();

    let final_bytes = if output.ends_with(".o") {
        if trace_enable { println!("[trace] creating object file"); }
        build_elf_from_asm_output(out)
    } else {
        // Flatten first section for .bin
        out.sections[0].data.clone()
    };

    if debug_mode && (show_bytes || dump_hex || dump_bin) { /* ... same as before ... */ }

    fs::write(&output, &final_bytes).expect("Failed to write output");

    if debug_mode && show_stats {
        println!("== STATS ==\nTokens: {}\nAST nodes: {}\nOutput bytes: {}\nTime: {} ms", tokens.len(), ast.items.len(), final_bytes.len(), elapsed.as_millis());
    }
    println!("Wrote {} bytes to {}", final_bytes.len(), output);
}

fn build_elf_from_asm_output(out: AssemblerOutput) -> Vec<u8> {
    let mut obj = ObjectFile::new(ObjectFormat::ELF64);
    
    for sec in out.sections {
        let kind = if sec.name == ".text" { SectionKind::Text }
                   else if sec.name == ".data" { SectionKind::Data }
                   else if sec.name == ".bss" { SectionKind::Bss }
                   else { SectionKind::ReadOnlyData };
        
        let idx = obj.add_section(&sec.name, kind, 16);
        obj.sections[idx].data = sec.data;
        
        for r in sec.relocs {
            obj.relocations.push(ObjectRelocation {
                section_index: idx,
                offset: r.offset,
                symbol: r.symbol,
                addend: r.addend,
                kind: match r.kind {
                    AsmRelocKind::Absolute64 => ObjRelocKind::Absolute64,
                    AsmRelocKind::Relative32 => ObjRelocKind::Relative32,
                },
            });
        }
    }

    for s in out.symbols {
        obj.symbols.push(ObjectSymbol {
            name: s.name,
            section_index: s.section_index,
            value: s.offset as u64,
            size: 0,
            binding: if s.is_global { SymbolBinding::Global } else { SymbolBinding::Local },
            visibility: SymbolVisibility::Default,
        });
    }

    obj.write().expect("Failed to create ELF object")
}

fn print_help() { /* ... same as before ... */ }
