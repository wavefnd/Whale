#[path = "../fuzz/support.rs"]
mod fuzz;

use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    path::PathBuf,
};

fn main() {
    let mut args = std::env::args().skip(1);
    let target =
        fuzz::Target::parse(&args.next().expect("target: assembler|object|ir|ast|bytecode"))
            .expect("unknown target");
    let mode = args.next().expect("cases count or --replay");
    if mode == "--replay" {
        let path = args.next().expect("replay path");
        let bytes = fuzz::read_bounded(std::path::Path::new(&path));
        checkpoint(target, &bytes, 0, None, &PathBuf::from(&path));
        fuzz::check(target, &bytes);
        println!("replay passed");
        return;
    }
    let cases: usize = mode.parse().unwrap();
    assert!((1..=100000).contains(&cases));
    let seed: u64 = args.next().unwrap_or_else(|| "1".into()).parse().unwrap();
    let corpus = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fuzz/corpus");
    let seeds = fuzz::seeds(&corpus, target);
    let mut seed_accepted = 0;
    for (path, bytes) in &seeds {
        checkpoint(target, bytes, seed, None, path);
        seed_accepted += usize::from(fuzz::check(target, bytes));
    }
    assert!(seed_accepted > 0, "corpus needs a valid seed");
    let mut rng = fuzz::Rng::new(seed);
    let mut accepted = seed_accepted;
    for case in 0..cases {
        let (path, bytes) = &seeds[rng.index(seeds.len())];
        let input = if case % 8 == 0 { bytes.clone() } else { fuzz::mutate(bytes, &mut rng) };
        checkpoint(target, &input, seed, Some(case), path);
        match catch_unwind(AssertUnwindSafe(|| fuzz::check(target, &input))) {
            Ok(valid) => accepted += usize::from(valid),
            Err(_) => {
                std::panic::set_hook(Box::new(|_| {}));
                let directory = std::env::var_os("WHALE_FUZZ_ARTIFACT_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| std::env::temp_dir().join("whale-fuzz"));
                std::fs::create_dir_all(&directory).unwrap();
                let crash = directory.join(format!("{}-{seed}-{case}.crash", target.name()));
                let original = directory.join(format!("{}-{seed}-{case}.original", target.name()));
                // Retain evidence before minimization: the process may hit its
                // independent time/memory limit while reducing a failing input.
                std::fs::write(&original, &input).unwrap();
                let replay = serde_json::json!({
                    "origin": path,
                    "replay": [std::env::current_exe().unwrap().to_str().expect("UTF-8 replay path"),
                               target.name(), "--replay",
                               original.to_str().expect("UTF-8 replay path")],
                });
                std::fs::write(
                    directory.join(format!("{}-{seed}-{case}.replay", target.name())),
                    serde_json::to_vec_pretty(&replay).unwrap(),
                )
                .unwrap();
                let attempts = std::cell::Cell::new(0usize);
                let started = std::time::Instant::now();
                let minimal = fuzz::minimize(input, |b| {
                    if attempts.get() >= 256 || started.elapsed().as_secs() >= 5 {
                        return false;
                    }
                    attempts.set(attempts.get() + 1);
                    checkpoint(target, b, seed, Some(case), path);
                    catch_unwind(AssertUnwindSafe(|| fuzz::check(target, b))).is_err()
                });
                std::fs::write(&crash, minimal).unwrap();
                eprintln!(
                    "seed={seed} case={case} origin={} original={} minimized={}",
                    path.display(),
                    original.display(),
                    crash.display()
                );
                std::process::exit(1);
            },
        }
    }
    assert!(accepted > 0, "fuzz invocation must execute at least one valid seed");
    println!(
        "{} seed={seed} cases={cases} accepted={accepted} max_input={} passed",
        target.name(),
        fuzz::MAX_INPUT
    );
}

// A native abort or timeout cannot reach catch_unwind. Publish the original and
// literal replay arguments before entering any native-backed library boundary.
fn checkpoint(
    target: fuzz::Target,
    bytes: &[u8],
    seed: u64,
    case: Option<usize>,
    origin: &std::path::Path,
) {
    assert!(bytes.len() <= fuzz::MAX_INPUT);
    let directory = std::env::var_os("WHALE_FUZZ_ARTIFACT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("whale-fuzz"));
    std::fs::create_dir_all(&directory).unwrap();
    let directory = directory.canonicalize().unwrap();
    use std::io::Write;

    static PREVIOUS: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);
    let mut previous = PREVIOUS.lock().unwrap();
    let mut file = tempfile::Builder::new()
        .prefix(&format!("{}.current-", target.name()))
        .tempfile_in(&directory)
        .unwrap();
    file.write_all(bytes).unwrap();
    let (file, original) = file.keep().unwrap();
    drop(file);
    let record = serde_json::json!({
        "target": target.name(), "seed": seed, "case": case, "origin": origin,
        "bytes": bytes.len(), "input": original, "ir_wave": ir::IR_WAVE_ENABLED,
        "replay": [std::env::current_exe().unwrap().to_str().expect("UTF-8 replay path"),
                   target.name(), "--replay",
                   original.to_str().expect("UTF-8 replay path")],
    });
    let metadata = directory.join(format!("{}.current.json", target.name()));
    let temporary = directory.join(format!("{}.current.json.tmp", target.name()));
    std::fs::write(&temporary, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
    if let Err(error) = std::fs::rename(temporary, metadata) {
        let _ = std::fs::remove_file(&original);
        panic!("unable to publish replay manifest: {error}");
    }
    if let Some(path) = previous.replace(original) {
        std::fs::remove_file(path).unwrap();
    }
}
