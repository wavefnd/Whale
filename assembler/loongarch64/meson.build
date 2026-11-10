# SPDX-License-Identifier: MPL-2.0
wave_loongarch64_dependencies = []
wave_loongarch64_directory = get_option('wave-loongarch64-dir')
if wave_loongarch64_directory == '' and not get_option('rust-reference')
  wave_loongarch64_directory = meson.current_build_dir()
  archive_name = module_target.contains('msvc') ? 'whale_wave_loongarch64.lib' : 'libwhale_wave_loongarch64.a'
  wave_loongarch64_module = custom_target('wave-loongarch64',
    output: [archive_name, 'module.meta', 'provenance.json'],
    command: [python, files('../../tools/vex_module.py'), '--module', 'loongarch64',
              '--vex', vex, '--toolchain', wave_toolchain,
              '--build-dir', meson.project_build_root() / 'vex/loongarch64', '--state-dir', wave_state,
              '--output-dir', '@OUTDIR@', '--depfile', '@DEPFILE@',
              '--target', module_target, '--profile', wave_profile],
    depfile: 'module.d', depends: wave_prepared,
    depend_files: files('vex.ws', '../../tools/vex_module.py', '../../tools/wave_modules.py'),
    build_always_stale: true, console: true)
  wave_loongarch64_dependencies += [wave_loongarch64_module]
elif wave_loongarch64_directory != ''
  if not fs.is_absolute(wave_loongarch64_directory)
    error('-Dwave-loongarch64-dir must be absolute.')
  endif
  archive_name = host_machine.system() == 'windows' ? 'whale_wave_loongarch64.lib' : 'libwhale_wave_loongarch64.a'
  foreach artifact : [archive_name, 'module.meta', 'provenance.json']
    if not fs.is_file(wave_loongarch64_directory / artifact)
      error('Missing Wave loongarch64 artifact: ' + (wave_loongarch64_directory / artifact))
    endif
  endforeach
endif
wave_assembler_all_dependencies += wave_loongarch64_dependencies
