# SPDX-License-Identifier: MPL-2.0
subdir('asmobj')
wave_assembler_all_dependencies = []
wave_assembler_dependencies = []
wave_assembler_directory = get_option('wave-assembler-dir')
if wave_assembler_directory == '' and not get_option('rust-reference')
  wave_assembler_directory = meson.current_build_dir()
  archive_name = module_target.contains('msvc') ? 'whale_wave_assembler.lib' : 'libwhale_wave_assembler.a'
  wave_assembler_module = custom_target('wave-assembler',
    output: [archive_name, 'module.meta', 'provenance.json'],
    command: [python, files('../tools/vex_module.py'), '--module', 'assembler',
              '--vex', vex, '--toolchain', wave_toolchain,
              '--build-dir', meson.project_build_root() / 'vex/assembler', '--state-dir', wave_state,
              '--output-dir', '@OUTDIR@', '--depfile', '@DEPFILE@',
              '--target', module_target, '--profile', wave_profile],
    depfile: 'module.d', depends: wave_prepared,
    depend_files: files('vex.ws', '../tools/vex_module.py', '../tools/wave_modules.py'),
    build_always_stale: true, console: true)
  wave_assembler_dependencies += [wave_assembler_module]
elif wave_assembler_directory != ''
  if not fs.is_absolute(wave_assembler_directory)
    error('-Dwave-assembler-dir must be absolute.')
  endif
  archive_name = host_machine.system() == 'windows' ? 'whale_wave_assembler.lib' : 'libwhale_wave_assembler.a'
  foreach artifact : [archive_name, 'module.meta', 'provenance.json']
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
wave_aarch64_directory = ''
if get_option('asm-aarch64')
  subdir('aarch64')
endif
wave_riscv64_directory = ''
if get_option('asm-riscv64')
  subdir('riscv64')
endif
wave_loongarch64_directory = ''
if get_option('asm-loongarch64')
  subdir('loongarch64')
endif
