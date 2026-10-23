# Contributing to Auralis Studio

Thanks for taking the time. This document covers the build, the branch
conventions, and what CI will check before a pull request can merge.

中文贡献指南见 [README.zh-CN.md](./README.zh-CN.md)。

## The one rule that outranks everything else

**Never claim something this project has not measured.** `AGENTS.md` states it
and it is the reason the codebase looks the way it does:

- A container format is not "supported" because the decoder compiled — it is
  supported when a real sample round-trips through import → DSP. See
  [the format table in the README](./README.md#container-support--measured--not-assumed).
- A DSP figure is not "fast enough" until `docs/BENCHMARKS.md` has the number.
- A metric is not calibrated until the algorithm and its preconditions are
  implemented and validated. No SPL, LUFS, SNR, or THD claims yet.

If you cannot verify it, say so in the pull request. A pull request that
reports honest limitations is more useful than one that quietly overstates.

## Prerequisites

| Tool | Version | Notes |
|---|---|---|
| Rust | 1.99.0 | Pinned by `rust-toolchain.toml`; do not bump it in a PR |
| MSVC build tools | — | `+crt-static` is enabled, see ADR-013 |
| Node.js | 22.12+ or 24 | Required by Electron 45 and by vitest 5 (the unit tests need `node >= 22.12`). CI also builds the renderer on Node 20 (typecheck + build only) |
| Python | 3.12 | Only for the numpy oracle cross-check |

Full local setup, including mirror configuration and the on-disk paths this
project assumes, is in [docs/DEV_ENVIRONMENT.md](./docs/DEV_ENVIRONMENT.md).

## Build and run

```powershell
# Frontend only
cd apps/desktop
npm install
npm run dev

# Full app (starts Vite + Electron; build the engine first)
cargo build -p auralis-engine
npm run electron:dev

# Production build
npm run electron:start
```

## The checks CI runs

Run these before you push. They are the same commands `.github/workflows/ci.yml`
executes, and branch protection requires them to pass.

```powershell
# Architecture boundaries: core/dsp stay pure, frontend does no audio math
./scripts/check-boundaries.ps1

cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked

# IPC payload sizes, printed (ADR-018)
cargo test -p auralis-desktop ipc --locked -- --nocapture

# DSP cross-validation against an independent numpy implementation
./scripts/check-oracle.ps1

cd apps/desktop && npm run typecheck
```

> [!NOTE]
> **`check-oracle.ps1` needs a release build first.** It shells out to
> `target\release\auralis-studio.exe --dump-reference`. Run
> `cargo build --workspace --release --locked` before it.

> [!WARNING]
> **The release binary is a GUI-subsystem executable.** Calling it directly from
> PowerShell does not reliably wait, so `$LASTEXITCODE` can come back empty.
> Use `Start-Process -Wait -PassThru` in any script that needs the exit code —
> `scripts/check-oracle.ps1` already does this.

## Architecture rules your change must not break

Enforced by `scripts/check-boundaries.ps1` and by review:

1. **React never computes audio.** dB conversion, FFT, decimation, and
   envelope building all happen in Rust. The frontend receives finished
   buffers.
2. **`auralis-core` and `auralis-dsp` have no UI, no device API, no filesystem,
   no database.** They take data in and give data back.
3. **The audio callback never blocks, never allocates unboundedly, never
   waits on the UI.** It copies samples and bumps counters. Everything else
   happens on a worker thread. `docs/BENCHMARKS.md` §Conclusion 1 is the
   measurement behind this rule: a 4096-point FFT takes 271 µs against a
   20.8 µs callback budget.
4. **Real-time data flow uses bounded queues.** The SPSC ring reports dropped
   frames; dropped frames are instrumented, not silently swallowed.

## DSP change checklist

If your change touches `crates/auralis-dsp`, all of these apply:

- [ ] Document units, reference level, scaling, and window correction.
- [ ] State whether you are producing an amplitude spectrum, a power spectrum,
      or a power spectral density. They are not interchangeable.
- [ ] Handle silence, DC, NaN/Inf, partial frames, and the Nyquist/DC bins
      explicitly.
- [ ] Add a deterministic test — no randomness, no wall-clock, no fixtures
      from the internet.
- [ ] If CI asserts a named DSP test exists (see `dsp-regression` in
      `ci.yml`), keep that name stable or update the list in the same commit.

## Branch and commit conventions

```
main / master   integration branch, protected
feature/…       new work
fix/…           bug fixes
docs/…          documentation only
```

Commit messages in this repository describe *why*, and name the constraint that
forces the design. The M4 capture work, for example, is not "add cpal" — it is
"buffer recycling, because the callback may not allocate."

## Adding a dependency

Dependencies are declared once in `[workspace.dependencies]` in the root
`Cargo.toml` (ADR-007). Members reference them with `xxx = { workspace = true }`.
Use `cargo add -p <crate>` to resolve a currently-compatible version, then move
the result up to the workspace table.

`Cargo.lock` is committed. CI builds with `--locked`.

## Reporting bugs

Use the [bug report template](./.github/ISSUE_TEMPLATE/bug_report.yml). A DSP
bug report needs the sample rate, the expected and actual numbers, and what you
fed in — "the spectrum looks wrong" is not reproducible.
