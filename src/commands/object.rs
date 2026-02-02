use std::fs;
use std::process;
use std::time::Instant;
use object::{ObjectFile, ObjectFormat, ObjectSymbol, SectionKind, SymbolBinding};

pub fn run(args: Vec<String>) {
    if args.is_empty() {
        print_help();
        return;
    }

    let mut input = None;
    let mut output = None;

    let mut debug_mode = false;
    let mut _show_ast = false;
    let mut _show_token = false;
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
            "--help" => { print_help(); return; }
            "-o" => { if i + 1 < args.len() { output = Some(args[i + 1].clone()); i += 1; } }
            "--debug-whale" => debug_mode = true,
            "--bytes" => show_bytes = true,
            "--dump-hex" => dump_hex = true,
            "--dump-bin" => dump_bin = true,
            "--dump-json" => dump_json = true,
            "--stats" => show_stats = true,
            "--trace" => trace_enable = true,
            s if input.is_none() && !s.starts_with('-') => input = Some(s.to_string()),
            _ => {}
        }
        i += 1;
    }

    let input = input.unwrap_or_else(|| { eprintln!("Error: No input file"); process::exit(1); });
    let output = output.unwrap_or_else(|| { eprintln!("Error: No output file (-o)"); process::exit(1); });

    if trace_enable { println!("[trace] reading input file: {}", input); }
    let start_time = Instant::now();

    let bytes = fs::read(&input).unwrap_or_else(|e| { eprintln!("Failed to read input: {}", e); process::exit(1); });

    let mut obj = ObjectFile::new(ObjectFormat::ELF64);
    let sec_idx = obj.add_section(".text", SectionKind::Text, 16);
    obj.sections[sec_idx].data = bytes;
    
    obj.symbols.push(ObjectSymbol {
        name: "start".to_string(),
        section_index: Some(sec_idx),
        value: 0,
        size: 0,
        binding: SymbolBinding::Global,
        visibility: object::SymbolVisibility::Default,
    });

    let out_bytes = obj.write().expect("Failed to write ELF");
    let elapsed = start_time.elapsed();

    if debug_mode && (show_bytes || dump_hex || dump_bin) { /* ... same as before ... */ }

    fs::write(&output, &out_bytes).expect("Failed to write output");

    if debug_mode && show_stats {
        println!("== STATS ==\nInput bytes: {}\nOutput bytes: {}\nTime: {} ms", obj.sections[sec_idx].data.len(), out_bytes.len(), elapsed.as_millis());
    }
    println!("Created object file {} ({} bytes)", output, out_bytes.len());
}

pub fn print_help() { /* ... same as before ... */ }
