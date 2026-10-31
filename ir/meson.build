# SPDX-License-Identifier: MPL-2.0
wave_ir_dependencies = []
wave_ir_directory = get_option('wave-ir-dir')
if wave_ir_directory == '' and wave_source != ''
  wave_ir_directory = meson.current_build_dir()
  module_target = cargo_host_target
  if module_target == ''
    target_os = {'linux': 'unknown-linux-gnu', 'darwin': 'apple-darwin', 'windows': 'pc-windows-msvc'}
    module_target = host_machine.cpu_family() + '-' + target_os[host_machine.system()]
  endif
  archive_name = module_target.contains('msvc') ? 'whale_wave_ir.lib' : 'libwhale_wave_ir.a'
  arguments = ['--wave-source', wave_source, '--out-dir', '@OUTDIR@',
               '--bootstrap-dir', wave_cache, '--target', module_target,
               '--rust-toolchain', get_option('wave-rust-toolchain')]
  if get_option('wave-allow-network')
    arguments += ['--allow-network']
  endif
  wave_ir_module = custom_target('wave-ir',
    output: [archive_name, 'wave-ir.meta', 'provenance.json'],
    command: [python, files('../tools/build_wave_ir.py'), arguments],
    depend_files: files(
      'src/lib.rs',
      'model.wave',
      'layout.wave',
      'lexer.wave',
      'header.wave',
      'type_parser.wave',
      'literal.wave',
      'expression_parser.wave',
      'instruction_parser.wave',
      'module_parser.wave',
      'lower_hir.wave',
      'lower_model.wave',
      'lower_output.wave',
      'lower_expression.wave',
      'lower_storage.wave',
      'lower_statement.wave',
      'lower_driver.wave',
      'builder.wave',
      'builder_namespace.wave',
      'builder_body.wave',
      'verify.wave',
      'verifier_index.wave',
      'verifier_ssa.wave',
      'printer.wave',
      'cfg.wave',
      'shape.wave',
      'source.wave',
      'memory.wave',
      'lower_limits.wave',
      'wide.wave',
      'integer.wave',
      'float_model.wave',
      'float_arithmetic.wave',
      'float_convert.wave',
      'effects.wave',
      'passes.wave',
      'fold_integer.wave',
      'constant.wave',
      'constant_dependencies.wave',
      'runtime_model.wave',
      'runtime_prepare.wave',
      'runtime_execute.wave',
      'runtime_driver.wave',
      'bytecode_model.wave',
      'bytecode_inventory.wave',
      'bytecode_loader.wave',
      'bytecode_verify_types.wave',
      'bytecode_verify_instructions.wave',
      'bytecode_verify_flow.wave',
      'bytecode_verify.wave',
      'bytecode_writer.wave',
      'bytecode_runtime.wave',
      'entry.wave',
      'src/wave_layout.rs',
      'src/interpreter/integer.rs',
      'src/float/wave.rs',
      'src/effects_wave.rs',
      'src/pass_wave.rs',
      'src/fold_integer_wave.rs',
      'src/constant_wave.rs',
      'src/interpreter/runtime_wave.rs',
      'src/bytecode.rs',
      'src/verifier/wave_constants.rs',
      'src/source_wave.rs',
      'src/interpreter/memory_wave.rs',
      'src/lower_ast/wave_limits.rs',
      'src/parser/wave_lexer.rs',
      'src/parser/wave_header.rs',
      'src/parser/wave_text.rs',
      'src/parser/wave_type.rs',
      'src/parser/wave_literal.rs',
      'src/builder/wave.rs',
      'src/builder/namespace.rs',
      'src/builder/body.rs',
      'src/parser/wave_expression.rs',
      'src/parser/wave_instruction.rs',
      'src/parser/wave_module.rs',
      'src/lower_ast/wave_hir.rs',
      'src/lower_ast/wave_lower.rs',
      'src/verifier/wave_semantics.rs',
      'src/verifier/wave_index.rs',
      'src/verifier/wave_ssa.rs',
      'src/printer_wave.rs',
      'src/verifier/wave_cfg.rs',
      'src/verifier/wave_shape.rs',
      'src/function.rs',
      'src/global_storage.rs',
      '../tools/build_wave_ir.py',
      '../tools/wave_bootstrap.py',
    ),
    depends: wave_bootstrap, console: true)
  wave_ir_dependencies += [wave_ir_module]
elif wave_ir_directory != ''
  if not fs.is_absolute(wave_ir_directory)
    error('-Dwave-ir-dir must be absolute.')
  endif
  archive_name = host_machine.system() == 'windows' ? 'whale_wave_ir.lib' : 'libwhale_wave_ir.a'
  foreach artifact : [archive_name, 'wave-ir.meta', 'provenance.json']
    if not fs.is_file(wave_ir_directory / artifact)
      error('Missing Wave IR module artifact: ' + (wave_ir_directory / artifact))
    endif
  endforeach
endif
