# SPDX-License-Identifier: MPL-2.0
project('Whale', version: '0.1.0', meson_version: '>=1.8.0',
        default_options: ['buildtype=debug'])

# Cargo compiles the current Rust components. Meson owns product selection,
# dependency scheduling, the resulting executable, tests and installation.
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
if get_option('socket-cli')
  selected_features += ['socket-cli']
endif
if get_option('native-linker')
  selected_features += ['native-linker']
endif
if get_option('rust-reference')
  selected_features += ['rust-reference']
endif

cargo_host_target = get_option('cargo-host-target')
if meson.is_cross_build() and cargo_host_target == ''
  error('Cross builds require -Dcargo-host-target=RUST_TRIPLE for the Whale executable host. Assembler output selections do not choose its host.')
endif

# asmobj is one complete Wave source module. Cargo only links it through the
# data transport in toolchain; there is no Rust implementation or fallback.
wave_source = get_option('wave-source')
if not get_option('rust-reference') and wave_source == ''
  required_modules = ['asmobj-dir', 'wave-ir-dir', 'wave-assembler-dir']
  foreach arch : assembler_outputs
    required_modules += ['wave-' + arch + '-dir']
  endforeach
  foreach module : required_modules
    if get_option(module) == ''
      error('The default Wave product requires -Dwave-source=/absolute/Wave or every selected prebuilt module; missing -D' + module + '. Rust reference builds require explicit -Drust-reference=true.')
    endif
  endforeach
endif
wave_cache = get_option('wave-bootstrap-dir')
if wave_cache == ''
  wave_cache = meson.project_build_root() / 'wave-bootstrap'
elif not fs.is_absolute(wave_cache)
  error('-Dwave-bootstrap-dir must be absolute.')
endif
asmobj_directory = get_option('asmobj-dir')
wave_bootstrap = []
needs_wave_bootstrap = asmobj_directory == ''
if wave_source != ''
  needs_wave_bootstrap = needs_wave_bootstrap or get_option('wave-ir-dir') == '' or get_option('wave-assembler-dir') == ''
  foreach arch : ['amd64', 'aarch64', 'riscv64', 'loongarch64']
    if get_option('asm-' + arch)
      needs_wave_bootstrap = needs_wave_bootstrap or get_option('wave-' + arch + '-dir') == ''
    endif
  endforeach
endif
if needs_wave_bootstrap
  if wave_source == '' or not fs.is_absolute(wave_source) or not fs.is_dir(wave_source)
    error('Supply -Dwave-source=/absolute/Wave or -Dasmobj-dir=/absolute/built/module.')
  endif
  if build_machine.system() != 'linux' or build_machine.cpu_family() != 'x86_64'
    error('Building the pinned Wave compiler currently requires a Linux x86_64 build machine; use a prebuilt asmobj module on other build machines.')
  endif
  bootstrap_arguments = ['bootstrap', '--wave-source', wave_source, '--cache-dir', wave_cache,
                        '--rust-toolchain', get_option('wave-rust-toolchain'), '--output', '@OUTPUT@']
  if get_option('wave-allow-network')
    bootstrap_arguments += ['--allow-network']
  endif
  wave_bootstrap = custom_target('wave-bootstrap', output: 'wave-bootstrap-ready.json',
    command: [python, bridge, bootstrap_arguments],
    depend_files: files('tools/wave_bootstrap.py', 'tools/meson_cargo.py'), console: true)
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
