# SPDX-License-Identifier: MPL-2.0
wave_ir_dependencies = []
wave_ir_directory = get_option('wave-ir-dir')
if wave_ir_directory == '' and not get_option('rust-reference')
  wave_ir_directory = meson.current_build_dir()
  archive_name = module_target.contains('msvc') ? 'whale_wave_ir.lib' : 'libwhale_wave_ir.a'
  wave_ir_module = custom_target('wave-ir',
    output: [archive_name, 'module.meta', 'provenance.json'],
    command: [python, files('../tools/vex_module.py'), '--module', 'ir',
              '--vex', vex, '--toolchain', wave_toolchain,
              '--build-dir', meson.project_build_root() / 'vex/ir', '--state-dir', wave_state,
              '--output-dir', '@OUTDIR@', '--depfile', '@DEPFILE@',
              '--target', module_target, '--profile', wave_profile],
    depfile: 'module.d', depends: wave_prepared,
    depend_files: files('vex.ws', '../tools/vex_module.py', '../tools/wave_modules.py'),
    build_always_stale: true, console: true)
  wave_ir_dependencies += [wave_ir_module]
elif wave_ir_directory != ''
  if not fs.is_absolute(wave_ir_directory)
    error('-Dwave-ir-dir must be absolute.')
  endif
  archive_name = host_machine.system() == 'windows' ? 'whale_wave_ir.lib' : 'libwhale_wave_ir.a'
  foreach artifact : [archive_name, 'module.meta', 'provenance.json']
    if not fs.is_file(wave_ir_directory / artifact)
      error('Missing Wave ir artifact: ' + (wave_ir_directory / artifact))
    endif
  endforeach
endif
