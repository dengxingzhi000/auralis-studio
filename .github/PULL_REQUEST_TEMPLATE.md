# Pull request

## What this changes

<!-- One or two sentences. If this fixes an issue, end the line with "Closes #123". -->

## Why this way

<!--
The interesting part. Name the constraint that forced this design.

Examples of good answers:
  "Buffer recycling, because the callback may not allocate (docs/BENCHMARKS.md)."
  "Returns the buffer on overflow instead of dropping it, which is what makes
   recycling possible at all."
  "No active-item guard here: it would need a lock on the real-time path."

Avoid:
  "Cleaner." / "Follows best practice." / "Refactor."
-->

## How it was verified

<!--
Paste the actual commands and their actual results. "Tests pass" is not
verification — say which tests, and paste the output.

  cargo fmt --all -- --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace          # 205 passed; 0 failed
  cd apps/desktop && npm run typecheck
-->

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace --locked`
- [ ] `npm run typecheck` in `apps/desktop`
- [ ] `./scripts/check-boundaries.ps1` — if this touches crate boundaries
- [ ] `./scripts/check-oracle.ps1` — if this touches `auralis-dsp`

## Checklist

- [ ] No DSP moved into the frontend
- [ ] The audio callback still does no allocation, no locking, and no waiting
- [ ] New DSP behaviour has a deterministic test with documented units and
      normalization
- [ ] Silence, DC, NaN/Inf, and partial frames are handled explicitly
- [ ] `docs/` updated where behavior or interfaces changed
- [ ] An ADR was added if this reverses or replaces an earlier decision
- [ ] Known limitations are stated honestly, not omitted

> Reminder from `AGENTS.md`: do not claim formats, performance figures, or
> measurement accuracy that have not been measured. A pull request that
> reports what it could not verify is fine. One that quietly overstates is not.
