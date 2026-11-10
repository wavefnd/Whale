// SPDX-License-Identifier: MPL-2.0
//! Count live allocation requests around admission, including rejected arguments.
use ir::{bytecode::*, *};

use std::alloc::{GlobalAlloc, Layout, System};

use std::sync::atomic::{AtomicUsize, Ordering};

struct Counted;

static LIVE: AtomicUsize = AtomicUsize::new(0);

static PEAK: AtomicUsize = AtomicUsize::new(0);

fn allocated(bytes: usize) {
    let live = LIVE.fetch_add(bytes, Ordering::SeqCst) + bytes;
    PEAK.fetch_max(live, Ordering::SeqCst);
}

unsafe impl GlobalAlloc for Counted {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // Forward the identical allocation request to the system allocator.
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            allocated(layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::SeqCst);
        // The pointer and layout are the original allocation's pair.
        unsafe {
            System.dealloc(pointer, layout);
        }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        // Preserve the system allocator's failure and ownership contract.
        let next = unsafe { System.realloc(pointer, layout, size) };
        if !next.is_null() {
            LIVE.fetch_sub(layout.size(), Ordering::SeqCst);
            allocated(size);
        }
        next
    }
}

#[global_allocator]
static ALLOCATOR: Counted = Counted;

fn image(shared_text: bool) -> Vec<u8> {
    let expected = FunctionSignature::whale(vec![], Type::U64);
    let mut builder = ModuleBuilder::new("whale-bytecode-v1", DataLayout::default_64bit_le());
    let mut function = builder.begin_function(
        "entry",
        vec![("callback".into(), Type::FnPtr(Box::new(expected)))],
        Type::Void,
    );
    function.ret(None);
    function.finish();
    let mut module = builder.finish();
    module.functions[0].blocks[0].terminator = Some(Terminator::Trap {
        reason: if shared_text { "x".repeat(2 * 1024 * 1024) } else { "probe".into() },
    });
    if shared_text {
        module.sources.files.push(SourceFile { id: 0, name: "x".into() });
        module.sources.originals.push(OriginalNode {
            id: 0,
            span: Some(SourceSpan { file: 0, line: 1, column: 1, end_line: 1, end_column: 2 }),
        });
        module.sources.mappings.push(SourceMapping {
            target: SourceTarget::Terminator {
                function: FunctionId(0),
                block: module.functions[0].entry,
            },
            original: 0,
        });
    }
    let mut bytes = compile(&module, BytecodeLimits::default()).unwrap();
    if shared_text {
        // V1 permits a filename to reference an existing reason text. This
        // independent wire mutation reproduces retained-string amplification.
        let entry = 64 + 11 * 32;
        let offset = u64::from_le_bytes(bytes[entry + 8..entry + 16].try_into().unwrap()) as usize;
        assert_eq!(u64::from_le_bytes(bytes[entry + 16..entry + 24].try_into().unwrap()), 1);
        bytes[offset + 8..offset + 16].copy_from_slice(&0_u64.to_le_bytes());
    }
    bytes
}

fn measure(shared_text: bool) {
    let bytes = image(shared_text);
    let argument =
        BytecodeValue::null_function(FunctionSignature::whale(vec![Type::U64; 5000], Type::U64))
            .unwrap();
    let arguments = [argument];
    let limits = BytecodeLimits { max_workspace_bytes: 8 * 1024 * 1024, ..Default::default() };
    let baseline = LIVE.load(Ordering::SeqCst);
    PEAK.store(baseline, Ordering::SeqCst);
    let mut program = BytecodeProgram::load(&bytes, limits).unwrap();
    let loaded = LIVE.load(Ordering::SeqCst) - baseline;
    let result = program.run_values(FunctionId(0), &arguments, InterpreterOptions::default());
    assert!(result.is_err());
    if shared_text {
        assert!(matches!(result, Err(BytecodeError::Invalid { status: 2, .. })));
    }
    let peak = PEAK.load(Ordering::SeqCst) - baseline;
    drop(result);
    let retained = LIVE.load(Ordering::SeqCst) - baseline;
    assert!(
        peak <= limits.max_workspace_bytes as usize,
        "workspace {} < peak {peak}",
        limits.max_workspace_bytes
    );
    assert_eq!(retained, loaded, "rejected temporary types or scratch retained capacity");
    println!(
        "shared_text={shared_text} limit={} loaded={loaded} peak={peak} retained={retained}",
        limits.max_workspace_bytes
    );
}

fn main() {
    assert!(available(), "the workspace probe requires the actual Wave module");
    measure(true);
    measure(false);
}
