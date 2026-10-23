# Contributing to Whale

Keep contributions focused and include regression tests. Public PR descriptions
and review comments should be in English. Follow the
[Code of Conduct](CODE_OF_CONDUCT.md); component contacts are in [MAINTAINERS](MAINTAINERS).

## 1. Development setup

Use [BUILDING.md](BUILDING.md) for Meson, the pinned Wave compiler, dependencies
and installation. Rust's MSRV is **1.86.0**; CI uses Python **3.13**.

Rust component tests can run without Wave. The complete product and
assembly-to-object tests require the built Wave `asmobj` archive. A linked
executable does not need the Wave compiler at runtime.

## 2. Repository structure and module boundaries

| Sources | Responsibility |
| --- | --- |
| `assembler/<architecture>/` | Rust registers, operands, parser and encoder for that ISA |
| `assembler/src/` | Shared Rust assembly types and helpers |
| `assembler/asmobj/` | Complete Wave assembly-to-object module |
| `toolchain/`, `backend-api/` | Rust composition, backend interfaces and external data calls to `asmobj` |
| `ir/`, `target-spec/` | Rust IR semantics, target identities and layouts |
| `object/`, `linker/` | Rust object formats, relocations and executable layout |
| `reverse/` | Rust image inspection; decoder and CFG currently unavailable |
| `src/`, `tests/` | CLI and integration tests |
| `tools/`, `.github/` | Build and verification helpers |

Split source files by role. Shared modules must not import a concrete ISA.
`asmobj` contains only Wave sources; its Rust data boundary lives in
`toolchain/src/assembly_object.rs`. Object serialization belongs to `object`.
IR remains Rust.

### Adding an assembler or IR target

1. Add architecture sources under `assembler/<architecture>/`, or the owning IR backend.
2. Update `target-spec`, explicit dispatch and `src/builtins.rs` as needed.
3. Update Meson/Cargo source selections and Wave `asmobj` relocation mappings.
4. Test emitted bytes, metadata, rejected inputs and CLI behavior.

Official and contributed architectures follow the same process. Central enum,
dispatch and build-list edits are expected. Run `python3 tools/check_module_layers.py`
to check dependency direction. See [BUILDING.md](BUILDING.md#select-assembler-outputs-and-host-machines)
for product selection and executable-host versus output-target settings.

## 3. Submit a contribution

Target `wavefnd/Whale:master` from a branch in your fork. Check open issues for
existing work and dependencies; coordinate before starting a shared issue.
A documentation fix does not require a new issue.

Describe the problem, resulting behavior, compatibility changes and checks run.
Link relevant issues; use closing references only for completed requirements.
Keep fixes on the same PR branch. Do not merge or publish a release as part of
opening a PR.

Email patches may be sent to the component contact in [MAINTAINERS](MAINTAINERS)
with `[Whale PATCH]` in the subject.

## 4. Developer Certificate of Origin

Sign off every commit under the [Developer Certificate of Origin](https://developercertificate.org/):

```sh
git add path/to/changed-file
git commit -s -m "component: describe the change"
```

The `Signed-off-by` name and email must identify the contributor certifying the
change. A sign-off is separate from cryptographic commit signing. Preserve
existing authorship and attribution.

## 5. Local verification

For assembly-to-object and CLI checks, first build `asmobj` as described in
[BUILDING.md](BUILDING.md), then supply its archive explicitly:

```sh
export WHALE_ASMOBJ_DIR=/absolute/path/to/built-asmobj
python3 tools/format-rust.py --check
python3 -m unittest discover -s tools -p 'test_*.py' -v
python3 tools/check_module_layers.py
cargo test --workspace --locked
cargo test --workspace --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps --locked
cargo build --workspace --all-features --locked
python3 tools/ci_smoke.py --binary target/debug/whale --socket enabled
```

On Windows use `target/debug/whale.exe`. For Rust-only work, start with a focused
command such as `cargo test -p ir --all-features --locked` without the archive.
See the [Rust](.github/workflows/rust.yml) and [quality](.github/workflows/quality.yml)
workflows for MSRV, host, default-feature and optimized checks.

Report exact commands and results. Distinguish cross-produced objects from
executed native tests, and state skipped checks. Do not weaken tests or CI to
make a patch pass.

## 6. Code style and regression tests

Follow neighboring code. Check formatting before applying it, and keep unrelated
formatting out of the patch. Tests belong beside the affected component; exercise
public behavior and meaningful output, not just successful construction.
CLI tests should cover status, diagnostics and preservation of existing files.
Keep [README.md](README.md) examples and command help accurate.

## 7. Updating a PR branch

Commit or deliberately stash local changes, then rebase onto current upstream
`master`. Reconcile conflicts without discarding upstream fixes and rerun the
affected checks. Inspect `git diff --check` before pushing.

Use a normal push for new history. After rewriting an already published branch,
use `git push --force-with-lease`; inspect a rejected lease instead of forcing it.

## 8. Licensing and repository policies

Preserve [MPL-2.0](LICENSE) notices and attribution in [COPYRIGHT](COPYRIGHT) and
[NOTICE](NOTICE). Read [ai.txt](ai.txt) for the AI-use policy. Contributors remain
responsible for authorship, licensing, sign-off, review and accurate validation.
