# capsflip: guide for coding agents

capsflip brings Mac keyboard habits to Windows. v0.1 makes Caps Lock switch
the keyboard layout. Read [docs/design.md](docs/design.md) before changing
anything: it holds the decisions, the glossary and the architecture.

## Layout of the repository

- `crates/capsflip-core`: all logic. No Win32, no `unsafe`, written test-first.
- `crates/capsflip`: the Windows binary. It translates between Win32 and the
  core types and makes no decisions of its own.
- `crates/capsflip/examples/probe.rs`: a diagnostic for checking Win32
  behaviour on a real machine.
- `docs/`: design, Win32 notes, the manual test checklist.
- `scripts/check.ps1`: every quality gate in one command.

## How to work

Follow ub-mode: [.claude/skills/ub-mode/SKILL.md](.claude/skills/ub-mode/SKILL.md).
The skill names three tools that this project replaces with Rust equivalents:

| ub-mode | Here | Gate |
|---|---|---|
| Crapper | `cargo llvm-cov` + `cargo crap` | every function at CRAP 10 or below |
| Dryer | `cargo dupes` | no exact or near duplicates at 0.7 or above |
| Mutator | `cargo mutants` (core crate only) | no surviving mutants |

Run all of it with:

```
pwsh scripts/check.ps1
```

Add `-Fast` to skip mutation testing while iterating. CI runs the same script.

The duplication threshold is 0.7 rather than the 0.8 ub-mode names, because
the scale differs from Dryer's: on a hand-made sample, a copy with renamed
names, one changed constant and one added line scores 0.72 in `cargo dupes`,
and unrelated functions do not show up even at 0.5. `similarity-rs` was tried
first and rejected: it scores unrelated small functions between 0.70 and 0.87,
so no threshold separates duplicates from noise.

Rules that are specific to this project:

- A decision belongs in `capsflip-core`, behind a test that fails first. If the
  shell needs an `if` that is not about a Win32 error, the logic is in the
  wrong crate.
- The shell cannot be unit tested honestly, so do not write tests that mock
  Win32. Verify it with `docs/manual-test.md`, and use the probe to check an
  assumption before building on it.
- Never hard-code a layout handle. Layouts are found by language identifier.
- The hook callback only asks the engine and posts a message. Windows removes
  a slow hook silently.
- Commits are small and ordered. Each message says why, what changed and how
  it was proved.
- Work is tracked in GitHub issues under the `v0.1`, `v0.2` and `backlog`
  milestones. Deferred decisions live there, not in conversations.

## Tooling

One-time setup:

```
cargo install --locked cargo-llvm-cov cargo-mutants cargo-crap cargo-dupes
```

The compiler version, its components and the build targets are pinned in
`rust-toolchain.toml`, so local runs and CI see the same lints. Raise the
version deliberately, in its own commit.

Linking for ARM64 needs the MSVC ARM64 build tools. Without them, type check
with `cargo check --target aarch64-pc-windows-msvc`, which is what
`scripts/check.ps1` does; CI links the real ARM64 binary.
