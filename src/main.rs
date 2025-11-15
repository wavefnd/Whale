use assembler::{assemble, isa::AMD64};

fn main() {
    let src = r#"
        mov rax, 1
        mov rbx, 2
        label1:
        mov rcx, label1
    "#;

    let asm = assemble(src, &AMD64).unwrap();
    println!("Bytes: {:02X?}", asm.bytes);
    println!("Symbols: {:?}", asm.symbols);
    println!("Relocations: {:?}", asm.relocations);
}
