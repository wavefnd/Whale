# SPDX-License-Identifier: MPL-2.0
asmobj_dependencies = []
if asmobj_directory == ''
  asmobj_directory = meson.current_build_dir()
  module_target = cargo_host_target
  if module_target == ''
    target_os = {'linux': 'unknown-linux-gnu', 'darwin': 'apple-darwin', 'windows': 'pc-windows-msvc'}
    if not target_os.has_key(host_machine.system())
      error('Set cargo-host-target for this executable host.')
    endif
    module_target = host_machine.cpu_family() + '-' + target_os[host_machine.system()]
  endif
  archive_name = module_target.contains('msvc') ? 'whale_asmobj.lib' : 'libwhale_asmobj.a'
  arguments = ['--wave-source', wave_source, '--out-dir', '@OUTDIR@',
               '--bootstrap-dir', wave_cache, '--target', module_target,
               '--rust-toolchain', get_option('wave-rust-toolchain')]
  if get_option('wave-allow-network')
    arguments += ['--allow-network']
  endif
  asmobj_module = custom_target('asmobj',
    output: [archive_name, 'asmobj.meta', 'provenance.json'],
    command: [python, files('../../tools/build_asmobj.py'), arguments],
    depend_files: files('model.wave', 'sections.wave', 'symbols.wave', 'relocations.wave', 'entry.wave',
                        '../../tools/build_asmobj.py', '../../tools/wave_bootstrap.py'),
    depends: wave_bootstrap, console: true)
  asmobj_dependencies += [asmobj_module]
else
  if not fs.is_absolute(asmobj_directory)
    error('-Dasmobj-dir must be absolute.')
  endif
  archive_name = host_machine.system() == 'windows' ? 'whale_asmobj.lib' : 'libwhale_asmobj.a'
  foreach artifact : [archive_name, 'asmobj.meta']
    if not fs.is_file(asmobj_directory / artifact)
      error('Missing asmobj module artifact: ' + (asmobj_directory / artifact))
    endif
  endforeach
endif
