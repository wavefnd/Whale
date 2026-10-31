# SPDX-License-Identifier: MPL-2.0
wave_aarch64_dependencies = []
wave_aarch64_directory = get_option('wave-aarch64-dir')
if wave_aarch64_directory == '' and wave_source != ''
  wave_aarch64_directory = meson.current_build_dir()
  module_target = cargo_host_target
  if module_target == ''
    target_os = {'linux': 'unknown-linux-gnu', 'darwin': 'apple-darwin', 'windows': 'pc-windows-msvc'}
    module_target = host_machine.cpu_family() + '-' + target_os[host_machine.system()]
  endif
  archive_name = module_target.contains('msvc') ? 'whale_wave_aarch64.lib' : 'libwhale_wave_aarch64.a'
  arguments = ['--module', 'aarch64', '--wave-source', wave_source, '--out-dir', '@OUTDIR@',
               '--bootstrap-dir', wave_cache, '--target', module_target,
               '--rust-toolchain', get_option('wave-rust-toolchain')]
  if get_option('wave-allow-network')
    arguments += ['--allow-network']
  endif
  wave_aarch64_module = custom_target('wave-aarch64',
    output: [archive_name, 'wave-aarch64.meta', 'provenance.json'],
    command: [python, files('../../tools/build_wave_assembler.py'), arguments],
    depend_files: files(
      '../model.wave',
      '../declarations.wave',
      '../fixed_model.wave',
      '../fixed_declarations.wave',
      'registers.wave',
      'parser.wave',
      'operands.wave',
      'encoding.wave',
      'encoder.wave',
      'entry.wave',
      'src/wave.rs',
      'src/lib.rs',
      'src/encoder.rs',
      'src/parser.rs',
      '../../tools/build_wave_assembler.py',
      '../../tools/wave_bootstrap.py',
      '../build_support.rs',
    ),
    depends: wave_bootstrap, console: true)
  wave_aarch64_dependencies += [wave_aarch64_module]
elif wave_aarch64_directory != ''
  if not fs.is_absolute(wave_aarch64_directory)
    error('-Dwave-aarch64-dir must be absolute.')
  endif
  archive_name = host_machine.system() == 'windows' ? 'whale_wave_aarch64.lib' : 'libwhale_wave_aarch64.a'
  foreach artifact : [archive_name, 'wave-aarch64.meta', 'provenance.json']
    if not fs.is_file(wave_aarch64_directory / artifact)
      error('Missing Wave aarch64 artifact: ' + (wave_aarch64_directory / artifact))
    endif
  endforeach
endif
wave_assembler_all_dependencies += wave_aarch64_dependencies
