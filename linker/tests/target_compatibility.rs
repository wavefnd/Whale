use linker::{
    core::{linker::OutputFormat, symbol_table::SymbolTable},
    Linker,
};

use object::{Endian, Machine, ObjectFile, ObjectTarget, Target};

#[test]
fn link_inputs_are_validated_before_symbol_resolution_or_linking() {
    let amd64 = Target::X86_64WhaleLinux.object_target();
    for invalid in [
        ObjectTarget { machine: Machine::AArch64, ..amd64 },
        ObjectTarget { machine: Machine::RiscV64, ..amd64 },
        ObjectTarget { machine: Machine::LoongArch64, ..amd64 },
        ObjectTarget { endian: Endian::Big, ..amd64 },
        ObjectTarget { address_bits: 32, ..amd64 },
    ] {
        for bad_index in 0..2 {
            let targets = if bad_index == 0 { [invalid, amd64] } else { [amd64, invalid] };
            let objects = targets.map(ObjectFile::with_target);
            assert!(linker::core::executable::executable_layout(&objects, 1024 * 1024).is_err());
            assert!(linker::core::layout::Layout::compute(&objects, 0x400000).is_err());
            let mut table = SymbolTable::new();
            let error = table.resolve(&objects).unwrap_err();
            assert!(error.contains(&format!("link input {bad_index}")), "{error}");
            assert!(error.contains("unsupported object target"), "{error}");
            assert!(table.symbols.is_empty());
            table.resolve(&[ObjectFile::with_target(amd64)]).unwrap();

            let mut linker = Linker::new(OutputFormat::ELF64Executable);
            for object in objects {
                linker.add_object(object);
            }
            let error = linker.link().unwrap_err();
            assert!(error.contains(&format!("link input {bad_index}")), "{error}");
            assert!(error.contains("unsupported object target"), "{error}");
        }
    }
}
