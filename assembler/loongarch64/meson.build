# SPDX-License-Identifier: MPL-2.0
wave_loongarch64_dependencies = []
wave_loongarch64_directory = get_option('wave-loongarch64-dir')
if wave_loongarch64_directory == '' and wave_source != ''
  wave_loongarch64_directory = meson.current_build_dir()
  module_target = cargo_host_target
  if module_target == ''
    target_os = {'linux': 'unknown-linux-gnu', 'darwin': 'apple-darwin', 'windows': 'pc-windows-msvc'}
    module_target = host_machine.cpu_family() + '-' + target_os[host_machine.system()]
  endif
  archive_name = module_target.contains('msvc') ? 'whale_wave_loongarch64.lib' : 'libwhale_wave_loongarch64.a'
  arguments = ['--module', 'loongarch64', '--wave-source', wave_source, '--out-dir', '@OUTDIR@',
               '--bootstrap-dir', wave_cache, '--target', module_target,
               '--rust-toolchain', get_option('wave-rust-toolchain')]
  if get_option('wave-allow-network')
    arguments += ['--allow-network']
  endif
  wave_loongarch64_module = custom_target('wave-loongarch64',
    output: [archive_name, 'wave-loongarch64.meta', 'provenance.json'],
    command: [python, files('../../tools/build_wave_assembler.py'), arguments],
    depend_files: files(
      '../model.wave',
      '../declarations.wave',
      '../fixed_model.wave',
      '../fixed_declarations.wave',
      'model.wave',
      'registers.wave',
      'tables.wave',
      'operands.wave',
      'formats.wave',
      'branch.wave',
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
  wave_loongarch64_dependencies += [wave_loongarch64_module]
elif wave_loongarch64_directory != ''
  if not fs.is_absolute(wave_loongarch64_directory)
    error('-Dwave-loongarch64-dir must be absolute.')
  endif
  archive_name = host_machine.system() == 'windows' ? 'whale_wave_loongarch64.lib' : 'libwhale_wave_loongarch64.a'
  foreach artifact : [archive_name, 'wave-loongarch64.meta', 'provenance.json']
    if not fs.is_file(wave_loongarch64_directory / artifact)
      error('Missing Wave loongarch64 artifact: ' + (wave_loongarch64_directory / artifact))
    endif
  endforeach
endif
wave_assembler_all_dependencies += wave_loongarch64_dependencies
