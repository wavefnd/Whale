use ir::{bytecode::*, *};

#[test]
fn exact_encoded_byte_ceiling_accepts_the_same_valid_output() {
    if !available() {
        return;
    }
    let mut builder = ModuleBuilder::new("whale-bytecode-v1", DataLayout::default_64bit_le());
    let mut function = builder.begin_function("answer", vec![], Type::U32);
    let value = function.const_uint(Type::U32, 42);
    function.ret(Some(value));
    function.finish();
    let module = builder.finish();
    let bytes = compile(&module, BytecodeLimits::default()).unwrap();
    let exact = BytecodeLimits { max_input_bytes: bytes.len() as u64, ..Default::default() };
    assert_eq!(compile(&module, exact).unwrap(), bytes);
}
