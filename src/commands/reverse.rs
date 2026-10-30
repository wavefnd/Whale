use super::report::println;

use reverse::image::Image;

use std::{path::Path, process};

struct Options {
    input: String,
    output: Option<String>,
    raw: bool,
    base: Option<u64>,
    entry: Option<String>,
    json: bool,
    max_input: u64,
    max_output: usize,
    max_instructions: usize,
    max_work: u64,
    max_workspace: usize,
}

pub fn disasm(args: Vec<String>) {
    run("disasm", args);
}

pub fn reverse(mut args: Vec<String>) {
    if args.is_empty() || args == ["--help"] {
        help();
        return;
    }
    let mode = args.remove(0);
    run(&mode, args);
}

fn run(mode: &str, args: Vec<String>) {
    if args == ["--help"] {
        help();
        return;
    }
    if let Err(error) = execute(mode, args) {
        eprintln!("Error: {error}");
        process::exit(1);
    }
}

fn number(value: &str) -> Result<u64, String> {
    if let Some(hex) = value.strip_prefix("0x") {
        u64::from_str_radix(hex, 16).map_err(|_| "invalid hexadecimal address".into())
    } else {
        value.parse().map_err(|_| "invalid unsigned address/count".into())
    }
}

fn options(args: Vec<String>, mode: &str) -> Result<Options, String> {
    let mut input = None;
    let mut output = None;
    let mut raw = false;
    let mut base = None;
    let mut entry = None;
    let mut structured = false;
    let mut amd64 = false;
    let mut limits = std::collections::HashMap::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--amd64" => {
                if amd64 {
                    return Err("duplicate --amd64".into());
                }
                amd64 = true;
            },
            "--raw" => {
                if raw {
                    return Err("duplicate --raw".into());
                }
                raw = true;
            },
            "--json" => {
                if structured {
                    return Err("duplicate --json".into());
                }
                structured = true;
            },
            flag @ ("-o" | "--base" | "--entry" | "--max-input" | "--max-output"
            | "--max-instructions" | "--max-work" | "--max-workspace") => {
                let v = args
                    .get(i + 1)
                    .filter(|s| !s.starts_with('-'))
                    .ok_or_else(|| format!("{flag} requires a value"))?
                    .clone();
                match flag {
                    "-o" => {
                        if output.replace(v).is_some() {
                            return Err("duplicate -o".into());
                        }
                    },
                    "--base" => {
                        if base.replace(number(&v)?).is_some() {
                            return Err("duplicate --base".into());
                        }
                    },
                    "--entry" => {
                        if entry.replace(v).is_some() {
                            return Err("duplicate --entry".into());
                        }
                    },
                    _ => {
                        if limits.insert(flag.to_owned(), number(&v)?).is_some() {
                            return Err(format!("duplicate {flag}"));
                        }
                    },
                }
                i += 1;
            },
            value if !value.starts_with('-') && input.is_none() => input = Some(value.into()),
            value => return Err(format!("unknown option or surplus argument {value}")),
        }
        i += 1;
    }
    if raw && (!amd64 || base.is_none()) {
        return Err("raw bytes require --raw --amd64 --base ADDRESS".into());
    }
    if !raw && base.is_some() {
        return Err("--base is only valid with --raw".into());
    }
    let graph = matches!(mode, "cfg" | "xrefs");
    if !graph && (limits.contains_key("--max-work") || limits.contains_key("--max-workspace")) {
        return Err("--max-work/--max-workspace require cfg or xrefs".into());
    }
    let default_count = if graph { 16384 } else { 100000 };
    let count = usize::try_from(*limits.get("--max-instructions").unwrap_or(&default_count))
        .ok()
        .filter(|n| *n > 0 && *n <= 1_000_000)
        .ok_or("--max-instructions must be 1..1000000")?;
    let output_limit = usize::try_from(*limits.get("--max-output").unwrap_or(&(16 * 1024 * 1024)))
        .ok()
        .filter(|n| *n <= 256 * 1024 * 1024)
        .ok_or("--max-output exceeds 268435456")?;
    let input_limit = *limits.get("--max-input").unwrap_or(&(8 * 1024 * 1024));
    if input_limit > 256 * 1024 * 1024 {
        return Err("--max-input exceeds 268435456".into());
    }
    let max_work = *limits.get("--max-work").unwrap_or(&67108864);
    if !(1..=1073741824).contains(&max_work) {
        return Err("--max-work must be 1..1073741824".into());
    }
    let max_workspace = usize::try_from(*limits.get("--max-workspace").unwrap_or(&134217728))
        .ok()
        .filter(|n| (1..=536870912).contains(n))
        .ok_or("--max-workspace must be 1..536870912")?;
    Ok(Options {
        input: input.ok_or("missing input")?,
        output,
        raw,
        base,
        entry,
        json: structured,
        max_input: input_limit,
        max_output: output_limit,
        max_instructions: count,
        max_work,
        max_workspace,
    })
}

fn execute(mode: &str, args: Vec<String>) -> Result<(), String> {
    if !matches!(mode, "disasm" | "inspect" | "cfg" | "xrefs") {
        return Err(format!("unknown reverse command {mode}"));
    }
    let options = options(args, mode)?;
    let bytes = super::input::read(Path::new(&options.input), options.max_input)
        .map_err(|e| e.to_string())?;
    let image = if options.raw {
        Image::raw(&bytes, options.base.unwrap())?
    } else {
        Image::elf(&bytes, object::ObjectReadLimits::default())?
    };
    let rendered = match mode {
        "inspect" => super::reverse_render::inspect(&image, options.json, options.max_output)?,
        "disasm" => listing(&image, &options)?,
        _ => graph(&image, &options, mode)?,
    };
    if rendered.len() > options.max_output {
        return Err("reverse output byte limit exceeded".into());
    }
    if let Some(output) = &options.output {
        super::output::publish(Path::new(&options.input), Path::new(output), rendered.as_bytes())
            .map_err(|e| e.to_string())?;
    } else {
        use std::io::Write;
        std::io::stdout().lock().write_all(rendered.as_bytes()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn seed(image: &Image<'_>, entry: &str) -> Result<(usize, usize), String> {
    let named: Vec<_> = image.symbols.iter().filter(|s| s.name == entry).collect();
    if named.len() > 1 {
        return Err("entry symbol name is ambiguous".into());
    }
    if let Some(symbol) = named.first() {
        let region = symbol.region.ok_or("entry symbol is not in executable bytes")?;
        let offset =
            usize::try_from(symbol.offset).map_err(|_| "entry offset exceeds host capacity")?;
        if offset >= image.regions[region].bytes.len() {
            return Err("entry symbol is outside executable bytes".into());
        }
        return Ok((region, offset));
    }
    let address = number(entry).map_err(|_| format!("entry symbol {entry:?} not found"))?;
    address_seed(image, address)
}

fn address_seed(image: &Image<'_>, address: u64) -> Result<(usize, usize), String> {
    let candidates: Vec<_> = image
        .regions
        .iter()
        .filter(|r| address >= r.address && address - r.address < r.bytes.len() as u64)
        .collect();
    if candidates.len() != 1 {
        return Err("entry address is absent or ambiguous across code regions".into());
    }
    Ok((candidates[0].id, (address - candidates[0].address) as usize))
}

fn listing(image: &Image<'_>, options: &Options) -> Result<String, String> {
    if !reverse::backend_available() {
        return Err(reverse::DecodeError::CoreUnavailable.to_string());
    }
    let mut output =
        super::reverse_render::ListingOutput::new(image, options.json, options.max_output)?;
    let mut count = 0usize;
    let requested = options.entry.as_deref().map(|entry| seed(image, entry)).transpose()?;
    for r in &image.regions {
        if requested.is_some_and(|(id, _)| id != r.id) {
            continue;
        }
        let mut offset = requested.map_or_else(|| image.entry_offset(r), |(_, offset)| offset);
        output.region(r)?;
        while offset < r.bytes.len() {
            if count >= options.max_instructions {
                return Err("reverse instruction/work limit exceeded".into());
            }
            count += 1;
            match reverse::decode(r.bytes, offset) {
                Ok(inst) => {
                    output.instruction(image, r, &inst)?;
                    offset += inst.length as usize;
                },
                Err(
                    error @ (reverse::DecodeError::Invalid { .. }
                    | reverse::DecodeError::Truncated { .. }
                    | reverse::DecodeError::Unsupported { .. }
                    | reverse::DecodeError::Overlong { .. }),
                ) => {
                    output.data(r, offset, &error)?;
                    offset += 1;
                },
                Err(error) => return Err(error.to_string()),
            }
        }
    }
    output.finish()
}

fn graph(image: &Image<'_>, options: &Options, mode: &str) -> Result<String, String> {
    use reverse::{CfgLimits, CfgTarget, FlowOverride};
    if !reverse::backend_available() {
        return Err(reverse::CfgError::CoreUnavailable.to_string());
    }
    let (region_id, entry) = if let Some(entry) = &options.entry {
        seed(image, entry)?
    } else if let Some(address) = image.entry {
        address_seed(image, address)?
    } else {
        seed(image, "_start").map_err(|error| {
            format!("CFG requires --entry NAME|ADDRESS or a valid _start: {error}")
        })?
    };
    let region = &image.regions[region_id];
    // Stable borrowed symbol identities; relocation placeholders must never be
    // followed as if the linker had already resolved their encoded bytes.
    let names: Vec<&str> = image
        .relocations
        .iter()
        .filter(|r| r.region == region_id)
        .map(|r| r.symbol.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let overlays: Vec<_> = image
        .relocations
        .iter()
        .filter(|r| r.region == region_id)
        .map(|r| FlowOverride {
            field_offset: r.offset,
            width: r.width as u8,
            target: CfgTarget::Symbol(names.binary_search(&r.symbol.as_str()).unwrap() as u64),
        })
        .collect();
    let defaults = CfgLimits::default();
    let length = region.bytes.len();
    let limits = CfgLimits {
        max_input_bytes: usize::try_from(options.max_input).unwrap_or(usize::MAX),
        max_instructions: options.max_instructions.min(length),
        max_blocks: defaults.max_blocks.min(length),
        max_edges: defaults.max_edges.min(length.saturating_mul(4).max(overlays.len())),
        max_stops: defaults.max_stops.min(length),
        max_work: options.max_work,
        max_workspace_bytes: options.max_workspace,
        ..defaults
    };
    let result = reverse::analyze(region.bytes, region.address, &[entry], &overlays, limits)
        .map_err(|e| e.to_string())?;
    super::reverse_graph::render(
        image,
        region,
        &result,
        &names,
        super::reverse_graph::RenderOptions {
            mode,
            entry,
            structured: options.json,
            limit: options.max_output,
        },
    )
}

fn help() {
    println!("Usage:\n  whale reverse inspect <file.o|executable> [--json] [-o PATH]\n  whale reverse inspect --raw --amd64 --base ADDRESS <bytes> [--json]\n  whale disasm <file> [--json] [-o PATH]\n  whale reverse cfg|xrefs <file> [--entry NAME|ADDRESS] [--json]\n\nELF/raw AMD64 inspection is available without a decoder.\nDisassembly, CFG and xrefs are currently unavailable in this build.\n--max-input N          input bytes (default 8388608; cap 268435456)\n--max-output N         output bytes (default 16777216; cap 268435456)\n--max-instructions N   records (listing 100000; CFG 16384; cap 1000000)\n--max-work N           CFG shared work (default 67108864; cap 1073741824)\n--max-workspace N      CFG bytes (default 134217728; cap 536870912)");
}
