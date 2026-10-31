# SPDX-License-Identifier: MPL-2.0
wave_amd64_dependencies = []
wave_amd64_directory = get_option('wave-amd64-dir')
if wave_amd64_directory == '' and wave_source != ''
  wave_amd64_directory = meson.current_build_dir()
  module_target = cargo_host_target
  if module_target == ''
    target_os = {'linux': 'unknown-linux-gnu', 'darwin': 'apple-darwin', 'windows': 'pc-windows-msvc'}
    module_target = host_machine.cpu_family() + '-' + target_os[host_machine.system()]
  endif
  archive_name = module_target.contains('msvc') ? 'whale_wave_amd64.lib' : 'libwhale_wave_amd64.a'
  arguments = ['--module', 'amd64', '--wave-source', wave_source, '--out-dir', '@OUTDIR@',
               '--bootstrap-dir', wave_cache, '--target', module_target,
               '--rust-toolchain', get_option('wave-rust-toolchain')]
  if get_option('wave-allow-network')
    arguments += ['--allow-network']
  endif
  wave_amd64_module = custom_target('wave-amd64',
    output: [archive_name, 'wave-amd64.meta', 'provenance.json'],
    command: [python, files('../../tools/build_wave_assembler.py'), arguments],
    depend_files: files(
      'src/lib.rs',
      '../model.wave',
      '../declarations.wave',
      '../ast_model.wave',
      'parser_model.wave',
      'registers.wave',
      'expressions.wave',
      'operands.wave',
      'directives.wave',
      'parser.wave',
      'encoder_model.wave',
      'namespace.wave',
      'layout.wave',
      'address.wave',
      'emit.wave',
      'integer.wave',
      'sse.wave',
      'branch.wave',
      'directives_encode.wave',
      'encoder.wave',
      'src/ast_model.rs',
      'src/parser_wave.rs',
      'src/encoder_wave.rs',
      'src/encoder_namespace_errors.rs',
      'src/tables.rs',
      'src/encoder.rs',
      'src/encoding/address.rs',
      'src/encoding/modrm.rs',
      'src/encoding/rex.rs',
      'src/encoding/sib.rs',
      '../../tools/build_wave_assembler.py',
      '../../tools/wave_bootstrap.py',
      '../build_support.rs',
    ),
    depends: wave_bootstrap, console: true)
  wave_amd64_dependencies += [wave_amd64_module]
elif wave_amd64_directory != ''
  if not fs.is_absolute(wave_amd64_directory)
    error('-Dwave-amd64-dir must be absolute.')
  endif
  archive_name = host_machine.system() == 'windows' ? 'whale_wave_amd64.lib' : 'libwhale_wave_amd64.a'
  foreach artifact : [archive_name, 'wave-amd64.meta', 'provenance.json']
    if not fs.is_file(wave_amd64_directory / artifact)
      error('Missing Wave amd64 artifact: ' + (wave_amd64_directory / artifact))
    endif
  endforeach
endif
wave_assembler_all_dependencies += wave_amd64_dependencies
