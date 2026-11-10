# SPDX-License-Identifier: MPL-2.0
asmobj_dependencies = []
asmobj_directory = get_option('asmobj-dir')
if asmobj_directory == ''
  asmobj_directory = meson.current_build_dir()
  archive_name = module_target.contains('msvc') ? 'whale_asmobj.lib' : 'libwhale_asmobj.a'
  asmobj_module = custom_target('wave-asmobj',
    output: [archive_name, 'module.meta', 'provenance.json'],
    command: [python, files('../../tools/vex_module.py'), '--module', 'asmobj',
              '--vex', vex, '--toolchain', wave_toolchain,
              '--build-dir', meson.project_build_root() / 'vex/asmobj', '--state-dir', wave_state,
              '--output-dir', '@OUTDIR@', '--depfile', '@DEPFILE@',
              '--target', module_target, '--profile', wave_profile],
    depfile: 'module.d', depends: wave_prepared,
    depend_files: files('vex.ws', '../../tools/vex_module.py', '../../tools/wave_modules.py'),
    build_always_stale: true, console: true)
  asmobj_dependencies += [asmobj_module]
elif asmobj_directory != ''
  if not fs.is_absolute(asmobj_directory)
    error('-Dasmobj-dir must be absolute.')
  endif
  archive_name = host_machine.system() == 'windows' ? 'whale_asmobj.lib' : 'libwhale_asmobj.a'
  foreach artifact : [archive_name, 'module.meta', 'provenance.json']
    if not fs.is_file(asmobj_directory / artifact)
      error('Missing Wave asmobj artifact: ' + (asmobj_directory / artifact))
    endif
  endforeach
endif
