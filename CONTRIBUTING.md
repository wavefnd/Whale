# Contributing to Whale

Thank you for contributing to Whale, the Wave Foundation's low-level toolchain.
Code, regression tests, documentation, and review are all welcome. Keep each
contribution focused on one observable change.

Read the [Code of Conduct](CODE_OF_CONDUCT.md) and use [MAINTAINERS](MAINTAINERS)
to find the contact for the component you are changing. Public pull request
descriptions and review comments should be written in English.

## 1. Development setup

Install Git and stable Rust through rustup, including `rustfmt` and `clippy`.
Whale's minimum supported Rust version is 1.86.0, declared in
[Cargo.toml](Cargo.toml). Python is needed for the CI helper and CLI smoke tests;
CI uses Python 3.13. The tracked [Rust workflow](.github/workflows/rust.yml)
defines host, MSRV, and feature checks; the [quality workflow](.github/workflows/quality.yml)
defines formatting, lint, documentation, and workflow validation requirements.

The normal build and regression suite use Whale's own components. An LLVM SDK,
Wave compiler installation, or external assembler is not required to follow the
core contribution workflow. Optional cross-host and coverage checks have their
own prerequisites in the [Rust workflow](.github/workflows/rust.yml) and
[coverage workflow](.github/workflows/coverage.yml).

Fork `wavefnd/Whale` on GitHub, then replace `YOUR_USERNAME` below with your login:

```bash
git clone https://github.com/YOUR_USERNAME/Whale.git
cd Whale
git remote add upstream https://github.com/wavefnd/Whale.git
git fetch upstream
git switch -c fix/your-change upstream/master
```

For an existing checkout, inspect `git remote -v` first. `origin` should be your
fork and `upstream` should be `wavefnd/Whale`; do not add a duplicate remote or
change an unrelated remote silently. Start new work from the latest upstream
`master`, not from an old feature branch.

## 2. Repository map

| Area | Responsibility |
| --- | --- |
| [assembler/](assembler/) | Assembly tokenization, parsing, AMD64 encoding, symbols, and relocations |
| [object/](object/) | Object model and ELF serialization |
| [target-spec/](target-spec/) | Supported output targets, data layouts, and object identities |
| [ir/](ir/) | Shared IR, builders, verification, and feature-gated socket lowering |
| [linker/](linker/) | Linker component; consult the current capability status before assuming a full native link path exists |
| [src/](src/) | `whale` CLI and command integration |
| [tests/](tests/) | Repository-level integration tests |
| [tools/](tools/) | CI helpers, smoke checks, and their tests |
| [README.md](README.md) | Public usage examples and current capability boundaries |
| [.github/](.github/) | Contribution templates and CI configuration |

[README.md](README.md) distinguishes implemented and experimental capabilities.
The default build provides typed IR `verify` and `print`. The `socket-cli` feature
enables AST JSON `lower`; `--all-features` also
exercises the socket lowering tests. A host on which Whale runs is not
necessarily an architecture for which it can generate machine code.

For a focused regression, start with the affected component:

| Component | Source and test locations | Focused command from the repository root |
| --- | --- | --- |
| Assembler | [assembler/src/](assembler/src/) and [assembler/tests/](assembler/tests/) | `cargo test -p assembler --locked` |
| Object | [object/src/](object/src/) | `cargo test -p object --locked` |
| IR | [ir/src/](ir/src/) and [ir/tests/](ir/tests/) | `cargo test -p ir --all-features --locked` |
| Linker | [linker/src/](linker/src/) | `cargo test -p linker --locked` |
| CLI | [src/](src/) and [tests/](tests/) | `cargo test -p whale --all-features --locked` |
| CI helpers | [tools/](tools/) | `python3 -m unittest discover -s tools -p 'test_*.py' -v` |

Run the workspace checks in [Local verification](#5-local-verification) after
the focused regression; a single component check does not cover its consumers.

## 3. Submit a contribution

### Choosing a first issue

Start with the [open good first issues](https://github.com/wavefnd/Whale/issues?q=is%3Aissue%20is%3Aopen%20label%3A%22good%20first%20issue%22).
Read the completion criteria and linked dependencies before choosing one. For
example, [unknown-command exit status](https://github.com/wavefnd/Whale/issues/16)
has a small CLI regression, while [symbol extent validation](https://github.com/wavefnd/Whale/issues/163)
depends on the section-reference and BSS size rules. A label alone does not
mean those prerequisites or design decisions are complete. Check that an issue
is still open and unassigned, and comment with the bounded change you intend
to make so other contributors can coordinate.

### GitHub pull requests

Target `wavefnd/Whale:master` from a topic branch in your fork. Use the pull
request template to explain the change, motivation, compatibility impact, and
validation. A focused documentation contribution does not need a new issue just
to open a PR. Link an existing report or design discussion when it is relevant.

Stage only the intended files and sign off every commit:

```bash
git add path/to/changed-file
git commit -s -m "docs: clarify the assembler contribution workflow"
git push -u origin HEAD
```

Replace the example path and message with your actual change. Do not combine
unrelated refactors, generated artifacts, or repository-wide formatting with a
bug fix. When responding to review, push to the same PR branch and summarize
what changed and which checks were rerun.

### Email patches

Email patches can be sent to the component contact in [MAINTAINERS](MAINTAINERS).
Include `[Whale PATCH]` in the subject so the project is unambiguous. For a single
signed-off commit:

```bash
git format-patch -1 --subject-prefix="Whale PATCH"
git send-email --to=luna@lunastev.org 0001-*.patch
```

`git send-email` may require a separate package and mail configuration. Send only
the intended patch files, include validation results, and identify the base
commit. A series should have one logical change per patch and a cover letter.
Do not submit the same change independently by email and PR without linking the
two discussions. Whale does not currently provide Wave's maintainer-discovery
or patch-verification scripts; use the contacts and checks documented here.

## 4. Developer Certificate of Origin

Every submitted commit must include a `Signed-off-by:` trailer, added by
`git commit -s`, certifying the [Developer Certificate of Origin](https://developercertificate.org/).
Use your own contribution identity and email address:

```text
Signed-off-by: Your Name <email@example.com>
```

Sign-off is not the same as a cryptographic commit signature. Preserve existing
authorship and sign-offs when revising someone else's patch; do not sign on
another person's behalf. Contributions without the required sign-off need to be
corrected before acceptance.

## 5. Local verification

Run these checks from the repository root with the committed `Cargo.lock`:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --all-features --locked
cargo build --workspace --locked
```

For CLI changes, validate both feature configurations. Build and test the
default-feature binary before replacing it with the all-features build:

```bash
python3 -m unittest discover -s tools -p 'test_*.py' -v
cargo build --workspace --locked
python3 tools/ci_smoke.py --binary target/debug/whale --socket disabled
cargo build --workspace --all-features --locked
python3 tools/ci_smoke.py --binary target/debug/whale --socket enabled
```

On Windows, use `target/debug/whale.exe` and the Python command installed on your
host. Consult the [Rust workflow](.github/workflows/rust.yml) for release-mode,
MSRV, and cross-host checks, the [quality workflow](.github/workflows/quality.yml)
for warning-free rustdoc, and the [coverage workflow](.github/workflows/coverage.yml)
for coverage prerequisites. [tools/ci.py](tools/ci.py) retains command logs and
execution metadata in `.ci-artifacts/` for reproducing failures.
Those checks are not replaced by a successful `cargo check`.

Record exact commands, results, host, and toolchain for the checks you ran.
Identify checks you could not run and explain why; never describe a skipped
check or a hand-written model as an executed project regression. For prose-only
changes, verify paths, links, examples, and the final diff, and state that scope
in the PR rather than claiming unexecuted Rust tests passed.

### Regression tests

Put a regression beside the affected crate and exercise its public entry point
where practical. Establish the failing case before the fix, then retain both
negative and positive coverage. In particular:

- Assembler changes should check emitted bytes and relevant relocation metadata,
  not just successful assembly.
- Object changes should check the affected sections, symbols, offsets, and format
  metadata, not only a byte pattern found somewhere in the output.
- IR changes should check the structured verifier result and preserve valid
  neighboring types and signatures.
- CLI changes should cover exit status, diagnostics, and output-file behavior;
  keep the matching [README usage examples](README.md#usage) and command help current.

For example, when fixing unknown-command status, add a CLI regression in
`tests/` that runs `whale` with an unknown command, checks a nonzero exit status,
and checks the diagnostic. Keep a positive case for an existing supported
command. Run that named test with `cargo test -p whale --locked <test_name>`
and confirm it fails for the reported behavior before changing the dispatch
code. After the fix, rerun it and the default/all-feature workspace checks
above. Describe the observed failure and passing results in the PR.

Keep normal tests independent of an external assembler, linker, network access,
or a contributor's personal filesystem. Use isolated fixtures where applicable.
Do not remove existing tests, loosen expected results, or weaken CI merely to
make a patch pass.

## 6. Code style and formatting scope

Follow the surrounding Rust style: `snake_case` functions and variables,
`PascalCase` types, `SCREAMING_SNAKE_CASE` constants, same-line opening braces
(K&R style), and no trailing whitespace. Run the check-only formatting command
first. `cargo fmt --all` writes changes throughout the workspace and can reformat
files unrelated to your patch.

Inspect the diff after formatting. Keep unrelated formatting in a separate
change agreed with the maintainer. When a formatting failure appears to come
from unchanged code, update to the latest upstream base and compare that base
before expanding the PR. Report a remaining baseline failure with its command
and output rather than repeatedly reverting and reapplying unrelated files.

## 7. Updating a PR branch

First commit or deliberately stash local edits and confirm the working tree is
clean. Check that you are on your PR branch, not `master`, then:

```bash
git status --short
git remote -v
git fetch upstream
git rebase upstream/master
```

If a conflict occurs, stop and reconcile the intended change with the latest
upstream code. Stage only the resolved paths and run `git rebase --continue`.
Use `git rebase --abort` to return to the pre-rebase state when you need to
restart. Do not resolve a conflict by blindly replacing whole files with older
copies or discarding upstream fixes.

After a successful rebase, rerun the relevant checks and inspect the complete
PR diff before updating your already-published topic branch:

```bash
git diff --stat upstream/master...HEAD
git diff --check upstream/master...HEAD
git push --force-with-lease origin HEAD
```

Use a normal push when you have not rewritten published history. Coordinate any
history rewrite with other people using the branch. If the lease is rejected,
inspect the remote changes instead of retrying with `--force`. Git's
[rebase](https://git-scm.com/docs/git-rebase) and
[push](https://git-scm.com/docs/git-push) documentation describe recovery and
lease behavior in more detail.

## 8. Maintainers, licensing, and repository policies

Use [MAINTAINERS](MAINTAINERS) for review routing and private contact details.
Keep public technical discussion on the corresponding PR. Conduct reports go
to the private contact in [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

Whale is licensed under [MPL-2.0](LICENSE), except where a file explicitly states
otherwise. Preserve existing attribution and third-party notices; see
[COPYRIGHT](COPYRIGHT) and [NOTICE](NOTICE). Whale does not inherit Wave's
separate Apache-2.0 standard-library license split.

Read [ai.txt](ai.txt) for the repository's AI-use policy and permission contact.
Using assistance does not replace human responsibility for authorship,
licensing, sign-off, review, or accurate validation reports. Do not infer a
blanket permission exception from the availability of a contribution tool.

Thank you for helping make Whale's toolchain and contributor experience better.
