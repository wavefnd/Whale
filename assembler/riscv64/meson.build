# SPDX-License-Identifier: MPL-2.0
wave_riscv64_dependencies = []
wave_riscv64_directory = get_option('wave-riscv64-dir')
if wave_riscv64_directory == '' and wave_source != ''
  wave_riscv64_directory = meson.current_build_dir()
  module_target = cargo_host_target
  if module_target == ''
    target_os = {'linux': 'unknown-linux-gnu', 'darwin': 'apple-darwin', 'windows': 'pc-windows-msvc'}
    module_target = host_machine.cpu_family() + '-' + target_os[host_machine.system()]
  endif
  archive_name = module_target.contains('msvc') ? 'whale_wave_riscv64.lib' : 'libwhale_wave_riscv64.a'
  arguments = ['--module', 'riscv64', '--wave-source', wave_source, '--out-dir', '@OUTDIR@',
               '--bootstrap-dir', wave_cache, '--target', module_target,
               '--rust-toolchain', get_option('wave-rust-toolchain')]
  if get_option('wave-allow-network')
    arguments += ['--allow-network']
  endif
  wave_riscv64_module = custom_target('wave-riscv64',
    output: [archive_name, 'wave-riscv64.meta', 'provenance.json'],
    command: [python, files('../../tools/build_wave_assembler.py'), arguments],
    depend_files: files(
      '../model.wave',
      '../declarations.wave',
      '../fixed_model.wave',
      '../fixed_declarations.wave',
      'registers.wave',
      'operands.wave',
      'encoding.wave',
      'encoder.wave',
      'parser.wave',
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
  wave_riscv64_dependencies += [wave_riscv64_module]
elif wave_riscv64_directory != ''
  if not fs.is_absolute(wave_riscv64_directory)
    error('-Dwave-riscv64-dir must be absolute.')
  endif
  archive_name = host_machine.system() == 'windows' ? 'whale_wave_riscv64.lib' : 'libwhale_wave_riscv64.a'
  foreach artifact : [archive_name, 'wave-riscv64.meta', 'provenance.json']
    if not fs.is_file(wave_riscv64_directory / artifact)
      error('Missing Wave riscv64 artifact: ' + (wave_riscv64_directory / artifact))
    endif
  endforeach
endif
wave_assembler_all_dependencies += wave_riscv64_dependencies
