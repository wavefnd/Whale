# SPDX-License-Identifier: MPL-2.0
subdir('asmobj')
wave_assembler_all_dependencies = []
wave_assembler_dependencies = []
wave_assembler_directory = get_option('wave-assembler-dir')
if wave_assembler_directory == '' and wave_source != ''
  wave_assembler_directory = meson.current_build_dir()
  module_target = cargo_host_target
  if module_target == ''
    target_os = {'linux': 'unknown-linux-gnu', 'darwin': 'apple-darwin', 'windows': 'pc-windows-msvc'}
    module_target = host_machine.cpu_family() + '-' + target_os[host_machine.system()]
  endif
  archive_name = module_target.contains('msvc') ? 'whale_wave_assembler.lib' : 'libwhale_wave_assembler.a'
  arguments = ['--module', 'common', '--wave-source', wave_source, '--out-dir', '@OUTDIR@',
               '--bootstrap-dir', wave_cache, '--target', module_target,
               '--rust-toolchain', get_option('wave-rust-toolchain')]
  if get_option('wave-allow-network')
    arguments += ['--allow-network']
  endif
  wave_assembler_module = custom_target('wave-assembler',
    output: [archive_name, 'wave-assembler.meta', 'provenance.json'],
    command: [python, files('../tools/build_wave_assembler.py'), arguments],
    depend_files: files(
      'model.wave',
      'unicode.wave',
      'numeric.wave',
      'output.wave',
      'lexer.wave',
      'src/tokens.rs',
      'src/tokens_wave.rs',
      '../tools/build_wave_assembler.py',
      '../tools/wave_bootstrap.py',
      'build_support.rs',
    ),
    depends: wave_bootstrap, console: true)
  wave_assembler_dependencies += [wave_assembler_module]
elif wave_assembler_directory != ''
  if not fs.is_absolute(wave_assembler_directory)
    error('-Dwave-assembler-dir must be absolute.')
  endif
  archive_name = host_machine.system() == 'windows' ? 'whale_wave_assembler.lib' : 'libwhale_wave_assembler.a'
  foreach artifact : [archive_name, 'wave-assembler.meta', 'provenance.json']
    if not fs.is_file(wave_assembler_directory / artifact)
      error('Missing Wave assembler artifact: ' + (wave_assembler_directory / artifact))
    endif
  endforeach
endif
wave_assembler_all_dependencies += wave_assembler_dependencies
wave_amd64_directory = ''
if get_option('asm-amd64')
  subdir('amd64')
endif
if wave_amd64_directory != '' and wave_assembler_directory == ''
  error('The Wave AMD64 module requires -Dwave-assembler-dir or a Wave source build.')
endif
