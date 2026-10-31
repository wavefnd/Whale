#[path = "../fuzz/support.rs"]
mod fuzz;

#[test]
fn deterministic_seed_and_mutation_smoke_is_bounded() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fuzz/corpus");
    let mut targets = vec![fuzz::Target::Ir, fuzz::Target::Object];
    if ir::bytecode::available() {
        targets.push(fuzz::Target::Bytecode);
    }
    if cfg!(feature = "asm-amd64") {
        targets.push(fuzz::Target::Assembler);
    }
    for target in targets {
        let seeds = fuzz::seeds(&root, target);
        let mut rng = fuzz::Rng::new(0x5748414c45);
        let mut valid = 0;
        for (_, bytes) in &seeds {
            valid += usize::from(fuzz::check(target, bytes));
            for _ in 0..16 {
                fuzz::check(target, &fuzz::mutate(bytes, &mut rng));
            }
        }
        assert!(valid > 0);
    }
    #[cfg(feature = "socket-cli")]
    {
        let target = fuzz::Target::Ast;
        let seeds = fuzz::seeds(&root, target);
        let mut rng = fuzz::Rng::new(42);
        let mut valid = 0;
        for (_, bytes) in seeds {
            valid += usize::from(fuzz::check(target, &bytes));
            for _ in 0..16 {
                fuzz::check(target, &fuzz::mutate(&bytes, &mut rng));
            }
        }
        assert!(valid > 0);
    }
}

#[test]
fn minimized_failure_bytes_are_replayable_and_redundant_input_is_removed() {
    let result =
        fuzz::minimize(b"prefix BUG suffix".to_vec(), |b| b.windows(3).any(|v| v == b"BUG"));
    assert_eq!(result, b"BUG");
}
