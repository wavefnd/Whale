# SPDX-License-Identifier: MPL-2.0
project('Whale', version: '0.1.0', meson_version: '>=1.8.5',
        default_options: ['buildtype=debug'])

# Meson automates the product. Vex owns Wave packages; Cargo owns Rust bindings.
python = import('python').find_installation('python3')
cargo = find_program('cargo', native: true)
rustc = find_program('rustc', native: true)
fs = import('fs')
bridge = files('tools/meson_cargo.py')
selected_features = []
assembler_outputs = []
foreach arch : ['amd64', 'aarch64', 'riscv64', 'loongarch64']
  if get_option('asm-' + arch)
    selected_features += ['asm-' + arch]
    assembler_outputs += [arch]
  endif
endforeach
foreach feature : ['socket-cli', 'native-linker', 'rust-reference']
  if get_option(feature)
    selected_features += [feature]
  endif
endforeach
cargo_host_target = get_option('cargo-host-target')
if meson.is_cross_build() and cargo_host_target == ''
  error('Cross builds require -Dcargo-host-target=RUST_TRIPLE for the Whale executable host.')
endif
module_target = cargo_host_target
if module_target == ''
  target_os = {'linux': 'unknown-linux-gnu', 'darwin': 'apple-darwin', 'windows': 'pc-windows-msvc', 'freebsd': 'unknown-freebsd'}
  if not target_os.has_key(host_machine.system())
    error('Set cargo-host-target for this executable host.')
  endif
  module_target = host_machine.cpu_family() + '-' + target_os[host_machine.system()]
  if host_machine.cpu_family() == 'riscv64'
    module_target = 'riscv64gc-' + target_os[host_machine.system()]
  endif
endif
wave_profile = get_option('buildtype') in ['release', 'minsize', 'debugoptimized'] ? 'release' : 'dev'
needs_vex = get_option('asmobj-dir') == ''
if not get_option('rust-reference')
  foreach option : ['wave-ir-dir', 'wave-assembler-dir']
    needs_vex = needs_vex or get_option(option) == ''
  endforeach
  foreach arch : assembler_outputs
    needs_vex = needs_vex or get_option('wave-' + arch + '-dir') == ''
  endforeach
endif
wave_prepared = []
if needs_vex
  vex = find_program(get_option('vex'), native: true)
  wave_cache = get_option('wave-cache-dir')
  if wave_cache == ''
    wave_cache = meson.project_build_root() / 'wave-nightly'
  elif not fs.is_absolute(wave_cache)
    error('-Dwave-cache-dir must be absolute.')
  endif
  toolchain_arguments = ['--cache-dir', wave_cache, '--output', '@OUTPUT@']
  if get_option('wave-offline')
    toolchain_arguments += ['--offline']
  endif
  wave_toolchain = custom_target('wave-toolchain', output: 'wave-toolchain.json',
    command: [python, files('tools/wave_nightly.py'), toolchain_arguments],
    depend_files: files('tools/wave_nightly.py'),
    build_always_stale: true, console: true)
  wave_state = meson.project_build_root() / 'vex-state'
  wave_prepared = custom_target('wave-prepared', output: 'wave-ready.json',
    command: [python, files('tools/vex_module.py'), '--prepare', '--vex', vex,
              '--toolchain', wave_toolchain, '--build-dir', meson.project_build_root() / 'vex/prepare',
              '--state-dir', wave_state, '--ready', '@OUTPUT@'],
    depend_files: files('vex.ws', 'vex.lock', 'tools/wave_modules.py', 'tools/vex_module.py'),
    build_always_stale: true, console: true)
endif
subdir('assembler')
subdir('ir')

profile = 'dev'
if get_option('buildtype') in ['release', 'minsize']
  profile = 'release'
endif
optimization = get_option('optimization')
if optimization == 'plain'
  optimization = '0'
elif optimization == 'g'
  optimization = '1'
endif
debug_info = get_option('debug') ? '2' : '0'
binary_name = host_machine.system() == 'windows' ? 'whale.exe' : 'whale'
build_arguments = [
  'build', '--cargo', cargo.full_path(),
  '--rustc', rustc.full_path(),
  '--source-root', meson.project_source_root(),
  '--target-dir', meson.project_build_root() / 'cargo',
  '--output', '@OUTPUT@', '--depfile', '@DEPFILE@',
  '--profile', profile, '--opt-level', optimization, '--debug-info', debug_info,
  '--jobs', get_option('cargo-jobs').to_string(),
  '--host-system', host_machine.system(),
  '--host-cpu-family', host_machine.cpu_family(),
]
if cargo_host_target != ''
  build_arguments += ['--host-target', cargo_host_target]
endif
if get_option('cargo-offline')
  build_arguments += ['--offline']
endif
build_arguments += ['--asmobj-dir', asmobj_directory]
if wave_ir_directory != ''
  build_arguments += ['--wave-ir-dir', wave_ir_directory]
endif
if wave_assembler_directory != ''
  build_arguments += ['--wave-assembler-dir', wave_assembler_directory]
endif
if wave_amd64_directory != ''
  build_arguments += ['--wave-amd64-dir', wave_amd64_directory]
endif
if wave_aarch64_directory != ''
  build_arguments += ['--wave-aarch64-dir', wave_aarch64_directory]
endif
if wave_riscv64_directory != ''
  build_arguments += ['--wave-riscv64-dir', wave_riscv64_directory]
endif
if wave_loongarch64_directory != ''
  build_arguments += ['--wave-loongarch64-dir', wave_loongarch64_directory]
endif
foreach feature : selected_features
  build_arguments += ['--feature', feature]
endforeach

whale = custom_target('whale-cli',
  output: binary_name,
  command: [python, bridge, build_arguments],
  depend_files: files('Cargo.toml', 'Cargo.lock', 'tools/meson_cargo.py'),
  depfile: 'whale-cargo.d',
  depends: asmobj_dependencies + wave_ir_dependencies + wave_assembler_all_dependencies,
  build_by_default: true,
  console: true,
  install: true,
  install_dir: get_option('bindir'),
  install_mode: 'rwxr-xr-x',
)

test('build-helper', python, args: files('tools/test_meson_build.py'))
if not meson.is_cross_build() or get_option('host-runner').length() > 0
  smoke_arguments = ['smoke', '--binary', whale]
  foreach feature : selected_features
    smoke_arguments += ['--feature', feature]
  endforeach
  foreach argument : get_option('host-runner')
    smoke_arguments += ['--runner-arg=' + argument]
  endforeach
  test('cli-smoke', python, args: [bridge, smoke_arguments], timeout: 90)
else
  message('Host runtime smoke is omitted for this cross build; explicitly set -Dhost-runner=[...] to run it.')
endif

summary({
  'Build machine': build_machine.system() + '/' + build_machine.cpu_family(),
  'Whale executable host': host_machine.system() + '/' + host_machine.cpu_family(),
  'Cargo host target': cargo_host_target == '' ? 'native Rust toolchain host' : cargo_host_target,
  'Assembler output modules': assembler_outputs,
  'Complete Wave module': 'assembler/asmobj',
  'Wave IR sources': wave_ir_directory == '' ? 'Rust compatibility path' : wave_ir_directory,
  'Rust reference algorithms allowed': get_option('rust-reference'),
  'Cargo offline': get_option('cargo-offline'),
}, section: 'Whale source modules')
